//! Builds Physical Activity IG Observations. Pure: it only sees rows that triage admitted.

use super::export::{DailySteps, Workout};
use crate::canon::sha256_hex;
use serde_json::{Number, Value, json};

pub const DAILY_STEPS_SYSTEM: &str = "urn:phv:apple-health:daily-steps";
pub const WORKOUT_SYSTEM: &str = "urn:phv:apple-health:workout";
pub const WORKOUT_MEASURE_SYSTEM: &str = "urn:phv:apple-health:workout-measure";
pub const ACTIVITY_TYPE_SYSTEM: &str = "urn:phv:apple-health:workout-activity-type";
pub const SOURCE_ROW_SYSTEM: &str = "urn:phv:source-row";
/// Tags a day or workout with the `through` date of the export it came from.
pub const EXPORT_THROUGH_SYSTEM: &str = "urn:phv:export-through";
pub const TZ_CODE: &str = "http://hl7.org/fhir/StructureDefinition/tz-code";

const META_SOURCE: &str = "urn:phv:apple-health:activity-export";
const IG: &str = "http://hl7.org/fhir/us/physical-activity";
const LOINC: &str = "http://loinc.org";
const UCUM: &str = "http://unitsofmeasure.org";

pub const CODE_ACTIVITY_TYPE: &str = "73985-4";
pub const CODE_DURATION: &str = "55411-3";
pub const CODE_CALORIES: &str = "55424-6";
pub const CODE_DISTANCE: &str = "55412-1";
/// Every measure a workout can have, so an update can remove the ones it lost.
pub const MEASURE_CODES: [&str; 4] = [
    CODE_ACTIVITY_TYPE,
    CODE_DURATION,
    CODE_CALORIES,
    CODE_DISTANCE,
];

/// An ID from the natural key, not the content, so a changed row updates in place.
pub fn stable_id(system: &str, value: &str) -> String {
    sha256_hex(format!("Observation\n{system}\n{value}").as_bytes())[..32].to_string()
}

pub fn member_key(uuid: &str, code: &str) -> String {
    format!("{uuid}/{code}")
}

fn profile(name: &str) -> String {
    format!("{IG}/StructureDefinition/{name}")
}

fn tz_extension(time_zone: &str) -> Value {
    json!({"extension":[{"url":TZ_CODE,"valueCode":time_zone}]})
}

/// The elements every imported Observation has.
fn base(profile_name: &str, system: &str, key: &str, code: Value, patient: &str) -> Value {
    json!({
        "resourceType":"Observation",
        "id":stable_id(system, key),
        "meta":{"profile":[profile(profile_name)],"source":META_SOURCE},
        "identifier":[{"system":system,"value":key}],
        "status":"final",
        "category":[
            {"coding":[{"system":"http://terminology.hl7.org/CodeSystem/observation-category","code":"activity"}]},
            {"coding":[{"system":format!("{IG}/CodeSystem/pa-temporary-codes"),"code":"PhysicalActivity"}]}
        ],
        "code":code,
        "subject":{"reference":patient},
        "performer":[{"reference":patient}]
    })
}

/// The row hash says whether the stored record still matches its row, and the export date
/// says how new the file was. Records from before the date was tagged have only the hash.
fn tag_with_source(resource: &mut Value, row_hash: &str, through: Option<&str>) {
    let mut tags = vec![json!({"system":SOURCE_ROW_SYSTEM,"code":row_hash})];
    if let Some(through) = through {
        tags.push(json!({"system":EXPORT_THROUGH_SYSTEM,"code":through}));
    }
    resource["meta"]["tag"] = Value::Array(tags);
}

fn loinc(code: &str, display: &str) -> Value {
    json!({"coding":[{"system":LOINC,"code":code,"display":display}]})
}

fn quantity(value: Number, unit: &str, code: &str) -> Value {
    json!({"value":value,"unit":unit,"system":UCUM,"code":code})
}

pub fn day(day: &DailySteps, patient: &str, through: Option<&str>) -> Value {
    let mut resource = base(
        "pa-observation-time-measure",
        DAILY_STEPS_SYSTEM,
        &day.date,
        loinc("41950-7", "Number of steps in 24 hour Measured"),
        patient,
    );
    tag_with_source(&mut resource, &day.row_hash, through);
    resource["effectiveDateTime"] = json!(day.date);
    resource["_effectiveDateTime"] = tz_extension(&day.time_zone);
    resource["valueQuantity"] = quantity(day.steps.clone(), "steps per day", "/d");
    resource
}

/// The LOINC answer from the IG's `LL734-5` list, only where the meaning matches.
fn loinc_answer(activity_type: &str) -> Option<(&'static str, &'static str)> {
    match activity_type {
        "walking" => Some(("LA11834-1", "Walking")),
        "running" => Some(("LA11836-6", "Running")),
        "cycling" => Some(("LA11837-4", "Bicycling")),
        "swimming" => Some(("LA11838-2", "Swimming")),
        "functionalStrengthTraining" | "traditionalStrengthTraining" => {
            Some(("LA11839-0", "Weights"))
        }
        "mixedCardio" => Some(("LA11840-8", "Mixed")),
        _ => None,
    }
}

/// `downhillSkiing` becomes `Downhill skiing`.
fn readable(activity_type: &str) -> String {
    let mut out = String::new();
    for (i, c) in activity_type.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
        } else if c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn activity_value(activity_type: &str) -> Value {
    let mut coding = Vec::new();
    if let Some((code, display)) = loinc_answer(activity_type) {
        coding.push(json!({"system":LOINC,"code":code,"display":display}));
    }
    coding.push(json!({"system":ACTIVITY_TYPE_SYSTEM,"code":activity_type}));
    json!({"coding":coding,"text":readable(activity_type)})
}

/// Seconds as minutes, to 2 places.
fn minutes(seconds: f64) -> Number {
    format!("{:.2}", seconds / 60.0)
        .parse()
        .expect("a formatted decimal is a JSON number")
}

/// The panel first, then one member per measure. `reference` values use the final IDs.
pub fn workout(workout: &Workout, patient: &str, through: Option<&str>) -> Vec<Value> {
    let measure = |code: &str, display: &str, value_key: &str, value: Value| {
        let mut member = base(
            "pa-observation-activity-measure",
            WORKOUT_MEASURE_SYSTEM,
            &member_key(&workout.uuid, code),
            loinc(code, display),
            patient,
        );
        member["effectivePeriod"] = json!({"start":workout.start_text,"end":workout.end_text});
        member["device"] = json!({"display":workout.source_name});
        member[value_key] = value;
        member
    };

    let mut members = vec![
        measure(
            CODE_ACTIVITY_TYPE,
            "Exercise activity",
            "valueCodeableConcept",
            activity_value(&workout.activity_type),
        ),
        measure(
            CODE_DURATION,
            "Exercise duration",
            "valueQuantity",
            quantity(minutes(workout.duration_seconds), "minutes", "min"),
        ),
    ];
    if let Some(energy) = &workout.energy_kcal {
        members.push(measure(
            CODE_CALORIES,
            "Calories burned in unspecified time Pedometer",
            "valueQuantity",
            quantity(energy.clone(), "kilocalories", "kcal"),
        ));
    }
    if let Some(distance) = &workout.distance_meters {
        members.push(measure(
            CODE_DISTANCE,
            "Exercise distance unspecified time",
            "valueQuantity",
            quantity(distance.clone(), "meters", "m"),
        ));
    }

    let mut panel = base(
        "pa-observation-activity-group",
        WORKOUT_SYSTEM,
        &workout.uuid,
        json!({"coding":[{
            "system":format!("{IG}/CodeSystem/pa-temporary-codes"),
            "code":"PAPanel",
            "display":"Physical Activity Panel"
        }]}),
        patient,
    );
    tag_with_source(&mut panel, &workout.row_hash, through);
    panel["effectivePeriod"] = json!({"start":workout.start_text,"end":workout.end_text});
    if let Some(time_zone) = &workout.time_zone {
        panel["effectivePeriod"]["_start"] = tz_extension(time_zone);
    }
    panel["device"] = json!({"display":workout.source_name});
    panel["hasMember"] = members
        .iter()
        .map(|m| json!({"reference":format!("Observation/{}", m["id"].as_str().unwrap_or(""))}))
        .collect();

    let mut resources = vec![panel];
    resources.extend(members);
    resources
}
