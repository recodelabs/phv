//! The Vitals topic: Observations with category `vital-signs`.

use super::observation::{ObsRow, is_entered_in_error, obs_row};
use super::value::{OBSERVATION_CATEGORY, has_code, newest_first};
use serde_json::Value;

/// Fixed order for common measures, by LOINC code. Others follow by name.
const MEASURE_ORDER: [&str; 10] = [
    "loinc:85354-9", // blood pressure panel
    "loinc:55284-4", // blood pressure
    "loinc:8867-4",  // heart rate
    "loinc:29463-7", // body weight
    "loinc:8302-2",  // height
    "loinc:8310-5",  // temperature
    "loinc:59408-5", // oxygen saturation, pulse oximetry
    "loinc:2708-6",  // oxygen saturation, arterial
    "loinc:9279-1",  // respiratory rate
    "loinc:39156-5", // BMI
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measure {
    pub latest: ObsRow,
    pub count: usize,
}

pub fn is_vital(observation: &Value) -> bool {
    has_code(
        &observation["category"],
        OBSERVATION_CATEGORY,
        "vital-signs",
    )
}

/// Vital records, newest first, leaving out those entered in error.
pub fn vital_rows(observations: &[Value]) -> Vec<ObsRow> {
    let mut rows: Vec<ObsRow> = observations
        .iter()
        .filter(|o| is_vital(o) && !is_entered_in_error(o))
        .map(obs_row)
        .collect();
    newest_first(&mut rows);
    rows
}

/// The newest record of each measure. Expects rows from `vital_rows`.
pub fn measures(rows: &[ObsRow]) -> Vec<Measure> {
    let mut found: Vec<Measure> = Vec::new();
    for row in rows {
        match found.iter_mut().find(|m| m.latest.key == row.key) {
            Some(measure) => measure.count += 1,
            None => found.push(Measure {
                latest: row.clone(),
                count: 1,
            }),
        }
    }
    let rank = |key: &str| {
        MEASURE_ORDER
            .iter()
            .position(|k| *k == key)
            .unwrap_or(MEASURE_ORDER.len())
    };
    found.sort_by(|a, b| {
        rank(&a.latest.key)
            .cmp(&rank(&b.latest.key))
            .then_with(|| a.latest.name.cmp(&b.latest.name))
    });
    found
}

/// One measure's records, newest first. Expects rows from `vital_rows`.
pub fn history(rows: &[ObsRow], key: &str) -> Vec<ObsRow> {
    rows.iter().filter(|r| r.key == key).cloned().collect()
}
