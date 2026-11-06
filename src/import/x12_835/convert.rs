//! Builds FHIR R4 resources from a checked remittance. Pure: lookups are done by the caller.
//!
//! The codes follow the CARIN Blue Button IG (C4BB), but the records do not claim its
//! profiles: an 835 has no diagnoses, which C4BB requires on facility claims.

use super::parse::{Adjustment, Cents, Claim, Party, Person, Remittance, Service};
use crate::canon::sha256_hex;
use chrono::NaiveDate;
use serde_json::{Number, Value, json};

pub const NPI: &str = "http://hl7.org/fhir/sid/us-npi";
pub const META_SOURCE: &str = "urn:phv:x12-835";

const ADJUDICATION: &str = "http://terminology.hl7.org/CodeSystem/adjudication";
const C4BB_ADJUDICATION: &str = "http://hl7.org/fhir/us/carin-bb/CodeSystem/C4BBAdjudication";
const C4BB_DISCRIMINATOR: &str =
    "http://hl7.org/fhir/us/carin-bb/CodeSystem/C4BBAdjudicationDiscriminator";
const C4BB_CARE_TEAM: &str = "http://hl7.org/fhir/us/carin-bb/CodeSystem/C4BBClaimCareTeamRole";
const C4BB_IDENTIFIER: &str = "http://hl7.org/fhir/us/carin-bb/CodeSystem/C4BBIdentifierType";
const C4BB_SUPPORTING: &str = "http://hl7.org/fhir/us/carin-bb/CodeSystem/C4BBSupportingInfoType";
const CARC: &str = "https://x12.org/codes/claim-adjustment-reason-codes";
const CLAIM_TYPE: &str = "http://terminology.hl7.org/CodeSystem/claim-type";
const CPT: &str = "http://www.ama-assn.org/go/cpt";
const DATA_ABSENT: &str = "http://terminology.hl7.org/CodeSystem/data-absent-reason";
/// Whose responsibility an adjustment is. X12 publishes no FHIR system for these.
pub const GROUP_CODE: &str = "urn:phv:x12:claim-adjustment-group-code";
const FILING_INDICATOR: &str = "urn:phv:x12:claim-filing-indicator";
const HCPCS: &str = "https://www.cms.gov/Medicare/Coding/HCPCSReleaseCodeSets";
const PAYEE_TYPE: &str = "http://terminology.hl7.org/CodeSystem/payeetype";
const PLACE_OF_SERVICE: &str =
    "https://www.cms.gov/Medicare/Coding/place-of-service-codes/Place_of_Service_Code_Set";
const REVENUE: &str = "https://www.nubc.org/CodeSystem/RevenueCodes";
const TAX_ID: &str = "urn:oid:2.16.840.1.113883.4.4";
const TYPE_OF_BILL: &str = "https://www.nubc.org/CodeSystem/TypeOfBill";
const V2_0203: &str = "http://terminology.hl7.org/CodeSystem/v2-0203";

/// An ID from the natural key, not the content, so a claim is the same record in any file.
fn stable_id(resource_type: &str, system: &str, value: &str) -> String {
    sha256_hex(format!("{resource_type}\n{system}\n{value}").as_bytes())[..32].to_string()
}

/// The identifier system for a payer's own IDs.
fn payer_system(payer_id: &str) -> String {
    format!("urn:phv:x12-835:payer:{payer_id}")
}

pub fn payer_identifier_system() -> &'static str {
    "urn:phv:x12-835:payer-id"
}

fn coding(system: &str, code: &str) -> Value {
    json!({"coding":[{"system":system,"code":code}]})
}

fn decimal(text: &str) -> Value {
    Value::Number(serde_json::from_str::<Number>(text).expect("checked by the parser"))
}

fn money(amount: Cents) -> Value {
    json!({"value":decimal(&amount.text()),"currency":"USD"})
}

fn date(value: NaiveDate) -> String {
    value.format("%Y-%m-%d").to_string()
}

fn address(party: &Party) -> Option<Value> {
    let mut address = serde_json::Map::new();
    if let Some(line) = &party.line {
        address.insert("line".into(), json!([line]));
    }
    for (key, value) in [
        ("city", &party.city),
        ("state", &party.state),
        ("postalCode", &party.postal_code),
    ] {
        if let Some(value) = value {
            address.insert(key.into(), json!(value));
        }
    }
    (!address.is_empty()).then_some(Value::Object(address))
}

fn organization(id: String, party: &Party, identifiers: Vec<Value>) -> Value {
    let mut resource = json!({
        "resourceType":"Organization",
        "id":id,
        "meta":{"source":META_SOURCE},
        "identifier":identifiers,
        "name":party.name,
    });
    if let Some(address) = address(party) {
        resource["address"] = json!([address]);
    }
    resource
}

pub fn payer(remittance: &Remittance) -> Value {
    let system = payer_identifier_system();
    organization(
        stable_id("Organization", system, &remittance.payer_id),
        &remittance.payer,
        vec![json!({"system":system,"value":remittance.payer_id})],
    )
}

pub fn provider(party: &Party) -> Value {
    let npi = party.npi.as_deref().unwrap_or("");
    let mut identifiers = vec![json!({"system":NPI,"value":npi})];
    if let Some(tax_id) = &party.tax_id {
        identifiers.push(json!({
            "type":coding(V2_0203, "TAX"),
            "system":TAX_ID,
            "value":tax_id
        }));
    }
    organization(stable_id("Organization", NPI, npi), party, identifiers)
}

pub fn practitioner(person: &Person) -> Value {
    let npi = person.id.as_deref().unwrap_or("");
    let mut name = json!({"family":person.family});
    if !person.given.is_empty() {
        name["given"] = json!([person.given]);
    }
    json!({
        "resourceType":"Practitioner",
        "id":stable_id("Practitioner", NPI, npi),
        "meta":{"source":META_SOURCE},
        "identifier":[{"system":NPI,"value":npi}],
        "name":[name],
    })
}

/// A remittance only shows that the plan covered these claims, so `status` means the
/// coverage was in force for them. An 835 has no plan name or dates.
pub fn coverage(claim: &Claim, payer_id: &str, patient: &str, payer: &str) -> Value {
    let member = claim.patient.id.as_deref().unwrap_or("");
    let system = format!("{}:member", payer_system(payer_id));
    let mut kind = coding(FILING_INDICATOR, &claim.filing_indicator);
    if let Some(text) = filing_indicator_text(&claim.filing_indicator) {
        kind["text"] = json!(text);
    }
    json!({
        "resourceType":"Coverage",
        "id":stable_id("Coverage", &system, member),
        "meta":{"source":META_SOURCE},
        "identifier":[{"system":system,"value":member}],
        "status":"active",
        "type":kind,
        "subscriberId":member,
        "beneficiary":{"reference":patient},
        "payor":[{"reference":payer}],
    })
}

fn filing_indicator_text(code: &str) -> Option<&'static str> {
    Some(match code {
        "CI" => "Commercial insurance",
        "BL" => "Blue Cross/Blue Shield",
        "HM" => "HMO",
        "MA" => "Medicare Part A",
        "MB" => "Medicare Part B",
        "MC" => "Medicaid",
        "CH" => "TRICARE",
        "VA" => "Veterans Affairs",
        _ => return None,
    })
}

fn reason_text(code: &str) -> Option<&'static str> {
    Some(match code {
        "1" => "Deductible",
        "2" => "Coinsurance",
        "3" => "Copay",
        "45" => "Charge exceeds the contracted or fee schedule amount",
        _ => return None,
    })
}

/// Who an adjustment falls on. Every other code keeps its group and reason without a
/// narrower category, so nothing is guessed.
fn adjustment_category(adjustment: &Adjustment) -> (&'static str, &'static str) {
    match (adjustment.group.as_str(), adjustment.reason.as_str()) {
        ("PR", "1") => (ADJUDICATION, "deductible"),
        ("PR", "2") => (C4BB_ADJUDICATION, "coinsurance"),
        ("PR", "3") => (ADJUDICATION, "copay"),
        ("PR", _) => (C4BB_ADJUDICATION, "memberliability"),
        ("CO", "45") => (C4BB_ADJUDICATION, "discount"),
        _ => (C4BB_DISCRIMINATOR, "adjustmentreason"),
    }
}

fn adjudication(system: &str, code: &str, amount: Cents) -> Value {
    json!({"category":coding(system, code),"amount":money(amount)})
}

fn adjustment(adjustment: &Adjustment) -> Value {
    let (system, code) = adjustment_category(adjustment);
    let mut category = coding(system, code);
    category["coding"]
        .as_array_mut()
        .expect("built above")
        .push(json!({"system":GROUP_CODE,"code":adjustment.group}));
    let mut reason = coding(CARC, &adjustment.reason);
    if let Some(text) = reason_text(&adjustment.reason) {
        reason["text"] = json!(text);
    }
    json!({"category":category,"reason":reason,"amount":money(adjustment.amount)})
}

fn procedure_system(code: &str) -> &'static str {
    // HCPCS Level II codes start with a letter. CPT codes, Category II and III included, do not.
    match code.chars().next() {
        Some(first) if first.is_ascii_alphabetic() => HCPCS,
        _ => CPT,
    }
}

fn item(claim: &Claim, sequence: usize, service: &Service, rendering: bool) -> Value {
    let (procedure, revenue) = match service.qualifier.as_str() {
        "NU" => (
            coding(DATA_ABSENT, "not-applicable"),
            Some(service.code.as_str()),
        ),
        _ => (
            coding(procedure_system(&service.code), &service.code),
            service.revenue.as_deref(),
        ),
    };
    let mut item = json!({
        "sequence":sequence,
        "productOrService":procedure,
    });
    if let Some(revenue) = revenue {
        item["revenue"] = coding(REVENUE, revenue);
    }
    if rendering {
        item["careTeamSequence"] = json!([1]);
    }
    if !service.modifiers.is_empty() {
        let system = procedure_system(&service.code);
        item["modifier"] = Value::Array(
            service
                .modifiers
                .iter()
                .map(|m| coding(system, m))
                .collect(),
        );
    }
    if let Some(serviced) = service.date {
        item["servicedDate"] = json!(date(serviced));
    } else if let (Some(start), Some(end)) = (claim.statement_start, claim.statement_end) {
        item["servicedPeriod"] = json!({"start":date(start),"end":date(end)});
    } else if let Some(start) = claim.statement_start {
        item["servicedDate"] = json!(date(start));
    }
    if !claim.institutional {
        item["locationCodeableConcept"] = coding(PLACE_OF_SERVICE, &claim.facility);
    }
    if let Some(units) = &service.units {
        item["quantity"] = json!({"value":decimal(units)});
    }
    let mut lines = vec![adjudication(ADJUDICATION, "submitted", service.charge)];
    if let Some(allowed) = service.allowed {
        lines.push(adjudication(ADJUDICATION, "eligible", allowed));
    }
    lines.push(adjudication(ADJUDICATION, "benefit", service.paid));
    lines.extend(service.adjustments.iter().map(adjustment));
    item["adjudication"] = Value::Array(lines);
    item
}

fn billable_period(claim: &Claim) -> Value {
    let service_dates = claim.services.iter().filter_map(|s| s.date);
    let start = claim.statement_start.or(service_dates.clone().min());
    let end = claim.statement_end.or(service_dates.max());
    let mut period = serde_json::Map::new();
    if let Some(start) = start {
        period.insert("start".into(), json!(date(start)));
    }
    if let Some(end) = end {
        period.insert("end".into(), json!(date(end)));
    }
    Value::Object(period)
}

fn supporting_info(claim: &Claim) -> Vec<Value> {
    let category = |code: &str| coding(C4BB_SUPPORTING, code);
    let mut info = Vec::new();
    if let Some(received) = claim.received {
        info.push(json!({"category":category("clmrecvddate"),"timingDate":date(received)}));
    }
    if claim.institutional {
        info.push(json!({
            "category":category("typeofbill"),
            "code":coding(TYPE_OF_BILL, &format!("{}{}", claim.facility, claim.frequency))
        }));
    }
    info.push(json!({
        "category":category("patientaccountnumber"),
        "valueString":claim.patient_control
    }));
    if let Some(record) = &claim.medical_record {
        info.push(json!({"category":category("medicalrecordnumber"),"valueString":record}));
    }
    for (index, entry) in info.iter_mut().enumerate() {
        entry["sequence"] = json!(index + 1);
    }
    info
}

/// References to resources the caller has found or created.
pub struct Links<'a> {
    pub patient: &'a str,
    pub payer: &'a str,
    pub provider: &'a str,
    /// The provider's name as this file gives it, so a claim can be labelled without a lookup.
    pub provider_name: &'a str,
    pub coverage: &'a str,
    pub rendering: Option<&'a str>,
}

/// One claim, with what was billed, what the plan paid and what the patient owes.
pub fn explanation_of_benefit(remittance: &Remittance, claim: &Claim, links: &Links) -> Value {
    let system = format!(
        "{}:claim-control-number",
        payer_system(&remittance.payer_id)
    );
    let kind = if claim.institutional {
        "institutional"
    } else {
        "professional"
    };
    let mut totals = vec![adjudication(ADJUDICATION, "submitted", claim.charge)];
    if let Some(covered) = claim.coverage_amount {
        totals.push(adjudication(ADJUDICATION, "eligible", covered));
    }
    totals.push(adjudication(ADJUDICATION, "benefit", claim.paid));
    totals.push(adjudication(
        C4BB_ADJUDICATION,
        "memberliability",
        claim.patient_owes,
    ));
    let mut resource = json!({
        "resourceType":"ExplanationOfBenefit",
        "id":stable_id("ExplanationOfBenefit", &system, &claim.control_number),
        "meta":{"source":META_SOURCE},
        "identifier":[{
            "type":coding(C4BB_IDENTIFIER, "uc"),
            "system":system,
            "value":claim.control_number
        }],
        "status":"active",
        "type":coding(CLAIM_TYPE, kind),
        "use":"claim",
        "patient":{"reference":links.patient},
        "billablePeriod":billable_period(claim),
        "created":date(remittance.produced.unwrap_or(remittance.payment_date)),
        "insurer":{"reference":links.payer},
        "provider":{"reference":links.provider,"display":links.provider_name},
        "outcome":"complete",
        "payee":{
            "type":coding(PAYEE_TYPE, "provider"),
            "party":{"reference":links.provider}
        },
        "supportingInfo":supporting_info(claim),
        "insurance":[{"focal":true,"coverage":{"reference":links.coverage}}],
        "item":claim.services.iter().enumerate()
            .map(|(i, service)| item(claim, i + 1, service, links.rendering.is_some()))
            .collect::<Vec<_>>(),
        "total":totals,
        "payment":{
            "date":date(remittance.payment_date),
            "amount":money(claim.paid),
            "identifier":{
                "system":format!("{}:payment", payer_system(&remittance.payer_id)),
                "value":remittance.trace
            }
        },
    });
    if let Some(rendering) = links.rendering {
        resource["careTeam"] = json!([{
            "sequence":1,
            "provider":{"reference":rendering},
            "responsible":true,
            "role":coding(C4BB_CARE_TEAM, "rendering")
        }]);
    }
    resource
}
