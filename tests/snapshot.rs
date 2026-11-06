mod common;
use common::{add_patient, lab, phv};
use phv::{snapshot::capture, store::Vault, write::AddOptions};
use std::{fs, time::Duration};

#[test]
fn pristine_snapshot_creates_nothing() {
    let d = tempfile::tempdir().unwrap();
    let vault = Vault::init(&d.path().join("vault")).unwrap();
    assert!(
        capture(&vault)
            .unwrap_err()
            .to_string()
            .contains("existing .phv/lock")
    );
    assert!(!vault.root().join(".phv/lock").exists());
}

#[tokio::test]
async fn snapshot_is_stable_read_only_and_changes_with_records() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let file = phv.vault().root().join("records/Patient.ndjson");
    let before = fs::read(&file).unwrap();
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    let first = capture(phv.vault()).unwrap();
    assert_eq!(first["id"], capture(phv.vault()).unwrap()["id"]);
    assert_eq!(before, fs::read(&file).unwrap());
    assert_eq!(modified, fs::metadata(&file).unwrap().modified().unwrap());
    assert!(
        !phv.vault()
            .root()
            .join(".phv/tmp")
            .read_dir()
            .unwrap()
            .any(|_| true)
    );
    phv.add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    let second = capture(phv.vault()).unwrap();
    assert_ne!(first["id"], second["id"]);
    assert_eq!(second["records"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn snapshot_waits_for_writer_and_then_reads_the_completed_write() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let held = phv.vault().lock().unwrap();
    let vault = phv.vault().clone();
    let worker = std::thread::spawn(move || capture(&vault));
    std::thread::sleep(Duration::from_millis(50));
    assert!(!worker.is_finished());
    let resource = serde_json::json!({"resourceType":"Observation","id":"synthetic","status":"final","code":{"text":"Synthetic"}});
    phv.vault()
        .apply(&[phv::store::Change::Upsert {
            resource_type: "Observation".into(),
            id: "synthetic".into(),
            line: phv::canon::canon_resource(&resource),
        }])
        .unwrap();
    drop(held);
    let snapshot = worker.join().unwrap().unwrap();
    assert_eq!(snapshot["records"].as_array().unwrap().len(), 2);
    assert!(
        snapshot["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["resource"]["id"] == "synthetic")
    );
}

#[tokio::test]
async fn snapshot_rejects_a_changed_or_missing_source() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    phv.source_add("source.txt", b"synthetic source")
        .await
        .unwrap();
    let snapshot = capture(phv.vault()).unwrap();
    let source = snapshot["sources"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap();
    let path = phv.vault().root().join(source);
    fs::write(&path, "changed").unwrap();
    assert!(
        capture(phv.vault())
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
    fs::remove_file(path).unwrap();
    assert!(capture(phv.vault()).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn snapshot_works_with_read_only_files_and_rejects_symlinked_records() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let lock = phv.vault().root().join(".phv/lock");
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(capture(phv.vault()).is_ok());
    let record = phv.vault().root().join("records/Patient.ndjson");
    let target = phv.vault().root().join("patient-copy");
    fs::rename(&record, &target).unwrap();
    symlink(&target, &record).unwrap();
    assert!(capture(phv.vault()).is_err());
}
