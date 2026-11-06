//! Building the document: entry URNs, narratives, the Composition and the Bundle.

use super::select::{Section, clinical_date};
use crate::canon::{canon_value, derive_id, sha256_hex};
use crate::refs::{for_each_ref, local_target};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const XHTML: &str = "http://www.w3.org/1999/xhtml";

/// A name-based UUID (version 5 layout) from a SHA-256 of `seed`.
fn uuid_from(seed: &str) -> String {
    let h = sha256_hex(seed.as_bytes());
    let variant = (u8::from_str_radix(&h[16..17], 16).expect("hex") & 0x3) | 0x8;
    format!(
        "{}-{}-5{}-{variant:x}{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        &h[17..20],
        &h[20..32]
    )
}

/// The entry `fullUrl` for a vault record, the same every time.
pub(crate) fn entry_url(reference: &str) -> String {
    format!("urn:uuid:{}", uuid_from(reference))
}

/// Points every relative reference at the entry's URN.
pub(crate) fn rewrite_references(resource: &mut Value) {
    for_each_ref(resource, &mut |reference| {
        if local_target(reference).is_some() {
            *reference = entry_url(reference);
        }
    });
}

/// Adds a profile to `meta.profile` unless it is already there.
pub(crate) fn claim_profile(resource: &mut Value, profile: &str) {
    if !resource["meta"].is_object() {
        resource["meta"] = json!({});
    }
    let meta = &mut resource["meta"];
    if !meta["profile"].is_array() {
        meta["profile"] = json!([]);
    }
    let profiles = meta["profile"].as_array_mut().expect("just set");
    if !profiles.iter().any(|p| p == profile) {
        profiles.push(json!(profile));
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Readable text for a CodeableConcept: its text, else a coding's display, else its code.
fn concept_text(concept: &Value) -> Option<String> {
    if let Some(text) = concept["text"].as_str() {
        return Some(text.to_string());
    }
    let codings = concept["coding"].as_array()?;
    codings
        .iter()
        .find_map(|c| c["display"].as_str())
        .or_else(|| codings.iter().find_map(|c| c["code"].as_str()))
        .map(str::to_string)
}

/// The record's name, looking up a referenced Medication in `records`.
fn item_text(resource: &Value, records: &BTreeMap<String, Value>) -> String {
    let from_reference = |field: &str| {
        let reference = &resource[field];
        reference["reference"]
            .as_str()
            .and_then(|r| records.get(r))
            .and_then(|medication| concept_text(&medication["code"]))
            .or_else(|| reference["display"].as_str().map(str::to_string))
    };
    let text = match resource["resourceType"].as_str() {
        Some("MedicationStatement" | "MedicationRequest") => {
            concept_text(&resource["medicationCodeableConcept"])
                .or_else(|| from_reference("medicationReference"))
        }
        Some("Immunization") => concept_text(&resource["vaccineCode"]),
        _ => concept_text(&resource["code"]),
    };
    text.unwrap_or_else(|| "(no name recorded)".into())
}

fn quantity_text(quantity: &Value) -> Option<String> {
    let value = quantity.get("value")?;
    let value = value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_string);
    let unit = quantity["unit"].as_str().or(quantity["code"].as_str());
    let comparator = quantity["comparator"].as_str().unwrap_or("");
    Some(match unit {
        Some(unit) => format!("{comparator}{value} {unit}"),
        None => format!("{comparator}{value}"),
    })
}

/// A short detail: an Observation's value, a dose, or an allergy's criticality.
fn detail_text(resource: &Value) -> String {
    let observation_value = || {
        quantity_text(&resource["valueQuantity"])
            .or_else(|| resource["valueString"].as_str().map(str::to_string))
            .or_else(|| concept_text(&resource["valueCodeableConcept"]))
            .or_else(|| resource["valueBoolean"].as_bool().map(|b| b.to_string()))
            .or_else(|| {
                let parts: Vec<String> = resource["component"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| {
                        let name = concept_text(&c["code"])?;
                        Some(format!("{name}: {}", quantity_text(&c["valueQuantity"])?))
                    })
                    .collect();
                (!parts.is_empty()).then(|| parts.join(", "))
            })
    };
    let text = match resource["resourceType"].as_str() {
        Some("Observation") => observation_value(),
        Some("MedicationStatement") => resource["dosage"][0]["text"].as_str().map(str::to_string),
        Some("MedicationRequest") => resource["dosageInstruction"][0]["text"]
            .as_str()
            .map(str::to_string),
        Some("AllergyIntolerance") => resource["criticality"]
            .as_str()
            .map(|c| format!("criticality {c}")),
        Some("DiagnosticReport") => resource["conclusion"].as_str().map(str::to_string),
        _ => None,
    };
    text.unwrap_or_default()
}

fn status_text(resource: &Value) -> String {
    concept_text(&resource["clinicalStatus"])
        .or_else(|| resource["status"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn left_out_line(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => "<p>1 record was left out because it does not meet the IPS profile.</p>".into(),
        n => format!("<p>{n} records were left out because they do not meet the IPS profile.</p>"),
    }
}

/// The section's XHTML narrative, built only from escaped text.
pub(crate) fn narrative(
    entries: &[&Value],
    left_out: usize,
    records: &BTreeMap<String, Value>,
) -> String {
    let mut body = String::new();
    if entries.is_empty() {
        body.push_str("<p>No information is recorded in this vault.</p>");
    } else {
        body.push_str(
            "<table><thead><tr><th>Item</th><th>Details</th><th>Date</th><th>Status</th></tr></thead><tbody>",
        );
        for resource in entries {
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(&item_text(resource, records)),
                escape(&detail_text(resource)),
                escape(clinical_date(resource).unwrap_or("")),
                escape(&status_text(resource)),
            ));
        }
        body.push_str("</tbody></table>");
    }
    body.push_str(&left_out_line(left_out));
    format!("<div xmlns=\"{XHTML}\">{body}</div>")
}

/// One Composition section. `entries` are vault references in order.
pub(crate) fn section(
    section: Section,
    entries: &[String],
    left_out: usize,
    records: &BTreeMap<String, Value>,
) -> Value {
    let (code, display) = section.loinc();
    let resources: Vec<&Value> = entries.iter().map(|r| &records[r]).collect();
    let mut out = json!({
        "title": section.title(),
        "code": {"coding": [{"system": "http://loinc.org", "code": code, "display": display}]},
        "text": {"status": "generated", "div": narrative(&resources, left_out, records)},
    });
    if entries.is_empty() {
        out["emptyReason"] = json!({"coding": [{
            "system": "http://terminology.hl7.org/CodeSystem/list-empty-reason",
            "code": "unavailable",
            "display": "Unavailable"
        }]});
    } else {
        out["entry"] = entries
            .iter()
            .map(|r| json!({"reference": entry_url(r)}))
            .collect();
    }
    out
}

/// The Composition, with an ID derived from its content.
pub(crate) fn composition(patient: &str, generated_at: &str, sections: Vec<Value>) -> Value {
    let mut composition = json!({
        "resourceType": "Composition",
        "status": "final",
        "type": {"coding": [{"system": "http://loinc.org", "code": "60591-5", "display": "Patient summary Document"}]},
        "subject": {"reference": entry_url(patient)},
        "date": generated_at,
        "author": [{"reference": entry_url(patient)}],
        "title": "International Patient Summary",
        "section": sections,
    });
    let id = derive_id(&composition);
    composition["id"] = json!(id);
    composition
}

/// The document Bundle. `resources` are already rewritten, Composition first.
pub(crate) fn document(generated_at: &str, resources: Vec<Value>) -> Value {
    let entry: Vec<Value> = resources
        .into_iter()
        .map(|resource| {
            let reference = format!(
                "{}/{}",
                resource["resourceType"].as_str().unwrap_or(""),
                resource["id"].as_str().unwrap_or("")
            );
            json!({"fullUrl": entry_url(&reference), "resource": resource})
        })
        .collect();
    let mut bundle = json!({
        "resourceType": "Bundle",
        "type": "document",
        "timestamp": generated_at,
        "entry": entry,
    });
    let digest = uuid_from(&canon_value(&bundle));
    bundle["identifier"] =
        json!({"system": "urn:ietf:rfc:3986", "value": format!("urn:uuid:{digest}")});
    bundle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_urls_are_stable_v5_layout_uuids() {
        let url = entry_url("Patient/abc");
        assert_eq!(url, entry_url("Patient/abc"));
        assert_ne!(url, entry_url("Patient/abd"));
        let uuid = url.strip_prefix("urn:uuid:").unwrap();
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('5'));
        assert!(matches!(
            parts[3].chars().next(),
            Some('8' | '9' | 'a' | 'b')
        ));
    }

    #[test]
    fn narrative_escapes_record_text() {
        let condition =
            json!({"resourceType":"Condition","code":{"text":"<script>alert('x')</script> & co"}});
        let div = narrative(&[&condition], 2, &BTreeMap::new());
        assert!(div.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; co"));
        assert!(!div.contains("<script>"));
        assert!(div.contains("2 records were left out"));
    }

    #[test]
    fn decimals_keep_their_digits_in_the_narrative() {
        let observation: Value = serde_json::from_str(
            r#"{"resourceType":"Observation","valueQuantity":{"value":1.50,"unit":"mg/dL"}}"#,
        )
        .unwrap();
        assert_eq!(detail_text(&observation), "1.50 mg/dL");
    }
}
