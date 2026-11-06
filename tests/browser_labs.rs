mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, fixture, get, send};
use phv::browser::model::labs::{
    is_lab_report, lab_reports, report_detail, results_not_in_a_report, test_history,
};
use serde_json::json;

#[test]
fn a_report_is_a_lab_report_by_category_or_by_its_results() {
    let observations = fixture("Observation");
    let reports = fixture("DiagnosticReport");
    let find = |id: &str| reports.iter().find(|r| r["id"] == id).unwrap();
    assert!(
        is_lab_report(find("report-2025"), &observations),
        "LAB category"
    );
    assert!(
        is_lab_report(find("report-2026"), &observations),
        "laboratory results"
    );
    assert!(!is_lab_report(find("report-imaging"), &observations));
}

#[test]
fn reports_are_newest_first() {
    let rows = lab_reports(&fixture("DiagnosticReport"), &fixture("Observation"));
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["report-2026", "report-2025"]);
    assert_eq!(rows[0].result_count, 3);
}

#[test]
fn a_report_shows_results_as_stored_in_its_own_order() {
    let detail = report_detail(
        "report-2026",
        &fixture("DiagnosticReport"),
        &fixture("Observation"),
    )
    .unwrap();
    let names: Vec<&str> = detail.results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Glucose", "Creatinine", "CRP"]);
    assert_eq!(detail.results[1].lines, ["1.50 mg/dL"]);
    assert_eq!(detail.results[1].range.as_deref(), Some("0.70–1.30"));
    assert_eq!(detail.results[1].flag.as_deref(), Some("H"));
    assert_eq!(detail.results[0].range.as_deref(), Some("70 - 99 mg/dL"));
    assert_eq!(detail.results[0].flag, None);
    assert_eq!(detail.results[2].lines, ["<0.5 mg/L"]);
}

#[test]
fn a_missing_result_is_reported_not_hidden() {
    let reports = [json!({
        "resourceType": "DiagnosticReport", "id": "r",
        "category": [{"coding": [{"system": "http://terminology.hl7.org/CodeSystem/v2-0074", "code": "LAB"}]}],
        "result": [{"reference": "Observation/gone"}]
    })];
    let detail = report_detail("r", &reports, &[]).unwrap();
    assert!(detail.results.is_empty());
    assert_eq!(detail.missing, ["Observation/gone"]);
}

#[test]
fn an_imaging_report_is_not_a_lab_report() {
    assert_eq!(
        report_detail(
            "report-imaging",
            &fixture("DiagnosticReport"),
            &fixture("Observation")
        ),
        None
    );
}

#[test]
fn results_outside_every_report_are_listed_separately() {
    let rows = results_not_in_a_report(&fixture("DiagnosticReport"), &fixture("Observation"));
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["lab-vitamin-d-loose"]);
    assert_eq!(rows[0].lines, ["Sufficient"]);
}

#[test]
fn test_history_groups_by_loinc_across_different_names() {
    let rows = test_history(
        "loinc:2345-7",
        &fixture("DiagnosticReport"),
        &fixture("Observation"),
    );
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Glucose", "Blood sugar"]);
}

#[test]
fn test_history_without_loinc_needs_identical_text() {
    let reports = fixture("DiagnosticReport");
    let observations = fixture("Observation");
    assert_eq!(test_history("text:CRP", &reports, &observations).len(), 1);
    assert!(test_history("text:crp", &reports, &observations).is_empty());
}

#[tokio::test]
async fn the_page_lists_reports_and_loose_results_but_not_imaging() {
    let html = get(&app(), "/v/vault/labs").await.text();
    assert!(html.contains("Metabolic panel"));
    assert!(html.contains("Results not in a report"));
    assert!(html.contains("Vitamin D/25-OH"));
    assert!(!html.contains("Chest X-ray"));
}

#[tokio::test]
async fn a_report_page_links_its_original_file_and_fhir() {
    let html = get(&app(), "/v/vault/labs/reports/report-2026")
        .await
        .text();
    assert!(html.contains("1.50 mg/dL"));
    assert!(html.contains(
        "/v/vault/sources/1bb0ff96726fa961b697cbb7a3c9cf60d8b28c451054a3d044ba9095d4a0d412"
    ));
    assert!(html.contains("/v/vault/fhir/DiagnosticReport/report-2026"));
}

#[tokio::test]
async fn a_test_key_with_a_slash_round_trips_through_the_url() {
    let html = get(&app(), "/v/vault/labs").await.text();
    assert!(
        html.contains("/v/vault/labs/tests/text%3AVitamin%20D%2F25-OH"),
        "{html}"
    );
    let reply = get(&app(), "/v/vault/labs/tests/text%3AVitamin%20D%2F25-OH").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.text().contains("Sufficient"));
}

#[tokio::test]
async fn a_test_returns_a_partial_for_htmx() {
    let reply = send(
        &app(),
        "GET",
        "/v/vault/labs/tests/loinc%3A2345-7",
        &[("hx-request", "true")],
    )
    .await;
    assert!(!reply.text().contains("<html"));
    assert!(reply.text().contains("101 mg/dL"));
}

#[tokio::test]
async fn unknown_reports_and_tests_are_404() {
    let app = app();
    for uri in [
        "/v/vault/labs/reports/report-imaging",
        "/v/vault/labs/reports/nope",
        "/v/vault/labs/tests/nope",
    ] {
        assert_eq!(get(&app, uri).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
}
