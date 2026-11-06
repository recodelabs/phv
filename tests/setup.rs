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
    let mut child = Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(args)
        .env("HOME", &env.home)
        .env("PATH", &env.bin)
        .env_remove("PHV_VAULT")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
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
