//! Imports X12 835 remittance files, turning each claim into an ExplanationOfBenefit.
//!
//! A file is imported whole or not at all, except that a stored claim which differs from
//! the file is held for the person to look at and never overwritten.

mod convert;
mod parse;

use crate::canon::{canon_resource, parse_json};
use crate::error::{Issue, PhvError};
use crate::source::build_provenance;
use crate::store::Change;
use crate::write::{Phv, resource_ref};
use convert::{Links, NPI};
use parse::{Claim, Person};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

const AGENT: &str = "phv import x12-835";

#[derive(Debug, Default, Clone)]
pub struct ImportOptions {
    pub dry_run: bool,
    /// When this import happened, for Provenance.
    pub recorded_at: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ClaimTally {
    pub new: usize,
    pub unchanged: usize,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ImportReport {
    pub dry_run: bool,
    pub claims: ClaimTally,
    /// Claim control numbers already stored with different content, which were kept.
    pub held: Vec<String>,
    /// Coverage, Organization and Practitioner records made for this file.
    pub created: Vec<String>,
    /// The stored file. Absent in a dry run, when the file is not stored.
    pub document_reference: Option<String>,
    /// The file says it is test data, not a real payment.
    pub marked_as_test: bool,
    pub warnings: Vec<Issue>,
}

impl ImportReport {
    /// False when a claim was left out because the person should look at it.
    pub fn is_clean(&self) -> bool {
        self.held.is_empty()
    }
}

fn same_text(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// A file's claims must be for the vault's person, or they would land in the wrong record.
fn is_same_person(patient: &Value, person: &Person) -> bool {
    patient["name"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|name| {
            name["family"]
                .as_str()
                .is_some_and(|family| same_text(family, &person.family))
                && name["given"]
                    .as_array()
                    .and_then(|given| given.first())
                    .and_then(Value::as_str)
                    .is_some_and(|given| same_text(given, &person.given))
        })
}

fn has_identifier(resource: &Value, system: &str, value: &str) -> bool {
    resource["identifier"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|i| i["system"] == system && i["value"] == value)
}

/// The vault's records, read once and extended as this file adds to them.
struct Known {
    records: BTreeMap<String, Vec<Value>>,
}

impl Known {
    fn read(phv: &Phv) -> Result<Self, PhvError> {
        let mut records = BTreeMap::new();
        for resource_type in ["Coverage", "Organization", "Practitioner"] {
            let mut resources = Vec::new();
            for line in phv.vault().lines(resource_type)? {
                let value = parse_json(&line).map_err(|e| {
                    PhvError::Conflict(vec![Issue::error(
                        "PHV-LINE",
                        format!("records/{resource_type}.ndjson"),
                        format!("a line is not valid JSON, so phv cannot match records to it: {e}"),
                    )])
                })?;
                resources.push(value);
            }
            records.insert(resource_type.to_string(), resources);
        }
        Ok(Self { records })
    }

    /// The stored record that matches, or else `make()`, which is added to `created`.
    fn find_or_make(
        &mut self,
        resource_type: &str,
        matches: impl Fn(&Value) -> bool,
        make: impl FnOnce() -> Value,
        created: &mut Vec<Value>,
    ) -> String {
        let resources = self.records.entry(resource_type.to_string()).or_default();
        if let Some(found) = resources.iter().find(|r| matches(r)) {
            return resource_ref(found);
        }
        let made = make();
        let reference = resource_ref(&made);
        resources.push(made.clone());
        created.push(made);
        reference
    }
}

impl Phv {
    /// Imports one 835 file and stores the file itself as the records' source.
    pub async fn import_x12_835(
        &self,
        file_name: &str,
        bytes: &[u8],
        options: &ImportOptions,
    ) -> Result<ImportReport, PhvError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| PhvError::Usage(format!("{file_name} is not a text file")))?;
        let interchange =
            parse::parse(text).map_err(|e| PhvError::Usage(format!("{file_name}: {e}")))?;

        // Checked before the file is stored, so another person's file leaves nothing behind.
        let patient = self.vault_patient()?;
        for remittance in &interchange.remittances {
            for claim in &remittance.claims {
                check_person(file_name, &patient, claim)?;
            }
        }

        // `source_add` takes the vault lock itself, so this comes before the lock below.
        let source = if options.dry_run {
            None
        } else {
            Some(self.source_add(file_name, bytes).await?)
        };

        let _lock = self.vault().lock_async().await?;
        let patient_ref = resource_ref(&patient);
        let mut known = Known::read(self)?;
        let mut report = ImportReport {
            dry_run: options.dry_run,
            marked_as_test: interchange.marked_as_test,
            document_reference: source.as_ref().map(|s| s.document_reference.clone()),
            ..ImportReport::default()
        };

        let mut created = Vec::new();
        let mut claims = Vec::new();
        for remittance in &interchange.remittances {
            let payer = known.find_or_make(
                "Organization",
                |r| has_identifier(r, convert::payer_identifier_system(), &remittance.payer_id),
                || convert::payer(remittance),
                &mut created,
            );
            let npi = remittance.payee.npi.as_deref().unwrap_or("");
            let provider = known.find_or_make(
                "Organization",
                |r| has_identifier(r, NPI, npi),
                || convert::provider(&remittance.payee),
                &mut created,
            );
            for claim in &remittance.claims {
                let member = claim.patient.id.as_deref().unwrap_or("");
                let coverage = known.find_or_make(
                    "Coverage",
                    |r| r["subscriberId"] == member,
                    || convert::coverage(claim, &remittance.payer_id, &patient_ref, &payer),
                    &mut created,
                );
                let rendering = claim.rendering.as_ref().map(|person| {
                    let npi = person.id.as_deref().unwrap_or("");
                    known.find_or_make(
                        "Practitioner",
                        |r| has_identifier(r, NPI, npi),
                        || convert::practitioner(person),
                        &mut created,
                    )
                });
                let links = Links {
                    patient: &patient_ref,
                    payer: &payer,
                    provider: &provider,
                    provider_name: &remittance.payee.name,
                    coverage: &coverage,
                    rendering: rendering.as_deref(),
                };
                claims.push((
                    claim.control_number.clone(),
                    convert::explanation_of_benefit(remittance, claim, &links),
                ));
            }
        }

        let mut written = created.clone();
        // A held claim did not come from this file, so its Provenance does not name it. Every
        // other claim counts, so importing a file again finds the same Provenance.
        let mut targets = Vec::new();
        for (control_number, resource) in claims {
            let line = canon_resource(&resource);
            let id = resource["id"].as_str().unwrap_or("");
            match self.vault().get("ExplanationOfBenefit", id)? {
                None => {
                    report.claims.new += 1;
                    targets.push(resource_ref(&resource));
                    written.push(resource);
                }
                Some(stored) if stored == line => {
                    report.claims.unchanged += 1;
                    targets.push(resource_ref(&resource));
                }
                Some(_) => report.held.push(control_number),
            }
        }
        report.created = created.iter().map(resource_ref).collect();

        if let Some(source) = source.as_ref().filter(|_| !targets.is_empty()) {
            let provenance = build_provenance(
                self.vault(),
                &source.sha256,
                &targets,
                AGENT,
                &options.recorded_at,
            )?;
            let id = provenance["id"].as_str().unwrap_or("");
            if self.vault().get("Provenance", id)?.is_none() {
                written.push(provenance);
            }
        }

        let mut errors = Vec::new();
        for resource in &written {
            for issue in self.validator.validate(resource).await {
                if issue.is_error() {
                    errors.push(issue);
                } else if !report.warnings.contains(&issue) {
                    report.warnings.push(issue);
                }
            }
        }
        if !errors.is_empty() {
            return Err(PhvError::Invalid(errors));
        }
        let reference_issues = self.reference_issues(&written)?;
        if !reference_issues.is_empty() {
            return Err(PhvError::Conflict(reference_issues));
        }

        let changes: Vec<Change> = written
            .iter()
            .map(|resource| Change::Upsert {
                resource_type: resource["resourceType"].as_str().unwrap_or("").to_string(),
                id: resource["id"].as_str().unwrap_or("").to_string(),
                line: canon_resource(resource),
            })
            .collect();
        if options.dry_run {
            self.vault().check_changes(&changes)?;
        } else {
            self.vault().apply(&changes)?;
        }
        Ok(report)
    }

    fn vault_patient(&self) -> Result<Value, PhvError> {
        let lines = self.vault().lines("Patient")?;
        match lines.as_slice() {
            [line] => parse_json(line)
                .map_err(|e| PhvError::Usage(format!("the stored Patient cannot be read: {e}"))),
            [] => Err(PhvError::Usage(
                "this vault has no Patient, and claims need one: add the person first".into(),
            )),
            _ => Err(PhvError::Usage(
                "this vault has more than one Patient: run phv check".into(),
            )),
        }
    }
}

fn check_person(file_name: &str, patient: &Value, claim: &Claim) -> Result<(), PhvError> {
    if is_same_person(patient, &claim.patient) {
        return Ok(());
    }
    Err(PhvError::Usage(format!(
        "{file_name}: claim {} is for {} {}, who is not this vault's person, so nothing was imported",
        claim.control_number, claim.patient.given, claim.patient.family
    )))
}
