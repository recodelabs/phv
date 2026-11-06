mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, fixture, get, send};
use phv::browser::model::vitals::{history, measures, vital_rows};

#[test]
fn only_vital_signs_count() {
    let rows = vital_rows(&fixture("Observation"));
    assert!(rows.iter().any(|r| r.id == "vital-hr-2026"));
    assert!(!rows.iter().any(|r| r.id == "lab-glucose-2026"));
    assert!(!rows.iter().any(|r| r.id == "obs-no-category"));
}

#[test]
fn entered_in_error_is_left_out() {
    let rows = vital_rows(&fixture("Observation"));
    assert!(!rows.iter().any(|r| r.id == "vital-weight-error"));
}

#[test]
fn measures_show_the_newest_record_in_a_fixed_order() {
    let found = measures(&vital_rows(&fixture("Observation")));
    let names: Vec<&str> = found.iter().map(|m| m.latest.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Blood pressure",
            "Heart rate",
            "Body weight",
            "Height",
            "Temperature"
        ]
    );
    assert_eq!(found[0].latest.lines, ["118/76 mmHg"]);
    assert_eq!(found[0].count, 2);
    assert_eq!(found[2].latest.lines, ["71.2 kg"]);
}

#[test]
fn a_record_with_no_date_says_so() {
    let found = measures(&vital_rows(&fixture("Observation")));
    let temperature = found
        .iter()
        .find(|m| m.latest.name == "Temperature")
        .unwrap();
    assert_eq!(temperature.latest.date, "No date");
    assert_eq!(temperature.latest.lines, ["36.60 Cel"]);
}

#[test]
fn history_is_newest_first_and_keeps_written_numbers() {
    let rows = history(&vital_rows(&fixture("Observation")), "loinc:29463-7");
    let lines: Vec<&str> = rows.iter().map(|r| r.lines[0].as_str()).collect();
    assert_eq!(lines, ["71.2 kg", "72.50 kg"]);
}

#[tokio::test]
async fn the_page_lists_measures() {
    let reply = get(&app(), "/v/vault/vitals").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.text().contains("118/76 mmHg"));
    assert!(
        reply
            .text()
            .contains(r#"hx-get="/v/vault/vitals/loinc%3A29463-7""#)
    );
}

#[tokio::test]
async fn a_measure_returns_a_full_page_or_a_partial_for_htmx() {
    let app = app();
    let uri = "/v/vault/vitals/loinc%3A29463-7";
    let full = get(&app, uri).await.text();
    assert!(full.contains("<html"));
    assert!(full.contains("72.50 kg"));
    let partial = send(&app, "GET", uri, &[("hx-request", "true")])
        .await
        .text();
    assert!(!partial.contains("<html"));
    assert!(partial.contains("72.50 kg"));
}

#[tokio::test]
async fn an_unknown_measure_is_404() {
    let reply = get(&app(), "/v/vault/vitals/loinc%3Anope").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}
