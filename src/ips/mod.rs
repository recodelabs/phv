//! International Patient Summary (IPS 2.0.0) generation. Reads the vault and changes nothing.
//! See docs/superpowers/specs/2026-11-06-ips-design.md.

mod bundle;
mod check;
mod select;

pub(crate) use check::IpsValidator;

use crate::error::{Issue, PhvError};
use crate::refs::{collect_refs, local_target};
use crate::write::{Phv, resource_ref};
use select::{Section, clinical_date, observation_categories, section_for};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct IpsOptions {
    /// When the document was made, as an RFC 3339 date-time with a time zone. The caller
    /// reads the clock, so the library stays deterministic.
    pub generated_at: String,
}

/// A record that was left out of the IPS, and why.
#[derive(Debug, Clone, Serialize)]
pub struct Omitted {
    pub reference: String,
    /// The section it would have gone in, or `None` for a record entries refer to.
    pub section: Option<&'static str>,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IpsReport {
    pub bundle: Value,
    pub omitted: Vec<Omitted>,
    pub warnings: Vec<Issue>,
}

impl IpsReport {
    /// A one-line, ASCII summary of what was left out, if anything.
    pub fn summary(&self) -> Option<String> {
        match self.omitted.len() {
            0 => None,
            1 => Some(
                "1 record was left out of the IPS because it does not meet the IPS profile".into(),
            ),
            n => Some(format!(
                "{n} records were left out of the IPS because they do not meet the IPS profiles"
            )),
        }
    }
}

/// Every record in the vault, by `Type/id`.
type Records = BTreeMap<String, Value>;

/// Prefixes an issue's path with the record it is about.
fn about(reference: &str, mut issue: Issue) -> Issue {
    let resource_type = reference.split('/').next().unwrap_or("");
    let rest = issue
        .path
        .strip_prefix(resource_type)
        .unwrap_or(&issue.path);
    issue.path = format!("{reference}{rest}");
    issue
}

/// The local references a record makes, in order, without repeats.
fn local_refs(resource: &Value) -> Vec<String> {
    let mut seen = BTreeSet::new();
    collect_refs(resource)
        .into_iter()
        .filter(|r| local_target(r).is_some() && seen.insert(r.clone()))
        .collect()
}

/// IPS lab results need a performer, but the lab skill put it on the report only. In the
/// IPS copy, a laboratory Observation with no performer takes it from a report that lists it.
fn inherit_performers(records: &mut Records) {
    let mut found: BTreeMap<String, Value> = BTreeMap::new();
    for (reference, report) in records.iter() {
        if !reference.starts_with("DiagnosticReport/")
            || matches!(
                report["status"].as_str(),
                Some("entered-in-error" | "cancelled")
            )
        {
            continue;
        }
        let Some(performer) = report.get("performer").filter(|p| p.is_array()) else {
            continue;
        };
        for result in report["result"].as_array().into_iter().flatten() {
            if let Some(target) = result["reference"].as_str() {
                found
                    .entry(target.to_string())
                    .or_insert_with(|| performer.clone());
            }
        }
    }
    for (target, performer) in found {
        if let Some(observation) = records.get_mut(&target)
            && observation["resourceType"] == "Observation"
            && observation.get("performer").is_none()
            && observation_categories(observation).contains(&"laboratory")
        {
            observation["performer"] = performer;
        }
    }
}

impl Phv {
    async fn ips_validator(&self) -> Result<&IpsValidator, PhvError> {
        self.ips_validator.get_or_try_init(IpsValidator::new).await
    }

    fn all_records(&self) -> Result<Records, PhvError> {
        let mut records = Records::new();
        for resource_type in self.vault().resource_types()? {
            for (index, line) in self.vault().lines(&resource_type)?.into_iter().enumerate() {
                let resource: Value = serde_json::from_str(&line).map_err(|e| {
                    PhvError::Usage(format!(
                        "unreadable line {} in {resource_type}: {e}: run phv check",
                        index + 1
                    ))
                })?;
                records.insert(resource_ref(&resource), resource);
            }
        }
        Ok(records)
    }

    /// Builds an IPS document from the vault. Records that fail their IPS profile are left
    /// out and reported. The vault is not changed.
    pub async fn ips(&self, options: &IpsOptions) -> Result<IpsReport, PhvError> {
        chrono::DateTime::parse_from_rfc3339(&options.generated_at).map_err(|e| {
            PhvError::Usage(format!(
                "generated_at must be an RFC 3339 date-time with a time zone, got {:?}: {e}",
                options.generated_at
            ))
        })?;
        let validator = self.ips_validator().await?;
        let mut records = self.all_records()?;
        inherit_performers(&mut records);

        let patient = records
            .keys()
            .find(|r| r.starts_with("Patient/"))
            .cloned()
            .ok_or_else(|| {
                PhvError::Usage(
                    "the vault has no Patient, so there is no one to summarize: add one with phv setup or phv add".into(),
                )
            })?;

        // Section entries, newest first, then by reference.
        let mut chosen: BTreeMap<Section, Vec<String>> = BTreeMap::new();
        for (reference, resource) in &records {
            if let Some(section) = section_for(resource) {
                chosen.entry(section).or_default().push(reference.clone());
            }
        }
        for entries in chosen.values_mut() {
            entries.sort_by(|a, b| {
                let date = |r: &String| clinical_date(&records[r]).unwrap_or("");
                date(b).cmp(date(a)).then_with(|| a.cmp(b))
            });
        }
        let section_of: BTreeMap<&str, Section> = chosen
            .iter()
            .flat_map(|(section, refs)| refs.iter().map(move |r| (r.as_str(), *section)))
            .collect();

        // Check, leave out failures and their dependants, and repeat until nothing changes.
        let mut checked: BTreeMap<String, Vec<Issue>> = BTreeMap::new();
        let mut omitted: BTreeMap<String, Vec<Issue>> = BTreeMap::new();
        let included = loop {
            let mut included = BTreeSet::new();
            let mut pending: Vec<String> = std::iter::once(patient.clone())
                .chain(chosen.values().flatten().cloned())
                .filter(|r| !omitted.contains_key(r))
                .collect();
            while let Some(reference) = pending.pop() {
                if !included.insert(reference.clone()) {
                    continue;
                }
                for target in local_refs(&records[&reference]) {
                    if records.contains_key(&target) {
                        pending.push(target);
                    }
                }
            }

            let mut changed = false;
            for reference in &included {
                if !checked.contains_key(reference) {
                    let resource = &records[reference];
                    let issues = match check::profile_for(resource) {
                        Some(profile) => validator.check(resource, &profile).await,
                        None => Vec::new(),
                    };
                    checked.insert(reference.clone(), issues);
                }
                if omitted.contains_key(reference) {
                    continue;
                }
                let mut reasons: Vec<Issue> = checked[reference]
                    .iter()
                    .filter(|i| i.is_error())
                    .cloned()
                    .map(|i| about(reference, i))
                    .collect();
                for target in local_refs(&records[reference]) {
                    if !records.contains_key(&target) {
                        reasons.push(Issue::error(
                            "PHV-IPS-DANGLING",
                            reference.clone(),
                            format!("refers to {target}, which is not in the vault: run phv check"),
                        ));
                    } else if omitted.contains_key(&target) {
                        reasons.push(Issue::error(
                            "PHV-IPS-DEPENDS",
                            reference.clone(),
                            format!("refers to {target}, which was left out"),
                        ));
                    }
                }
                if !reasons.is_empty() {
                    omitted.insert(reference.clone(), reasons);
                    changed = true;
                }
            }
            if let Some(issues) = omitted.get(&patient) {
                let mut issues = issues.clone();
                issues.insert(
                    0,
                    Issue::error(
                        "PHV-IPS-PATIENT",
                        patient.clone(),
                        "the Patient does not meet the IPS Patient profile, which needs at least a name and birth date: correct it with phv replace",
                    ),
                );
                return Err(PhvError::Invalid(issues));
            }
            if !changed {
                break included;
            }
        };

        // Sections, then the document.
        let mut sections = Vec::new();
        for section in Section::ALL {
            let all = chosen.get(&section).map(Vec::as_slice).unwrap_or_default();
            let kept: Vec<String> = all
                .iter()
                .filter(|r| !omitted.contains_key(*r))
                .cloned()
                .collect();
            let left_out = all.len() - kept.len();
            if kept.is_empty() && left_out == 0 && !section.required() {
                continue;
            }
            sections.push(bundle::section(section, &kept, left_out, &records));
        }
        let mut composition = bundle::composition(&patient, &options.generated_at, sections);

        let composition_issues = validator
            .check(&composition, check::COMPOSITION_PROFILE)
            .await;
        bundle::claim_profile(&mut composition, check::COMPOSITION_PROFILE);

        let in_sections: BTreeSet<&String> = chosen.values().flatten().collect();
        let mut resources = vec![composition];
        let ordered = std::iter::once(&patient)
            .chain(
                Section::ALL
                    .iter()
                    .filter_map(|s| chosen.get(s))
                    .flatten()
                    .filter(|r| **r != patient),
            )
            .chain(
                included
                    .iter()
                    .filter(|r| **r != patient && !in_sections.contains(r)),
            )
            .filter(|r| included.contains(*r));
        for reference in ordered {
            let mut resource = records[reference].clone();
            if let Some(profile) = check::profile_for(&resource) {
                bundle::claim_profile(&mut resource, &profile);
            }
            bundle::rewrite_references(&mut resource);
            resources.push(resource);
        }
        let document = bundle::document(&options.generated_at, resources);

        let mut problems: Vec<Issue> = composition_issues
            .into_iter()
            .filter(Issue::is_error)
            .collect();
        problems.extend(check::check_document(&document));
        if !problems.is_empty() {
            problems.insert(
                0,
                Issue::error(
                    "PHV-IPS-BUG",
                    "Bundle",
                    "phv built an IPS that fails its own checks. This is a bug in phv, so no document was returned",
                ),
            );
            return Err(PhvError::Invalid(problems));
        }

        let warnings = included
            .iter()
            .flat_map(|r| {
                checked[r]
                    .iter()
                    .filter(|i| !i.is_error())
                    .cloned()
                    .map(|i| about(r, i))
            })
            .collect();
        let omitted = omitted
            .into_iter()
            .map(|(reference, issues)| Omitted {
                section: section_of.get(reference.as_str()).map(|s| s.title()),
                reference,
                issues,
            })
            .collect();
        Ok(IpsReport {
            bundle: document,
            omitted,
            warnings,
        })
    }
}
