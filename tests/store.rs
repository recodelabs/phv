use phv::store::{Change, Registry, Vault, line_id, resolve_vault_path};
use std::{fs, path::Path};

const A: &str = r#"{"resourceType":"Observation","id":"a","v":1.50}"#;
const B: &str = r#"{"resourceType":"Observation","id":"b","v":2}"#;
const C: &str = r#"{"resourceType":"Observation","id":"c","v":3}"#;

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("data");
    (dir, Vault::init(&root).unwrap())
}

fn upsert(id: &str, line: &str) -> Change {
    Change::Upsert {
        resource_type: "Observation".into(),
        id: id.into(),
        line: line.into(),
    }
}

fn file(v: &Vault) -> String {
    fs::read_to_string(v.root().join("records/Observation.ndjson")).unwrap()
}

#[test]
fn init_creates_the_layout_and_open_finds_it() {
    let (_d, v) = vault();
    assert!(v.root().join("phv.toml").exists());
    assert!(v.root().join("records").is_dir());
    assert!(v.root().join("sources").is_dir());
    assert_eq!(
        fs::read_to_string(v.root().join(".gitignore")).unwrap(),
        ".phv/\n"
    );
    assert!(Vault::open(v.root()).is_ok());
}

#[test]
fn init_refuses_an_existing_vault() {
    let (_d, v) = vault();
    assert!(Vault::init(v.root()).is_err());
}

#[test]
fn open_refuses_a_directory_without_phv_toml() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Vault::open(dir.path()).is_err());
}

#[test]
fn open_refuses_a_newer_format() {
    let (_d, v) = vault();
    fs::write(
        v.root().join("phv.toml"),
        "format_version = 999\nfhir_version = \"4.0.1\"\n",
    )
    .unwrap();
    assert!(Vault::open(v.root()).is_err());
}

#[test]
fn init_refuses_the_public_code_repo() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join(".git/config"),
        "[remote \"origin\"]\n\turl = git@github.com:recodelabs/phv.git\n",
    )
    .unwrap();
    assert!(Vault::init(&dir.path().join("inside")).is_err());
}

#[test]
fn init_allows_a_private_repo_with_a_similar_name() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join(".git/config"),
        "[remote \"origin\"]\n\turl = git@github.com:recodelabs/phv-data.git\n",
    )
    .unwrap();
    assert!(Vault::init(&dir.path().join("inside")).is_ok());
}

#[test]
fn lines_stay_sorted_by_id_whatever_the_insert_order() {
    let (_d, v) = vault();
    v.apply(&[upsert("c", C)]).unwrap();
    v.apply(&[upsert("a", A)]).unwrap();
    v.apply(&[upsert("b", B)]).unwrap();
    assert_eq!(file(&v), format!("{A}\n{B}\n{C}\n"));
}

#[test]
fn other_lines_are_copied_byte_for_byte() {
    let (_d, v) = vault();
    // This line is deliberately not canonical. A write must not reformat it.
    let odd = r#"{"resourceType":"Observation","id":"a",  "v":1.50}"#;
    let path = v.root().join("records/Observation.ndjson");
    fs::write(&path, format!("{odd}\n{C}\n")).unwrap();
    v.apply(&[upsert("b", B)]).unwrap();
    assert_eq!(file(&v), format!("{odd}\n{B}\n{C}\n"));
}

#[test]
fn a_no_op_leaves_the_file_untouched() {
    let (_d, v) = vault();
    v.apply(&[upsert("a", A)]).unwrap();
    let path = v.root().join("records/Observation.ndjson");
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    v.apply(&[upsert("a", A)]).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
}

#[test]
fn a_write_only_touches_its_own_type() {
    let (_d, v) = vault();
    v.apply(&[upsert("a", A)]).unwrap();
    let path = v.root().join("records/Observation.ndjson");
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    v.apply(&[Change::Upsert {
        resource_type: "Patient".into(),
        id: "p".into(),
        line: r#"{"resourceType":"Patient","id":"p"}"#.into(),
    }])
    .unwrap();
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
}

#[test]
fn removing_the_last_line_removes_the_file() {
    let (_d, v) = vault();
    v.apply(&[upsert("a", A)]).unwrap();
    v.apply(&[Change::Remove {
        resource_type: "Observation".into(),
        id: "a".into(),
    }])
    .unwrap();
    assert!(!v.root().join("records/Observation.ndjson").exists());
}

#[test]
fn an_unsorted_file_is_refused_not_rewritten() {
    let (_d, v) = vault();
    let path = v.root().join("records/Observation.ndjson");
    let unsorted = format!("{C}\n{A}\n");
    fs::write(&path, &unsorted).unwrap();
    assert!(v.apply(&[upsert("b", B)]).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), unsorted);
}

#[test]
fn resource_types_with_path_characters_are_refused() {
    let (_d, v) = vault();
    assert!(v.lines("../../etc/passwd").is_err());
    assert!(v.get("Observation/../x", "a").is_err());
}

#[test]
fn line_id_reads_the_id_without_parsing() {
    assert_eq!(line_id(A), Some("a"));
    assert_eq!(line_id("not json"), None);
}

#[test]
fn vault_path_precedence_is_flag_then_env_then_default() {
    let config = "default = \"maya\"\n[vaults]\nmaya = \"/from/config\"\nben = \"/ben\"\n";
    let resolve = |flag: Option<&str>, env: Option<&str>, cfg: Option<&str>| {
        resolve_vault_path(flag.map(Path::new), env, cfg).unwrap()
    };
    assert_eq!(
        resolve(Some("/f"), Some("/e"), Some(config)),
        Path::new("/f")
    );
    assert_eq!(resolve(None, Some("/e"), Some(config)), Path::new("/e"));
    assert_eq!(resolve(None, None, Some(config)), Path::new("/from/config"));
    assert!(resolve_vault_path(None, None, None).is_err());
}

#[test]
fn flag_and_env_accept_a_registered_name() {
    let config = "[vaults]\nben = \"/ben\"\n";
    assert_eq!(
        resolve_vault_path(Some(Path::new("ben")), None, Some(config)).unwrap(),
        Path::new("/ben")
    );
    assert_eq!(
        resolve_vault_path(None, Some("ben"), Some(config)).unwrap(),
        Path::new("/ben")
    );
    assert_eq!(
        resolve_vault_path(Some(Path::new("other")), None, Some(config)).unwrap(),
        Path::new("other")
    );
}

#[test]
fn a_config_with_no_default_is_an_error_when_nothing_is_given() {
    assert!(resolve_vault_path(None, None, Some("[vaults]\nben = \"/ben\"\n")).is_err());
}

#[test]
fn registering_sets_the_first_default_and_refuses_duplicates() {
    let mut registry = Registry::default();
    registry.register("maya", Path::new("/maya")).unwrap();
    registry.register("ben", Path::new("/ben")).unwrap();
    assert_eq!(registry.default.as_deref(), Some("maya"));
    assert!(registry.register("ben", Path::new("/other")).is_err());
    assert_eq!(Registry::parse(&registry.to_toml()).unwrap(), registry);
}

#[test]
fn only_simple_vault_names_are_allowed() {
    let mut registry = Registry::default();
    for bad in ["", "../x", "a/b", "Maya", "a b", ".hidden"] {
        assert!(
            registry.register(bad, Path::new("/x")).is_err(),
            "{bad:?} should be refused"
        );
    }
    assert!(registry.register("maya_2-b", Path::new("/x")).is_ok());
}

#[test]
fn use_vault_changes_the_default_only_for_a_known_name() {
    let mut registry = Registry::default();
    registry.register("maya", Path::new("/maya")).unwrap();
    registry.register("ben", Path::new("/ben")).unwrap();
    registry.use_vault("ben").unwrap();
    assert_eq!(registry.default.as_deref(), Some("ben"));
    assert!(registry.use_vault("nobody").is_err());
}

// ---- hardening ----

use phv::PhvError;

fn issue_codes(err: &PhvError) -> Vec<String> {
    err.issues().iter().map(|i| i.code.clone()).collect()
}

#[test]
fn init_keeps_an_existing_gitignore_and_appends_the_phv_entry() {
    for (before, after) in [
        ("target/\n", "target/\n.phv/\n"),
        ("target/", "target/\n.phv/\n"),
        ("target/\n.phv/\n", "target/\n.phv/\n"),
        ("/.phv/\nfoo\n", "/.phv/\nfoo\n"),
        ("# .phv/\n", "# .phv/\n.phv/\n"),
        ("a\r\nb\r\n", "a\r\nb\r\n.phv/\n"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(".gitignore"), before).unwrap();
        Vault::init(&root).unwrap();
        assert_eq!(
            fs::read_to_string(root.join(".gitignore")).unwrap(),
            after,
            "from {before:?}"
        );
    }
}

#[test]
fn line_id_ignores_nested_and_escaped_ids() {
    let nested = r#"{"resourceType":"Patient","name":[{"family":"x","id":"evil"}],"id":"real"}"#;
    assert_eq!(line_id(nested), None);
    assert_eq!(
        line_id(r#"{"resourceType":"Patient","name":[{"id":"evil"}]}"#),
        None
    );
    assert_eq!(line_id(r#"{"resourceType":"Patient","id":"a\"b"}"#), None);
    assert_eq!(line_id(r#"{"resourceType":"Patient","id":"a\\b"}"#), None);
    assert_eq!(
        line_id(r#"{"resourceType":"Patient","id":"real","active":true}"#),
        Some("real")
    );
}

fn assert_usage_and_nothing_written(v: &Vault, changes: &[Change]) {
    let err = v.apply(changes).unwrap_err();
    assert!(matches!(err, PhvError::Usage(_)), "{err:?}");
    assert!(v.resource_types().unwrap().is_empty());
}

#[test]
fn apply_rejects_inconsistent_upserts_and_writes_nothing() {
    let (_d, v) = vault();
    // The line's ID differs from the change's ID.
    assert_usage_and_nothing_written(&v, &[upsert("x", A)]);
    // The line is a different resource type.
    assert_usage_and_nothing_written(&v, &[upsert("p", r#"{"resourceType":"Patient","id":"p"}"#)]);
    // The line holds a newline or carriage return.
    assert_usage_and_nothing_written(
        &v,
        &[upsert(
            "a",
            "{\"resourceType\":\"Observation\",\"id\":\"a\",\"v\":\"x\ny\"}",
        )],
    );
    assert_usage_and_nothing_written(
        &v,
        &[upsert(
            "a",
            "{\"resourceType\":\"Observation\",\"id\":\"a\",\"v\":\"x\ry\"}",
        )],
    );
    // One bad change stops the good one before it.
    assert_usage_and_nothing_written(&v, &[upsert("a", A), upsert("x", B)]);
}

#[test]
fn crlf_files_are_refused_and_left_alone() {
    let (_d, v) = vault();
    let path = v.root().join("records/Observation.ndjson");
    let crlf = format!("{A}\r\n{C}\r\n");
    fs::write(&path, &crlf).unwrap();
    let err = v.check_line_endings("Observation").unwrap_err();
    assert!(matches!(err, PhvError::Conflict(_)));
    assert_eq!(issue_codes(&err), ["PHV-NEWLINE"]);
    let err = v.apply(&[upsert("b", B)]).unwrap_err();
    assert!(matches!(err, PhvError::Conflict(_)));
    assert_eq!(issue_codes(&err), ["PHV-NEWLINE"]);
    assert_eq!(fs::read_to_string(&path).unwrap(), crlf);
}

#[test]
fn a_missing_final_newline_is_refused_and_left_alone() {
    let (_d, v) = vault();
    let path = v.root().join("records/Observation.ndjson");
    let no_newline = format!("{A}\n{C}");
    fs::write(&path, &no_newline).unwrap();
    let err = v.check_line_endings("Observation").unwrap_err();
    assert_eq!(issue_codes(&err), ["PHV-NEWLINE"]);
    assert!(v.apply(&[upsert("b", B)]).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), no_newline);
}

#[test]
fn line_endings_pass_for_good_empty_and_missing_files() {
    let (_d, v) = vault();
    assert!(v.check_line_endings("Observation").is_ok());
    v.apply(&[upsert("a", A)]).unwrap();
    assert!(v.check_line_endings("Observation").is_ok());
    fs::write(v.root().join("records/Empty.ndjson"), "").unwrap();
    assert!(v.check_line_endings("Empty").is_ok());
    assert!(v.check_line_endings("../x").is_err());
}

#[test]
fn records_holds_only_ndjson_files_after_writes() {
    let (_d, v) = vault();
    v.apply(&[upsert("a", A), upsert("b", B)]).unwrap();
    v.apply(&[upsert("a", A)]).unwrap();
    v.apply(&[Change::Remove {
        resource_type: "Observation".into(),
        id: "a".into(),
    }])
    .unwrap();
    let names: Vec<_> = fs::read_dir(v.root().join("records"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["Observation.ndjson"]);
    // Temporary files live under .phv/tmp and are gone after the rename.
    let tmp = v.root().join(".phv/tmp");
    assert!(tmp.is_dir());
    assert_eq!(fs::read_dir(tmp).unwrap().count(), 0);
}

fn repo_with_config(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".git/config"), config).unwrap();
    dir
}

fn remote(url: &str) -> String {
    format!("[remote \"origin\"]\n\turl = {url}\n")
}

#[test]
fn the_public_repo_is_refused_in_every_url_form() {
    for url in [
        "git@github.com:recodelabs/phv.git",
        "git@github.com:recodelabs/phv",
        "git@github.com:RecodeLabs/phv.git",
        "https://github.com/recodelabs/phv",
        "https://github.com/recodelabs/phv.git",
        "https://github.com/RecodeLabs/phv.git",
        "https://github.com/recodelabs/phv/",
        "https://github.com/recodelabs/phv.git/",
        "https://user:token@github.com/recodelabs/phv.git",
        "ssh://git@github.com/recodelabs/phv.git",
        "recodelabs/phv",
    ] {
        let dir = repo_with_config(&remote(url));
        assert!(
            Vault::init(&dir.path().join("inside")).is_err(),
            "{url} should be refused"
        );
    }
    let dir = repo_with_config("[remote \"origin\"]\n\turl=git@github.com:recodelabs/phv.git\n");
    assert!(Vault::init(&dir.path().join("inside")).is_err());
}

#[test]
fn the_public_repo_is_refused_when_the_url_is_quoted_or_has_a_comment() {
    for url in [
        "\"https://github.com/recodelabs/phv.git\"",
        "\"git@github.com:recodelabs/phv.git\" # the code",
        "https://github.com/recodelabs/phv.git # the code",
        "https://github.com/recodelabs/phv.git; the code",
        "https://github.com/recodelabs/phv.git#the-code",
        "  \"https://github.com/recodelabs/phv\"  ",
    ] {
        let dir = repo_with_config(&remote(url));
        assert!(
            Vault::init(&dir.path().join("inside")).is_err(),
            "{url} should be refused"
        );
    }
    // A comment does not turn a private repo into the public one, and a quoted `#` is data.
    for url in [
        "https://github.com/recodelabs/phv-data.git # recodelabs/phv",
        "\"https://github.com/recodelabs/phv-data.git\"",
    ] {
        let dir = repo_with_config(&remote(url));
        assert!(
            Vault::init(&dir.path().join("inside")).is_ok(),
            "{url} should be allowed"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_into_the_public_repo_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(
        repo.join(".git/config"),
        remote("git@github.com:recodelabs/phv.git"),
    )
    .unwrap();
    fs::create_dir_all(repo.join("sub")).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(repo.join("sub"), &link).unwrap();
    // The vault folder does not exist yet; only `link` does.
    assert!(Vault::init(&link.join("data")).is_err());
    assert!(!repo.join("sub/data").exists());
    // A link to a folder outside the repo is still allowed.
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let ok_link = dir.path().join("ok-link");
    std::os::unix::fs::symlink(&outside, &ok_link).unwrap();
    assert!(Vault::init(&ok_link.join("data")).is_ok());
}

#[test]
fn similar_repo_names_are_allowed() {
    for url in [
        "git@github.com:recodelabs/phv-data.git",
        "git@github.com:notrecodelabs/phv.git",
        "https://github.com/notrecodelabs/phv",
        "https://github.com/recodelabs/phv-vault",
        "https://github.com/recodelabs/phv/extra",
    ] {
        let dir = repo_with_config(&remote(url));
        assert!(
            Vault::init(&dir.path().join("inside")).is_ok(),
            "{url} should be allowed"
        );
    }
}

#[test]
fn a_worktree_of_the_public_repo_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
    fs::write(
        main.join(".git/config"),
        remote("git@github.com:recodelabs/phv.git"),
    )
    .unwrap();
    fs::write(main.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
    let wt = dir.path().join("wt");
    fs::create_dir_all(&wt).unwrap();
    // A relative gitdir resolves against the directory that holds the .git file.
    fs::write(wt.join(".git"), "gitdir: ../main/.git/worktrees/wt\n").unwrap();
    assert!(Vault::init(&wt.join("inside")).is_err());
    // An absolute gitdir works too.
    fs::write(
        wt.join(".git"),
        format!("gitdir: {}\n", main.join(".git/worktrees/wt").display()),
    )
    .unwrap();
    assert!(Vault::init(&wt.join("inside2")).is_err());
}

#[test]
fn a_submodule_style_git_file_is_checked_without_commondir() {
    let dir = tempfile::tempdir().unwrap();
    let gitdir = dir.path().join("modules/phv");
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(
        gitdir.join("config"),
        remote("https://github.com/recodelabs/phv.git"),
    )
    .unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join(".git"), "gitdir: ../modules/phv\n").unwrap();
    assert!(Vault::init(&sub.join("inside")).is_err());
}

#[test]
fn a_worktree_of_a_private_repo_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
    fs::write(
        main.join(".git/config"),
        remote("git@github.com:recodelabs/phv-data.git"),
    )
    .unwrap();
    fs::write(main.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
    let wt = dir.path().join("wt");
    fs::create_dir_all(&wt).unwrap();
    fs::write(wt.join(".git"), "gitdir: ../main/.git/worktrees/wt\n").unwrap();
    assert!(Vault::init(&wt.join("inside")).is_ok());
}

#[cfg(unix)]
mod unix_modes {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn new_folders_are_private_and_data_files_are_0600() {
        let (_d, v) = vault();
        let _lock = v.lock().unwrap();
        for sub in ["", "records", "sources", ".phv"] {
            assert_eq!(mode(&v.root().join(sub)), 0o700, "{sub:?}");
        }
        v.apply(&[upsert("a", A)]).unwrap();
        assert_eq!(mode(&v.root().join("records/Observation.ndjson")), 0o600);
        assert_eq!(mode(&v.root().join(".phv/tmp")), 0o700);
    }

    #[test]
    fn existing_folders_keep_their_mode() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        fs::create_dir_all(root.join("records")).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(root.join("records"), fs::Permissions::from_mode(0o750)).unwrap();
        Vault::init(&root).unwrap();
        assert_eq!(mode(&root), 0o755);
        assert_eq!(mode(&root.join("records")), 0o750);
        assert_eq!(mode(&root.join("sources")), 0o700);
    }
}

#[test]
fn resource_types_skips_names_that_are_not_valid_types() {
    let (_d, v) = vault();
    v.apply(&[upsert("a", A)]).unwrap();
    for name in ["a.b.ndjson", "bad name.ndjson", "x-y.ndjson", ".ndjson"] {
        fs::write(v.root().join("records").join(name), "").unwrap();
    }
    assert_eq!(v.resource_types().unwrap(), ["Observation"]);
}

#[test]
fn open_passes_through_io_errors_other_than_not_found() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("phv.toml")).unwrap();
    let err = Vault::open(dir.path()).unwrap_err();
    assert!(matches!(err, PhvError::Io(_)), "{err:?}");
    let empty = tempfile::tempdir().unwrap();
    let err = Vault::open(empty.path()).unwrap_err();
    assert!(err.to_string().contains("no phv.toml"), "{err}");
}

#[test]
fn an_empty_flag_or_env_counts_as_unset() {
    let config = "default = \"maya\"\n[vaults]\nmaya = \"/from/config\"\n";
    let r = |flag: Option<&str>, env: Option<&str>| {
        resolve_vault_path(flag.map(Path::new), env, Some(config)).unwrap()
    };
    assert_eq!(r(Some(""), Some("")), Path::new("/from/config"));
    assert_eq!(r(Some(""), Some("/e")), Path::new("/e"));
    assert_eq!(r(None, Some("")), Path::new("/from/config"));
    assert!(resolve_vault_path(Some(Path::new("")), Some(""), None).is_err());
}

#[test]
fn missing_vault_folders_are_recreated_on_write() {
    let (_d, v) = vault();
    fs::remove_dir_all(v.root().join("records")).unwrap();
    v.apply(&[upsert("a", A)]).unwrap();
    assert_eq!(v.resource_types().unwrap(), ["Observation"]);
    assert_eq!(file(&v), format!("{A}\n"));
}

#[test]
fn resource_types_is_empty_when_records_is_missing() {
    let (_d, v) = vault();
    fs::remove_dir_all(v.root().join("records")).unwrap();
    assert!(v.resource_types().unwrap().is_empty());
}

#[test]
fn write_atomic_creates_a_missing_parent_folder() {
    let (_d, v) = vault();
    fs::remove_dir_all(v.root().join("sources")).unwrap();
    let target = v.root().join("sources/note.txt");
    v.write_atomic(&target, b"hi").unwrap();
    assert_eq!(fs::read_to_string(target).unwrap(), "hi");
}
