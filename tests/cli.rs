use serde_json::Value;
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn phv(vault: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(["--vault", vault.to_str().unwrap()])
        .args(args)
        .env_remove("PHV_VAULT")
        .output()
        .unwrap()
}

fn phv_stdin(vault: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(["--vault", vault.to_str().unwrap()])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // phv may exit before reading, for example on a bad flag, so a broken pipe is fine here.
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    child.wait_with_output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn new_vault() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("data");
    let o = Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(["init", vault.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    (dir, vault)
}

fn patient(vault: &Path) -> String {
    let o = phv_stdin(
        vault,
        &["add", "-", "--json"],
        r#"{"resourceType":"Patient","birthDate":"1980-01-01"}"#,
    );
    assert!(o.status.success(), "{o:?}");
    let report: Value = serde_json::from_str(&out(&o)).unwrap();
    report["created"][0].as_str().unwrap().to_string()
}

#[test]
fn a_missing_vault_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(phv(dir.path(), &["list", "Patient"]).status.code(), Some(2));
}

#[test]
fn malformed_json_exits_2() {
    let (_d, v) = new_vault();
    assert_eq!(
        phv_stdin(&v, &["add", "-"], "{not json").status.code(),
        Some(2)
    );
}

#[test]
fn duplicate_keys_exit_2_and_write_nothing() {
    let (_d, v) = new_vault();
    let input =
        r#"{"resourceType":"Patient","resourceType":"Observation","birthDate":"1980-01-01"}"#;
    assert_eq!(phv_stdin(&v, &["add", "-"], input).status.code(), Some(2));
    assert!(!v.join("records/Patient.ndjson").exists());
    assert!(!v.join("records/Observation.ndjson").exists());
}

#[test]
fn add_get_list_rm_work_end_to_end() {
    let (_d, v) = new_vault();
    let p = patient(&v);
    let lab = format!(
        r#"{{"resourceType":"Observation","status":"final","code":{{"text":"x"}},"subject":{{"reference":"{p}"}},"valueQuantity":{{"value":1.50}}}}"#
    );
    let o = phv_stdin(&v, &["add", "-", "--json"], &lab);
    assert!(o.status.success(), "{o:?}");
    let report: Value = serde_json::from_str(&out(&o)).unwrap();
    let reference = report["created"][0].as_str().unwrap().to_string();

    let got = phv(&v, &["get", &reference]);
    assert!(out(&got).contains(r#""value":1.50"#));

    let listed = phv(&v, &["list", "Observation", "--where", "status = 'final'"]);
    assert_eq!(out(&listed).lines().count(), 1);

    assert!(phv(&v, &["rm", &reference]).status.success());
    assert_eq!(phv(&v, &["get", &reference]).status.code(), Some(2));
}

#[test]
fn an_invalid_resource_exits_1_and_explains_why() {
    let (_d, v) = new_vault();
    let o = phv_stdin(
        &v,
        &["add", "-"],
        r#"{"resourceType":"Observation","code":{"text":"x"}}"#,
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("status"),
        "{o:?}"
    );
}

#[test]
fn validate_writes_nothing_and_uses_exit_codes() {
    let (_d, v) = new_vault();
    let p = patient(&v);
    let good = format!(
        r#"{{"resourceType":"Observation","status":"final","code":{{"text":"x"}},"subject":{{"reference":"{p}"}}}}"#
    );
    assert!(phv_stdin(&v, &["validate", "-"], &good).status.success());
    assert_eq!(
        phv_stdin(&v, &["validate", "-"], r#"{"resourceType":"Observation"}"#)
            .status
            .code(),
        Some(1)
    );
    assert!(!v.join("records/Observation.ndjson").exists());
}

#[test]
fn source_add_then_add_with_source_records_provenance() {
    let (d, v) = new_vault();
    let p = patient(&v);
    let file = d.path().join("lab.pdf");
    std::fs::write(&file, b"%PDF synthetic").unwrap();
    let o = phv(&v, &["source", "add", file.to_str().unwrap(), "--json"]);
    assert!(o.status.success(), "{o:?}");
    let report: Value = serde_json::from_str(&out(&o)).unwrap();
    let sha = report["sha256"].as_str().unwrap();

    let lab = format!(
        r#"{{"resourceType":"Observation","status":"final","code":{{"text":"x"}},"subject":{{"reference":"{p}"}}}}"#
    );
    let o = phv_stdin(&v, &["add", "-", "--source", sha, "--agent", "test"], &lab);
    assert!(o.status.success(), "{o:?}");
    assert!(v.join("records/Provenance.ndjson").exists());
}

#[test]
fn source_requires_agent() {
    let (_d, v) = new_vault();
    let o = phv_stdin(&v, &["add", "-", "--source", "abc"], "{}");
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn check_exits_0_when_healthy_and_1_with_problems() {
    let (_d, v) = new_vault();
    patient(&v);
    assert!(phv(&v, &["check"]).status.success());
    std::fs::write(v.join("records/Observation.ndjson"), "broken\n").unwrap();
    assert_eq!(phv(&v, &["check"]).status.code(), Some(1));
}

#[test]
fn a_filter_that_gets_no_answers_warns_on_stderr_only() {
    let (_d, v) = new_vault();
    patient(&v);
    let o = phv(
        &v,
        &["list", "Patient", "--where", "brithDate = @1980-01-01"],
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(out(&o), "");
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("no true or false answer"),
        "{o:?}"
    );
}

#[test]
fn import_still_works_alongside_the_other_commands() {
    let o = Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(["import", "--help"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
}

#[test]
fn a_bad_target_exits_2() {
    let (_d, v) = new_vault();
    assert_eq!(phv(&v, &["get", "no-slash"]).status.code(), Some(2));
}

#[test]
fn source_add_records_a_category_and_the_time_it_was_added() {
    let (d, v) = new_vault();
    let file = d.path().join("receipt.pdf");
    std::fs::write(&file, b"%PDF synthetic").unwrap();
    let o = phv(
        &v,
        &[
            "source",
            "add",
            file.to_str().unwrap(),
            "--category",
            "receipt",
        ],
    );
    assert!(o.status.success(), "{o:?}");
    let text = std::fs::read_to_string(v.join("records/DocumentReference.ndjson")).unwrap();
    assert!(text.contains(r#""code":"receipt""#), "{text}");
    assert!(text.contains(r#""date":"20"#), "{text}");
}

#[test]
fn source_add_refuses_an_unknown_category() {
    let (d, v) = new_vault();
    let file = d.path().join("receipt.pdf");
    std::fs::write(&file, b"%PDF synthetic").unwrap();
    let o = phv(
        &v,
        &[
            "source",
            "add",
            file.to_str().unwrap(),
            "--category",
            "nonsense",
        ],
    );
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert!(!v.join("records/DocumentReference.ndjson").exists());
}
