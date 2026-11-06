use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

struct Env {
    _dir: tempfile::TempDir,
    home: PathBuf,
    bin: PathBuf,
    gh_log: PathBuf,
}

/// An empty PATH directory, so `git` and `gh` are absent unless a test adds them.
fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let gh_log = dir.path().join("gh.log");
    Env {
        _dir: dir,
        home,
        bin,
        gh_log,
    }
}

fn add_git(env: &Env) {
    let path = std::env::var("PATH").unwrap();
    let git = std::env::split_paths(&path)
        .map(|d| d.join("git"))
        .find(|p| p.exists())
        .expect("git is installed for tests");
    symlink(git, env.bin.join("git")).unwrap();
}

/// A fake `gh` that records its arguments. `auth status` exits with `auth_code`.
fn add_gh(env: &Env, auth_code: i32) {
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = auth ]; then exit {auth_code}; fi\necho \"$@\" >> {}\n",
        env.gh_log.display()
    );
    let path = env.bin.join("gh");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn phv(env: &Env, args: &[&str], answers: &[&str]) -> Output {
    phv_with(env, args, answers, true)
}

/// With `identity` false, no Git author or committer variables are set.
fn phv_with(env: &Env, args: &[&str], answers: &[&str], identity: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_phv"));
    command
        .args(args)
        .env("HOME", &env.home)
        .env("PATH", &env.bin)
        .env_remove("PHV_VAULT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if identity {
        command
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid");
    } else {
        for key in [
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL",
            "EMAIL",
        ] {
            command.env_remove(key);
        }
    }
    let mut child = command.spawn().unwrap();
    let mut text = answers.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    let _ = child.stdin.take().unwrap().write_all(text.as_bytes());
    child.wait_with_output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn vault(env: &Env, name: &str) -> PathBuf {
    env.home.join("health-vaults").join(name)
}

// Answers: name, location, family, given, birth date, then the Git and GitHub questions.
const PERSON: [&str; 5] = ["maya", "", "Example", "Maya", "1980-01-01"];

fn answers(extra: &[&'static str]) -> Vec<&'static str> {
    PERSON
        .iter()
        .copied()
        .chain(extra.iter().copied())
        .collect()
}

#[test]
fn works_with_no_git_installed() {
    let e = env();
    let o = phv(&e, &["setup"], &answers(&[]));
    assert!(o.status.success(), "{o:?}");
    assert!(
        stdout(&o).contains("Git is not installed"),
        "{}",
        stdout(&o)
    );
    let root = vault(&e, "maya");
    assert!(root.join("phv.toml").exists());
    assert!(!root.join(".git").exists());
    assert_eq!(
        fs::read_to_string(root.join("records/Patient.ndjson"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn git_can_be_declined() {
    let e = env();
    add_git(&e);
    let o = phv(&e, &["setup"], &answers(&["n"]));
    assert!(o.status.success(), "{o:?}");
    assert!(!vault(&e, "maya").join(".git").exists());
}

#[test]
fn accepting_git_makes_a_first_commit_and_skips_github_without_gh() {
    let e = env();
    add_git(&e);
    let o = phv(&e, &["setup"], &answers(&[""]));
    assert!(o.status.success(), "{o:?}");
    assert!(vault(&e, "maya").join(".git").exists());
    assert!(stdout(&o).contains("gh"), "{}", stdout(&o));
    let log = Command::new(e.bin.join("git"))
        .args([
            "-C",
            vault(&e, "maya").to_str().unwrap(),
            "log",
            "--oneline",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).lines().count(), 1);
    let status = Command::new(e.bin.join("git"))
        .args([
            "-C",
            vault(&e, "maya").to_str().unwrap(),
            "status",
            "--short",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&status.stdout), "");
}

#[test]
fn github_defaults_to_no_and_warns_first() {
    let e = env();
    add_git(&e);
    add_gh(&e, 0);
    let o = phv(&e, &["setup"], &answers(&["", ""]));
    assert!(o.status.success(), "{o:?}");
    assert!(stdout(&o).contains("GitHub can read"), "{}", stdout(&o));
    assert!(!e.gh_log.exists(), "gh must not run without a yes");
}

#[test]
fn github_yes_creates_a_private_repo() {
    let e = env();
    add_git(&e);
    add_gh(&e, 0);
    let o = phv(&e, &["setup"], &answers(&["", "y", ""]));
    assert!(o.status.success(), "{o:?}");
    let log = fs::read_to_string(&e.gh_log).unwrap();
    assert!(log.contains("repo create phv-vault-maya"), "{log}");
    assert!(log.contains("--private"), "{log}");
    assert!(log.contains("--push"), "{log}");
    assert!(!log.contains("--public"), "{log}");
}

#[test]
fn logged_out_gh_is_not_offered() {
    let e = env();
    add_git(&e);
    add_gh(&e, 1);
    let o = phv(&e, &["setup"], &answers(&[""]));
    assert!(o.status.success(), "{o:?}");
    assert!(stdout(&o).contains("gh auth login"), "{}", stdout(&o));
    assert!(!e.gh_log.exists());
}

#[test]
fn a_failed_github_step_keeps_the_local_vault() {
    let e = env();
    add_git(&e);
    let path = e.bin.join("gh");
    fs::write(
        &path,
        "#!/bin/sh\nif [ \"$1\" = auth ]; then exit 0; fi\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let o = phv(&e, &["setup"], &answers(&["", "y", ""]));
    assert!(o.status.success(), "{o:?}");
    assert!(stdout(&o).contains("GitHub step failed"), "{}", stdout(&o));
    assert!(vault(&e, "maya").join("phv.toml").exists());
}

#[test]
fn each_family_member_gets_a_separate_vault() {
    let e = env();
    assert!(phv(&e, &["setup"], &answers(&[])).status.success());
    let ben = ["ben", "", "Example", "Ben", "2015-06-01"];
    assert!(phv(&e, &["setup"], &ben).status.success());

    let listed = stdout(&phv(&e, &["vaults"], &[]));
    assert!(
        listed.contains("maya") && listed.contains("ben"),
        "{listed}"
    );
    assert!(listed.lines().any(|l| l.starts_with("* maya")), "{listed}");

    assert!(stdout(&phv(&e, &["list", "Patient"], &[])).contains("1980-01-01"));
    assert!(stdout(&phv(&e, &["--vault", "ben", "list", "Patient"], &[])).contains("2015-06-01"));

    assert!(phv(&e, &["use", "ben"], &[]).status.success());
    assert!(stdout(&phv(&e, &["list", "Patient"], &[])).contains("2015-06-01"));
    assert_eq!(phv(&e, &["use", "nobody"], &[]).status.code(), Some(2));
}

#[test]
fn a_duplicate_or_invalid_name_is_refused() {
    let e = env();
    assert!(phv(&e, &["setup"], &answers(&[])).status.success());
    assert_eq!(phv(&e, &["setup"], &answers(&[])).status.code(), Some(2));
    assert_eq!(phv(&e, &["setup"], &["../evil"]).status.code(), Some(2));
    assert!(!e.home.join("evil").exists());
}

#[test]
fn a_bad_birth_date_is_asked_again_and_patient_can_be_skipped() {
    let e = env();
    let o = phv(
        &e,
        &["setup"],
        &["maya", "", "Example", "Maya", "not-a-date", "1980-01-01"],
    );
    assert!(o.status.success(), "{o:?}");
    assert!(stdout(&o).contains("1980-01-01"), "{}", stdout(&o));
    assert!(
        fs::read_to_string(vault(&e, "maya").join("records/Patient.ndjson"))
            .unwrap()
            .contains("1980-01-01")
    );

    let skipped = phv(&e, &["setup"], &["ben", "", "", "", ""]);
    assert!(skipped.status.success(), "{skipped:?}");
    assert!(!vault(&e, "ben").join("records/Patient.ndjson").exists());
}

#[test]
fn running_out_of_answers_stops_instead_of_looping() {
    let e = env();
    assert_eq!(phv(&e, &["setup"], &["maya"]).status.code(), Some(2));
}

#[test]
fn a_custom_location_is_used_and_tilde_is_expanded() {
    let e = env();
    let o = phv(&e, &["setup"], &["maya", "~/elsewhere/maya", "", "", ""]);
    assert!(o.status.success(), "{o:?}");
    assert!(e.home.join("elsewhere/maya/phv.toml").exists());
}

#[test]
fn replace_and_rm_warn_when_a_vault_has_no_git() {
    let e = env();
    assert!(phv(&e, &["setup"], &answers(&[])).status.success());
    let root: &Path = &vault(&e, "maya");
    let line = fs::read_to_string(root.join("records/Patient.ndjson")).unwrap();
    let id = line
        .split("\"id\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let o = phv(&e, &["rm", &format!("Patient/{id}")], &[]);
    assert!(o.status.success(), "{o:?}");
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("cannot be undone"),
        "{o:?}"
    );
}

#[test]
fn setup_shows_how_to_connect_a_claude_session() {
    let e = env();
    let o = phv(&e, &["setup"], &answers(&[]));
    assert!(
        stdout(&o).contains("claude mcp add phv-maya -- phv --vault maya mcp"),
        "{}",
        stdout(&o)
    );
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn registered(env: &Env) -> String {
    stdout(&phv(env, &["vaults"], &[]))
}

/// Makes a vault without registering it, as `phv init` does.
fn init_unregistered(env: &Env, path: &Path) {
    let made = Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(["init", path.to_str().unwrap()])
        .env("HOME", &env.home)
        .env("PATH", &env.bin)
        .status()
        .unwrap();
    assert!(made.success());
}

#[test]
fn a_non_empty_folder_is_refused_and_nothing_is_registered() {
    let e = env();
    let docs = e.home.join("Documents");
    fs::create_dir_all(&docs).unwrap();
    fs::write(docs.join("taxes.txt"), "not a vault file").unwrap();
    let o = phv(&e, &["setup"], &["maya", "~/Documents"]);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert!(stderr(&o).contains("Documents"), "{}", stderr(&o));
    assert!(!docs.join("phv.toml").exists());
    assert!(!docs.join("records").exists());
    assert!(registered(&e).contains("no vaults yet"));
}

#[test]
fn the_home_folder_is_refused_as_a_location() {
    let e = env();
    fs::write(e.home.join("taxes.txt"), "not a vault file").unwrap();
    for answer in ["~/", "~"] {
        let o = phv(&e, &["setup"], &["maya", answer]);
        assert_eq!(o.status.code(), Some(2), "{answer}: {o:?}");
        assert!(!e.home.join("phv.toml").exists());
    }
    assert!(registered(&e).contains("no vaults yet"));
}

#[test]
fn an_existing_empty_folder_works() {
    let e = env();
    fs::create_dir_all(e.home.join("empty")).unwrap();
    let o = phv(&e, &["setup"], &["maya", "~/empty", "", "", ""]);
    assert!(o.status.success(), "{o:?}");
    assert!(e.home.join("empty/phv.toml").exists());
}

#[test]
fn the_first_commit_holds_only_vault_files() {
    let e = env();
    add_git(&e);
    let root = e.home.join("existing");
    init_unregistered(&e, &root);
    fs::write(root.join("notes.txt"), "not a vault file").unwrap();
    let o = phv(
        &e,
        &["setup"],
        &["maya", "~/existing", "Example", "Maya", "1980-01-01", ""],
    );
    assert!(o.status.success(), "{o:?}");
    let files = Command::new(e.bin.join("git"))
        .args(["-C", root.to_str().unwrap(), "ls-files"])
        .output()
        .unwrap();
    let files = String::from_utf8_lossy(&files.stdout).to_string();
    assert!(files.contains("phv.toml"), "{files}");
    assert!(files.contains("records/Patient.ndjson"), "{files}");
    for file in files.lines() {
        assert!(
            file == "phv.toml"
                || file == ".gitignore"
                || file.starts_with("records/")
                || file.starts_with("sources/"),
            "unexpected tracked file {file}"
        );
    }
}

#[test]
fn a_failed_commit_does_not_offer_github() {
    let e = env();
    add_git(&e);
    add_gh(&e, 0);
    fs::write(
        e.home.join(".gitconfig"),
        "[user]\n\tuseConfigOnly = true\n",
    )
    .unwrap();
    let o = phv_with(&e, &["setup"], &answers(&[""]), false);
    assert!(o.status.success(), "{o:?}");
    let out = stdout(&o);
    assert!(out.contains("git config --global user.name"), "{out}");
    assert!(out.contains("git config --global user.email"), "{out}");
    assert!(!out.contains("GitHub can read"), "{out}");
    assert!(!e.gh_log.exists(), "gh must not be asked to create a repo");
    assert!(vault(&e, "maya").join("phv.toml").exists());
}

#[test]
fn rerunning_an_interrupted_setup_says_what_is_left() {
    let e = env();
    let stopped = phv(&e, &["setup"], &["maya", ""]);
    assert_eq!(stopped.status.code(), Some(2), "{stopped:?}");

    let again = phv(&e, &["setup"], &["maya"]);
    assert_eq!(again.status.code(), Some(2), "{again:?}");
    let message = stderr(&again);
    assert!(message.contains("already exists at"), "{message}");
    assert!(
        message.contains(vault(&e, "maya").to_str().unwrap()),
        "{message}"
    );
    assert!(message.contains("phv --vault maya add"), "{message}");
    assert!(message.contains("git init"), "{message}");

    assert!(
        phv(&e, &["--vault", "maya", "list", "Patient"], &[])
            .status
            .success()
    );
}

#[test]
fn an_existing_unregistered_vault_is_registered() {
    let e = env();
    init_unregistered(&e, &e.home.join("kept"));
    let o = phv(&e, &["setup"], &["maya", "~/kept", "", "", ""]);
    assert!(o.status.success(), "{o:?}");
    let listed = registered(&e);
    assert!(
        listed.contains("maya") && listed.contains("kept"),
        "{listed}"
    );
}

#[test]
fn a_rejected_birth_date_gets_a_plain_message() {
    let e = env();
    let o = phv(
        &e,
        &["setup"],
        &["maya", "", "Example", "Maya", "31/01/1980", "1980-01-31"],
    );
    assert!(o.status.success(), "{o:?}");
    let out = stdout(&o);
    assert!(
        out.contains("That doesn't look like a date. Use year-month-day, for example 1980-01-31, or press Enter to leave it out."),
        "{out}"
    );
    assert!(!out.contains("FS1014"), "{out}");
}

#[test]
fn a_repo_name_that_could_be_a_flag_is_asked_again() {
    let e = env();
    add_git(&e);
    add_gh(&e, 0);
    let o = phv(
        &e,
        &["setup"],
        &answers(&["", "y", "--public", "-x", "bad name", "ok-name"]),
    );
    assert!(o.status.success(), "{o:?}");
    let log = fs::read_to_string(&e.gh_log).unwrap();
    assert!(log.contains("repo create ok-name"), "{log}");
    assert!(!log.contains("--public"), "{log}");
    assert_eq!(log.lines().count(), 1, "{log}");
}

#[test]
fn the_git_question_only_accepts_yes_or_no() {
    let e = env();
    add_git(&e);
    let o = phv(&e, &["setup"], &answers(&["nope", "n"]));
    assert!(o.status.success(), "{o:?}");
    assert!(stdout(&o).contains("y or n"), "{}", stdout(&o));
    assert!(!vault(&e, "maya").join(".git").exists());

    let o = phv(&e, &["setup"], &["ben", "", "", "", "", "yes"]);
    assert!(o.status.success(), "{o:?}");
    assert!(vault(&e, "ben").join(".git").exists());

    let out = phv(&e, &["setup"], &["cy", "", "", "", "", "maybe"]);
    assert_eq!(out.status.code(), Some(2), "EOF still stops: {out:?}");
}

#[test]
fn the_github_retry_hint_names_the_vault_folder() {
    let e = env();
    add_git(&e);
    let path = e.bin.join("gh");
    fs::write(
        &path,
        "#!/bin/sh\nif [ \"$1\" = auth ]; then exit 0; fi\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let o = phv(&e, &["setup"], &answers(&["", "y", ""]));
    assert!(o.status.success(), "{o:?}");
    let out = stdout(&o);
    let hint = out.split("GitHub step failed").nth(1).expect("a hint");
    assert!(hint.contains(vault(&e, "maya").to_str().unwrap()), "{hint}");
}
