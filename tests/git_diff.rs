mod common;

use common::{add_patient, lab, phv};
use phv::write::{AddOptions, Phv};
use std::{path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn init_repo(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
}

async fn vault_with_labs(count: usize) -> (tempfile::TempDir, Phv, String, Vec<String>) {
    let (dir, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let mut created = Vec::new();
    for i in 0..count {
        let report = phv
            .add(&lab(&patient, &format!("{i}.50")), &AddOptions::default())
            .await
            .unwrap();
        created.push(report.created[0].clone());
    }
    init_repo(phv.vault().root());
    (dir, phv, patient, created)
}

#[tokio::test]
async fn adding_a_resource_changes_exactly_one_line_in_one_file() {
    let (_d, phv, patient, _) = vault_with_labs(50).await;
    phv.add(&lab(&patient, "99.50"), &AddOptions::default())
        .await
        .unwrap();
    assert_eq!(
        git(phv.vault().root(), &["diff", "--numstat"]).trim(),
        "1\t0\trecords/Observation.ndjson"
    );
}

#[tokio::test]
async fn replacing_a_resource_changes_exactly_one_line() {
    let (_d, phv, patient, created) = vault_with_labs(50).await;
    let id = created[25].split_once('/').unwrap().1.to_string();
    phv.replace("Observation", &id, &lab(&patient, "7.75"), false)
        .await
        .unwrap();
    assert_eq!(
        git(phv.vault().root(), &["diff", "--numstat"]).trim(),
        "1\t1\trecords/Observation.ndjson"
    );
}

#[tokio::test]
async fn removing_a_resource_changes_exactly_one_line() {
    let (_d, phv, _patient, created) = vault_with_labs(50).await;
    let id = created[25].split_once('/').unwrap().1.to_string();
    phv.remove("Observation", &id).await.unwrap();
    assert_eq!(
        git(phv.vault().root(), &["diff", "--numstat"]).trim(),
        "0\t1\trecords/Observation.ndjson"
    );
}

#[tokio::test]
async fn repeating_an_add_leaves_git_clean() {
    let (_d, phv, patient, _) = vault_with_labs(5).await;
    phv.add(&lab(&patient, "0.50"), &AddOptions::default())
        .await
        .unwrap();
    assert_eq!(git(phv.vault().root(), &["status", "--short"]), "");
}

#[tokio::test]
async fn the_lock_file_stays_out_of_git() {
    let (_d, phv, _patient, _) = vault_with_labs(1).await;
    assert!(phv.vault().root().join(".phv/lock").exists());
    assert_eq!(git(phv.vault().root(), &["status", "--short"]), "");
}

#[tokio::test]
async fn stored_files_pass_a_plain_sort_check() {
    let (_d, phv, _patient, _) = vault_with_labs(50).await;
    let file = phv.vault().root().join("records/Observation.ndjson");
    let status = Command::new("sort")
        .env("LC_ALL", "C")
        .arg("-c")
        .arg(file)
        .status()
        .unwrap();
    assert!(status.success());
}
