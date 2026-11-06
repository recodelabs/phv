mod browser_common;

use browser_common::get;
use browser_common::{app, fixture};
use phv::browser::model::explorer::topic_of;

#[test]
fn records_know_which_topic_shows_them() {
    let observations = fixture("Observation");
    let find = |id: &str| observations.iter().find(|o| o["id"] == id).unwrap().clone();
    let topic = |resource| topic_of(&resource, &observations).map(|(label, _)| label);
    assert_eq!(topic(find("vital-hr-2026")), Some("Vitals"));
    assert_eq!(topic(find("lab-glucose-2026")), Some("Labs"));
    assert_eq!(topic(find("obs-no-category")), None);
    let reports = fixture("DiagnosticReport");
    let report = |id: &str| reports.iter().find(|r| r["id"] == id).unwrap().clone();
    assert_eq!(
        topic_of(&report("report-2026"), &observations),
        Some(("Labs", "labs/reports/report-2026".to_string()))
    );
    assert_eq!(topic_of(&report("report-imaging"), &observations), None);
    assert_eq!(topic_of(&fixture("Encounter")[0], &observations), None);
}

#[tokio::test]
async fn a_record_page_names_its_topic_and_source() {
    let html = get(&app(), "/v/vault/fhir/Observation/lab-glucose-2026")
        .await
        .text();
    assert!(html.contains("Shown under"));
    assert!(html.contains(r#"href="/v/vault/labs""#));
    assert!(html.contains("metabolic-panel.pdf"));
    assert!(html.contains("Added by phv-add-lab"));
}

#[tokio::test]
async fn a_record_with_no_topic_or_source_leaves_them_out() {
    let html = get(&app(), "/v/vault/fhir/Encounter/encounter-1")
        .await
        .text();
    assert!(!html.contains("Shown under"));
    assert!(!html.contains("Added by"));
}

#[test]
fn a_record_entered_in_error_claims_no_topic() {
    let observations = fixture("Observation");
    let lab = serde_json::json!({
        "resourceType": "DiagnosticReport", "id": "r", "status": "entered-in-error",
        "category": [{"coding": [{"system": "http://terminology.hl7.org/CodeSystem/v2-0074", "code": "LAB"}]}]
    });
    assert_eq!(topic_of(&lab, &observations), None);
    let wrong = observations
        .iter()
        .find(|o| o["id"] == "vital-weight-error")
        .unwrap();
    assert_eq!(topic_of(wrong, &observations), None);
    let condition = fixture("Condition");
    let retracted = condition
        .iter()
        .find(|c| c["id"] == "condition-error")
        .unwrap();
    assert_eq!(topic_of(retracted, &observations), None);
}

#[tokio::test]
async fn a_source_link_uses_the_files_title_even_when_the_document_has_a_type() {
    let dir = tempfile::tempdir().unwrap();
    browser_common::copy_fixture(dir.path());
    let path = dir.path().join("records/DocumentReference.ndjson");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace(
            r#""status":"current","#,
            r#""status":"current","type":{"text":"Report"},"#,
        ),
    )
    .unwrap();
    let app = phv::browser::router(phv::browser::Vaults::single(dir.path().to_path_buf()));
    let html = get(&app, "/v/vault/fhir/Observation/lab-glucose-2026")
        .await
        .text();
    assert!(html.contains("metabolic-panel.pdf"), "{html}");
}
