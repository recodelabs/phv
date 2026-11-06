mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, fixture, get};
use phv::browser::model::overview::{Sources, overview};

#[test]
fn the_overview_gathers_each_topic() {
    let patients = fixture("Patient");
    let observations = fixture("Observation");
    let reports = fixture("DiagnosticReport");
    let conditions = fixture("Condition");
    let statements = fixture("MedicationStatement");
    let requests = fixture("MedicationRequest");
    let medications = fixture("Medication");
    let allergies = fixture("AllergyIntolerance");
    let view = overview(&Sources {
        patients: &patients,
        observations: &observations,
        reports: &reports,
        conditions: &conditions,
        statements: &statements,
        requests: &requests,
        medications: &medications,
        allergies: &allergies,
    });
    assert_eq!(view.birth_date.as_deref(), Some("14 Mar 1988"));
    assert_eq!(view.measures[0].latest.lines, ["118/76 mmHg"]);
    assert_eq!(view.latest_report.unwrap().name, "Metabolic panel");
    assert_eq!(view.conditions.len(), 2);
    assert_eq!(view.medicines.len(), 1);
    assert_eq!(view.allergies.len(), 1);
}

#[tokio::test]
async fn the_page_shows_the_person_and_what_is_active() {
    let reply = get(&app(), "/v/vault/overview").await;
    assert_eq!(reply.status, StatusCode::OK);
    let html = reply.text();
    assert!(html.contains("Maya Example"));
    assert!(html.contains("Born 14 Mar 1988"));
    assert!(html.contains("118/76 mmHg"));
    assert!(html.contains("Metabolic panel"));
    assert!(html.contains("Levothyroxine 50 mcg"));
    assert!(html.contains("Status not recorded"));
    assert!(
        !html.contains("Sinusitis"),
        "resolved conditions are not active"
    );
    assert!(
        !html.contains("Wrong diagnosis"),
        "entered in error stays out"
    );
    assert!(html.contains("3 documents"));
}

#[tokio::test]
async fn a_vault_with_no_patient_falls_back_to_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let vault = phv::store::Vault::init(&dir.path().join("v")).unwrap();
    let app = phv::browser::router(phv::browser::Vaults::single(vault.root().to_path_buf()));
    let html = get(&app, "/v/vault/overview").await.text();
    assert!(html.contains("<h1>vault</h1>"));
    assert!(html.contains("No vitals yet."));
}

#[tokio::test]
async fn counts_match_the_other_pages_and_notice_unreadable_lines() {
    let dir = tempfile::tempdir().unwrap();
    browser_common::copy_fixture(dir.path());
    let path = dir.path().join("records/DocumentReference.ndjson");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("{not json\n");
    text.push_str("{\"resourceType\":\"DocumentReference\",\"id\":\"zz\",\"status\":\"entered-in-error\",\"content\":[]}\n");
    std::fs::write(&path, text).unwrap();
    let app = phv::browser::router(phv::browser::Vaults::single(dir.path().to_path_buf()));
    let html = get(&app, "/v/vault/overview").await.text();
    assert!(html.contains("3 documents"), "{html}");
    assert!(html.contains("1 records couldn"), "{html}");
}
