//! Checks importer output against the Physical Activity IG's own profiles. phv validates
//! base FHIR at runtime, so this test is what ties the importer to the IG.
//!
//! As the vault design records, octofhir needs profiles translated from their snapshots.
//! All test data is synthetic.

mod common;

use common::{add_patient, phv, records};
use octofhir_fhirpath::{FhirPathEngine, create_function_registry};
use octofhir_fhirschema::{
    EmbeddedSchemaProvider, FhirValidator, FhirVersion, get_schemas, translate,
};
use phv::import::apple_activity::ImportOptions;
use serde_json::{Value, json};
use std::sync::Arc;

const BASE: &str = "http://hl7.org/fhir/us/physical-activity/StructureDefinition";

async fn ig_validator() -> FhirValidator {
    let mut schemas = get_schemas(FhirVersion::R4).clone();
    for (name, text) in [
        (
            "pa-observation-activity-group",
            include_str!("fixtures/physical-activity-ig/pa-observation-activity-group.json"),
        ),
        (
            "pa-observation-activity-measure",
            include_str!("fixtures/physical-activity-ig/pa-observation-activity-measure.json"),
        ),
        (
            "pa-observation-time-measure",
            include_str!("fixtures/physical-activity-ig/pa-observation-time-measure.json"),
        ),
    ] {
        let mut definition: Value = serde_json::from_str(text).unwrap();
        definition["differential"] = definition["snapshot"].clone();
        let schema = translate(serde_json::from_value(definition).unwrap(), None).unwrap();
        schemas.insert(format!("{BASE}/{name}"), schema);
    }
    let engine = FhirPathEngine::new(
        Arc::new(create_function_registry()),
        Arc::new(EmbeddedSchemaProvider::r4()),
    )
    .await
    .unwrap();
    FhirValidator::from_schemas(schemas, Some(Arc::new(engine)))
}

fn export() -> String {
    let rows = [
        json!({"date":"2026-01-01","kind":"dailySteps","steps":5000,"timeZone":"Etc/GMT+4","v":1}),
        json!({"activityType":"cycling","activityTypeRaw":13,"distanceMeters":9994.5,"durationSeconds":1307.4,
            "end":"2026-01-10T08:21:47Z","energyKcal":208.5,"kind":"workout","sourceName":"Bike",
            "start":"2026-01-10T08:00:00Z","timeZone":"America/New_York","uuid":"AAAAAAAA-0000-4000-8000-000000000001","v":1}),
        json!({"activityType":"downhillSkiing","activityTypeRaw":61,"durationSeconds":600,
            "end":"2026-01-11T08:10:00Z","kind":"workout","sourceName":"Watch",
            "start":"2026-01-11T08:00:00Z","uuid":"BBBBBBBB-0000-4000-8000-000000000002","v":1}),
        json!({"counts":{"dailySteps":1,"workouts":2},"failures":[],"kind":"summary","through":"2026-01-31","timeZone":"Etc/GMT+4","v":1}),
    ];
    rows.iter().map(|r| format!("{r}\n")).collect()
}

async fn imported() -> Vec<Value> {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    phv.import_apple_activity(&export(), &ImportOptions::default())
        .await
        .unwrap();
    records(&phv, "Observation")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

async fn findings(validator: &FhirValidator, resource: &Value) -> Vec<String> {
    let profile = resource["meta"]["profile"][0].as_str().unwrap().to_string();
    validator
        .validate(resource, vec![profile])
        .await
        .errors
        .iter()
        .map(|e| {
            let path: Vec<String> = e.path.iter().map(ToString::to_string).collect();
            format!(
                "{} {}: {}",
                e.error_type,
                path.join("."),
                e.message.clone().unwrap_or_default()
            )
        })
        .collect()
}

#[tokio::test]
async fn every_imported_resource_conforms_to_the_profile_it_claims() {
    let validator = ig_validator().await;
    let resources = imported().await;
    // One day, a panel with 4 members, and a panel with 2.
    assert_eq!(resources.len(), 1 + 5 + 3);
    for resource in &resources {
        let found = findings(&validator, resource).await;
        assert!(found.is_empty(), "{found:#?}\n{resource:#}");
    }
}

fn first_with_code(resources: &[Value], code: &str) -> Value {
    resources
        .iter()
        .find(|r| r["code"]["coding"][0]["code"] == code)
        .unwrap()
        .clone()
}

fn without(mut resource: Value, key: &str) -> Value {
    resource.as_object_mut().unwrap().remove(key);
    resource
}

/// The check above means nothing unless the profiles really do reject bad records.
#[tokio::test]
async fn the_profiles_reject_records_that_break_them() {
    let validator = ig_validator().await;
    let resources = imported().await;
    let day = first_with_code(&resources, "41950-7");
    let panel = first_with_code(&resources, "PAPanel");

    let mut one_category = day.clone();
    one_category["category"].as_array_mut().unwrap().pop();
    let mut wrong_category = day.clone();
    wrong_category["category"][1]["coding"][0]["code"] = json!("Other");
    let mut no_unit_system = day.clone();
    no_unit_system["valueQuantity"]
        .as_object_mut()
        .unwrap()
        .remove("system");

    for (name, broken) in [
        ("only one category", one_category),
        ("wrong category", wrong_category),
        ("no subject", without(day.clone(), "subject")),
        (
            "no effective time",
            without(
                without(day.clone(), "effectiveDateTime"),
                "_effectiveDateTime",
            ),
        ),
        ("no UCUM system", no_unit_system),
        ("panel with no members", without(panel.clone(), "hasMember")),
    ] {
        let found = findings(&validator, &broken).await;
        assert!(!found.is_empty(), "{name} should fail against the profile");
    }
}

/// octofhir 0.3.37 does not enforce these profile rules from a snapshot, even when they
/// are broken, so they are checked directly: a panel's fixed code and prohibited value,
/// Period start and end, the unit system's value, the quantity code, and the time-based
/// profile's prohibited CodeableConcept. Remove a check once octofhir covers it.
#[tokio::test]
async fn rules_the_validator_does_not_enforce_hold() {
    let resources = imported().await;
    for resource in &resources {
        let profile = resource["meta"]["profile"][0].as_str().unwrap();
        let has_quantity = resource.get("valueQuantity").is_some();
        let has_concept = resource.get("valueCodeableConcept").is_some();
        if profile.ends_with("activity-group") {
            let code = &resource["code"]["coding"][0];
            assert_eq!(
                code["system"],
                "http://hl7.org/fhir/us/physical-activity/CodeSystem/pa-temporary-codes"
            );
            assert_eq!(code["code"], "PAPanel");
            assert!(!has_quantity && !has_concept);
        } else {
            assert!(has_quantity || has_concept, "{resource:#}");
        }
        if profile.ends_with("time-measure") {
            assert!(!has_concept);
        }
        if has_quantity {
            assert_eq!(
                resource["valueQuantity"]["system"],
                "http://unitsofmeasure.org"
            );
            assert!(resource["valueQuantity"]["code"].is_string());
            assert!(resource["valueQuantity"]["value"].is_number());
        }
        if let Some(period) = resource.get("effectivePeriod") {
            assert!(period["start"].is_string() && period["end"].is_string());
        }
    }
}

/// Codes outside the IG's value sets are a deliberate choice, and must stay short.
#[tokio::test]
async fn only_distance_is_outside_the_igs_activity_codes() {
    // From ValueSet pa-observation-code-activity and pa-observation-code-time.
    let in_ig = ["73985-4", "55411-3", "55424-6", "41950-7", "PAPanel"];
    let outside: Vec<String> = imported()
        .await
        .iter()
        .map(|r| r["code"]["coding"][0]["code"].as_str().unwrap().to_string())
        .filter(|code| !in_ig.contains(&code.as_str()))
        .collect();
    assert_eq!(outside, vec!["55412-1".to_string()]);
}
