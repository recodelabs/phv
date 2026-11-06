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

fn expand_home(answer: &str, home: &Path) -> PathBuf {
    match answer.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(answer),
    }
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
    let phv = Phv::new(Vault::open(root)?).await?;
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
                println!(
                    "That was not accepted: {}\nTry the birth date again, or press Enter to leave it out.",
                    issues.first().map_or_else(String::new, ToString::to_string)
                );
            }
            Err(other) => return Err(other),
        }
    }
}

/// Returns true if the vault now uses Git.
fn setup_git(root: &Path) -> Result<bool, PhvError> {
    println!("\nGit keeps a history of every change, so you can review and undo them.");
    if !succeeds("git", &["--version"], None) {
        println!(
            "Git is not installed, so this vault will have no history or undo. It works fine without it."
        );
        return Ok(false);
    }
    let answer = ask("Use Git for this vault? [Y/n]")?;
    if answer.eq_ignore_ascii_case("n") || answer.eq_ignore_ascii_case("no") {
        println!("No Git. Changes will be final, with no history or undo.");
        return Ok(false);
    }
    let ok = succeeds("git", &["init", "-q", "-b", "main"], Some(root))
        && succeeds("git", &["add", "-A"], Some(root))
        && succeeds("git", &["commit", "-qm", "Create health vault"], Some(root));
    if ok {
        println!("Created a Git repo with a first commit.");
    } else {
        println!(
            "Git could not make the first commit. Set your name and email with git config, then commit in the vault."
        );
    }
    Ok(root.join(".git").exists())
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
    let answer = ask("Create a private GitHub repo for this vault? [y/N]")?;
    if !(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")) {
        println!("No GitHub repo. This vault stays on this machine.");
        return Ok(());
    }
    let default_repo = format!("phv-vault-{name}");
    let answer = ask(&format!("Repo name [{default_repo}]:"))?;
    let repo = if answer.is_empty() {
        default_repo
    } else {
        answer
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
            "GitHub step failed. Your local vault is fine. To retry, run: gh repo create {repo} --private --source=. --remote=origin --push"
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

    let vault = Vault::init(&root)?;
    registry.register(&name, vault.root())?;
    save_registry(home, &registry)?;
    println!("Created {}.", vault.root().display());

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
