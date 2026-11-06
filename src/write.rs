//! The write path. Every write takes the lock, validates everything, then writes.

use crate::canon::{canon_resource, strip_volatile};
use crate::error::{Issue, PhvError};
use crate::refs::{collect_refs, local_target};
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
                if reference.starts_with("urn:") {
                    issues.push(Issue::error(
                        "PHV-REF",
                        &from,
                        format!("{reference} is not in this submission"),
                    ));
                } else if let Some((target_type, target_id)) = local_target(&reference) {
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
        let _lock = self.vault.lock()?;
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
                    "the stored record was changed by phv replace and differs from this content: use phv replace to change it again",
                )),
            }
        }
        if !conflicts.is_empty() {
            return Err(PhvError::Conflict(conflicts));
        }
        if !options.dry_run {
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
        let _lock = self.vault.lock()?;
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
        if !dry_run {
            self.vault.apply(&[Change::Upsert {
                resource_type: resource_type.into(),
                id: id.into(),
                line,
            }])?;
        }
        Ok(report)
    }

    pub async fn remove(&self, resource_type: &str, id: &str) -> Result<(), PhvError> {
        let _lock = self.vault.lock()?;
        if self.vault.get(resource_type, id)?.is_none() {
            return Err(PhvError::NotFound(format!("{resource_type}/{id}")));
        }
        let needle = format!("\"reference\":\"{resource_type}/{id}\"");
        let mut users = Vec::new();
        for other_type in self.vault.resource_types()? {
            for line in self.vault.lines(&other_type)? {
                if line.contains(&needle) {
                    users.push(Issue::error(
                        "PHV-IN-USE",
                        format!("{other_type}/{}", line_id(&line).unwrap_or("?")),
                        format!("references {resource_type}/{id}"),
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
