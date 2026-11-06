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
