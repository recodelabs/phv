//! All FHIR: every record of every type, and the topic page that shows each one.

use super::activity;
use super::clinical::verified_in_error;
use super::labs::{is_lab_observation, is_lab_report};
use super::observation::{clinical_date, is_entered_in_error};
use super::value::{concept_name, encode_segment, friendly_date, id_of, number_text};
use super::vitals::is_vital;
use crate::refs::{collect_refs, local_target};
use serde_json::Value;

pub const PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRow {
    pub id: String,
    pub id_url: String,
    pub label: String,
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefRow {
    pub reference: String,
    /// `Type/id` path to link to, when it resolves in this vault.
    pub target: Option<String>,
}

/// A short label for a record of any type, or an empty string.
pub fn label_of(resource: &Value) -> String {
    if let Some(title) = resource["content"][0]["attachment"]["title"].as_str() {
        return title.to_string();
    }
    if resource["resourceType"] == "ExplanationOfBenefit" {
        return claim_label(resource);
    }
    // Organizations and some other types have one name rather than a list.
    if let Some(name) = resource["name"].as_str() {
        return name.to_string();
    }
    for key in ["code", "vaccineCode", "medicationCodeableConcept", "type"] {
        if let Some(name) = concept_name(&resource[key]) {
            return name;
        }
    }
    if let Some(name) = patient_name(resource) {
        return name;
    }
    String::new()
}

/// Who billed, and how the charge was split.
fn claim_label(claim: &Value) -> String {
    let total = |code: &str| {
        claim["total"]
            .as_array()?
            .iter()
            .find(|t| t["category"]["coding"][0]["code"] == code)
            .and_then(|t| number_text(&t["amount"]["value"]))
    };
    let mut parts: Vec<String> = claim["provider"]["display"]
        .as_str()
        .map(str::to_string)
        .into_iter()
        .collect();
    for (code, text) in [
        ("benefit", "plan paid"),
        ("memberliability", "patient owes"),
    ] {
        if let Some(amount) = total(code) {
            parts.push(format!("{text} ${amount}"));
        }
    }
    parts.join(" · ")
}

/// `name[0].text`, otherwise given names followed by the family name.
pub fn patient_name(resource: &Value) -> Option<String> {
    let name = &resource["name"][0];
    if let Some(text) = name["text"].as_str() {
        return Some(text.to_string());
    }
    let mut parts: Vec<&str> = name["given"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| g.as_str())
        .collect();
    parts.extend(name["family"].as_str());
    (!parts.is_empty()).then(|| parts.join(" "))
}

pub fn record_row(resource: &Value) -> RecordRow {
    let id = id_of(resource);
    let date = clinical_date(resource)
        .or_else(|| resource["date"].as_str().map(str::to_string))
        .or_else(|| resource["recorded"].as_str().map(str::to_string))
        .or_else(|| {
            resource["billablePeriod"]["start"]
                .as_str()
                .map(str::to_string)
        })
        .map(|d| friendly_date(&d))
        .unwrap_or_default();
    RecordRow {
        id_url: encode_segment(&id),
        id,
        label: label_of(resource),
        date,
    }
}

/// Pages count from 1. A page past the end is empty.
pub fn page_of<T: Clone>(items: &[T], page: usize) -> Vec<T> {
    let start = page.saturating_sub(1).saturating_mul(PAGE_SIZE);
    items.iter().skip(start).take(PAGE_SIZE).cloned().collect()
}

pub fn page_count(total: usize) -> usize {
    total.div_ceil(PAGE_SIZE).max(1)
}

pub fn pretty_json(resource: &Value) -> String {
    serde_json::to_string_pretty(resource).unwrap_or_default()
}

/// Every reference in a record, with a link when it resolves.
pub fn references(resource: &Value, exists: impl Fn(&str, &str) -> bool) -> Vec<RefRow> {
    let mut rows: Vec<RefRow> = collect_refs(resource)
        .into_iter()
        .map(|reference| {
            let target = local_target(&reference)
                .filter(|(kind, id)| exists(kind, id))
                .map(|(kind, id)| format!("{kind}/{}", encode_segment(id)));
            RefRow { reference, target }
        })
        .collect();
    rows.sort_by(|a, b| a.reference.cmp(&b.reference));
    rows.dedup();
    rows
}

/// The topic page a record shows up under, as a label and a path below the vault.
pub fn topic_of(resource: &Value, observations: &[Value]) -> Option<(&'static str, String)> {
    if is_entered_in_error(resource) || verified_in_error(resource) {
        return None;
    }
    let simple = |label, path: &str| Some((label, path.to_string()));
    match resource["resourceType"].as_str()? {
        "Observation" if is_vital(resource) => simple("Vitals", "vitals"),
        "Observation" if is_lab_observation(resource) => simple("Labs", "labs"),
        "Observation" if activity::tab_of(resource).is_some() => {
            activity::tab_of(resource).and_then(|path| simple("Activity", path))
        }
        "DiagnosticReport" if is_lab_report(resource, observations) => Some((
            "Labs",
            format!("labs/reports/{}", encode_segment(&id_of(resource))),
        )),
        "Condition" => simple("Conditions", "conditions"),
        "MedicationStatement" | "MedicationRequest" => simple("Medicines", "medicines"),
        "AllergyIntolerance" => simple("Allergies", "allergies"),
        "Immunization" => simple("Vaccines", "vaccines"),
        "DocumentReference" => simple("Documents", "documents"),
        "ExplanationOfBenefit" => Some((
            "Claims",
            format!("claims/{}", encode_segment(&id_of(resource))),
        )),
        _ => None,
    }
}
