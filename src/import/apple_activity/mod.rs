//! Imports an activity file from the HealthKit export app.
//!
//! See docs/superpowers/specs/2026-11-06-apple-activity-import-design.md.

mod convert;
mod export;
mod triage;

use crate::canon::{canon_resource, parse_json};
use crate::error::{Issue, PhvError};
use crate::refs::collect_refs;
use crate::store::{Change, line_id};
use crate::write::Phv;
use convert::{MEASURE_CODES, WORKOUT_MEASURE_SYSTEM};
use serde::Serialize;
use serde_json::Value;
use triage::{Action, VaultIndex};

#[derive(Debug, Default, Clone)]
pub struct ImportOptions {
    pub dry_run: bool,
    /// Imports both workouts when two sources recorded one session.
    pub keep_overlaps: bool,
}

/// What happened to one kind of row.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Tally {
    pub new: usize,
    pub changed: usize,
    pub unchanged: usize,
}

impl Tally {
    fn count(&mut self, action: Action) {
        match action {
            Action::New => self.new += 1,
            Action::Changed => self.changed += 1,
        }
    }
}

/// A row with a bad value, which was left out of the import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rejected {
    /// The date or UUID, or the line number when the row has neither.
    pub key: String,
    pub message: String,
}

/// A stored day kept because the export counted it in another time zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TimeZoneHold {
    pub date: String,
    pub stored_time_zone: Option<String>,
    pub exported_time_zone: String,
}

/// A stored day or workout kept because this file is older than the newest export in the
/// vault, so its values would have gone backwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OlderExportHold {
    /// `day` or `workout`.
    pub kind: String,
    /// The date or UUID.
    pub key: String,
    pub file_through: String,
    pub vault_through: String,
}

/// A stored day or workout kept because it was replaced by hand, which leaves it without
/// the tag that says which export row it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditedHold {
    /// `day` or `workout`.
    pub kind: String,
    /// The date or UUID.
    pub key: String,
}

/// A measure a changed workout no longer has, kept because another record references it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeptMember {
    pub workout: String,
    /// `Observation/<id>`.
    pub member: String,
    /// The LOINC code of the measure.
    pub measure: String,
    pub referenced_by: Vec<String>,
}

/// A workout skipped because another source recorded the same session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Overlap {
    pub skipped: String,
    pub skipped_source: String,
    pub kept: String,
    pub kept_source: String,
    pub activity_type: String,
    pub start: String,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ImportReport {
    pub dry_run: bool,
    pub daily_steps: Tally,
    pub workouts: Tally,
    /// Resources created, updated or removed, or that would be in a dry run.
    pub resources_written: usize,
    pub duplicates_in_file: usize,
    pub conflicts: Vec<String>,
    pub changed_days: Vec<String>,
    pub other_time_zone: Vec<TimeZoneHold>,
    pub older_export: Vec<OlderExportHold>,
    pub edited_since_import: Vec<EditedHold>,
    pub kept_members: Vec<KeptMember>,
    pub overlaps: Vec<Overlap>,
    pub rejected: Vec<Rejected>,
    pub export_failures: Vec<Value>,
    pub warnings: Vec<Issue>,
}

impl ImportReport {
    /// False when a row was left out because of a problem the person should look at.
    pub fn is_clean(&self) -> bool {
        self.rejected.is_empty() && self.conflicts.is_empty()
    }
}

impl Phv {
    /// Imports one export file. Rows already in the vault never reach the converter.
    pub async fn import_apple_activity(
        &self,
        text: &str,
        options: &ImportOptions,
    ) -> Result<ImportReport, PhvError> {
        let export = export::parse(text)?;

        // Held from triage through the write, so what triage read is still true at the write.
        let _lock = self.vault().lock_async().await?;
        let patient = self.only_patient()?;
        let index = VaultIndex::from_lines(&self.vault().lines("Observation")?)?;
        let triage = triage::triage(&export, &index, options.keep_overlaps);

        let mut report = ImportReport {
            dry_run: options.dry_run,
            daily_steps: triage.daily_steps,
            workouts: triage.workout_tally,
            duplicates_in_file: triage.duplicates_in_file,
            conflicts: triage.conflicts,
            other_time_zone: triage.other_time_zone,
            older_export: triage.older_export,
            edited_since_import: triage.edited_since_import,
            overlaps: triage.overlaps,
            rejected: export.rejected,
            export_failures: export.failures,
            ..ImportReport::default()
        };

        let through = export.through.map(|date| date.to_string());
        let through = through.as_deref();
        let mut resources = Vec::new();
        let mut stale = Vec::new();
        for (day, action) in &triage.days {
            let converted = vec![convert::day(day, &patient, through)];
            if self.keep_valid(&day.date, &converted, &mut report).await {
                report.daily_steps.count(*action);
                if *action == Action::Changed {
                    report.changed_days.push(day.date.clone());
                }
                resources.extend(converted);
            }
        }
        for (workout, action) in &triage.workouts {
            let converted = convert::workout(workout, &patient, through);
            if self
                .keep_valid(&workout.uuid, &converted, &mut report)
                .await
            {
                report.workouts.count(*action);
                if *action == Action::Changed {
                    stale.extend(stale_members(&workout.uuid, &converted));
                }
                resources.extend(converted);
            }
        }

        let reference_issues = self.reference_issues(&resources)?;
        if !reference_issues.is_empty() {
            return Err(PhvError::Conflict(reference_issues));
        }
        let (removals, kept) = self.removable(stale, &resources)?;
        report.kept_members = kept;

        let mut changes: Vec<Change> = resources
            .iter()
            .map(|resource| Change::Upsert {
                resource_type: "Observation".into(),
                id: resource["id"].as_str().unwrap_or("").to_string(),
                line: canon_resource(resource),
            })
            .collect();
        changes.extend(removals);
        report.resources_written = changes.len();
        if options.dry_run {
            self.vault().check_changes(&changes)?;
        } else {
            self.vault().apply(&changes)?;
        }
        Ok(report)
    }

    /// `Patient/<id>` for the vault's one Patient. An import stops without one, because
    /// the IG requires a subject.
    fn only_patient(&self) -> Result<String, PhvError> {
        let lines = self.vault().lines("Patient")?;
        let ids: Vec<&str> = lines.iter().filter_map(|l| line_id(l)).collect();
        match ids.as_slice() {
            [id] => Ok(format!("Patient/{id}")),
            [] => Err(PhvError::Usage(
                "this vault has no Patient, and activity records need one: add the person first"
                    .into(),
            )),
            _ => Err(PhvError::Usage(
                "this vault has more than one Patient: run phv check".into(),
            )),
        }
    }

    /// Validates one row's resources together. Errors reject that row alone, and a
    /// workout is never written without all its members.
    async fn keep_valid(&self, key: &str, resources: &[Value], report: &mut ImportReport) -> bool {
        let mut errors = Vec::new();
        for resource in resources {
            for issue in self.validator.validate(resource).await {
                if issue.is_error() {
                    let text = issue.to_string();
                    if !errors.contains(&text) {
                        errors.push(text);
                    }
                } else if !report.warnings.contains(&issue) {
                    report.warnings.push(issue);
                }
            }
        }
        if errors.is_empty() {
            return true;
        }
        report.rejected.push(Rejected {
            key: key.to_string(),
            message: errors.join("; "),
        });
        false
    }
}

/// A measure a changed workout no longer has.
struct StaleMember {
    workout: String,
    measure: &'static str,
    id: String,
}

/// The measures a changed workout no longer has.
fn stale_members(uuid: &str, converted: &[Value]) -> Vec<StaleMember> {
    MEASURE_CODES
        .iter()
        .map(|&measure| StaleMember {
            workout: uuid.to_string(),
            measure,
            id: convert::stable_id(WORKOUT_MEASURE_SYSTEM, &convert::member_key(uuid, measure)),
        })
        .filter(|member| !converted.iter().any(|r| r["id"] == member.id.as_str()))
        .collect()
}

impl Phv {
    /// Splits stale members into removals and the ones another record references, which
    /// stay, as `phv rm` would refuse to remove them. The stored copies of `written`
    /// are ignored, since the import is replacing them: a changed panel stops listing
    /// the members it drops.
    fn removable(
        &self,
        stale: Vec<StaleMember>,
        written: &[Value],
    ) -> Result<(Vec<Change>, Vec<KeptMember>), PhvError> {
        // A member that is not stored cannot be referenced from a valid vault.
        let mut stored = Vec::new();
        let mut absent = Vec::new();
        for member in stale {
            if self.vault().get("Observation", &member.id)?.is_some() {
                stored.push(member);
            } else {
                absent.push(member);
            }
        }

        let mut users: Vec<Vec<String>> = vec![Vec::new(); stored.len()];
        if !stored.is_empty() {
            let replaced: Vec<&str> = written.iter().filter_map(|r| r["id"].as_str()).collect();
            for resource_type in self.vault().resource_types()? {
                for (position, line) in self.vault().lines(&resource_type)?.iter().enumerate() {
                    // Cheap test first: any reference to a member contains its ID.
                    if !stored.iter().any(|m| line.contains(&m.id)) {
                        continue;
                    }
                    if resource_type == "Observation"
                        && line_id(line).is_some_and(|id| replaced.contains(&id))
                    {
                        continue;
                    }
                    // Read as JSON, so spacing and escapes like `\/` cannot hide a reference.
                    let Ok(value) = parse_json(line) else {
                        return Err(PhvError::Conflict(vec![Issue::error(
                            "PHV-LINE",
                            format!("records/{resource_type}.ndjson:{}", position + 1),
                            "this line is not valid JSON, so phv cannot tell whether it references a workout measure the import would remove: fix the line first",
                        )]));
                    };
                    let refs = collect_refs(&value);
                    for (member, users) in stored.iter().zip(users.iter_mut()) {
                        if refs.contains(&format!("Observation/{}", member.id)) {
                            users.push(format!("{resource_type}/{}", line_id(line).unwrap_or("?")));
                        }
                    }
                }
            }
        }

        let mut removals = Vec::new();
        let mut kept = Vec::new();
        for (member, users) in stored.into_iter().zip(users) {
            if users.is_empty() {
                removals.push(member);
            } else {
                kept.push(KeptMember {
                    workout: member.workout,
                    member: format!("Observation/{}", member.id),
                    measure: member.measure.to_string(),
                    referenced_by: users,
                });
            }
        }
        removals.extend(absent);
        let removals = removals
            .into_iter()
            .map(|member| Change::Remove {
                resource_type: "Observation".into(),
                id: member.id,
            })
            .collect();
        Ok((removals, kept))
    }
}
