mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, copy_fixture, fixture, get};
use phv::browser::model::activity::{
    Level, Period, Unit, buckets_of_distance, buckets_of_minutes, buckets_of_steps,
    friendly_duration, step_calendar, step_days, steps_filters, summary, type_rows,
    workout_calendar, workouts, workouts_filters,
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
    let years = buckets_of_steps(&days, &Period::All, Level::Year);
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
    let months = buckets_of_steps(&days, &period(Some("2026"), None), Level::Month);
    assert_eq!(months.len(), 12);
    assert_eq!(months[0].label, "Jan");
    assert_eq!(months[0].text, "22,000");
    assert_eq!(months[2].value, None, "no records is not zero");
    assert_eq!(months[2].text, "No data");

    let january = buckets_of_steps(&days, &period(Some("2026"), Some("1")), Level::Day);
    assert_eq!(january.len(), 31);
    assert_eq!(january[1].text, "6,000");
    assert_eq!(january[2].value, None, "the 3rd has no record");
    assert_eq!(january[0].query, None, "days are the last level");
}

#[test]
fn february_has_the_right_number_of_days() {
    let days = step_days(&fixture("Observation")).days;
    assert_eq!(
        buckets_of_steps(&days, &period(Some("2026"), Some("2")), Level::Day).len(),
        28
    );
    assert_eq!(
        buckets_of_steps(&days, &period(Some("2024"), Some("2")), Level::Day).len(),
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

#[test]
fn every_period_can_be_shown_at_any_finer_level() {
    assert_eq!(Period::All.default_level(), Level::Year);
    assert_eq!(period(Some("2026"), None).default_level(), Level::Month);
    assert_eq!(period(Some("2026"), Some("1")).default_level(), Level::Day);
    assert!(Period::All.allows(Level::Day));
    assert!(period(Some("2026"), None).allows(Level::Day));
    assert!(!period(Some("2026"), None).allows(Level::Year));
    assert!(!period(Some("2026"), Some("1")).allows(Level::Month));
}

#[test]
fn all_years_can_show_every_month_or_every_day_between_first_and_last() {
    let days = step_days(&fixture("Observation")).days;
    let months = buckets_of_steps(&days, &Period::All, Level::Month);
    let labels: Vec<&str> = months.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Dec 2025", "Jan 2026", "Feb 2026"]);
    assert_eq!(months[0].query.as_deref(), Some("year=2025&month=12"));

    let every_day = buckets_of_steps(&days, &Period::All, Level::Day);
    assert_eq!(every_day.len(), 47, "30 Dec 2025 through 14 Feb 2026");
    assert_eq!(every_day[0].label, "30 Dec 2025");
    assert_eq!(every_day[0].text, "8,000");
    assert_eq!(every_day[4].value, None, "3 Jan has no record");
    assert_eq!(every_day[46].text, "7,777");
}

#[test]
fn a_year_can_show_every_day() {
    let days = step_days(&fixture("Observation")).days;
    let shown = buckets_of_steps(&days, &period(Some("2026"), None), Level::Day);
    assert_eq!(shown.len(), 365);
    assert_eq!(shown[0].label, "1 Jan 2026");
    assert_eq!(shown[0].text, "4,000");
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
    let all = workouts(&fixture("Observation"), Unit::Km);
    let bar = workouts_filters(&all, &Period::All, Some("Walking"), Unit::Km);
    let year = |value: &str| bar.period.years.iter().find(|o| o.value == value).unwrap();
    assert!(year("2026").enabled, "the walk was in 2026");
    assert!(!year("2025").enabled, "no walks in 2025");
    assert_eq!(bar.period.years[1].value, "2026", "newest first");
    assert!(!bar.period.month_enabled, "pick a year first");
}

#[test]
fn workout_months_show_which_have_data_and_activities_show_their_count() {
    let all = workouts(&fixture("Observation"), Unit::Km);
    let bar = workouts_filters(&all, &period(Some("2026"), Some("1")), None, Unit::Km);
    assert!(bar.period.month_enabled);
    let month = |value: &str| bar.period.months.iter().find(|o| o.value == value).unwrap();
    assert!(month("1").enabled && month("1").selected);
    assert!(month("2").enabled);
    assert!(!month("3").enabled, "no workouts in March");
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
fn workouts_step_between_periods_that_have_workouts() {
    let all = workouts(&fixture("Observation"), Unit::Km);
    let january = workouts_filters(&all, &period(Some("2026"), Some("1")), None, Unit::Km);
    let previous = january.period.previous.unwrap();
    assert_eq!(previous.label, "Dec 2025");
    assert_eq!(previous.query, "year=2025&month=12");
    assert_eq!(january.period.next.unwrap().label, "Feb 2026");
    let year = workouts_filters(&all, &period(Some("2026"), None), Some("Cycling"), Unit::Km);
    assert_eq!(
        year.period.previous.unwrap().query,
        "year=2025&type=Cycling"
    );
    assert_eq!(year.period.next, None);
    let everything = workouts_filters(&all, &Period::All, None, Unit::Km);
    assert_eq!(everything.period.previous, None);
}

#[test]
fn steps_filters_use_step_days_and_keep_the_chosen_level() {
    let days = step_days(&fixture("Observation")).days;
    let bar = steps_filters(&days, &period(Some("2026"), None), Level::Day);
    let previous = bar.period.previous.unwrap();
    assert_eq!(previous.label, "2025");
    assert_eq!(previous.query, "year=2025&by=day");
    let levels: Vec<(&str, bool, bool)> = bar
        .levels
        .iter()
        .map(|l| (l.label, l.enabled, l.on))
        .collect();
    assert_eq!(
        levels,
        [
            ("Year", false, false),
            ("Month", true, false),
            ("Day", true, true)
        ]
    );
    assert_eq!(
        bar.levels[1].query, "year=2026",
        "the natural level needs no parameter"
    );
    assert_eq!(bar.levels[2].query, "year=2026&by=day");
    let natural = steps_filters(&days, &period(Some("2026"), None), Level::Month);
    assert_eq!(natural.period.previous.unwrap().query, "year=2025");
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

const STEPS: &str = "/v/vault/activity";
const WORKOUTS: &str = "/v/vault/activity/workouts";

#[tokio::test]
async fn steps_and_workouts_are_separate_tabs() {
    let app = app();
    let steps = get(&app, STEPS).await.text();
    assert!(steps.contains(r#"data-chart="steps""#));
    assert!(steps.contains(r#"data-x="2026" data-y="29777""#), "{steps}");
    assert!(steps.contains("47,277"));
    assert!(!steps.contains("Distance by activity"));
    assert!(!steps.contains("Distance units"), "steps have no distance");
    assert!(!steps.contains(r#"name="type""#));

    let workouts = get(&app, WORKOUTS).await.text();
    assert!(workouts.contains("Distance by activity"));
    assert!(workouts.contains("recorded workouts"));
    assert!(!workouts.contains(r#"data-chart="steps""#));
    assert!(!workouts.contains("47,277"));
    assert!(workouts.contains("Distance units"));
}

#[tokio::test]
async fn the_active_tab_is_marked_and_switching_keeps_the_period() {
    let app = app();
    let steps = get(&app, "/v/vault/activity?year=2026&month=1")
        .await
        .text();
    assert!(
        steps.contains(
            r#"<a href="/v/vault/activity?year=2026&#38;month=1" aria-current="page">Steps</a>"#
        ),
        "{steps}"
    );
    assert!(
        steps
            .contains(r#"<a href="/v/vault/activity/workouts?year=2026&#38;month=1">Workouts</a>"#),
        "{steps}"
    );
    let workouts = get(&app, "/v/vault/activity/workouts?year=2026&type=Cycling")
        .await
        .text();
    assert!(
        workouts.contains(r#"<a href="/v/vault/activity?year=2026">Steps</a>"#),
        "{workouts}"
    );
    assert!(
        workouts.contains(r#"aria-current="page">Workouts</a>"#),
        "{workouts}"
    );
}

#[tokio::test]
async fn only_activity_pages_load_the_charts() {
    let app = app();
    assert!(
        get(&app, STEPS)
            .await
            .text()
            .contains("/assets/plot.min.js")
    );
    assert!(
        get(&app, WORKOUTS)
            .await
            .text()
            .contains("/assets/plot.min.js")
    );
    assert!(
        !get(&app, "/v/vault/vitals")
            .await
            .text()
            .contains("plot.min.js")
    );
}

#[tokio::test]
async fn steps_can_be_shown_by_year_month_or_day() {
    let app = app();
    let by_month = get(&app, "/v/vault/activity?by=month").await.text();
    assert!(
        by_month.contains(r#"data-x="Jan 2026" data-y="22000""#),
        "{by_month}"
    );
    assert!(
        by_month.contains(r#"href="/v/vault/activity?by=day""#),
        "{by_month}"
    );
    let by_day = get(&app, "/v/vault/activity?year=2026&by=day").await.text();
    assert!(
        by_day.contains(r#"data-x="14 Feb 2026" data-y="7777""#),
        "{by_day}"
    );
    assert!(
        by_day.contains(r#"aria-current="true">Day</a>"#),
        "{by_day}"
    );
}

#[tokio::test]
async fn a_level_that_does_not_fit_the_period_falls_back_to_its_natural_one() {
    let app = app();
    for uri in [
        "/v/vault/activity?year=2026&by=year",
        "/v/vault/activity?year=2026&by=nonsense",
    ] {
        let reply = get(&app, uri).await;
        assert_eq!(reply.status, StatusCode::OK, "{uri}");
        assert!(
            reply.text().contains(r#"data-x="Jan" data-y="22000""#),
            "{uri}"
        );
    }
}

#[tokio::test]
async fn drilling_into_a_year_follows_the_period() {
    let app = app();
    let year = get(&app, "/v/vault/activity?year=2026").await.text();
    assert!(year.contains("year=2026&#38;month=1"), "{year}");
    let month = get(&app, "/v/vault/activity/workouts?year=2026&month=1")
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
    let html = get(&app(), "/v/vault/activity/workouts?type=Cycling")
        .await
        .text();
    assert!(html.contains(r#"data-chart="distance-time""#));
    assert!(html.contains("24.60 km"));
    assert!(!html.contains("5.20 km"));
}

#[tokio::test]
async fn a_bad_period_is_404_on_both_tabs() {
    let app = app();
    for path in [STEPS, WORKOUTS] {
        for query in ["?year=abc", "?year=2026&month=13", "?month=1"] {
            let uri = format!("{path}{query}");
            assert_eq!(get(&app, &uri).await.status, StatusCode::NOT_FOUND, "{uri}");
        }
    }
}

#[tokio::test]
async fn an_empty_vault_says_so_on_both_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let vault = phv::store::Vault::init(&dir.path().join("v")).unwrap();
    let app = router(Vaults::single(vault.root().to_path_buf()));
    assert!(get(&app, STEPS).await.text().contains("No steps yet."));
    assert!(
        get(&app, WORKOUTS)
            .await
            .text()
            .contains("No workouts yet.")
    );
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
    let html = get(&app, STEPS).await.text();
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
async fn activity_records_name_the_right_tab() {
    let observations = fixture("Observation");
    let id_of = |needle: &str| {
        observations
            .iter()
            .find(|o| o.to_string().contains(needle))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let app = app();
    let panel = get(
        &app,
        &format!("/v/vault/fhir/Observation/{}", id_of("PAPanel")),
    )
    .await
    .text();
    assert!(
        panel.contains(r#"href="/v/vault/activity/workouts""#),
        "{panel}"
    );
    let steps = get(
        &app,
        &format!("/v/vault/fhir/Observation/{}", id_of("41950-7")),
    )
    .await
    .text();
    assert!(steps.contains(r#"href="/v/vault/activity""#), "{steps}");
}

#[tokio::test]
async fn each_tab_has_its_own_filter_form_that_works_without_javascript() {
    let app = app();
    let html = get(&app, "/v/vault/activity/workouts?year=2026&type=Cycling")
        .await
        .text();
    assert!(
        html.contains(r#"<form class="filters" method="get" action="/v/vault/activity/workouts""#),
        "{html}"
    );
    assert!(
        html.contains(r#"name="year""#)
            && html.contains(r#"name="month""#)
            && html.contains(r#"name="type""#)
    );
    assert!(html.contains(r#"<option value="2026" selected>"#), "{html}");
    assert!(
        html.contains(r#"<option value="Cycling" selected>"#),
        "{html}"
    );
    assert!(
        html.contains(">Apply</button>"),
        "needed with JavaScript off"
    );
    assert!(
        html.contains(r#"<a class="clear" href="/v/vault/activity/workouts">Clear</a>"#),
        "{html}"
    );
    let steps = get(&app, "/v/vault/activity?year=2026").await.text();
    assert!(
        steps.contains(r#"<form class="filters" method="get" action="/v/vault/activity""#),
        "{steps}"
    );
}

#[tokio::test]
async fn the_month_filter_waits_for_a_year() {
    let app = app();
    for path in [STEPS, WORKOUTS] {
        let html = get(&app, path).await.text();
        assert!(html.contains(r#"<select name="month" disabled>"#), "{html}");
        assert!(!html.contains(">Clear</a>"), "nothing to clear");
    }
}

#[tokio::test]
async fn choosing_an_activity_focuses_the_workouts_tab() {
    let html = get(&app(), "/v/vault/activity/workouts?type=Cycling")
        .await
        .text();
    assert!(!html.contains(r#"data-chart="distance-type""#));
    assert!(html.contains("34.6 km"), "{html}");
    assert!(html.contains("1 h 42 min"), "{html}");
    assert!(html.contains(">2</b>"), "{html}");
}

#[tokio::test]
async fn an_activity_with_no_distance_charts_time_instead() {
    let html = get(&app(), "/v/vault/activity/workouts?type=Yoga")
        .await
        .text();
    assert!(html.contains(r#"data-chart="activity-time""#), "{html}");
    assert!(!html.contains(r#"data-chart="distance-time""#));
}

#[tokio::test]
async fn a_month_can_chart_one_activity_by_day() {
    let html = get(
        &app(),
        "/v/vault/activity/workouts?year=2026&month=1&type=Cycling",
    )
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
    let chosen = get(&app, "/v/vault/activity/workouts?unit=mi").await;
    assert!(chosen.text().contains("15.29 mi"), "{}", chosen.text());
    assert!(chosen.text().contains(r#"data-axis="Distance (mi)""#));
    let cookie = chosen.header("set-cookie").to_string();
    assert!(cookie.starts_with("phv_unit=mi"), "{cookie}");
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");

    let remembered =
        browser_common::send(&app, "GET", WORKOUTS, &[("cookie", "phv_unit=mi")]).await;
    assert!(remembered.text().contains("15.29 mi"));
    assert_eq!(
        remembered.header("set-cookie"),
        "",
        "nothing new to remember"
    );

    let back = browser_common::send(
        &app,
        "GET",
        "/v/vault/activity/workouts?unit=km",
        &[("cookie", "phv_unit=mi")],
    )
    .await;
    assert!(back.text().contains("24.60 km"));
    assert!(back.header("set-cookie").starts_with("phv_unit=km"));
}

#[tokio::test]
async fn kilometers_are_the_default_and_a_bad_unit_is_ignored() {
    let app = app();
    assert!(get(&app, WORKOUTS).await.text().contains("24.60 km"));
    let bad = get(&app, "/v/vault/activity/workouts?unit=furlong").await;
    assert_eq!(bad.status, StatusCode::OK);
    assert!(bad.text().contains("24.60 km"));
    assert_eq!(bad.header("set-cookie"), "");
    let cookie =
        browser_common::send(&app, "GET", WORKOUTS, &[("cookie", "phv_unit=furlong")]).await;
    assert!(cookie.text().contains("24.60 km"));
}

#[tokio::test]
async fn unit_links_keep_the_other_filters() {
    let html = get(&app(), "/v/vault/activity/workouts?year=2026&type=Cycling")
        .await
        .text();
    assert!(
        html.contains("year=2026&#38;type=Cycling&#38;unit=mi"),
        "{html}"
    );
}

#[tokio::test]
async fn distance_by_activity_links_to_each_activity_and_keeps_the_period() {
    let html = get(&app(), "/v/vault/activity/workouts?year=2026")
        .await
        .text();
    assert!(
        html.contains(
            r#"<a href="/v/vault/activity/workouts?year=2026&#38;type=Cycling">Cycling</a>"#
        ),
        "{html}"
    );
    assert!(html.contains("Click a bar to see that activity."), "{html}");
}

#[test]
fn the_steps_calendar_has_one_entry_per_recorded_day_in_the_period() {
    let days = step_days(&fixture("Observation")).days;
    let year = step_calendar(&days, &period(Some("2026"), None));
    assert_eq!(year.len(), 4);
    assert_eq!(year[3].date, "2026-02-14");
    assert_eq!(year[3].value, "7777");
    assert_eq!(year[3].title, "14 Feb 2026 · 7,777 steps");
    assert_eq!(year[3].query, "year=2026&month=2");
    assert_eq!(step_calendar(&days, &Period::All).len(), 6);
}

#[test]
fn the_workout_calendar_adds_up_each_day_and_follows_the_activity() {
    let all = workouts(&fixture("Observation"), Unit::Km);
    let everything = workout_calendar(&all, &Period::All, None);
    assert_eq!(everything.len(), 5);
    let ride = everything.iter().find(|d| d.date == "2026-01-04").unwrap();
    assert_eq!(ride.value, "72");
    assert_eq!(ride.title, "4 Jan 2026 · Cycling · 1 h 12 min");
    assert_eq!(ride.query, "year=2026&month=1");

    let cycling = workout_calendar(&all, &Period::All, Some("Cycling"));
    assert_eq!(cycling.len(), 2);
    assert_eq!(cycling[0].query, "year=2025&month=12&type=Cycling");
}

#[test]
fn two_workouts_on_one_day_share_a_square() {
    let mut all = workouts(&fixture("Observation"), Unit::Km);
    let ride = all.iter().find(|w| w.date == "4 Jan 2026").unwrap().clone();
    all.push(ride);
    let shown = workout_calendar(&all, &period(Some("2026"), Some("1")), None);
    let day = shown.iter().find(|d| d.date == "2026-01-04").unwrap();
    assert_eq!(day.value, "144");
    assert_eq!(day.title, "4 Jan 2026 · 2 workouts: Cycling · 2 h 24 min");
}

#[test]
fn a_workout_with_no_duration_still_counts_as_a_day() {
    let panel = json!({
        "id": "p", "status": "final",
        "category": [{"coding": [{"system": "http://terminology.hl7.org/CodeSystem/observation-category", "code": "activity"}]}],
        "code": {"coding": [{"code": "PAPanel"}]},
        "effectivePeriod": {"start": "2026-01-01T08:00:00Z"}
    });
    let all = workouts(&[panel], Unit::Km);
    let shown = workout_calendar(&all, &Period::All, None);
    assert_eq!(shown[0].value, "1");
    assert_eq!(shown[0].title, "1 Jan 2026 · Workout");
}

#[tokio::test]
async fn both_tabs_show_a_calendar_for_a_year_or_all_years_but_not_a_month() {
    let app = app();
    for path in [STEPS, WORKOUTS] {
        for query in ["", "?year=2026"] {
            let html = get(&app, &format!("{path}{query}")).await.text();
            assert!(html.contains(r#"data-chart="calendar""#), "{path}{query}");
            assert!(html.contains("<h2>Calendar</h2>"), "{path}{query}");
            assert!(
                html.contains(r#"class="heat-legend" hidden"#),
                "{path}{query}"
            );
        }
        let month = get(&app, &format!("{path}?year=2026&month=1")).await.text();
        assert!(
            !month.contains(r#"data-chart="calendar""#),
            "the day chart covers a month"
        );
    }
}

#[tokio::test]
async fn the_calendar_table_stays_closed_without_javascript_and_links_to_each_month() {
    let html = get(&app(), "/v/vault/activity?year=2026").await.text();
    assert!(html.contains(r#"<details class="table-view">"#), "{html}");
    assert!(
        html.contains(r#"data-x="2026-02-14" data-y="7777""#),
        "{html}"
    );
    assert!(
        html.contains(r#"href="/v/vault/activity?year=2026&#38;month=2""#),
        "{html}"
    );
}

#[tokio::test]
async fn a_workout_calendar_follows_the_chosen_activity() {
    let html = get(&app(), "/v/vault/activity/workouts?type=Yoga")
        .await
        .text();
    assert!(
        html.contains(r#"data-x="2026-01-01" data-y="40""#),
        "{html}"
    );
    assert!(!html.contains(r#"data-x="2026-01-04""#), "{html}");
}
