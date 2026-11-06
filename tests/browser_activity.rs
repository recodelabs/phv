mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, copy_fixture, fixture, get};
use phv::browser::model::activity::{
    Period, Unit, buckets_of_distance, buckets_of_minutes, buckets_of_steps, filter_bar,
    friendly_duration, step_days, summary, type_rows, workouts,
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
    let all = workouts(&fixture("Observation"), Unit::Km);
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
    let all = workouts(&[panel], Unit::Km);
    assert_eq!(all[0].activity, "Workout");
    assert_eq!(all[0].duration, None);
}

#[test]
fn distance_by_activity_is_largest_first_and_keeps_activities_without_distance() {
    let all = workouts(&fixture("Observation"), Unit::Km);
    let rows = type_rows(&all, &Period::All, Unit::Km);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Cycling", "Running", "Walking", "Yoga"]);
    assert_eq!(rows[0].count, 2);
    assert_eq!(rows[0].distance.as_deref(), Some("34.6 km"));
    assert_eq!(rows[0].chart_distance.as_deref(), Some("34.6"));
    assert_eq!(rows[3].distance, None);
    let january = type_rows(&all, &period(Some("2026"), Some("1")), Unit::Km);
    assert_eq!(january[0].distance.as_deref(), Some("24.6 km"));
    assert!(!january.iter().any(|r| r.name == "Walking"));
}

#[test]
fn one_activitys_distance_is_bucketed_over_time() {
    let all = workouts(&fixture("Observation"), Unit::Km);
    let years = buckets_of_distance(&all, "Cycling", &Period::All, Unit::Km);
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

#[test]
fn distances_can_be_shown_in_miles() {
    let all = workouts(&fixture("Observation"), Unit::Mi);
    let ride = all.iter().find(|w| w.date == "4 Jan 2026").unwrap();
    assert_eq!(ride.distance.as_deref(), Some("15.29 mi"));
    let rows = type_rows(&all, &Period::All, Unit::Mi);
    assert_eq!(rows[0].distance.as_deref(), Some("21.5 mi"));
    assert_eq!(rows[0].chart_distance.as_deref(), Some("21.5"));
    let years = buckets_of_distance(&all, "Cycling", &Period::All, Unit::Mi);
    assert_eq!(years[1].text, "15.3 mi");
}

#[test]
fn units_only_accept_km_and_mi() {
    assert_eq!(Unit::parse("mi"), Some(Unit::Mi));
    assert_eq!(Unit::parse("km"), Some(Unit::Km));
    assert_eq!(Unit::parse("furlong"), None);
}

#[test]
fn an_activitys_filters_offer_only_periods_it_happened_in() {
    let observations = fixture("Observation");
    let days = step_days(&observations).days;
    let all = workouts(&observations, Unit::Km);
    let bar = filter_bar(&days, &all, &Period::All, Some("Walking"), Unit::Km);
    let year = |value: &str| bar.years.iter().find(|o| o.value == value).unwrap();
    assert!(year("2026").enabled, "the walk was in 2026");
    assert!(!year("2025").enabled, "no walks in 2025");
    assert_eq!(bar.years[1].value, "2026", "newest first");
    assert!(!bar.month_enabled, "pick a year first");
}

#[test]
fn months_show_which_have_data_and_activities_show_their_count() {
    let observations = fixture("Observation");
    let days = step_days(&observations).days;
    let all = workouts(&observations, Unit::Km);
    let period = period(Some("2026"), Some("1"));
    let bar = filter_bar(&days, &all, &period, None, Unit::Km);
    assert!(bar.month_enabled);
    let month = |value: &str| bar.months.iter().find(|o| o.value == value).unwrap();
    assert!(month("1").enabled && month("1").selected);
    assert!(month("2").enabled);
    assert!(!month("3").enabled, "no records in March");
    let labels: Vec<&str> = bar
        .activities
        .iter()
        .filter(|o| o.enabled)
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "All activities (3)",
            "Cycling (1)",
            "Running (1)",
            "Yoga (1)"
        ]
    );
    let walking = bar
        .activities
        .iter()
        .find(|o| o.value == "Walking")
        .unwrap();
    assert!(
        !walking.enabled,
        "listed, so the menu does not jump, but not in this month"
    );
}

#[test]
fn previous_and_next_step_between_periods_that_have_data() {
    let observations = fixture("Observation");
    let days = step_days(&observations).days;
    let all = workouts(&observations, Unit::Km);
    let january = filter_bar(
        &days,
        &all,
        &period(Some("2026"), Some("1")),
        None,
        Unit::Km,
    );
    let previous = january.previous.unwrap();
    assert_eq!(previous.label, "Dec 2025");
    assert_eq!(previous.query, "year=2025&month=12");
    let next = january.next.unwrap();
    assert_eq!(
        next.label, "Feb 2026",
        "skips nothing, since February has data"
    );
    let year = filter_bar(
        &days,
        &all,
        &period(Some("2026"), None),
        Some("Cycling"),
        Unit::Km,
    );
    assert_eq!(year.previous.unwrap().query, "year=2025&type=Cycling");
    assert_eq!(year.next, None);
    assert_eq!(
        filter_bar(&days, &all, &Period::All, None, Unit::Km).previous,
        None
    );
}

#[test]
fn an_activity_with_no_distance_can_chart_its_time() {
    let all = workouts(&fixture("Observation"), Unit::Km);
    let years = buckets_of_minutes(&all, "Yoga", &Period::All);
    let shown: Vec<(&str, Option<&str>, &str)> = years
        .iter()
        .map(|b| (b.label.as_str(), b.value.as_deref(), b.text.as_str()))
        .collect();
    assert_eq!(shown, [("2026", Some("40"), "40 min")]);
}

#[test]
fn drilling_into_an_activitys_chart_keeps_the_activity() {
    let all = workouts(&fixture("Observation"), Unit::Km);
    let years = buckets_of_distance(&all, "Cycling", &Period::All, Unit::Km);
    assert_eq!(years[1].query.as_deref(), Some("year=2026&type=Cycling"));
}

#[tokio::test]
async fn one_form_holds_every_filter_and_works_without_javascript() {
    let html = get(&app(), "/v/vault/activity?year=2026&type=Cycling")
        .await
        .text();
    assert!(
        html.contains(r#"<form class="filters" method="get""#),
        "{html}"
    );
    assert!(html.contains(r#"name="year""#));
    assert!(html.contains(r#"name="month""#));
    assert!(html.contains(r#"name="type""#));
    assert!(html.contains(r#"<option value="2026" selected>"#), "{html}");
    assert!(
        html.contains(r#"<option value="Cycling" selected>"#),
        "{html}"
    );
    assert!(
        html.contains(">Apply</button>"),
        "needed with JavaScript off"
    );
    assert!(html.contains(">Clear</a>"));
}

#[tokio::test]
async fn the_month_filter_waits_for_a_year() {
    let html = get(&app(), "/v/vault/activity").await.text();
    assert!(html.contains(r#"<select name="month" disabled>"#), "{html}");
    assert!(!html.contains(">Clear</a>"), "nothing to clear");
}

#[tokio::test]
async fn an_activity_filter_scopes_the_whole_page() {
    let html = get(&app(), "/v/vault/activity?type=Cycling").await.text();
    assert!(
        !html.contains(r#"data-chart="steps""#),
        "steps are not about cycling"
    );
    assert!(!html.contains(r#"data-chart="distance-type""#));
    assert!(html.contains("34.6 km"), "{html}");
    assert!(html.contains("1 h 42 min"), "{html}");
    assert!(html.contains(">2</b>"), "{html}");
}

#[tokio::test]
async fn an_activity_with_no_distance_charts_time_instead() {
    let html = get(&app(), "/v/vault/activity?type=Yoga").await.text();
    assert!(html.contains(r#"data-chart="activity-time""#), "{html}");
    assert!(!html.contains(r#"data-chart="distance-time""#));
}

#[tokio::test]
async fn a_month_can_chart_one_activity_by_day() {
    let html = get(&app(), "/v/vault/activity?year=2026&month=1&type=Cycling")
        .await
        .text();
    assert!(html.contains(r#"data-chart="distance-time""#), "{html}");
}

#[tokio::test]
async fn a_period_with_no_data_says_so_instead_of_listing_gaps() {
    let html = get(&app(), "/v/vault/activity?year=2026&month=6")
        .await
        .text();
    assert!(html.contains("No steps recorded in Jun 2026."), "{html}");
    assert!(!html.contains(r#"data-chart="steps""#));
    assert!(!html.contains("No data"), "{html}");
}

#[tokio::test]
async fn choosing_miles_is_remembered() {
    let app = app();
    let chosen = get(&app, "/v/vault/activity?unit=mi").await;
    assert!(chosen.text().contains("15.29 mi"), "{}", chosen.text());
    assert!(chosen.text().contains(r#"data-axis="Distance (mi)""#));
    let cookie = chosen.header("set-cookie").to_string();
    assert!(cookie.starts_with("phv_unit=mi"), "{cookie}");
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");

    let remembered = browser_common::send(
        &app,
        "GET",
        "/v/vault/activity",
        &[("cookie", "phv_unit=mi")],
    )
    .await;
    assert!(remembered.text().contains("15.29 mi"));
    assert_eq!(
        remembered.header("set-cookie"),
        "",
        "nothing new to remember"
    );

    let back = browser_common::send(
        &app,
        "GET",
        "/v/vault/activity?unit=km",
        &[("cookie", "phv_unit=mi")],
    )
    .await;
    assert!(back.text().contains("24.60 km"));
    assert!(back.header("set-cookie").starts_with("phv_unit=km"));
}

#[tokio::test]
async fn kilometers_are_the_default_and_a_bad_unit_is_ignored() {
    let app = app();
    assert!(
        get(&app, "/v/vault/activity")
            .await
            .text()
            .contains("24.60 km")
    );
    let bad = get(&app, "/v/vault/activity?unit=furlong").await;
    assert_eq!(bad.status, StatusCode::OK);
    assert!(bad.text().contains("24.60 km"));
    assert_eq!(bad.header("set-cookie"), "");
    let cookie = browser_common::send(
        &app,
        "GET",
        "/v/vault/activity",
        &[("cookie", "phv_unit=furlong")],
    )
    .await;
    assert!(cookie.text().contains("24.60 km"));
}

#[tokio::test]
async fn unit_links_keep_the_other_filters() {
    let html = get(&app(), "/v/vault/activity?year=2026&type=Cycling")
        .await
        .text();
    assert!(
        html.contains("year=2026&#38;type=Cycling&#38;unit=mi"),
        "{html}"
    );
}
