//! One Observation as a table row, shared by vitals and labs.

use super::value::{
    Dated, LOINC, code_in_system, concept_name, date_or_none, encode_segment, id_of, number_text,
    result_lines,
};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObsRow {
    pub id: String,
    pub raw_date: Option<String>,
    pub date: String,
    pub name: String,
    pub key: String,
    pub key_url: String,
    pub lines: Vec<String>,
    pub flag: Option<String>,
    pub range: Option<String>,
}

impl Dated for ObsRow {
    fn sort_date(&self) -> Option<&str> {
        self.raw_date.as_deref()
    }
    fn sort_id(&self) -> &str {
        &self.id
    }
}

pub fn is_entered_in_error(resource: &Value) -> bool {
    resource["status"] == "entered-in-error"
}

/// The clinically relevant date of an Observation or DiagnosticReport.
pub fn clinical_date(resource: &Value) -> Option<String> {
    [
        &resource["effectiveDateTime"],
        &resource["effectiveInstant"],
        &resource["effectivePeriod"]["start"],
        &resource["issued"],
    ]
    .into_iter()
    .find_map(|v| v.as_str())
    .map(str::to_string)
}

/// Groups by LOINC code, otherwise by exact text. Different names are never merged.
pub fn group_key(observation: &Value) -> String {
    if let Some(code) = code_in_system(&observation["code"], LOINC) {
        return format!("loinc:{code}");
    }
    match concept_name(&observation["code"]) {
        Some(name) => format!("text:{name}"),
        None => format!("id:{}", id_of(observation)),
    }
}

fn range_text(observation: &Value) -> Option<String> {
    let range = &observation["referenceRange"][0];
    if let Some(text) = range["text"].as_str() {
        return Some(text.to_string());
    }
    match (
        number_text(&range["low"]["value"]),
        number_text(&range["high"]["value"]),
    ) {
        (Some(low), Some(high)) => Some(format!("{low}–{high}")),
        (Some(low), None) => Some(format!("≥{low}")),
        (None, Some(high)) => Some(format!("≤{high}")),
        (None, None) => None,
    }
}

pub fn obs_row(observation: &Value) -> ObsRow {
    let raw_date = clinical_date(observation);
    let key = group_key(observation);
    ObsRow {
        id: id_of(observation),
        date: date_or_none(raw_date.as_deref()),
        raw_date,
        name: concept_name(&observation["code"]).unwrap_or_else(|| "Unnamed".into()),
        key_url: encode_segment(&key),
        key,
        lines: result_lines(observation),
        flag: concept_name(&observation["interpretation"][0]),
        range: range_text(observation),
    }
}
