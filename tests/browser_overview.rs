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
        today: "2026-11-06",
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
    assert_eq!(view.age.as_deref(), Some("38 years"));
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

#[test]
fn age_reads_naturally_at_every_stage_of_life() {
    use phv::browser::model::overview::age_at;
    let cases = [
        ("2026-11-01", "2026-11-06", "5 days"),
        ("2026-11-05", "2026-11-06", "1 day"),
        ("2026-11-06", "2026-11-06", "Born today"),
        ("2026-10-06", "2026-11-06", "1 month"),
        ("2026-10-07", "2026-11-06", "30 days"),
        ("2025-08-06", "2026-11-06", "15 months"),
        ("2024-11-07", "2026-11-06", "23 months"),
        ("2024-11-06", "2026-11-06", "2 years"),
        ("2021-08-01", "2026-11-06", "5 years 3 months"),
        ("2021-10-06", "2026-11-06", "5 years 1 month"),
        ("2008-11-07", "2026-11-06", "17 years 11 months"),
        ("2008-11-06", "2026-11-06", "18 years"),
        ("1988-03-14", "2026-11-06", "38 years"),
        ("1988-11-07", "2026-11-06", "37 years"),
    ];
    for (born, today, expected) in cases {
        assert_eq!(age_at(born, today).as_deref(), Some(expected), "{born}");
    }
}

#[test]
fn a_leap_day_birthday_and_month_ends_count_correctly() {
    use phv::browser::model::overview::age_at;
    assert_eq!(
        age_at("2024-01-31", "2024-03-01").as_deref(),
        Some("1 month")
    );
    assert_eq!(
        age_at("2020-02-29", "2026-02-28").as_deref(),
        Some("5 years 11 months")
    );
    assert_eq!(
        age_at("2020-02-29", "2026-03-01").as_deref(),
        Some("6 years")
    );
    assert_eq!(
        age_at("2024-02-20", "2024-03-01").as_deref(),
        Some("10 days"),
        "a leap February"
    );
}

#[test]
fn a_partial_birth_date_gives_an_approximate_age() {
    use phv::browser::model::overview::age_at;
    assert_eq!(
        age_at("1988", "2026-11-06").as_deref(),
        Some("About 38 years")
    );
    assert_eq!(
        age_at("1988-03", "2026-11-06").as_deref(),
        Some("About 38 years")
    );
    assert_eq!(age_at("2026", "2026-11-06"), None, "too rough for a baby");
    assert_eq!(
        age_at("2021-08", "2026-11-06").as_deref(),
        Some("About 5 years")
    );
}

#[test]
fn a_birth_date_that_makes_no_sense_gives_no_age() {
    use phv::browser::model::overview::age_at;
    assert_eq!(age_at("2030-01-01", "2026-11-06"), None, "in the future");
    assert_eq!(age_at("soon", "2026-11-06"), None);
    assert_eq!(age_at("1988-13-01", "2026-11-06"), None);
}

#[test]
fn no_age_is_shown_for_someone_who_has_died() {
    use phv::browser::model::overview::age_of;
    use serde_json::json;
    let today = "2026-11-06";
    let living = json!({"birthDate": "1988-03-14"});
    assert_eq!(age_of(&living, today).as_deref(), Some("38 years"));
    let said_no = json!({"birthDate": "1988-03-14", "deceasedBoolean": false});
    assert_eq!(age_of(&said_no, today).as_deref(), Some("38 years"));
    for died in [
        json!({"birthDate": "1988-03-14", "deceasedBoolean": true}),
        json!({"birthDate": "1988-03-14", "deceasedDateTime": "2020-01-01"}),
    ] {
        assert_eq!(age_of(&died, today), None, "{died}");
    }
}

#[tokio::test]
async fn the_overview_shows_the_age_beside_the_birth_date() {
    let html = get(&app(), "/v/vault/overview").await.text();
    let line = html
        .lines()
        .find(|l| l.contains("Born 14 Mar 1988"))
        .unwrap();
    assert!(line.contains("Born 14 Mar 1988 · "), "{line}");
    assert!(line.contains(" years · "), "{line}");
}
