//! Which records go in which IPS section.

use serde_json::Value;

const OBSERVATION_CATEGORY: &str = "http://terminology.hl7.org/CodeSystem/observation-category";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Section {
    Problems,
    Allergies,
    Medications,
    Immunizations,
    Results,
    Procedures,
    PastIllness,
    SocialHistory,
    VitalSigns,
}

impl Section {
    /// In the order of the IPS Composition profile.
    pub(crate) const ALL: [Section; 9] = [
        Section::Problems,
        Section::Allergies,
        Section::Medications,
        Section::Immunizations,
        Section::Results,
        Section::Procedures,
        Section::PastIllness,
        Section::SocialHistory,
        Section::VitalSigns,
    ];

    pub(crate) fn title(self) -> &'static str {
        match self {
            Section::Problems => "Problems",
            Section::Allergies => "Allergies and intolerances",
            Section::Medications => "Medications",
            Section::Immunizations => "Immunizations",
            Section::Results => "Results",
            Section::Procedures => "History of procedures",
            Section::PastIllness => "History of past illness",
            Section::SocialHistory => "Social history",
            Section::VitalSigns => "Vital signs",
        }
    }

    /// The section's LOINC code and its current LOINC display, which the HL7 validator
    /// checks against the terminology server.
    pub(crate) fn loinc(self) -> (&'static str, &'static str) {
        match self {
            Section::Problems => ("11450-4", "Problem list - Reported"),
            Section::Allergies => ("48765-2", "Allergies and adverse reactions Document"),
            Section::Medications => ("10160-0", "History of Medication use Narrative"),
            Section::Immunizations => ("11369-6", "History of Immunization note"),
            Section::Results => ("30954-2", "Relevant diagnostic tests/laboratory data note"),
            Section::Procedures => ("47519-4", "History of Procedures Document"),
            Section::PastIllness => ("11348-0", "History of Past illness note"),
            Section::SocialHistory => ("29762-2", "Social history note"),
            Section::VitalSigns => ("8716-3", "Vital signs note"),
        }
    }

    /// IPS requires these sections, even when they are empty.
    pub(crate) fn required(self) -> bool {
        matches!(
            self,
            Section::Problems | Section::Allergies | Section::Medications
        )
    }
}

/// The codes of a CodeableConcept's codings, in order.
pub(crate) fn codes(concept: &Value) -> impl Iterator<Item = &str> {
    concept["coding"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["code"].as_str())
}

/// The first code of a status CodeableConcept, such as `Condition.clinicalStatus`.
fn status_code<'a>(resource: &'a Value, field: &str) -> Option<&'a str> {
    codes(&resource[field]).next()
}

/// The observation category codes in the standard code system.
pub(crate) fn observation_categories(resource: &Value) -> Vec<&str> {
    resource["category"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|category| category["coding"].as_array().into_iter().flatten())
        .filter(|c| c["system"] == OBSERVATION_CATEGORY)
        .filter_map(|c| c["code"].as_str())
        .collect()
}

/// A Condition or AllergyIntolerance that was recorded by mistake or ruled out.
fn not_true(resource: &Value) -> bool {
    matches!(
        status_code(resource, "verificationStatus"),
        Some("entered-in-error" | "refuted")
    )
}

/// The section a record belongs in, or `None` when it belongs in none.
pub(crate) fn section_for(resource: &Value) -> Option<Section> {
    let status = resource["status"].as_str();
    match resource["resourceType"].as_str()? {
        "Condition" => {
            if not_true(resource) {
                return None;
            }
            match status_code(resource, "clinicalStatus") {
                None | Some("active" | "recurrence" | "relapse") => Some(Section::Problems),
                Some("inactive" | "remission" | "resolved") => Some(Section::PastIllness),
                Some(_) => None,
            }
        }
        "AllergyIntolerance" => {
            if not_true(resource) {
                return None;
            }
            match status_code(resource, "clinicalStatus") {
                None | Some("active") => Some(Section::Allergies),
                Some(_) => None,
            }
        }
        "MedicationStatement" => {
            matches!(status, Some("active" | "intended" | "on-hold" | "unknown"))
                .then_some(Section::Medications)
        }
        "MedicationRequest" => {
            matches!(status, Some("active" | "on-hold" | "unknown")).then_some(Section::Medications)
        }
        "Immunization" => (status == Some("completed")).then_some(Section::Immunizations),
        "Procedure" => (status == Some("completed")).then_some(Section::Procedures),
        "DiagnosticReport" => {
            (!matches!(status, Some("entered-in-error" | "cancelled"))).then_some(Section::Results)
        }
        "Observation" => {
            if matches!(status, Some("entered-in-error" | "cancelled")) {
                return None;
            }
            let categories = observation_categories(resource);
            let has = |code: &str| categories.contains(&code);
            if has("laboratory") || has("imaging") {
                Some(Section::Results)
            } else if has("vital-signs") {
                Some(Section::VitalSigns)
            } else if has("social-history") {
                Some(Section::SocialHistory)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The record's clinical date, used to sort a section newest first.
pub(crate) fn clinical_date(resource: &Value) -> Option<&str> {
    const FIELDS: [&[&str]; 14] = [
        &["effectiveDateTime"],
        &["effectivePeriod", "start"],
        &["effectiveInstant"],
        &["onsetDateTime"],
        &["onsetPeriod", "start"],
        &["occurrenceDateTime"],
        &["performedDateTime"],
        &["performedPeriod", "start"],
        &["authoredOn"],
        &["recordedDate"],
        &["dateAsserted"],
        &["issued"],
        &["recorded"],
        &["date"],
    ];
    FIELDS.iter().find_map(|path| {
        path.iter()
            .try_fold(resource, |value, key| value.get(key))
            .and_then(Value::as_str)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn conditions_split_by_clinical_status() {
        let condition = |status: &str| json!({"resourceType":"Condition","clinicalStatus":{"coding":[{"code":status}]}});
        assert_eq!(section_for(&condition("active")), Some(Section::Problems));
        assert_eq!(
            section_for(&condition("resolved")),
            Some(Section::PastIllness)
        );
        assert_eq!(
            section_for(&json!({"resourceType":"Condition"})),
            Some(Section::Problems)
        );
        let mut refuted = condition("active");
        refuted["verificationStatus"] = json!({"coding":[{"code":"refuted"}]});
        assert_eq!(section_for(&refuted), None);
    }

    #[test]
    fn activity_observations_belong_nowhere() {
        let observation = json!({"resourceType":"Observation","status":"final",
            "category":[{"coding":[{"system":OBSERVATION_CATEGORY,"code":"activity"}]}]});
        assert_eq!(section_for(&observation), None);
    }
}
