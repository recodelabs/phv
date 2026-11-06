//! `phv import apple-activity`, run as a person would run it. All test data is synthetic.

use phv::store::Vault;
use phv::write::{AddOptions, Phv};
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output};

const SUMMARY: &str = r#"{"counts":{"dailySteps":1,"workouts":0},"failures":[],"kind":"summary","through":"2026-01-31","timeZone":"Etc/UTC","v":1}"#;
const DAY: &str =
    r#"{"date":"2026-01-01","kind":"dailySteps","steps":5000,"timeZone":"Etc/UTC","v":1}"#;

async fn vault_with_patient(root: &Path) {
    let phv = Phv::new(Vault::init(root).unwrap()).await.unwrap();
    phv.add(
        &json!({"resourceType":"Patient","name":[{"family":"Example"}]}),
        &AddOptions::default(),
    )
    .await
    .unwrap();
}

fn phv_import(vault: &Path, file: &Path, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_phv"))
        .arg("--vault")
        .arg(vault)
        .args(["import", "apple-activity"])
        .arg(file)
        .args(flags)
        .env_remove("PHV_VAULT")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[tokio::test]
async fn imports_then_reports_nothing_new() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    vault_with_patient(&vault).await;
    let file = dir.path().join("activity.ndjson");
    std::fs::write(&file, format!("{DAY}\n{SUMMARY}\n")).unwrap();

    let dry = phv_import(&vault, &file, &["--dry-run"]);
    assert_eq!(dry.status.code(), Some(0), "{dry:?}");
    assert!(stdout(&dry).contains("would write 1"), "{}", stdout(&dry));
    assert!(!vault.join("records/Observation.ndjson").exists());

    let first = phv_import(&vault, &file, &[]);
    assert_eq!(first.status.code(), Some(0), "{first:?}");
    assert!(stdout(&first).contains("1 new"), "{}", stdout(&first));
    assert!(vault.join("records/Observation.ndjson").exists());

    let second = phv_import(&vault, &file, &["--json"]);
    assert_eq!(second.status.code(), Some(0), "{second:?}");
    let report: Value = serde_json::from_str(&stdout(&second)).unwrap();
    assert_eq!(report["daily_steps"]["unchanged"], 1);
    assert_eq!(report["resources_written"], 0);
}

#[tokio::test]
async fn a_cut_off_file_exits_2_and_a_rejected_row_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    vault_with_patient(&vault).await;
    let file = dir.path().join("activity.ndjson");

    std::fs::write(&file, format!("{DAY}\n")).unwrap();
    let cut_off = phv_import(&vault, &file, &[]);
    assert_eq!(cut_off.status.code(), Some(2), "{cut_off:?}");
    assert!(String::from_utf8_lossy(&cut_off.stderr).contains("cut off"));

    let bad = r#"{"date":"2026-01-01","kind":"dailySteps","steps":-1,"timeZone":"Etc/UTC","v":1}"#;
    std::fs::write(&file, format!("{bad}\n{SUMMARY}\n")).unwrap();
    let rejected = phv_import(&vault, &file, &[]);
    assert_eq!(rejected.status.code(), Some(1), "{rejected:?}");
    assert!(
        stdout(&rejected).contains("2026-01-01"),
        "{}",
        stdout(&rejected)
    );
}

#[tokio::test]
async fn a_missing_file_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    vault_with_patient(&vault).await;
    let missing = phv_import(&vault, &dir.path().join("nope.ndjson"), &[]);
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
}
