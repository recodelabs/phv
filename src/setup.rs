//! Interactive walkthrough that creates one person's vault. This is the only place phv runs `git` or `gh`.

use crate::error::PhvError;
use crate::store::{Registry, Vault};
use crate::write::{AddOptions, Phv};
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const GITHUB_WARNING: &str = "GitHub can read everything you push: phv does not encrypt it. That includes \
health records and original documents, even in a private repo. phv does not recommend this until it supports \
encrypted backups.";

pub fn config_path(home: &Path) -> PathBuf {
    home.join(".config/phv/config.toml")
}

pub fn load_registry(home: &Path) -> Result<Registry, PhvError> {
    match std::fs::read_to_string(config_path(home)) {
        Ok(text) => Registry::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn save_registry(home: &Path, registry: &Registry) -> Result<(), PhvError> {
    let path = config_path(home);
    let dir = path.parent().expect("the config path has a parent");
    std::fs::create_dir_all(dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(registry.to_toml().as_bytes())?;
    temp.persist(&path).map_err(|e| PhvError::Io(e.error))?;
    Ok(())
}

/// Prints a question and reads one line. Running out of input is an error, so setup cannot loop forever.
fn ask(question: &str) -> Result<String, PhvError> {
    print!("{question} ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line)? == 0 {
        println!();
        return Err(PhvError::Usage("setup stopped: no more input".into()));
    }
    Ok(line.trim().to_string())
}

/// Asks a yes or no question. Anything but a clear answer is asked again.
fn ask_yes_no(question: &str, default_yes: bool) -> Result<bool, PhvError> {
    loop {
        let answer = ask(question)?;
        if answer.is_empty() {
            return Ok(default_yes);
        }
        if answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes") {
            return Ok(true);
        }
        if answer.eq_ignore_ascii_case("n") || answer.eq_ignore_ascii_case("no") {
            return Ok(false);
        }
        println!("Please answer y or n.");
    }
}

fn expand_home(answer: &str, home: &Path) -> PathBuf {
    if answer == "~" {
        return home.to_path_buf();
    }
    match answer.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(answer),
    }
}

/// Accepts a GitHub repo name, or `owner/name`. A leading `-` is refused so it can never be read as a flag.
fn is_valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

/// Opens an unregistered vault that is already at `root`, or creates one in a new or empty folder.
/// Nothing is created or registered if the folder holds other files.
fn open_or_create(root: &Path) -> Result<Vault, PhvError> {
    let root = std::path::absolute(root)?;
    if root.join("phv.toml").exists() {
        println!(
            "{} is already a vault that was not registered. Registering it.",
            root.display()
        );
        return Vault::open(&root);
    }
    match std::fs::read_dir(&root) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(PhvError::Usage(format!(
                    "{} already has files in it. Choose a new or empty folder, so nothing unrelated is saved with your health records.",
                    root.display()
                )));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(PhvError::Usage(format!(
                "cannot use {} for a vault: {e}",
                root.display()
            )));
        }
    }
    Vault::init(&root).map_err(|e| match e {
        PhvError::Usage(message) => PhvError::Usage(format!(
            "cannot create a vault at {}: {message}",
            root.display()
        )),
        PhvError::Io(e) => PhvError::Io(std::io::Error::new(
            e.kind(),
            format!("cannot create a vault at {}: {e}", root.display()),
        )),
        other => other,
    })
}

/// True if the command runs and exits successfully, with its output hidden.
fn succeeds(program: &str, args: &[&str], dir: Option<&Path>) -> bool {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command.status().is_ok_and(|status| status.success())
}

async fn add_patient(root: &Path) -> Result<(), PhvError> {
    let vault = Vault::open(root)?;
    if !vault.lines("Patient")?.is_empty() {
        println!("\nThis vault already has its Patient, so there is nothing to add.");
        return Ok(());
    }
    let phv = Phv::new(vault).await?;
    println!("\nNow the person's details. Press Enter to skip any of them.");
    let family = ask("Family name:")?;
    let given = ask("Given name:")?;
    loop {
        let birth_date = ask("Birth date (YYYY-MM-DD):")?;
        let mut patient = json!({"resourceType": "Patient"});
        if !family.is_empty() || !given.is_empty() {
            let mut name = json!({});
            if !family.is_empty() {
                name["family"] = json!(family);
            }
            if !given.is_empty() {
                name["given"] = json!([given]);
            }
            patient["name"] = json!([name]);
        }
        if !birth_date.is_empty() {
            patient["birthDate"] = json!(birth_date);
        }
        if patient.as_object().is_some_and(|p| p.len() == 1) {
            println!("Skipped. You can add a Patient later with phv add.");
            return Ok(());
        }
        match phv.add(&patient, &AddOptions::default()).await {
            Ok(report) => {
                if birth_date.is_empty() {
                    println!("Added {}.", report.created.join(", "));
                } else {
                    println!("Added {} born {birth_date}.", report.created.join(", "));
                }
                return Ok(());
            }
            Err(PhvError::Invalid(issues)) => {
                if issues.iter().any(|i| i.path.contains("birthDate")) {
                    println!(
                        "That doesn't look like a date. Use year-month-day, for example 1980-01-31, or press Enter to leave it out."
                    );
                } else {
                    println!(
                        "That was not accepted: {}\nTry the birth date again, or press Enter to leave it out.",
                        issues.first().map_or_else(String::new, ToString::to_string)
                    );
                }
            }
            Err(other) => return Err(other),
        }
    }
}

/// Returns true only if the vault now has a Git repo with its first commit.
fn setup_git(root: &Path) -> Result<bool, PhvError> {
    println!("\nGit keeps a history of every change, so you can review and undo them.");
    if !succeeds("git", &["--version"], None) {
        println!(
            "Git is not installed, so this vault will have no history or undo. It works fine without it."
        );
        return Ok(false);
    }
    if !ask_yes_no("Use Git for this vault? [Y/n]", true)? {
        println!("No Git. Changes will be final, with no history or undo.");
        return Ok(false);
    }
    // Stage vault files by name, so nothing else in the folder can be committed or pushed.
    let committed = succeeds("git", &["init", "-q", "-b", "main"], Some(root))
        && succeeds(
            "git",
            &["add", "--", "phv.toml", ".gitignore", "records", "sources"],
            Some(root),
        )
        && succeeds("git", &["commit", "-qm", "Create health vault"], Some(root));
    if committed {
        println!("Created a Git repo with a first commit.");
    } else {
        println!(
            "Git could not make the first commit, most often because it does not know your name yet. To fix it, run:\n  git config --global user.name \"Your Name\"\n  git config --global user.email \"you@example.com\"\nThen, in {}, run:\n  git add -- phv.toml .gitignore records sources\n  git commit -m \"Create health vault\"\nGitHub is not offered until the vault has a commit.",
            root.display()
        );
    }
    Ok(committed)
}

fn setup_github(root: &Path, name: &str) -> Result<(), PhvError> {
    // `gh auth status` doubles as the presence check: failing to start means gh is not installed.
    // Running `gh --version` as well would be an extra gh call before the person says yes.
    let auth = Command::new("gh")
        .args(["auth", "status"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let Ok(auth) = auth else {
        println!(
            "\nThe GitHub CLI (gh) is not installed, so there is no GitHub repo. This vault stays on this machine."
        );
        return Ok(());
    };
    if !auth.success() {
        println!(
            "\ngh is not logged in, so there is no GitHub repo. To add one later, run gh auth login, then create a private repo."
        );
        return Ok(());
    }
    println!("\n{GITHUB_WARNING}");
    if !ask_yes_no("Create a private GitHub repo for this vault? [y/N]", false)? {
        println!("No GitHub repo. This vault stays on this machine.");
        return Ok(());
    }
    let default_repo = format!("phv-vault-{name}");
    let repo = loop {
        let answer = ask(&format!("Repo name [{default_repo}]:"))?;
        if answer.is_empty() {
            break default_repo;
        }
        if is_valid_repo_name(&answer) {
            break answer;
        }
        println!(
            "A repo name uses letters, numbers, . _ - and /, and cannot start with -. Try again."
        );
    };
    let source = format!("--source={}", root.display());
    // Always private. phv has no option to create a public repo.
    if succeeds(
        "gh",
        &[
            "repo",
            "create",
            &repo,
            "--private",
            &source,
            "--remote=origin",
            "--push",
        ],
        Some(root),
    ) {
        println!("Created private repo {repo} and pushed this vault.");
    } else {
        println!(
            "GitHub step failed. Your local vault is fine. To retry, run this in {}: gh repo create {repo} --private --source=. --remote=origin --push",
            root.display()
        );
    }
    Ok(())
}

pub async fn run(home: &Path) -> Result<(), PhvError> {
    println!(
        "This creates a health vault for one person. Run phv setup again for each family member.\n"
    );
    let mut registry = load_registry(home)?;

    let name = ask("Short name for this vault, in lowercase letters and numbers, such as maya:")?;
    let default_path = home.join("health-vaults").join(&name);
    // Check the name before asking anything else or creating a folder from it.
    if let Some(existing) = registry.vaults.get(&name) {
        return Err(PhvError::Usage(format!(
            "a vault named {name} already exists at {}. To add the person's details, run phv --vault {name} add. To use Git, run git init in that folder.",
            existing.display()
        )));
    }
    let mut updated = registry.clone();
    updated.register(&name, &default_path)?;

    let answer = ask(&format!(
        "Where should it live? [{}]",
        default_path.display()
    ))?;
    let root = if answer.is_empty() {
        default_path
    } else {
        expand_home(&answer, home)
    };

    // Check the folder before creating or registering anything.
    let vault = open_or_create(&root)?;
    registry.register(&name, vault.root())?;
    save_registry(home, &registry)?;
    println!("Vault {name} is at {}.", vault.root().display());

    add_patient(vault.root()).await?;

    if setup_git(vault.root())? {
        setup_github(vault.root(), &name)?;
    }

    println!("\nDone. Vault {name} is ready.");
    if registry.default.as_deref() == Some(name.as_str()) {
        println!("It is your default vault.");
    } else {
        println!("Use it with phv --vault {name}, or make it the default with phv use {name}.");
    }
    println!("\nTo use this vault from a Claude Code session, run:");
    println!("  claude mcp add phv-{name} -- phv --vault {name} mcp");
    println!(
        "Claude runs on Anthropic's servers, so anything a session reads from the vault goes to Anthropic."
    );
    Ok(())
}
