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
