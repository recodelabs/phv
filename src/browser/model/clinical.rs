//! Conditions, medicines, allergies and vaccines as simple lists.

use super::value::{
    Dated, capitalized, concept_name, date_or_none, encode_segment, id_of, newest_first,
};
use crate::refs::local_target;
use serde_json::Value;

const STATUS_NOT_RECORDED: &str = "Status not recorded";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClinicalRow {
    pub resource_type: String,
    pub id: String,
    pub id_url: String,
    pub raw_date: Option<String>,
    pub date: String,
    pub name: String,
    pub status: String,
    pub detail: Option<String>,
    /// Status `active`, or no status at all. `false` for vaccines, which are never "active now".
    pub active_now: bool,
}

impl Dated for ClinicalRow {
    fn sort_date(&self) -> Option<&str> {
        self.raw_date.as_deref()
    }
    fn sort_id(&self) -> &str {
        &self.id
    }
}

fn first_text(values: &[&Value]) -> Option<String> {
    values.iter().find_map(|v| v.as_str()).map(str::to_string)
}

/// The code of a status held as a CodeableConcept, such as `clinicalStatus`.
fn concept_code(concept: &Value) -> Option<String> {
    concept["coding"][0]["code"]
        .as_str()
        .map(str::to_string)
        .or_else(|| concept_name(concept))
}

fn dated_row(
    resource: &Value,
    raw_date: Option<String>,
    name: String,
    status: Option<String>,
    detail: Option<String>,
    can_be_active: bool,
) -> ClinicalRow {
    ClinicalRow {
        resource_type: resource["resourceType"].as_str().unwrap_or("").to_string(),
        id: id_of(resource),
        id_url: encode_segment(&id_of(resource)),
        date: date_or_none(raw_date.as_deref()),
        raw_date,
        name,
        active_now: can_be_active && status.as_deref().is_none_or(means_active),
        status: status.map_or_else(|| STATUS_NOT_RECORDED.to_string(), |s| capitalized(&s)),
        detail,
    }
}

/// `recurrence` and `relapse` are kinds of `active` in FHIR's condition status hierarchy.
fn means_active(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "active" | "recurrence" | "relapse"
    )
}

pub fn verified_in_error(resource: &Value) -> bool {
    concept_code(&resource["verificationStatus"]).as_deref() == Some("entered-in-error")
}

/// A diagnosis or allergy that was ruled out is never active, whatever its status says.
fn not_refuted(resource: &Value) -> bool {
    concept_code(&resource["verificationStatus"]).as_deref() != Some("refuted")
}

pub fn conditions(resources: &[Value]) -> Vec<ClinicalRow> {
    let mut rows: Vec<ClinicalRow> = resources
        .iter()
        .filter(|r| !verified_in_error(r))
        .map(|r| {
            dated_row(
                r,
                first_text(&[
                    &r["onsetDateTime"],
                    &r["onsetPeriod"]["start"],
                    &r["recordedDate"],
                ]),
                concept_name(&r["code"]).unwrap_or_else(|| "Unnamed condition".into()),
                concept_code(&r["clinicalStatus"]),
                None,
                not_refuted(r),
            )
        })
        .collect();
    newest_first(&mut rows);
    rows
}

pub fn allergies(resources: &[Value]) -> Vec<ClinicalRow> {
    let mut rows: Vec<ClinicalRow> = resources
        .iter()
        .filter(|r| !verified_in_error(r))
        .map(|r| {
            let reactions: Vec<String> = r["reaction"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|reaction| reaction["manifestation"].as_array().into_iter().flatten())
                .filter_map(concept_name)
                .collect();
            dated_row(
                r,
                first_text(&[&r["onsetDateTime"], &r["recordedDate"]]),
                concept_name(&r["code"]).unwrap_or_else(|| "Unnamed allergy".into()),
                concept_code(&r["clinicalStatus"]),
                (!reactions.is_empty()).then(|| reactions.join(", ")),
                not_refuted(r),
            )
        })
        .collect();
    newest_first(&mut rows);
    rows
}

/// The concept, then the referenced `Medication`, then the reference's `display`.
fn medicine_name(resource: &Value, medications: &[Value]) -> String {
    if let Some(name) = concept_name(&resource["medicationCodeableConcept"]) {
        return name;
    }
    let reference = &resource["medicationReference"];
    let referenced = reference["reference"]
        .as_str()
        .and_then(local_target)
        .filter(|(kind, _)| *kind == "Medication")
        .and_then(|(_, id)| medications.iter().find(|m| m["id"] == id))
        .and_then(|m| concept_name(&m["code"]));
    referenced
        .or_else(|| reference["display"].as_str().map(str::to_string))
        .unwrap_or_else(|| "Unnamed medicine".to_string())
}

/// `MedicationStatement` and `MedicationRequest` together.
pub fn medicines(
    statements: &[Value],
    requests: &[Value],
    medications: &[Value],
) -> Vec<ClinicalRow> {
    let mut rows: Vec<ClinicalRow> = statements
        .iter()
        .chain(requests)
        .filter(|r| r["status"] != "entered-in-error")
        .map(|r| {
            dated_row(
                r,
                first_text(&[
                    &r["effectiveDateTime"],
                    &r["effectivePeriod"]["start"],
                    &r["authoredOn"],
                    &r["dateAsserted"],
                ]),
                medicine_name(r, medications),
                r["status"].as_str().map(str::to_string),
                first_text(&[&r["dosage"][0]["text"], &r["dosageInstruction"][0]["text"]]),
                true,
            )
        })
        .collect();
    newest_first(&mut rows);
    rows
}

pub fn vaccines(resources: &[Value]) -> Vec<ClinicalRow> {
    let mut rows: Vec<ClinicalRow> = resources
        .iter()
        .filter(|r| r["status"] != "entered-in-error")
        .map(|r| {
            let mut row = dated_row(
                r,
                r["occurrenceDateTime"].as_str().map(str::to_string),
                concept_name(&r["vaccineCode"]).unwrap_or_else(|| "Unnamed vaccine".into()),
                r["status"].as_str().map(str::to_string),
                None,
                false,
            );
            if r["status"] == "not-done" {
                row.status = "Not given".to_string();
            }
            if row.raw_date.is_none()
                && let Some(written) = r["occurrenceString"].as_str()
            {
                row.date = written.to_string();
            }
            row
        })
        .collect();
    newest_first(&mut rows);
    rows
}

pub fn active_now(rows: &[ClinicalRow]) -> Vec<ClinicalRow> {
    rows.iter().filter(|r| r.active_now).cloned().collect()
}
