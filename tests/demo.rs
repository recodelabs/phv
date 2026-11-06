//! The fake data in `demo/` must keep importing cleanly, because the docs use it.

use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn phv(vault: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(["--vault", vault.to_str().unwrap()])
        .args(args)
        .output()
        .unwrap()
}

fn new_vault() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("data");
    let o = Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(["init", vault.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    (dir, vault)
}

fn assert_ok(o: &Output) {
    assert!(o.status.success(), "{o:?}");
}

fn line_count(path: PathBuf) -> usize {
    std::fs::read_to_string(path).unwrap().lines().count()
}

#[test]
fn the_demo_patient_imports_and_passes_check() {
    let (_d, vault) = new_vault();
    assert_ok(&phv(&vault, &["add", "demo/patient-fhir-bundle.json"]));
    assert_ok(&phv(&vault, &["check"]));
    assert_eq!(line_count(vault.join("records/Patient.ndjson")), 1);
    assert_eq!(line_count(vault.join("records/Observation.ndjson")), 7);
}

#[test]
fn the_demo_claim_files_store_as_documents() {
    let (_d, vault) = new_vault();
    let mut files: Vec<PathBuf> = std::fs::read_dir("demo/claims-835")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    assert_eq!(files.len(), 18);
    for file in &files {
        assert_ok(&phv(&vault, &["source", "add", file.to_str().unwrap()]));
    }
    assert_ok(&phv(&vault, &["check"]));
    assert_eq!(line_count(vault.join("records/DocumentReference.ndjson")), 18);
}

#[test]
fn the_demo_lab_draft_validates() {
    let (_d, vault) = new_vault();
    assert_ok(&phv(&vault, &["validate", "demo/lab-2025-02-10/draft.fhir.json"]));
}
