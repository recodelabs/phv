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
