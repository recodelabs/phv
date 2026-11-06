//! Imports an activity file from the HealthKit export app.
//!
//! See docs/superpowers/specs/2026-11-06-apple-activity-import-design.md.

mod convert;
mod export;
mod triage;

use crate::canon::canon_resource;
use crate::error::{Issue, PhvError};
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
            overlaps: triage.overlaps,
            rejected: export.rejected,
            export_failures: export.failures,
            ..ImportReport::default()
        };

        let mut resources = Vec::new();
        let mut removals = Vec::new();
        for (day, action) in &triage.days {
            let converted = vec![convert::day(day, &patient)];
            if self.keep_valid(&day.date, &converted, &mut report).await {
                report.daily_steps.count(*action);
                if *action == Action::Changed {
                    report.changed_days.push(day.date.clone());
                }
                resources.extend(converted);
            }
        }
        for (workout, action) in &triage.workouts {
            let converted = convert::workout(workout, &patient);
            if self
                .keep_valid(&workout.uuid, &converted, &mut report)
                .await
            {
                report.workouts.count(*action);
                if *action == Action::Changed {
                    removals.extend(stale_members(&workout.uuid, &converted));
                }
                resources.extend(converted);
            }
        }

        let reference_issues = self.reference_issues(&resources)?;
        if !reference_issues.is_empty() {
            return Err(PhvError::Conflict(reference_issues));
        }

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

/// Removals for the measures a changed workout no longer has.
fn stale_members(uuid: &str, converted: &[Value]) -> Vec<Change> {
    MEASURE_CODES
        .iter()
        .map(|code| convert::stable_id(WORKOUT_MEASURE_SYSTEM, &convert::member_key(uuid, code)))
        .filter(|id| !converted.iter().any(|r| r["id"] == id.as_str()))
        .map(|id| Change::Remove {
            resource_type: "Observation".into(),
            id,
        })
        .collect()
}
