//! Checks against the IPS profiles, and the Bundle checks octofhir cannot make.

use super::select::{Section, codes, observation_categories};
use crate::error::{Issue, PhvError};
use crate::refs::local_target;
use crate::validate::{base_schemas, findings, octofhir_validator};
use octofhir_fhirschema::{FhirValidator, translate};
use serde_json::Value;
use std::collections::HashSet;

const PROFILES: &str = include_str!("../../data/ips-profiles.json");
const IPS: &str = "http://hl7.org/fhir/uv/ips/StructureDefinition/";
pub(crate) const COMPOSITION_PROFILE: &str =
    "http://hl7.org/fhir/uv/ips/StructureDefinition/Composition-uv-ips";
const VITAL_SIGNS: &str = "http://hl7.org/fhir/StructureDefinition/vitalsigns";

pub(crate) struct IpsValidator {
    inner: FhirValidator,
}

impl IpsValidator {
    /// Translates the embedded profiles. Done once, on the first IPS.
    pub(crate) async fn new() -> Result<Self, PhvError> {
        let bug = |e: String| PhvError::Io(std::io::Error::other(format!("IPS profiles: {e}")));
        let data: Value = serde_json::from_str(PROFILES).map_err(|e| bug(e.to_string()))?;
        let mut schemas = base_schemas().clone();
        for definition in data["profiles"].as_array().into_iter().flatten() {
            let url = definition["url"].as_str().unwrap_or_default().to_string();
            let mut definition = definition.clone();
            // octofhir translates the differential, so give it the full snapshot.
            definition["differential"] = definition["snapshot"].clone();
            let structure =
                serde_json::from_value(definition).map_err(|e| bug(format!("{url}: {e}")))?;
            let schema = translate(structure, None).map_err(|e| bug(format!("{url}: {e}")))?;
            schemas.insert(url, schema);
        }
        Ok(Self {
            inner: octofhir_validator(schemas).await?,
        })
    }

    /// Findings for one resource against one profile.
    pub(crate) async fn check(&self, resource: &Value, profile: &str) -> Vec<Issue> {
        let result = self
            .inner
            .validate(resource, vec![profile.to_string()])
            .await;
        findings(resource, &result)
            .into_iter()
            .filter(|issue| !is_extension_slice_error(issue))
            .collect()
    }
}

/// octofhir 0.3.37 cannot tell extension slices apart, so any extension on a profile with
/// extension slices, such as the IPS Patient, "matches multiple slices" (FS1008). Drop that
/// finding at an `extension` path. Remove this when it is fixed upstream.
fn is_extension_slice_error(issue: &Issue) -> bool {
    issue.code == "FS1008"
        && issue
            .path
            .rsplit('.')
            .next()
            .is_some_and(|last| last.starts_with("extension["))
}

/// The profile a resource is checked against, or `None` for base FHIR only, which every
/// stored record already passed.
pub(crate) fn profile_for(resource: &Value) -> Option<String> {
    let resource_type = resource["resourceType"].as_str()?;
    let name = match resource_type {
        "Patient"
        | "Condition"
        | "AllergyIntolerance"
        | "Immunization"
        | "DiagnosticReport"
        | "Procedure"
        | "Organization"
        | "Practitioner"
        | "PractitionerRole"
        | "Device"
        | "DeviceUseStatement"
        | "Specimen"
        | "ImagingStudy"
        | "Medication"
        | "MedicationStatement"
        | "MedicationRequest"
        | "Composition" => {
            format!("{resource_type}-uv-ips")
        }
        "Flag" => "Flag-alert-uv-ips".into(),
        "Observation" => {
            let categories = observation_categories(resource);
            let has = |code: &str| categories.contains(&code);
            let coded = |code: &str| codes(&resource["code"]).any(|c| c == code);
            if has("laboratory") {
                "Observation-results-laboratory-pathology-uv-ips".into()
            } else if has("imaging") {
                "Observation-results-radiology-uv-ips".into()
            } else if has("vital-signs") {
                return Some(VITAL_SIGNS.into());
            } else if coded("72166-2") {
                "Observation-tobaccouse-uv-ips".into()
            } else if coded("74013-4") {
                "Observation-alcoholuse-uv-ips".into()
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(format!("{IPS}{name}"))
}

/// Checks on the whole document. Any finding here is a bug in phv.
pub(crate) fn check_document(bundle: &Value) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut fail = |path: String, message: String| {
        issues.push(Issue::error("PHV-IPS-DOCUMENT", path, message));
    };
    let entries = bundle["entry"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();

    let mut full_urls = HashSet::new();
    for (index, entry) in entries.iter().enumerate() {
        match entry["fullUrl"].as_str() {
            Some(url) if url.starts_with("urn:uuid:") => {
                if !full_urls.insert(url) {
                    fail(
                        format!("Bundle.entry[{index}].fullUrl"),
                        format!("{url} is repeated"),
                    );
                }
            }
            _ => fail(
                format!("Bundle.entry[{index}].fullUrl"),
                "missing or not a urn:uuid".into(),
            ),
        }
    }

    let composition = &entries
        .first()
        .map(|e| &e["resource"])
        .unwrap_or(&Value::Null);
    if composition["resourceType"] != "Composition" {
        fail(
            "Bundle.entry[0]".into(),
            "the first entry is not a Composition".into(),
        );
    }
    let patient_url = entries
        .iter()
        .find(|e| e["resource"]["resourceType"] == "Patient")
        .and_then(|e| e["fullUrl"].as_str());
    if patient_url.is_none() || composition["subject"]["reference"].as_str() != patient_url {
        fail(
            "Composition.subject".into(),
            "the subject is not the Patient entry".into(),
        );
    }

    let sections = composition["section"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    for section in Section::ALL.iter().filter(|s| s.required()) {
        let (code, _) = section.loinc();
        if !sections
            .iter()
            .any(|s| codes(&s["code"]).any(|c| c == code))
        {
            fail(
                "Composition.section".into(),
                format!(
                    "the required {} section ({code}) is missing",
                    section.title()
                ),
            );
        }
    }
    for (index, section) in sections.iter().enumerate() {
        let has_entries = section["entry"].as_array().is_some_and(|e| !e.is_empty());
        let has_reason = section.get("emptyReason").is_some();
        if has_entries == has_reason {
            fail(
                format!("Composition.section[{index}]"),
                "a section needs entries or an emptyReason, but not both (ips-comp-1, cmp-2)"
                    .into(),
            );
        }
        if section["text"]["div"].as_str().is_none() {
            fail(
                format!("Composition.section[{index}].text"),
                "no narrative".into(),
            );
        }
    }

    for entry in entries {
        let mut resource = entry["resource"].clone();
        let name = format!(
            "{}/{}",
            resource["resourceType"].as_str().unwrap_or(""),
            resource["id"].as_str().unwrap_or("")
        );
        crate::refs::for_each_ref(&mut resource, &mut |reference| {
            if reference.starts_with("urn:uuid:") {
                if !full_urls.contains(reference.as_str()) {
                    fail(
                        name.clone(),
                        format!("{reference} is not an entry in the Bundle"),
                    );
                }
            } else if local_target(reference).is_some() {
                fail(
                    name.clone(),
                    format!("relative reference {reference} was not rewritten"),
                );
            }
        });
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_extension_slice_errors_are_dropped() {
        let issue = |path: &str| Issue::error("FS1008", path, "Item matches multiple slices");
        assert!(is_extension_slice_error(&issue("Patient.extension[0]")));
        assert!(!is_extension_slice_error(&issue("Composition.section[0]")));
        assert!(!is_extension_slice_error(&Issue::error(
            "FS1011",
            "Patient.extension[0]",
            "x"
        )));
    }

    #[test]
    fn every_profile_phv_names_is_embedded() {
        let data: Value = serde_json::from_str(PROFILES).unwrap();
        let embedded: HashSet<&str> = data["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["url"].as_str().unwrap())
            .collect();
        let mut resources: Vec<Value> = [
            "Patient",
            "Condition",
            "AllergyIntolerance",
            "Immunization",
            "DiagnosticReport",
            "Procedure",
            "Organization",
            "Practitioner",
            "PractitionerRole",
            "Device",
            "DeviceUseStatement",
            "Specimen",
            "ImagingStudy",
            "Medication",
            "MedicationStatement",
            "MedicationRequest",
            "Composition",
            "Flag",
        ]
        .iter()
        .map(|t| json!({"resourceType": t}))
        .collect();
        for category in ["laboratory", "imaging", "vital-signs"] {
            resources.push(json!({"resourceType":"Observation","category":[{"coding":[{
                "system":"http://terminology.hl7.org/CodeSystem/observation-category","code":category}]}]}));
        }
        for code in ["72166-2", "74013-4"] {
            resources.push(json!({"resourceType":"Observation","code":{"coding":[{"code":code}]}}));
        }
        for resource in resources {
            let profile = profile_for(&resource).unwrap();
            assert!(
                embedded.contains(profile.as_str()),
                "{profile} is not embedded"
            );
        }
    }

    #[test]
    fn observations_get_profiles_by_category_and_code() {
        let category = |code: &str| {
            json!({"resourceType":"Observation","category":[{"coding":[{
                "system":"http://terminology.hl7.org/CodeSystem/observation-category","code":code}]}]})
        };
        assert!(
            profile_for(&category("laboratory"))
                .unwrap()
                .ends_with("laboratory-pathology-uv-ips")
        );
        assert_eq!(profile_for(&category("vital-signs")).unwrap(), VITAL_SIGNS);
        assert_eq!(profile_for(&category("activity")), None);
        assert_eq!(profile_for(&json!({"resourceType":"Encounter"})), None);
    }
}
