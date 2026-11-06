mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, copy_fixture, fixture, get};
use phv::browser::model::activity::{
    Period, buckets_of_distance, buckets_of_steps, friendly_duration, step_days, summary,
    type_rows, workouts,
};
use phv::browser::{Vaults, router};
use serde_json::json;

fn period(year: Option<&str>, month: Option<&str>) -> Period {
    Period::parse(year, month).unwrap()
}

#[test]
fn periods_accept_only_real_years_and_months() {
    assert_eq!(period(None, None), Period::All);
    assert_eq!(period(Some("2026"), None).title(), "2026");
    assert_eq!(period(Some("2026"), Some("1")).title(), "Jan 2026");
    assert_eq!(period(Some("2026"), Some("1")).query(), "year=2026&month=1");
    assert_eq!(Period::parse(Some("abc"), None), None);
    assert_eq!(Period::parse(Some("2026"), Some("13")), None);
    assert_eq!(Period::parse(None, Some("1")), None);
    assert!(period(Some("2026"), Some("1")).contains("2026-01-31"));
    assert!(!period(Some("2026"), Some("1")).contains("2026-11-01"));
}

#[test]
fn steps_come_only_from_activity_step_records() {
    let data = step_days(&fixture("Observation"));
    assert_eq!(data.days.len(), 6);
    assert_eq!(data.days[0].date, "2025-12-30");
    assert_eq!(data.days[0].steps, 8000);
    assert_eq!(data.duplicate_days, 0);
    assert_eq!(data.unusable, 0);
}

#[test]
fn a_second_record_for_a_day_never_double_counts() {
    let step = |id: &str, steps: u64| {
        json!({
            "id": id, "status": "final",
            "category": [{"coding": [{"system": "http://terminology.hl7.org/CodeSystem/observation-category", "code": "activity"}]}],
            "code": {"coding": [{"system": "http://loinc.org", "code": "41950-7"}]},
            "effectiveDateTime": "2026-01-01",
            "valueQuantity": {"value": steps}
        })
    };
    let data = step_days(&[step("a", 100), step("b", 300)]);
    assert_eq!(data.days.len(), 1);
    assert_eq!(data.days[0].steps, 300);
    assert_eq!(data.duplicate_days, 1);
}

#[test]
fn records_that_cannot_be_counted_are_reported_not_guessed() {
    let base = |value: serde_json::Value| {
        let mut record = json!({
            "id": "a", "status": "final",
            "category": [{"coding": [{"system": "http://terminology.hl7.org/CodeSystem/observation-category", "code": "activity"}]}],
            "code": {"coding": [{"system": "http://loinc.org", "code": "41950-7"}]},
            "effectiveDateTime": "2026-01-01"
        });
        record["valueQuantity"] = value;
        record
    };
    let mut undated = base(json!({"value": 5}));
    undated.as_object_mut().unwrap().remove("effectiveDateTime");
    let data = step_days(&[undated, base(json!({"value": -5})), base(json!({}))]);
    assert!(data.days.is_empty());
    assert_eq!(data.unusable, 3);
}

#[test]
fn entered_in_error_steps_do_not_count() {
    let record = json!({
        "id": "a", "status": "entered-in-error",
        "category": [{"coding": [{"system": "http://terminology.hl7.org/CodeSystem/observation-category", "code": "activity"}]}],
        "code": {"coding": [{"system": "http://loinc.org", "code": "41950-7"}]},
        "effectiveDateTime": "2026-01-01", "valueQuantity": {"value": 5}
    });
    let data = step_days(&[record]);
    assert!(data.days.is_empty());
    assert_eq!(data.unusable, 0);
}

#[test]
fn all_years_are_continuous_and_a_gap_stays_a_gap() {
    let days = step_days(&fixture("Observation")).days;
    let years = buckets_of_steps(&days, &Period::All);
    let shown: Vec<(&str, Option<&str>)> = years
        .iter()
        .map(|b| (b.label.as_str(), b.value.as_deref()))
        .collect();
    assert_eq!(shown, [("2025", Some("17500")), ("2026", Some("29777"))]);
    assert_eq!(years[1].query.as_deref(), Some("year=2026"));
}

#[test]
fn a_year_shows_twelve_months_and_a_month_shows_every_day() {
    let days = step_days(&fixture("Observation")).days;
    let months = buckets_of_steps(&days, &period(Some("2026"), None));
    assert_eq!(months.len(), 12);
    assert_eq!(months[0].label, "Jan");
    assert_eq!(months[0].text, "22,000");
    assert_eq!(months[2].value, None, "no records is not zero");
    assert_eq!(months[2].text, "No data");

    let january = buckets_of_steps(&days, &period(Some("2026"), Some("1")));
    assert_eq!(january.len(), 31);
    assert_eq!(january[1].text, "6,000");
    assert_eq!(january[2].value, None, "the 3rd has no record");
    assert_eq!(january[0].query, None, "days are the last level");
}

#[test]
fn february_has_the_right_number_of_days() {
    let days = step_days(&fixture("Observation")).days;
    assert_eq!(
        buckets_of_steps(&days, &period(Some("2026"), Some("2"))).len(),
        28
    );
    assert_eq!(
        buckets_of_steps(&days, &period(Some("2024"), Some("2"))).len(),
        29
    );
}

#[test]
fn averages_count_only_recorded_days() {
    let days = step_days(&fixture("Observation")).days;
    let january = summary(&days, &period(Some("2026"), Some("1")));
    assert_eq!(january.total, "22,000");
    assert_eq!(january.average, "7,333");
    assert_eq!(january.days, "3");
}

#[test]
fn durations_read_naturally() {
    assert_eq!(friendly_duration(48.0), "48 min");
    assert_eq!(friendly_duration(72.0), "1 h 12 min");
    assert_eq!(friendly_duration(60.0), "1 h");
    assert_eq!(friendly_duration(0.4), "0 min");
}

#[test]
fn workouts_gather_their_measures_with_friendly_units() {
    let all = workouts(&fixture("Observation"));
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].activity, "Walking", "newest first");
    let ride = all.iter().find(|w| w.date == "4 Jan 2026").unwrap();
    assert_eq!(ride.activity, "Cycling");
    assert_eq!(ride.duration.as_deref(), Some("1 h 12 min"));
    assert_eq!(ride.distance.as_deref(), Some("24.60 km"));
    assert_eq!(ride.calories.as_deref(), Some("513 kcal"));
    let yoga = all.iter().find(|w| w.activity == "Yoga").unwrap();
    assert_eq!(yoga.distance, None);
    assert_eq!(yoga.calories, None);
}

#[test]
fn a_panel_with_a_missing_member_still_shows() {
    let panel = json!({
        "id": "p", "status": "final",
        "category": [{"coding": [{"system": "http://terminology.hl7.org/CodeSystem/observation-category", "code": "activity"}]}],
        "code": {"coding": [{"code": "PAPanel"}]},
        "effectivePeriod": {"start": "2026-01-01T08:00:00Z"},
        "hasMember": [{"reference": "Observation/gone"}]
    });
    let all = workouts(&[panel]);
    assert_eq!(all[0].activity, "Workout");
    assert_eq!(all[0].duration, None);
}

#[test]
fn distance_by_activity_is_largest_first_and_keeps_activities_without_distance() {
    let all = workouts(&fixture("Observation"));
    let rows = type_rows(&all, &Period::All);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Cycling", "Running", "Walking", "Yoga"]);
    assert_eq!(rows[0].count, 2);
    assert_eq!(rows[0].distance.as_deref(), Some("34.6 km"));
    assert_eq!(rows[0].chart_km.as_deref(), Some("34.6"));
    assert_eq!(rows[3].distance, None);
    let january = type_rows(&all, &period(Some("2026"), Some("1")));
    assert_eq!(january[0].distance.as_deref(), Some("24.6 km"));
    assert!(!january.iter().any(|r| r.name == "Walking"));
}

#[test]
fn one_activitys_distance_is_bucketed_over_time() {
    let all = workouts(&fixture("Observation"));
    let years = buckets_of_distance(&all, "Cycling", &Period::All);
    let shown: Vec<(&str, &str)> = years
        .iter()
        .map(|b| (b.label.as_str(), b.text.as_str()))
        .collect();
    assert_eq!(shown, [("2025", "10.0 km"), ("2026", "24.6 km")]);
}

#[tokio::test]
async fn the_page_works_as_tables_and_only_it_loads_the_charts() {
    let app = app();
    let html = get(&app, "/v/vault/activity").await.text();
    assert!(html.contains(r#"data-chart="steps""#));
    assert!(html.contains(r#"data-x="2026" data-y="29777""#), "{html}");
    assert!(html.contains("47,277"), "{html}");
    assert!(html.contains("/assets/plot.min.js"));
    assert!(html.contains("Distance by activity"));
    assert!(html.contains("recorded workouts"), "{html}");
    let other = get(&app, "/v/vault/vitals").await.text();
    assert!(!other.contains("plot.min.js"));
}

#[tokio::test]
async fn drilling_down_follows_the_period() {
    let app = app();
    let year = get(&app, "/v/vault/activity?year=2026").await.text();
    assert!(
        year.contains(r#"href="/v/vault/activity?year=2026&amp;month=1""#)
            || year.contains("year=2026&#38;month=1"),
        "{year}"
    );
    let month = get(&app, "/v/vault/activity?year=2026&month=1")
        .await
        .text();
    assert!(month.contains("Jan 2026"));
    assert!(month.contains("24.60 km"));
    assert!(
        !month.contains("3.92 km"),
        "February's walk is outside January"
    );
}

#[tokio::test]
async fn an_activity_filter_narrows_the_list_and_shows_its_distance_over_time() {
    let html = get(&app(), "/v/vault/activity?type=Cycling").await.text();
    assert!(html.contains(r#"data-chart="distance-time""#));
    assert!(html.contains("24.60 km"));
    assert!(!html.contains("5.20 km"));
}

#[tokio::test]
async fn a_bad_period_is_404() {
    let app = app();
    for uri in [
        "/v/vault/activity?year=abc",
        "/v/vault/activity?year=2026&month=13",
        "/v/vault/activity?month=1",
    ] {
        assert_eq!(get(&app, uri).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn an_empty_vault_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let vault = phv::store::Vault::init(&dir.path().join("v")).unwrap();
    let app = router(Vaults::single(vault.root().to_path_buf()));
    let html = get(&app, "/v/vault/activity").await.text();
    assert!(html.contains("No activity yet."), "{html}");
}

#[tokio::test]
async fn a_notice_explains_records_it_could_not_count() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    let path = dir.path().join("records/Observation.ndjson");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str(r#"{"resourceType":"Observation","id":"zz","status":"final","category":[{"coding":[{"code":"activity","system":"http://terminology.hl7.org/CodeSystem/observation-category"}]}],"code":{"coding":[{"code":"41950-7","system":"http://loinc.org"}]}}
"#);
    std::fs::write(&path, text).unwrap();
    let app = router(Vaults::single(dir.path().to_path_buf()));
    let html = get(&app, "/v/vault/activity").await.text();
    assert!(
        html.contains("1 step records have no usable date or count"),
        "{html}"
    );
}

#[tokio::test]
async fn the_overview_shows_the_latest_days_steps() {
    let html = get(&app(), "/v/vault/overview").await.text();
    assert!(html.contains("7,777"), "{html}");
    assert!(html.contains("Steps · 14 Feb 2026"), "{html}");
}

#[tokio::test]
async fn activity_records_name_their_topic() {
    let observations = fixture("Observation");
    let panel = observations
        .iter()
        .find(|o| o.to_string().contains("PAPanel"))
        .unwrap();
    let path = format!(
        "/v/vault/fhir/Observation/{}",
        panel["id"].as_str().unwrap()
    );
    let html = get(&app(), &path).await.text();
    assert!(html.contains(r#"href="/v/vault/activity""#), "{html}");
}
