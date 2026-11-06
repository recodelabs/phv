//! The write path. Every write takes the lock, validates everything, then writes.

use crate::canon::{canon_resource, parse_json, strip_volatile};
use crate::error::{Issue, PhvError};
use crate::read::{check_resource_type, is_known_resource_type};
use crate::refs::{ReferenceKind, collect_refs, reference_kind};
use crate::source::build_provenance;
use crate::store::{Change, Vault, line_id};
use crate::submission::prepare;
use crate::validate::Validator;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default, Clone)]
pub struct AddOptions {
    /// SHA-256 of a stored source file. Requires `agent` and `recorded_at`.
    pub source: Option<String>,
    pub agent: Option<String>,
    pub recorded_at: Option<String>,
    pub dry_run: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct WriteReport {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    pub warnings: Vec<Issue>,
    pub dry_run: bool,
}

pub struct Phv {
    vault: Vault,
    pub(crate) validator: Validator,
    /// Built on the first IPS, so other commands don't pay for the IPS profiles.
    pub(crate) ips_validator: tokio::sync::OnceCell<crate::ips::IpsValidator>,
}

pub(crate) fn resource_ref(resource: &Value) -> String {
    format!(
        "{}/{}",
        resource["resourceType"].as_str().unwrap_or(""),
        resource["id"].as_str().unwrap_or("")
    )
}

impl Phv {
    pub async fn new(vault: Vault) -> Result<Self, PhvError> {
        Ok(Self {
            vault,
            validator: Validator::new().await?,
            ips_validator: tokio::sync::OnceCell::new(),
        })
    }

    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    async fn validate_all(&self, resources: &[Value]) -> Vec<Issue> {
        let mut issues = Vec::new();
        for resource in resources {
            issues.extend(self.validator.validate(resource).await);
        }
        issues
    }

    /// Broken local references, plus any resource that would be a second Patient.
    pub(crate) fn reference_issues(&self, resources: &[Value]) -> Result<Vec<Issue>, PhvError> {
        let submitted: HashSet<String> = resources.iter().map(resource_ref).collect();
        let mut vault_ids: HashMap<String, HashSet<String>> = HashMap::new();
        let mut issues = Vec::new();
        for resource in resources {
            let from = resource_ref(resource);
            for reference in collect_refs(resource) {
                match reference_kind(&reference) {
                    ReferenceKind::External => {}
                    ReferenceKind::Unresolvable => issues.push(Issue::error(
                        "PHV-REF",
                        &from,
                        format!("{reference} is not in this submission"),
                    )),
                    ReferenceKind::Malformed => issues.push(Issue::error(
                        "PHV-REF",
                        &from,
                        format!("{reference} is not a valid reference: use Type/id, a fragment, an absolute URL, or display only"),
                    )),
                    // A lowercase or misspelled type would match a file on a case-insensitive disk.
                    ReferenceKind::Local(target_type, _) if !is_known_resource_type(target_type) => {
                        issues.push(Issue::error(
                            "PHV-REF",
                            &from,
                            format!("{reference} does not name a FHIR R4 resource type: type names are case-sensitive, such as Patient"),
                        ));
                    }
                    ReferenceKind::Local(target_type, target_id) => {
                        if submitted.contains(&reference) {
                            continue;
                        }
                        if !vault_ids.contains_key(target_type) {
                            let ids = self
                                .vault
                                .lines(target_type)?
                                .iter()
                                .filter_map(|l| line_id(l).map(str::to_string))
                                .collect();
                            vault_ids.insert(target_type.to_string(), ids);
                        }
                        if !vault_ids[target_type].contains(target_id) {
                            issues.push(Issue::error(
                                "PHV-REF",
                                &from,
                                format!("{reference} is not in the vault or this submission: include it, point to an existing resource, or use display only"),
                            ));
                        }
                    }
                }
            }
        }

        // Only a submitted Patient can make a second one, so a hand-edited vault can still take other records.
        if resources.iter().any(|r| r["resourceType"] == "Patient") {
            let mut patients: HashSet<String> = self
                .vault
                .lines("Patient")?
                .iter()
                .filter_map(|l| line_id(l).map(str::to_string))
                .collect();
            for resource in resources.iter().filter(|r| r["resourceType"] == "Patient") {
                patients.insert(resource["id"].as_str().unwrap_or("").to_string());
            }
            if patients.len() > 1 {
                issues.push(Issue::error(
                    "PHV-PATIENT",
                    "Patient",
                    "a vault holds one person: use phv replace to update the existing Patient",
                ));
            }
        }
        Ok(issues)
    }

    /// The same checks as `add`, without writing. Returns every finding.
    pub async fn validate_submission(&self, input: &Value) -> Result<Vec<Issue>, PhvError> {
        let resources = prepare(input)?;
        let mut issues = self.validate_all(&resources).await;
        issues.extend(self.reference_issues(&resources)?);
        Ok(issues)
    }

    pub async fn add(&self, input: &Value, options: &AddOptions) -> Result<WriteReport, PhvError> {
        if options.source.is_none() && (options.agent.is_some() || options.recorded_at.is_some()) {
            return Err(PhvError::Usage(
                "agent and recorded_at only apply together with a source".into(),
            ));
        }
        let _lock = self.vault.lock_async().await?;
        let mut resources = prepare(input)?;

        if let Some(sha256) = &options.source {
            let agent = options
                .agent
                .as_deref()
                .ok_or_else(|| PhvError::Usage("--source needs --agent".into()))?;
            let recorded = options
                .recorded_at
                .as_deref()
                .ok_or_else(|| PhvError::Usage("--source needs a recorded time".into()))?;
            let targets: Vec<String> = resources.iter().map(resource_ref).collect();
            let provenance = build_provenance(&self.vault, sha256, &targets, agent, recorded)?;
            resources.push(provenance);
        }

        let issues = self.validate_all(&resources).await;
        let (errors, warnings): (Vec<Issue>, Vec<Issue>) =
            issues.into_iter().partition(Issue::is_error);
        if !errors.is_empty() {
            return Err(PhvError::Invalid(errors));
        }
        let reference_issues = self.reference_issues(&resources)?;
        if !reference_issues.is_empty() {
            return Err(PhvError::Conflict(reference_issues));
        }

        let mut report = WriteReport {
            warnings,
            dry_run: options.dry_run,
            ..WriteReport::default()
        };
        let mut changes = Vec::new();
        let mut conflicts = Vec::new();
        for resource in &resources {
            let reference = resource_ref(resource);
            let resource_type = resource["resourceType"].as_str().unwrap_or("").to_string();
            let id = resource["id"].as_str().unwrap_or("").to_string();
            let line = canon_resource(resource);
            match self.vault.get(&resource_type, &id)? {
                None => {
                    report.created.push(reference);
                    changes.push(Change::Upsert { resource_type, id, line });
                }
                // A repeated add gives the same Provenance ID with a later time. That is not a change.
                Some(stored) if stored == line || resource_type == "Provenance" => report.unchanged.push(reference),
                // The ID comes from this content, but the stored record differs, so it was replaced.
                Some(_) => conflicts.push(Issue::error(
                    "PHV-REPLACED",
                    &reference,
                    "the stored record differs from this content, because it may have been replaced or edited: use phv replace to change it",
                )),
            }
        }
        if !conflicts.is_empty() {
            return Err(PhvError::Conflict(conflicts));
        }
        if options.dry_run {
            self.vault.check_changes(&changes)?;
        } else {
            self.vault.apply(&changes)?;
        }
        Ok(report)
    }

    pub async fn replace(
        &self,
        resource_type: &str,
        id: &str,
        input: &Value,
        dry_run: bool,
    ) -> Result<WriteReport, PhvError> {
        check_resource_type(resource_type)?;
        let _lock = self.vault.lock_async().await?;
        let Some(existing) = self.vault.get(resource_type, id)? else {
            return Err(PhvError::NotFound(format!("{resource_type}/{id}")));
        };
        if input.get("resourceType").and_then(Value::as_str) != Some(resource_type) {
            return Err(PhvError::Usage(format!(
                "input must be a {resource_type} resource"
            )));
        }

        let mut resource = input.clone();
        strip_volatile(&mut resource);
        resource["id"] = json!(id);
        let resources = [resource];

        let issues = self.validate_all(&resources).await;
        let (errors, warnings): (Vec<Issue>, Vec<Issue>) =
            issues.into_iter().partition(Issue::is_error);
        if !errors.is_empty() {
            return Err(PhvError::Invalid(errors));
        }
        let reference_issues = self.reference_issues(&resources)?;
        if !reference_issues.is_empty() {
            return Err(PhvError::Conflict(reference_issues));
        }

        let reference = format!("{resource_type}/{id}");
        let line = canon_resource(&resources[0]);
        let mut report = WriteReport {
            warnings,
            dry_run,
            ..WriteReport::default()
        };
        if line == existing {
            report.unchanged.push(reference);
            return Ok(report);
        }
        report.updated.push(reference);
        let changes = [Change::Upsert {
            resource_type: resource_type.into(),
            id: id.into(),
            line,
        }];
        if dry_run {
            self.vault.check_changes(&changes)?;
        } else {
            self.vault.apply(&changes)?;
        }
        Ok(report)
    }

    pub async fn remove(&self, resource_type: &str, id: &str) -> Result<(), PhvError> {
        check_resource_type(resource_type)?;
        let _lock = self.vault.lock_async().await?;
        if self.vault.get(resource_type, id)?.is_none() {
            return Err(PhvError::NotFound(format!("{resource_type}/{id}")));
        }
        let target = format!("{resource_type}/{id}");
        let mut users = Vec::new();
        for other_type in self.vault.resource_types()? {
            for (position, line) in self.vault.lines(&other_type)?.iter().enumerate() {
                if other_type == resource_type && line_id(line) == Some(id) {
                    continue;
                }
                // Read the line as JSON, so spacing and escapes like `\/` cannot hide a reference.
                let Ok(value) = parse_json(line) else {
                    return Err(PhvError::Conflict(vec![Issue::error(
                        "PHV-LINE",
                        format!("records/{other_type}.ndjson:{}", position + 1),
                        "this line is not valid JSON, so phv cannot tell whether it references the resource being removed: fix the line first",
                    )]));
                };
                if collect_refs(&value).contains(&target) {
                    users.push(Issue::error(
                        "PHV-IN-USE",
                        format!("{other_type}/{}", line_id(line).unwrap_or("?")),
                        format!("references {target}: use phv replace to drop the reference first"),
                    ));
                }
            }
        }
        if !users.is_empty() {
            return Err(PhvError::Conflict(users));
        }
        self.vault.apply(&[Change::Remove {
            resource_type: resource_type.into(),
            id: id.into(),
        }])
    }
}
