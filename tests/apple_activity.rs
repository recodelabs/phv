//! The Apple activity importer. All test data is synthetic.

mod common;

use common::{add_patient, lab, phv, records};
use phv::error::PhvError;
use phv::import::apple_activity::{ImportOptions, ImportReport, Tally};
use phv::store::line_id;
use phv::write::{AddOptions, Phv};
use serde_json::{Value, json};
use std::collections::HashMap;

const UUID_A: &str = "AAAAAAAA-0000-4000-8000-000000000001";
const UUID_B: &str = "BBBBBBBB-0000-4000-8000-000000000002";
const UUID_C: &str = "CCCCCCCC-0000-4000-8000-000000000003";

const LOINC: &str = "http://loinc.org";

fn day(date: &str, steps: i64) -> Value {
    json!({"date":date,"kind":"dailySteps","steps":steps,"timeZone":"Etc/UTC","v":1})
}

fn run(uuid: &str, source: &str, start: &str, end: &str) -> Value {
    json!({"activityType":"running","activityTypeRaw":37,"durationSeconds":1800,"end":end,"energyKcal":310.5,
        "kind":"workout","sourceName":source,"start":start,"uuid":uuid,"v":1})
}

fn morning_run(uuid: &str, source: &str) -> Value {
    run(uuid, source, "2026-01-10T08:00:00Z", "2026-01-10T08:30:00Z")
}

/// An export file with a correct summary line.
fn export(rows: &[Value]) -> String {
    let count = |kind: &str| rows.iter().filter(|r| r["kind"] == kind).count();
    let summary = json!({"counts":{"dailySteps":count("dailySteps"),"workouts":count("workout")},
        "failures":[],"kind":"summary","through":"2026-01-31","timeZone":"Etc/UTC","v":1});
    lines(rows.iter().chain([&summary]))
}

fn lines<'a>(rows: impl Iterator<Item = &'a Value>) -> String {
    rows.map(|r| format!("{r}\n")).collect()
}

async fn import(phv: &Phv, text: &str) -> ImportReport {
    phv.import_apple_activity(text, &ImportOptions::default())
        .await
        .unwrap()
}

fn observations(phv: &Phv) -> Vec<Value> {
    records(phv, "Observation")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn with_code(phv: &Phv, code: &str) -> Vec<Value> {
    observations(phv)
        .into_iter()
        .filter(|o| o["code"]["coding"][0]["code"] == code)
        .collect()
}

fn usage_error(result: Result<ImportReport, PhvError>) -> String {
    match result {
        Err(PhvError::Usage(message)) => message,
        other => panic!("expected a usage error, got {other:?}"),
    }
}

// Reading the file

#[tokio::test]
async fn refuses_a_file_cut_off_before_the_summary() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let message = usage_error(
        phv.import_apple_activity(
            &lines([day("2026-01-01", 5000)].iter()),
            &ImportOptions::default(),
        )
        .await,
    );
    assert!(message.contains("cut off"), "{message}");
    assert_eq!(records(&phv, "Observation"), "");
}

#[tokio::test]
async fn refuses_counts_that_do_not_match_the_summary() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let text = export(&[day("2026-01-01", 5000), day("2026-01-02", 6000)]);
    let without_second_day: String = text
        .lines()
        .filter(|l| !l.contains("2026-01-02"))
        .map(|l| format!("{l}\n"))
        .collect();
    let message = usage_error(
        phv.import_apple_activity(&without_second_day, &ImportOptions::default())
            .await,
    );
    assert!(message.contains("summary"), "{message}");
    assert_eq!(records(&phv, "Observation"), "");
}

#[tokio::test]
async fn refuses_a_file_it_cannot_read() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let summary = r#"{"counts":{"dailySteps":0,"workouts":0},"failures":[],"kind":"summary","through":"2026-01-31","timeZone":"Etc/UTC","v":1}"#;
    for (text, reason) in [
        (String::new(), "cut off"),
        (format!("not json\n{summary}\n"), "not valid JSON"),
        (
            format!("{{\"kind\":\"somethingNew\",\"v\":1}}\n{summary}\n"),
            "unknown kind",
        ),
        (
            format!(
                "{{\"date\":\"2026-01-01\",\"kind\":\"dailySteps\",\"steps\":1,\"timeZone\":\"Etc/UTC\",\"v\":2}}\n{summary}\n"
            ),
            "version",
        ),
        (format!("{summary}\n{summary}\n"), "joined together"),
    ] {
        let message = usage_error(
            phv.import_apple_activity(&text, &ImportOptions::default())
                .await,
        );
        assert!(message.contains(reason), "{reason}: {message}");
    }
    assert_eq!(records(&phv, "Observation"), "");
}

#[tokio::test]
async fn reports_failures_listed_in_the_summary() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let summary = json!({"counts":{"dailySteps":1,"workouts":0},
        "failures":[{"error":"denied","query":"workouts"}],
        "kind":"summary","through":"2026-01-31","timeZone":"Etc/UTC","v":1});
    let report = import(&phv, &lines([day("2026-01-01", 5000), summary].iter())).await;
    assert_eq!(report.export_failures.len(), 1);
    assert_eq!(report.daily_steps.new, 1);
}

#[tokio::test]
async fn a_bad_row_is_rejected_alone() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let bad_end = run(
        UUID_B,
        "Watch",
        "2026-01-10T08:00:00Z",
        "2026-01-10T07:00:00Z",
    );
    let report = import(
        &phv,
        &export(&[
            day("2026-01-01", -5),
            day("2026-13-40", 5),
            day("2026-01-03", 7000),
            bad_end,
        ]),
    )
    .await;
    assert_eq!(report.daily_steps.new, 1);
    assert_eq!(report.workouts.new, 0);
    assert_eq!(report.rejected.len(), 3, "{:?}", report.rejected);
    assert!(!report.is_clean());
    assert_eq!(with_code(&phv, "41950-7").len(), 1);
    assert_eq!(with_code(&phv, "PAPanel").len(), 0);
}

#[tokio::test]
async fn stops_when_the_vault_has_no_patient() {
    let (_d, phv) = phv().await;
    let message = usage_error(
        phv.import_apple_activity(
            &export(&[day("2026-01-01", 5000)]),
            &ImportOptions::default(),
        )
        .await,
    );
    assert!(message.contains("Patient"), "{message}");
}

// FHIR mapping

#[tokio::test]
async fn daily_steps_follow_the_time_based_profile() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    import(&phv, &export(&[day("2026-01-01", 5000)])).await;

    let mut stored = observations(&phv).remove(0);
    let id = stored["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 32);
    let hash = stored["meta"]["tag"][0]["code"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(hash.len(), 64);
    stored["meta"]["tag"][0]["code"] = json!("HASH");
    stored["id"] = json!("ID");
    assert_eq!(
        stored,
        json!({
            "resourceType":"Observation","id":"ID",
            "meta":{
                "profile":["http://hl7.org/fhir/us/physical-activity/StructureDefinition/pa-observation-time-measure"],
                "source":"urn:phv:apple-health:activity-export",
                "tag":[{"system":"urn:phv:source-row","code":"HASH"}]
            },
            "identifier":[{"system":"urn:phv:apple-health:daily-steps","value":"2026-01-01"}],
            "status":"final",
            "category":[
                {"coding":[{"system":"http://terminology.hl7.org/CodeSystem/observation-category","code":"activity"}]},
                {"coding":[{"system":"http://hl7.org/fhir/us/physical-activity/CodeSystem/pa-temporary-codes","code":"PhysicalActivity"}]}
            ],
            "code":{"coding":[{"system":LOINC,"code":"41950-7","display":"Number of steps in 24 hour Measured"}]},
            "subject":{"reference":patient},
            "performer":[{"reference":patient}],
            "effectiveDateTime":"2026-01-01",
            "_effectiveDateTime":{"extension":[{"url":"http://hl7.org/fhir/StructureDefinition/tz-code","valueCode":"Etc/UTC"}]},
            "valueQuantity":{"value":5000,"unit":"steps per day","system":"http://unitsofmeasure.org","code":"/d"}
        })
    );
}

#[tokio::test]
async fn a_workout_becomes_a_panel_with_a_member_per_measure() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let mut row = morning_run(UUID_A, "Watch");
    row["distanceMeters"] = json!(5012.3);
    row["timeZone"] = json!("Etc/GMT+4");
    row["durationSeconds"] = json!(855.4);
    let report = import(&phv, &export(&[row])).await;
    assert_eq!(report.workouts.new, 1);
    assert_eq!(report.resources_written, 5);

    let panel = with_code(&phv, "PAPanel").remove(0);
    assert_eq!(
        panel["meta"]["profile"][0],
        "http://hl7.org/fhir/us/physical-activity/StructureDefinition/pa-observation-activity-group"
    );
    assert_eq!(
        panel["identifier"][0]["system"],
        "urn:phv:apple-health:workout"
    );
    assert_eq!(panel["identifier"][0]["value"], UUID_A);
    assert_eq!(panel["effectivePeriod"]["start"], "2026-01-10T08:00:00Z");
    assert_eq!(panel["effectivePeriod"]["end"], "2026-01-10T08:30:00Z");
    assert_eq!(
        panel["effectivePeriod"]["_start"]["extension"][0]["valueCode"],
        "Etc/GMT+4"
    );
    assert_eq!(panel["device"]["display"], "Watch");
    assert_eq!(panel["subject"]["reference"], patient.as_str());
    assert!(panel.get("valueQuantity").is_none());
    assert_eq!(panel["hasMember"].as_array().unwrap().len(), 4);

    for (code, value, unit) in [
        ("55411-3", "14.26", "min"),
        ("55424-6", "310.5", "kcal"),
        ("55412-1", "5012.3", "m"),
    ] {
        let member = with_code(&phv, code).remove(0);
        assert_eq!(
            member["meta"]["profile"][0],
            "http://hl7.org/fhir/us/physical-activity/StructureDefinition/pa-observation-activity-measure"
        );
        assert_eq!(member["valueQuantity"]["value"].to_string(), value);
        assert_eq!(member["valueQuantity"]["code"], unit);
        assert_eq!(
            member["valueQuantity"]["system"],
            "http://unitsofmeasure.org"
        );
        assert_eq!(
            member["effectivePeriod"],
            json!({"start":"2026-01-10T08:00:00Z","end":"2026-01-10T08:30:00Z"})
        );
        assert_eq!(
            member["identifier"][0]["value"],
            format!("{UUID_A}/{code}").as_str()
        );
    }

    let ids: Vec<String> = observations(&phv)
        .iter()
        .filter(|o| o["code"]["coding"][0]["code"] != "PAPanel")
        .map(|o| format!("Observation/{}", o["id"].as_str().unwrap()))
        .collect();
    for member in panel["hasMember"].as_array().unwrap() {
        assert!(ids.contains(&member["reference"].as_str().unwrap().to_string()));
    }
}

#[tokio::test]
async fn missing_measures_get_no_member() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let row = json!({"activityType":"cycling","activityTypeRaw":13,"durationSeconds":60,"end":"2026-01-10T08:01:00Z",
        "kind":"workout","sourceName":"Strava","start":"2026-01-10T08:00:00Z","uuid":UUID_A,"v":1});
    let report = import(&phv, &export(&[row])).await;
    assert_eq!(report.resources_written, 3);
    assert!(with_code(&phv, "55424-6").is_empty());
    assert!(with_code(&phv, "55412-1").is_empty());
    assert_eq!(
        with_code(&phv, "PAPanel")[0]["hasMember"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn activity_types_use_the_loinc_answer_only_when_it_matches() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let cases = [
        ("walking", Some(("LA11834-1", "Walking")), "Walking"),
        ("running", Some(("LA11836-6", "Running")), "Running"),
        ("cycling", Some(("LA11837-4", "Bicycling")), "Cycling"),
        ("swimming", Some(("LA11838-2", "Swimming")), "Swimming"),
        (
            "functionalStrengthTraining",
            Some(("LA11839-0", "Weights")),
            "Functional strength training",
        ),
        ("mixedCardio", Some(("LA11840-8", "Mixed")), "Mixed cardio"),
        ("downhillSkiing", None, "Downhill skiing"),
        ("yoga", None, "Yoga"),
    ];
    let rows: Vec<Value> = cases
        .iter()
        .enumerate()
        .map(|(i, (name, _, _))| {
            json!({"activityType":name,"activityTypeRaw":1,"durationSeconds":60,
                "end":format!("2026-01-{:02}T08:01:00Z",i+1),"kind":"workout","sourceName":"Watch",
                "start":format!("2026-01-{:02}T08:00:00Z",i+1),"uuid":format!("AAAAAAAA-0000-4000-8000-00000000000{i}"),"v":1})
        })
        .collect();
    let report = import(&phv, &export(&rows)).await;
    assert_eq!(report.workouts.new, cases.len());

    let values: HashMap<String, Value> = with_code(&phv, "73985-4")
        .into_iter()
        .map(|o| {
            let value = o["valueCodeableConcept"].clone();
            let apple = value["coding"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["system"] == "urn:phv:apple-health:workout-activity-type")
                .expect("always has Apple's name")["code"]
                .as_str()
                .unwrap()
                .to_string();
            (apple, value)
        })
        .collect();
    for (name, answer, text) in cases {
        let value = &values[name];
        assert_eq!(value["text"], text);
        let loinc: Vec<&Value> = value["coding"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["system"] == LOINC)
            .collect();
        match answer {
            Some((code, display)) => {
                assert_eq!(loinc[0]["code"], code);
                assert_eq!(loinc[0]["display"], display);
            }
            None => assert!(loinc.is_empty(), "{name}"),
        }
    }
}

// Finding duplicates: repeats in the file

#[tokio::test]
async fn identical_repeats_in_a_file_collapse() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let report = import(
        &phv,
        &export(&[
            day("2026-01-01", 5000),
            day("2026-01-01", 5000),
            morning_run(UUID_A, "Watch"),
            morning_run(UUID_A, "Watch"),
        ]),
    )
    .await;
    assert_eq!(report.duplicates_in_file, 2);
    assert_eq!(report.daily_steps.new, 1);
    assert_eq!(report.workouts.new, 1);
    assert_eq!(with_code(&phv, "41950-7").len(), 1);
    assert_eq!(with_code(&phv, "PAPanel").len(), 1);
    assert!(report.is_clean());
}

#[tokio::test]
async fn different_rows_with_one_key_are_reported_and_not_imported() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let mut other_run = morning_run(UUID_A, "Watch");
    other_run["energyKcal"] = json!(1);
    let report = import(
        &phv,
        &export(&[
            day("2026-01-01", 5000),
            day("2026-01-01", 6000),
            day("2026-01-02", 7000),
            morning_run(UUID_A, "Watch"),
            other_run,
        ]),
    )
    .await;
    assert_eq!(
        report.conflicts,
        vec!["2026-01-01".to_string(), UUID_A.to_string()]
    );
    assert_eq!(report.daily_steps.new, 1);
    assert_eq!(report.workouts.new, 0);
    assert!(!report.is_clean());
    assert_eq!(with_code(&phv, "41950-7").len(), 1);
}

#[tokio::test]
async fn a_uuid_in_another_case_is_the_same_workout() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    import(&phv, &export(&[morning_run(UUID_A, "Watch")])).await;
    let report = import(
        &phv,
        &export(&[morning_run(&UUID_A.to_lowercase(), "Watch")]),
    )
    .await;
    assert_eq!(report.workouts.new, 0);
    assert_eq!(report.workouts.changed, 1);
    assert_eq!(with_code(&phv, "PAPanel").len(), 1);
}

// Finding duplicates: rows already in the vault

#[tokio::test]
async fn importing_the_same_file_again_changes_nothing() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let text = export(&[
        day("2026-01-01", 5000),
        day("2026-01-02", 6000),
        morning_run(UUID_A, "Watch"),
    ]);
    import(&phv, &text).await;
    let before = records(&phv, "Observation");

    let report = import(&phv, &text).await;
    assert_eq!(records(&phv, "Observation"), before);
    assert_eq!(report.daily_steps.unchanged, 2);
    assert_eq!(report.workouts.unchanged, 1);
    assert_eq!(report.daily_steps.new + report.daily_steps.changed, 0);
    assert_eq!(report.workouts.new + report.workouts.changed, 0);
    assert_eq!(report.resources_written, 0);
}

#[tokio::test]
async fn a_later_full_export_only_adds_what_is_new() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    import(
        &phv,
        &export(&[day("2026-01-01", 5000), morning_run(UUID_A, "Watch")]),
    )
    .await;
    let before = records(&phv, "Observation");

    let report = import(
        &phv,
        &export(&[
            day("2026-01-01", 5000),
            day("2026-01-02", 6000),
            morning_run(UUID_A, "Watch"),
            run(
                UUID_B,
                "Watch",
                "2026-01-11T08:00:00Z",
                "2026-01-11T08:30:00Z",
            ),
        ]),
    )
    .await;
    assert_eq!(
        (report.daily_steps.new, report.daily_steps.unchanged),
        (1, 1)
    );
    assert_eq!((report.workouts.new, report.workouts.unchanged), (1, 1));
    // One day, and one panel with three members.
    assert_eq!(report.resources_written, 5);
    let after = records(&phv, "Observation");
    for line in before.lines() {
        assert!(after.contains(line), "a stored line changed: {line}");
    }
    assert_eq!(after.lines().count(), before.lines().count() + 5);
}

#[tokio::test]
async fn a_late_watch_sync_updates_the_day_in_place() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    import(
        &phv,
        &export(&[day("2026-01-01", 5000), day("2026-01-02", 6000)]),
    )
    .await;
    let before = with_code(&phv, "41950-7");

    let report = import(
        &phv,
        &export(&[day("2026-01-01", 5000), day("2026-01-02", 6500)]),
    )
    .await;
    assert_eq!(report.daily_steps.changed, 1);
    assert_eq!(report.changed_days, vec!["2026-01-02".to_string()]);

    let after = with_code(&phv, "41950-7");
    assert_eq!(after.len(), 2);
    let ids = |o: &[Value]| o.iter().map(|o| o["id"].clone()).collect::<Vec<_>>();
    assert_eq!(ids(&after), ids(&before));
    let changed = after
        .iter()
        .find(|o| o["effectiveDateTime"] == "2026-01-02")
        .unwrap();
    assert_eq!(changed["valueQuantity"]["value"].to_string(), "6500");
}

#[tokio::test]
async fn a_new_home_time_zone_keeps_stored_days_and_adds_new_ones() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    import(&phv, &export(&[day("2026-01-01", 5000)])).await;
    let before = records(&phv, "Observation");

    let moved = |date: &str, steps: i64| {
        let mut row = day(date, steps);
        row["timeZone"] = json!("Etc/GMT+5");
        row
    };
    let report = import(
        &phv,
        &export(&[moved("2026-01-01", 4100), moved("2026-01-02", 6000)]),
    )
    .await;
    assert_eq!(report.daily_steps.new, 1);
    assert_eq!(report.daily_steps.changed, 0);
    assert_eq!(report.other_time_zone.len(), 1);
    assert_eq!(report.other_time_zone[0].date, "2026-01-01");
    assert_eq!(
        report.other_time_zone[0].stored_time_zone.as_deref(),
        Some("Etc/UTC")
    );
    assert_eq!(report.other_time_zone[0].exported_time_zone, "Etc/GMT+5");
    let after = records(&phv, "Observation");
    assert!(after.contains(before.trim_end()));
    assert_eq!(after.lines().count(), 2);
}

#[tokio::test]
async fn a_changed_workout_updates_in_place_and_drops_stale_members() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let mut with_distance = morning_run(UUID_A, "Watch");
    with_distance["distanceMeters"] = json!(5000);
    import(&phv, &export(&[with_distance])).await;
    assert_eq!(with_code(&phv, "55412-1").len(), 1);
    let panel_id = with_code(&phv, "PAPanel")[0]["id"].clone();

    let mut edited = morning_run(UUID_A, "Watch");
    edited["energyKcal"] = json!(400);
    let report = import(&phv, &export(&[edited])).await;
    assert_eq!(report.workouts.changed, 1);

    assert_eq!(with_code(&phv, "PAPanel")[0]["id"], panel_id);
    assert_eq!(
        with_code(&phv, "PAPanel")[0]["hasMember"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(with_code(&phv, "55412-1").is_empty());
    assert_eq!(
        with_code(&phv, "55424-6")[0]["valueQuantity"]["value"].to_string(),
        "400"
    );
    assert_eq!(observations(&phv).len(), 4);
}

#[tokio::test]
async fn a_workout_left_out_of_a_later_export_stays() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    import(&phv, &export(&[morning_run(UUID_A, "Watch")])).await;
    let before = records(&phv, "Observation");
    import(&phv, &export(&[])).await;
    assert_eq!(records(&phv, "Observation"), before);
}

#[tokio::test]
async fn other_observations_are_left_alone() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let added = phv
        .add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    let id = added.created[0].strip_prefix("Observation/").unwrap();
    let stored = records(&phv, "Observation");
    let line = stored.lines().find(|l| line_id(l) == Some(id)).unwrap();

    import(&phv, &export(&[day("2026-01-01", 5000)])).await;
    assert!(records(&phv, "Observation").contains(line));
    assert_eq!(records(&phv, "Observation").lines().count(), 2);
}

// Finding duplicates: one session from two sources

fn peloton_ride(uuid: &str) -> Value {
    json!({"activityType":"cycling","activityTypeRaw":13,"distanceMeters":9994,"durationSeconds":1307,
        "end":"2026-01-10T08:21:47Z","energyKcal":208,"kind":"workout","sourceName":"Bike",
        "start":"2026-01-10T08:00:00Z","uuid":uuid,"v":1})
}

fn watch_ride(uuid: &str) -> Value {
    json!({"activityType":"cycling","activityTypeRaw":13,"durationSeconds":1360,
        "end":"2026-01-10T08:22:40Z","energyKcal":331.5,"kind":"workout","sourceName":"Watch",
        "start":"2026-01-10T08:00:01Z","uuid":uuid,"v":1})
}

#[tokio::test]
async fn the_same_session_from_two_sources_keeps_the_richer_one() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let report = import(&phv, &export(&[watch_ride(UUID_A), peloton_ride(UUID_B)])).await;
    assert_eq!(report.workouts.new, 1);
    assert_eq!(report.overlaps.len(), 1);
    assert_eq!(report.overlaps[0].skipped, UUID_A);
    assert_eq!(report.overlaps[0].kept, UUID_B);
    assert_eq!(report.overlaps[0].skipped_source, "Watch");
    assert_eq!(report.overlaps[0].kept_source, "Bike");
    assert!(report.is_clean());
    let panels = with_code(&phv, "PAPanel");
    assert_eq!(panels.len(), 1);
    assert_eq!(panels[0]["identifier"][0]["value"], UUID_B);
}

#[tokio::test]
async fn row_order_does_not_change_which_session_is_kept() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    import(&phv, &export(&[peloton_ride(UUID_B), watch_ride(UUID_A)])).await;
    let first = records(&phv, "Observation");

    let (_d2, phv2) = common::phv().await;
    add_patient(&phv2).await;
    import(&phv2, &export(&[watch_ride(UUID_A), peloton_ride(UUID_B)])).await;
    assert_eq!(records(&phv2, "Observation"), first);
}

#[tokio::test]
async fn a_tie_goes_to_the_longer_workout_then_the_lower_uuid() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let longer = {
        let mut row = morning_run(UUID_C, "Bike");
        row["durationSeconds"] = json!(1900);
        row
    };
    let report = import(&phv, &export(&[morning_run(UUID_A, "Watch"), longer])).await;
    assert_eq!(report.overlaps[0].kept, UUID_C);

    let (_d2, phv2) = common::phv().await;
    add_patient(&phv2).await;
    let report = import(
        &phv2,
        &export(&[morning_run(UUID_B, "Watch"), morning_run(UUID_A, "Bike")]),
    )
    .await;
    assert_eq!(report.overlaps[0].kept, UUID_A);
}

#[tokio::test]
async fn a_stored_session_is_never_displaced_and_the_skip_is_stable() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    import(&phv, &export(&[watch_ride(UUID_A)])).await;
    let before = records(&phv, "Observation");

    // The richer ride arrives later, and the stored one stays.
    let text = export(&[watch_ride(UUID_A), peloton_ride(UUID_B)]);
    for _ in 0..2 {
        let report = import(&phv, &text).await;
        assert_eq!(report.workouts.new, 0);
        assert_eq!(report.workouts.unchanged, 1);
        assert_eq!(report.overlaps.len(), 1);
        assert_eq!(report.overlaps[0].skipped, UUID_B);
        assert_eq!(report.overlaps[0].kept, UUID_A);
        assert_eq!(records(&phv, "Observation"), before);
    }
}

#[tokio::test]
async fn workouts_that_do_not_match_are_kept() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    // A different activity at the same time.
    let mut walk = morning_run(UUID_B, "Phone");
    walk["activityType"] = json!("walking");
    // The same source overlapping itself.
    let same_source = run(
        UUID_C,
        "Watch",
        "2026-01-10T08:10:00Z",
        "2026-01-10T08:40:00Z",
    );
    // A different source, overlapping for under half of the shorter one.
    let brief = run(
        "DDDDDDDD-0000-4000-8000-000000000004",
        "Bike",
        "2026-01-10T07:40:00Z",
        "2026-01-10T08:05:00Z",
    );
    let report = import(
        &phv,
        &export(&[morning_run(UUID_A, "Watch"), walk, same_source, brief]),
    )
    .await;
    assert_eq!(report.overlaps, vec![]);
    assert_eq!(report.workouts.new, 4);
}

#[tokio::test]
async fn sessions_are_compared_one_to_one_and_not_chained() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    // Two separate Watch runs, with a long Bike workout overlapping both. If the Watch
    // runs win, both must stay even though the Bike workout links them.
    let mut first = run(
        UUID_A,
        "Watch",
        "2026-01-10T08:00:00Z",
        "2026-01-10T08:30:00Z",
    );
    let mut second = run(
        UUID_B,
        "Watch",
        "2026-01-10T08:35:00Z",
        "2026-01-10T09:05:00Z",
    );
    first["distanceMeters"] = json!(1000);
    second["distanceMeters"] = json!(1000);
    let mut long = run(
        UUID_C,
        "Bike",
        "2026-01-10T08:15:00Z",
        "2026-01-10T08:50:00Z",
    );
    long["energyKcal"] = Value::Null;
    long.as_object_mut().unwrap().remove("energyKcal");
    let report = import(&phv, &export(&[first, second, long])).await;
    assert_eq!(report.workouts.new, 2);
    assert_eq!(report.overlaps.len(), 1);
    assert_eq!(report.overlaps[0].skipped, UUID_C);
}

#[tokio::test]
async fn keep_overlaps_imports_both() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let options = ImportOptions {
        keep_overlaps: true,
        ..ImportOptions::default()
    };
    let report = phv
        .import_apple_activity(
            &export(&[watch_ride(UUID_A), peloton_ride(UUID_B)]),
            &options,
        )
        .await
        .unwrap();
    assert_eq!(report.workouts.new, 2);
    assert!(report.overlaps.is_empty());
}

// Writing

#[tokio::test]
async fn a_dry_run_reports_and_writes_nothing() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let options = ImportOptions {
        dry_run: true,
        ..ImportOptions::default()
    };
    let report = phv
        .import_apple_activity(
            &export(&[day("2026-01-01", 5000), morning_run(UUID_A, "Watch")]),
            &options,
        )
        .await
        .unwrap();
    assert!(report.dry_run);
    assert_eq!(report.daily_steps.new, 1);
    assert_eq!(report.workouts.new, 1);
    assert_eq!(report.resources_written, 5);
    assert_eq!(records(&phv, "Observation"), "");
}

#[tokio::test]
async fn output_passes_the_vault_validator_with_no_findings_and_stays_sorted() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let mut row = morning_run(UUID_A, "Watch");
    row["distanceMeters"] = json!(5012.3);
    row["timeZone"] = json!("Etc/GMT+4");
    let report = import(
        &phv,
        &export(&[day("2026-01-01", 5000), day("2026-01-02", 1), row]),
    )
    .await;
    assert_eq!(report.warnings, vec![]);
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    assert!(phv.check().await.unwrap().is_empty());
    let stored = records(&phv, "Observation");
    let ids: Vec<&str> = stored.lines().filter_map(line_id).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert_eq!(ids.len(), 7);
}

#[tokio::test]
async fn a_row_failing_fhir_validation_is_rejected_alone_and_keeps_what_is_stored() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let stored_day = day("2026-01-01", 5000);
    let stored_run = morning_run(UUID_A, "Watch");
    import(&phv, &export(&[stored_day.clone(), stored_run])).await;
    let before = records(&phv, "Observation");

    // These pass reading and triage, then fail FHIR validation: a year FHIR does not
    // allow, and a code with two spaces in a row.
    let mut bad_new_run = run(
        UUID_B,
        "Watch",
        "2026-01-11T08:00:00Z",
        "2026-01-11T08:30:00Z",
    );
    bad_new_run["activityType"] = json!("a  b");
    let mut bad_edit = morning_run(UUID_A, "Watch");
    bad_edit["activityType"] = json!("a  b");
    let report = import(
        &phv,
        &export(&[stored_day, day("0000-01-01", 6000), bad_new_run, bad_edit]),
    )
    .await;

    assert_eq!(
        report.daily_steps,
        Tally {
            new: 0,
            changed: 0,
            unchanged: 1
        }
    );
    assert_eq!(report.workouts, Tally::default());
    assert_eq!(report.rejected.len(), 3, "{:?}", report.rejected);
    assert!(report.rejected.iter().all(|r| r.message.contains("FS1014")));
    assert!(!report.is_clean());
    assert_eq!(report.resources_written, 0);
    assert_eq!(records(&phv, "Observation"), before);
}

#[tokio::test]
async fn text_that_could_not_identify_or_be_stored_is_rejected_when_read() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let mut blank_uuid = morning_run("   ", "Watch");
    blank_uuid["energyKcal"] = json!(1);
    let blank_source = morning_run(UUID_A, "  ");
    let mut spaced_zone = day("2026-01-01", 5000);
    spaced_zone["timeZone"] = json!("Etc/UTC  x");
    let mut spaced_workout_zone = morning_run(UUID_B, "Watch");
    spaced_workout_zone["timeZone"] = json!("Etc/UTC  x");
    let report = import(
        &phv,
        &export(&[blank_uuid, blank_source, spaced_zone, spaced_workout_zone]),
    )
    .await;
    assert_eq!(report.rejected.len(), 4, "{:?}", report.rejected);
    assert_eq!(records(&phv, "Observation"), "");
}

#[tokio::test(flavor = "current_thread")]
async fn a_waiting_import_does_not_block_the_runtime() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let phv = std::sync::Arc::new(phv);
    let guard = phv.vault().lock_async().await.unwrap();
    let other = phv.clone();
    let text = export(&[day("2026-01-01", 5000)]);
    let task = tokio::spawn(async move {
        other
            .import_apple_activity(&text, &ImportOptions::default())
            .await
    });
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(!task.is_finished());
    drop(guard);
    task.await.unwrap().unwrap();
    assert_eq!(with_code(&phv, "41950-7").len(), 1);
}

#[tokio::test]
async fn a_dry_run_refuses_what_a_real_import_would_refuse() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let file = phv.vault().root().join("records/Observation.ndjson");
    std::fs::write(
        &file,
        "{\"resourceType\":\"Observation\",\"id\":\"b\"}\n{\"resourceType\":\"Observation\",\"id\":\"a\"}\n",
    )
    .unwrap();
    let options = ImportOptions {
        dry_run: true,
        ..ImportOptions::default()
    };
    let result = phv
        .import_apple_activity(&export(&[day("2026-01-01", 5000)]), &options)
        .await;
    assert!(matches!(result, Err(PhvError::Conflict(_))), "{result:?}");
}

#[tokio::test]
async fn zero_calories_means_no_calories() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let mut row = morning_run(UUID_A, "Watch");
    row["energyKcal"] = json!(0);
    let report = import(&phv, &export(&[row])).await;
    assert_eq!(report.resources_written, 3);
    assert!(with_code(&phv, "55424-6").is_empty());
    assert_eq!(
        with_code(&phv, "PAPanel")[0]["hasMember"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn zero_calories_does_not_count_as_a_field_when_choosing_a_session() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    // The real calories win even though the zero one is longer.
    let mut zero = morning_run(UUID_A, "Watch");
    zero["energyKcal"] = json!(0.0);
    zero["durationSeconds"] = json!(1900);
    let real = morning_run(UUID_B, "Bike");
    let report = import(&phv, &export(&[zero, real])).await;
    assert_eq!(report.overlaps[0].skipped, UUID_A);
    assert_eq!(report.overlaps[0].kept, UUID_B);
}
