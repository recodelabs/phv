mod common;

use common::{add_patient, lab, phv, records};
use phv::PhvError;
use phv::write::AddOptions;

const PDF: &[u8] = b"%PDF-1.4 synthetic test bytes";

fn options(sha: &str) -> AddOptions {
    AddOptions {
        source: Some(sha.into()),
        agent: Some("phv-add-lab".into()),
        recorded_at: Some("2026-11-06T12:00:00Z".into()),
        dry_run: false,
    }
}

#[tokio::test]
async fn source_add_stores_the_file_and_a_document_reference() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let report = phv.source_add("Lab Report.PDF", PDF).await.unwrap();
    assert!(!report.already_imported);
    assert_eq!(report.sha256.len(), 64);
    let stored = phv
        .vault()
        .root()
        .join(format!("sources/{}.pdf", report.sha256));
    assert_eq!(std::fs::read(stored).unwrap(), PDF);

    let line = records(&phv, "DocumentReference");
    assert!(
        line.contains(&format!(r#""url":"sources/{}.pdf""#, report.sha256)),
        "{line}"
    );
    assert!(
        line.contains(r#""contentType":"application/pdf""#),
        "{line}"
    );
    assert!(line.contains(r#""status":"current""#), "{line}");
    assert!(line.contains(r#""system":"urn:phv:sha256""#), "{line}");
    assert!(line.contains(r#""reference":"Patient/"#), "{line}");
}

#[tokio::test]
async fn an_ndjson_source_gets_its_own_content_type() {
    let (_d, phv) = phv().await;
    phv.source_add("health-export.ndjson", b"{}\n")
        .await
        .unwrap();
    assert!(records(&phv, "DocumentReference").contains(r#""contentType":"application/x-ndjson""#));
}

#[tokio::test]
async fn adding_the_same_file_again_changes_nothing_and_says_so() {
    let (_d, phv) = phv().await;
    let first = phv.source_add("a.pdf", PDF).await.unwrap();
    let before = records(&phv, "DocumentReference");
    let second = phv.source_add("renamed.pdf", PDF).await.unwrap();
    assert!(second.already_imported);
    assert_eq!(second.document_reference, first.document_reference);
    assert_eq!(records(&phv, "DocumentReference"), before);
}

#[tokio::test]
async fn path_parts_in_a_file_name_cannot_escape_sources() {
    let (_d, phv) = phv().await;
    let report = phv.source_add("../../evil.../x.p/d..f", PDF).await.unwrap();
    assert!(
        phv.vault()
            .root()
            .join("sources")
            .read_dir()
            .unwrap()
            .count()
            == 1
    );
    assert!(!report.sha256.contains('/'));
    assert!(phv.check().await.unwrap().iter().all(|i| !i.is_error()));
}

#[tokio::test]
async fn add_with_source_links_new_resources_through_provenance() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let source = phv.source_add("a.pdf", PDF).await.unwrap();
    let report = phv
        .add(&lab(&patient, "1.50"), &options(&source.sha256))
        .await
        .unwrap();
    assert_eq!(report.created.len(), 2);
    let provenance = records(&phv, "Provenance");
    assert!(
        provenance.contains(r#""display":"phv-add-lab""#),
        "{provenance}"
    );
    assert!(
        provenance.contains(&format!(r#""reference":"{}""#, source.document_reference)),
        "{provenance}"
    );
    assert!(
        provenance.contains(r#""recorded":"2026-11-06T12:00:00Z""#),
        "{provenance}"
    );
    assert!(
        provenance.contains(r#""reference":"Observation/"#),
        "{provenance}"
    );
}

#[tokio::test]
async fn repeating_an_add_with_a_later_time_changes_nothing() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let source = phv.source_add("a.pdf", PDF).await.unwrap();
    phv.add(&lab(&patient, "1.50"), &options(&source.sha256))
        .await
        .unwrap();
    let before = records(&phv, "Provenance");

    let later = AddOptions {
        recorded_at: Some("2027-01-01T00:00:00Z".into()),
        ..options(&source.sha256)
    };
    let report = phv.add(&lab(&patient, "1.50"), &later).await.unwrap();
    assert!(report.created.is_empty());
    assert_eq!(records(&phv, "Provenance"), before);
}

#[tokio::test]
async fn an_unknown_source_is_refused() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let err = phv
        .add(&lab(&patient, "1.50"), &options(&"0".repeat(64)))
        .await
        .unwrap_err();
    assert!(matches!(err, PhvError::NotFound(_)), "{err:?}");
    assert_eq!(records(&phv, "Observation"), "");
}

fn stored_path(phv: &phv::write::Phv, sha: &str) -> std::path::PathBuf {
    phv.vault().root().join(format!("sources/{sha}.pdf"))
}

#[tokio::test]
async fn a_deleted_source_file_is_restored_on_re_add() {
    let (_d, phv) = phv().await;
    let first = phv.source_add("a.pdf", PDF).await.unwrap();
    assert!(!first.restored);
    let path = stored_path(&phv, &first.sha256);
    std::fs::remove_file(&path).unwrap();
    let before = records(&phv, "DocumentReference");

    let second = phv.source_add("a.pdf", PDF).await.unwrap();
    assert!(second.already_imported);
    assert!(second.restored);
    assert_eq!(std::fs::read(&path).unwrap(), PDF);
    assert_eq!(records(&phv, "DocumentReference"), before);
}

#[tokio::test]
async fn a_corrupted_source_file_is_restored_on_re_add() {
    let (_d, phv) = phv().await;
    let first = phv.source_add("a.pdf", PDF).await.unwrap();
    let path = stored_path(&phv, &first.sha256);
    std::fs::write(&path, b"damaged").unwrap();

    let second = phv.source_add("a.pdf", PDF).await.unwrap();
    assert!(second.already_imported && second.restored);
    assert_eq!(std::fs::read(&path).unwrap(), PDF);
}

#[tokio::test]
async fn an_intact_source_file_is_not_rewritten() {
    use std::os::unix::fs::MetadataExt;
    let (_d, phv) = phv().await;
    let first = phv.source_add("a.pdf", PDF).await.unwrap();
    let path = stored_path(&phv, &first.sha256);
    let before = std::fs::metadata(&path).unwrap();

    let second = phv.source_add("a.pdf", PDF).await.unwrap();
    assert!(second.already_imported);
    assert!(!second.restored);
    let after = std::fs::metadata(&path).unwrap();
    assert_eq!(before.ino(), after.ino());
    assert_eq!(before.mtime_nsec(), after.mtime_nsec());
}

#[tokio::test]
async fn a_stored_url_outside_sources_is_refused_not_followed() {
    let (_d, phv) = phv().await;
    let first = phv.source_add("a.pdf", PDF).await.unwrap();
    let file = phv.vault().root().join("records/DocumentReference.ndjson");
    let edited = std::fs::read_to_string(&file).unwrap().replace(
        &format!("sources/{}.pdf", first.sha256),
        "sources/../../evil",
    );
    std::fs::write(&file, edited).unwrap();

    let err = phv.source_add("a.pdf", PDF).await.unwrap_err();
    assert!(matches!(err, PhvError::Conflict(_)), "{err:?}");
    assert!(!phv.vault().root().join("../evil").exists());
    assert!(!phv.vault().root().join("evil").exists());

    let edited = std::fs::read_to_string(&file)
        .unwrap()
        .replace("sources/../../evil", "elsewhere/x.pdf");
    std::fs::write(&file, edited).unwrap();
    let err = phv.source_add("a.pdf", PDF).await.unwrap_err();
    assert!(matches!(err, PhvError::Conflict(_)), "{err:?}");
}

#[tokio::test]
async fn non_canonical_source_hashes_are_refused_with_usage() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let source = phv.source_add("a.pdf", PDF).await.unwrap();
    let sha = &source.sha256;
    let bad = [
        sha[..32].to_string(),
        format!("{}zzzz", &sha[..32]),
        format!("{}/../x", &sha[..32]),
        sha.to_uppercase(),
    ];
    for value in bad {
        let err = phv
            .add(&lab(&patient, "1.50"), &options(&value))
            .await
            .unwrap_err();
        assert!(matches!(err, PhvError::Usage(_)), "{value}: {err:?}");
    }
    assert_eq!(records(&phv, "Observation"), "");
    assert_eq!(records(&phv, "Provenance"), "");
}

#[tokio::test]
async fn a_source_whose_full_hash_differs_from_the_stored_identifier_is_not_found() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let source = phv.source_add("a.pdf", PDF).await.unwrap();
    // Same first 32 characters, different tail: a different file, not this document.
    let other = format!("{}{}", &source.sha256[..32], "0".repeat(32));
    assert_ne!(other, source.sha256);
    let err = phv
        .add(&lab(&patient, "1.50"), &options(&other))
        .await
        .unwrap_err();
    assert!(matches!(err, PhvError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn a_blank_agent_is_refused() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let source = phv.source_add("a.pdf", PDF).await.unwrap();
    for agent in ["", "   \t"] {
        let opts = AddOptions {
            agent: Some(agent.into()),
            ..options(&source.sha256)
        };
        let err = phv.add(&lab(&patient, "1.50"), &opts).await.unwrap_err();
        assert!(matches!(err, PhvError::Usage(_)), "{agent:?}: {err:?}");
    }
    assert_eq!(records(&phv, "Observation"), "");
}

fn title_of(line: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    value["content"][0]["attachment"]["title"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn a_windows_path_keeps_only_the_file_name_as_title() {
    let (_d, phv) = phv().await;
    let report = phv.source_add(r"C:\Users\me\scan.pdf", PDF).await.unwrap();
    let line = records(&phv, "DocumentReference");
    assert_eq!(title_of(&line), "scan.pdf");
    assert!(!line.contains("Users"), "{line}");
    assert!(stored_path(&phv, &report.sha256).exists());
}

#[tokio::test]
async fn a_very_long_name_is_capped_and_keeps_its_extension() {
    let (_d, phv) = phv().await;
    let report = phv
        .source_add(&format!("{}.pdf", "a".repeat(5000)), PDF)
        .await
        .unwrap();
    assert_eq!(
        title_of(&records(&phv, "DocumentReference"))
            .chars()
            .count(),
        255
    );
    assert!(stored_path(&phv, &report.sha256).exists());
}

#[tokio::test]
async fn a_multibyte_name_past_the_cap_does_not_panic() {
    let (_d, phv) = phv().await;
    phv.source_add(&format!("{}.pdf", "é".repeat(300)), PDF)
        .await
        .unwrap();
    let title = title_of(&records(&phv, "DocumentReference"));
    assert_eq!(title.chars().count(), 255);
    assert!(title.chars().all(|c| c == 'é'));
}

fn edit_document_reference(phv: &phv::write::Phv, from: &str, to: &str) {
    let file = phv.vault().root().join("records/DocumentReference.ndjson");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains(from), "{text}");
    std::fs::write(&file, text.replace(from, to)).unwrap();
}

#[tokio::test]
async fn a_url_pointing_at_another_sources_file_is_refused_and_that_file_survives() {
    let (_d, phv) = phv().await;
    let a = phv.source_add("a.pdf", PDF).await.unwrap();
    let other = b"%PDF-1.4 a different synthetic file";
    let b = phv.source_add("b.pdf", other).await.unwrap();
    edit_document_reference(
        &phv,
        &format!("sources/{}.pdf", a.sha256),
        &format!("sources/{}.pdf", b.sha256),
    );

    let err = phv.source_add("a.pdf", PDF).await.unwrap_err();
    match &err {
        PhvError::Conflict(issues) => assert_eq!(issues[0].code, "PHV-SOURCE-URL"),
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read(stored_path(&phv, &b.sha256)).unwrap(), other);
}

#[tokio::test]
async fn a_url_in_a_subfolder_of_sources_is_refused() {
    let (_d, phv) = phv().await;
    let a = phv.source_add("a.pdf", PDF).await.unwrap();
    edit_document_reference(
        &phv,
        &format!("sources/{}.pdf", a.sha256),
        &format!("sources/sub/{}.pdf", a.sha256),
    );
    let err = phv.source_add("a.pdf", PDF).await.unwrap_err();
    assert!(matches!(err, PhvError::Conflict(_)), "{err:?}");
    assert!(!phv.vault().root().join("sources/sub").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn a_url_through_a_symlinked_folder_writes_nothing_outside_the_vault() {
    let (_d, phv) = phv().await;
    let outside = tempfile::tempdir().unwrap();
    let a = phv.source_add("a.pdf", PDF).await.unwrap();
    std::os::unix::fs::symlink(outside.path(), phv.vault().root().join("sources/link")).unwrap();
    edit_document_reference(
        &phv,
        &format!("sources/{}.pdf", a.sha256),
        "sources/link/x.pdf",
    );
    std::fs::remove_file(stored_path(&phv, &a.sha256)).unwrap();

    let err = phv.source_add("a.pdf", PDF).await.unwrap_err();
    assert!(matches!(err, PhvError::Conflict(_)), "{err:?}");
    assert_eq!(outside.path().read_dir().unwrap().count(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_at_the_source_path_is_replaced_not_followed() {
    let (_d, phv) = phv().await;
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("secret.txt");
    std::fs::write(&target, b"outside data").unwrap();
    let a = phv.source_add("a.pdf", PDF).await.unwrap();
    let path = stored_path(&phv, &a.sha256);
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();

    let again = phv.source_add("a.pdf", PDF).await.unwrap();
    assert!(again.restored);
    assert_eq!(std::fs::read(&target).unwrap(), b"outside data");
    assert!(
        std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_file()
    );
    assert_eq!(std::fs::read(&path).unwrap(), PDF);
}

#[cfg(unix)]
#[tokio::test]
async fn a_fifo_at_the_source_path_is_replaced_without_hanging() {
    let (_d, phv) = phv().await;
    let a = phv.source_add("a.pdf", PDF).await.unwrap();
    let path = stored_path(&phv, &a.sha256);
    std::fs::remove_file(&path).unwrap();
    let status = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());

    let again = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        phv.source_add("a.pdf", PDF),
    )
    .await
    .expect("source_add hung on a FIFO")
    .unwrap();
    assert!(again.restored);
    assert!(
        std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_file()
    );
    assert_eq!(std::fs::read(&path).unwrap(), PDF);
}

#[tokio::test]
async fn an_unreadable_stored_source_record_points_at_check_not_source_add() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let a = phv.source_add("a.pdf", PDF).await.unwrap();
    edit_document_reference(&phv, r#""status":"current""#, r#""status":"#);

    let err = phv
        .add(&lab(&patient, "1.50"), &options(&a.sha256))
        .await
        .unwrap_err();
    match &err {
        PhvError::Conflict(issues) => {
            assert_eq!(issues[0].code, "PHV-SOURCE-URL");
            assert!(
                issues[0].message.contains("phv check"),
                "{}",
                issues[0].message
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(records(&phv, "Observation"), "");
}
