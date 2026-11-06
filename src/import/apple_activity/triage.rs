//! Decides which rows are worth converting, before any FHIR is built.

use super::convert::{
    ACTIVITY_TYPE_SYSTEM, CODE_ACTIVITY_TYPE, DAILY_STEPS_SYSTEM, EXPORT_THROUGH_SYSTEM,
    SOURCE_ROW_SYSTEM, TZ_CODE, WORKOUT_MEASURE_SYSTEM, WORKOUT_SYSTEM, member_key,
};
use super::export::{DailySteps, Export, Workout};
use super::{EditedHold, OlderExportHold, Overlap, Tally, TimeZoneHold};
use crate::canon::parse_json;
use crate::error::{Issue, PhvError};
use crate::store::line_id;
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
struct StoredDay {
    row_hash: Option<String>,
    time_zone: Option<String>,
}

#[derive(Debug, Clone)]
struct StoredWorkout {
    row_hash: Option<String>,
    session: Session,
}

/// What the importer has already written, read back from the vault's Observations.
#[derive(Debug, Default)]
pub struct VaultIndex {
    days: HashMap<String, StoredDay>,
    workouts: HashMap<String, StoredWorkout>,
    /// The newest export date tagged on any stored day or workout. Records stored before
    /// the date was tagged have none, so a vault of only those has no date to compare.
    through: Option<NaiveDate>,
}

/// What it takes to tell whether two workouts are one session.
#[derive(Debug, Clone)]
struct Session {
    uuid: String,
    activity_type: String,
    source: String,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

impl Session {
    fn of(workout: &Workout) -> Self {
        Self {
            uuid: workout.uuid.clone(),
            activity_type: workout.activity_type.clone(),
            source: workout.source_name.clone(),
            start: workout.start,
            end: workout.end,
        }
    }

    /// Same activity, different sources, overlapping for at least half of the shorter one.
    fn is_same_as(&self, other: &Session) -> bool {
        if self.activity_type != other.activity_type || self.source == other.source {
            return false;
        }
        let overlap = (self.end.min(other.end) - self.start.max(other.start)).num_milliseconds();
        let shorter = (self.end - self.start)
            .num_milliseconds()
            .min((other.end - other.start).num_milliseconds());
        overlap > 0 && overlap * 2 >= shorter
    }
}

fn tag_code(resource: &Value, system: &str) -> Option<String> {
    resource["meta"]["tag"]
        .as_array()?
        .iter()
        .find(|tag| tag["system"] == system)?["code"]
        .as_str()
        .map(str::to_string)
}

fn row_hash_of(resource: &Value) -> Option<String> {
    tag_code(resource, SOURCE_ROW_SYSTEM)
}

fn through_of(resource: &Value) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(&tag_code(resource, EXPORT_THROUGH_SYSTEM)?, "%Y-%m-%d").ok()
}

fn time_zone_of(extension_holder: &Value) -> Option<String> {
    extension_holder["extension"]
        .as_array()?
        .iter()
        .find(|e| e["url"] == TZ_CODE)?["valueCode"]
        .as_str()
        .map(str::to_string)
}

fn identifier_in<'a>(resource: &'a Value, system: &str) -> Option<&'a str> {
    resource["identifier"]
        .as_array()?
        .iter()
        .find(|i| i["system"] == system)?["value"]
        .as_str()
}

fn time(resource: &Value, name: &str) -> Option<DateTime<Utc>> {
    let text = resource["effectivePeriod"][name].as_str()?;
    Some(DateTime::parse_from_rfc3339(text).ok()?.with_timezone(&Utc))
}

impl VaultIndex {
    /// Reads the lines this importer wrote and leaves every other line unparsed.
    pub fn from_lines(lines: &[String]) -> Result<Self, PhvError> {
        let mut index = Self::default();
        let mut activity_types: HashMap<String, String> = HashMap::new();
        let mut panels = Vec::new();

        for line in lines.iter().filter(|l| l.contains("urn:phv:apple-health:")) {
            let resource = parse_json(line).map_err(|e| {
                PhvError::Conflict(vec![Issue::error(
                    "PHV-LINE",
                    format!("Observation/{}", line_id(line).unwrap_or("?")),
                    format!("this stored line cannot be read: {e}"),
                )])
            })?;
            if identifier_in(&resource, DAILY_STEPS_SYSTEM).is_some()
                || identifier_in(&resource, WORKOUT_SYSTEM).is_some()
            {
                index.through = index.through.max(through_of(&resource));
            }
            if let Some(date) = identifier_in(&resource, DAILY_STEPS_SYSTEM) {
                let stored = StoredDay {
                    row_hash: row_hash_of(&resource),
                    time_zone: time_zone_of(&resource["_effectiveDateTime"]),
                };
                index.days.insert(date.to_string(), stored);
            } else if let Some(key) = identifier_in(&resource, WORKOUT_MEASURE_SYSTEM) {
                let name = resource["valueCodeableConcept"]["coding"]
                    .as_array()
                    .and_then(|c| c.iter().find(|c| c["system"] == ACTIVITY_TYPE_SYSTEM))
                    .and_then(|c| c["code"].as_str());
                if let Some(name) = name {
                    activity_types.insert(key.to_string(), name.to_string());
                }
            } else if identifier_in(&resource, WORKOUT_SYSTEM).is_some() {
                panels.push(resource);
            }
        }

        for panel in panels {
            let uuid = identifier_in(&panel, WORKOUT_SYSTEM)
                .unwrap_or("")
                .to_string();
            let start = time(&panel, "start");
            let end = time(&panel, "end");
            let (Some(start), Some(end)) = (start, end) else {
                continue;
            };
            let session = Session {
                activity_type: activity_types
                    .get(&member_key(&uuid, CODE_ACTIVITY_TYPE))
                    .cloned()
                    .unwrap_or_default(),
                source: panel["device"]["display"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                start,
                end,
                uuid: uuid.clone(),
            };
            let stored = StoredWorkout {
                row_hash: row_hash_of(&panel),
                session,
            };
            index.workouts.insert(uuid, stored);
        }
        Ok(index)
    }
}

/// Why a row is going on to the converter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    New,
    Changed,
}

/// Rows left after triage. The tallies count only what triage skipped, and the caller adds
/// each row in `days` and `workouts` that it goes on to write.
#[derive(Debug, Default)]
pub struct Triage {
    /// New and changed rows to convert.
    pub days: Vec<(DailySteps, Action)>,
    pub workouts: Vec<(Workout, Action)>,
    pub daily_steps: Tally,
    pub workout_tally: Tally,
    pub duplicates_in_file: usize,
    pub conflicts: Vec<String>,
    pub other_time_zone: Vec<TimeZoneHold>,
    pub older_export: Vec<OlderExportHold>,
    pub edited_since_import: Vec<EditedHold>,
    pub overlaps: Vec<Overlap>,
}

/// How a row compares with the record stored for its key.
enum Stored {
    /// Someone replaced the record by hand, so it no longer says which row it came from.
    Edited,
    Unchanged,
    /// The row differs, and the file is older than the newest export the vault holds.
    Older,
    Differs,
}

fn judge(stored_hash: Option<&str>, row_hash: &str, file_is_older: bool) -> Stored {
    match stored_hash {
        None => Stored::Edited,
        Some(hash) if hash == row_hash => Stored::Unchanged,
        Some(_) if file_is_older => Stored::Older,
        Some(_) => Stored::Differs,
    }
}

/// One row per key. Identical repeats collapse. A key with different rows is set aside
/// and returned in `conflicts`, since nothing says which is right.
fn unique<T: Clone>(
    rows: &[T],
    key: impl Fn(&T) -> &str,
    hash: impl Fn(&T) -> &str,
    duplicates: &mut usize,
    conflicts: &mut Vec<String>,
) -> Vec<T> {
    let mut by_key: BTreeMap<&str, Vec<&T>> = BTreeMap::new();
    for row in rows {
        by_key.entry(key(row)).or_default().push(row);
    }
    let mut out = Vec::with_capacity(by_key.len());
    for (key, group) in by_key {
        *duplicates += group
            .iter()
            .filter(|row| hash(row) == hash(group[0]))
            .count()
            - 1;
        if group.iter().all(|row| hash(row) == hash(group[0])) {
            out.push(group[0].clone());
        } else {
            conflicts.push(key.to_string());
        }
    }
    out
}

fn optional_measures(workout: &Workout) -> usize {
    usize::from(workout.energy_kcal.is_some()) + usize::from(workout.distance_meters.is_some())
}

/// Best first: most measures, then longest, then lowest UUID, so the order of the rows
/// in the file never decides which is kept.
fn best_first(a: &Workout, b: &Workout) -> std::cmp::Ordering {
    optional_measures(b)
        .cmp(&optional_measures(a))
        .then(b.duration_seconds.total_cmp(&a.duration_seconds))
        .then(a.uuid.cmp(&b.uuid))
}

pub fn triage(export: &Export, index: &VaultIndex, keep_overlaps: bool) -> Triage {
    let mut out = Triage::default();

    let days = unique(
        &export.days,
        |d| d.date.as_str(),
        |d| d.row_hash.as_str(),
        &mut out.duplicates_in_file,
        &mut out.conflicts,
    );
    // Only a file older than the newest export in the vault is held. With no date on either
    // side there is nothing to compare, so the file is taken as the newer one.
    let older_than = match (export.through, index.through) {
        (Some(file), Some(vault)) if file < vault => Some((file, vault)),
        _ => None,
    };
    let older_hold = |kind: &str, key: &str| {
        let (file, vault) = older_than.expect("only called for an older file");
        OlderExportHold {
            kind: kind.to_string(),
            key: key.to_string(),
            file_through: file.to_string(),
            vault_through: vault.to_string(),
        }
    };
    let edited_hold = |kind: &str, key: &str| EditedHold {
        kind: kind.to_string(),
        key: key.to_string(),
    };

    for day in days {
        let Some(stored) = index.days.get(&day.date) else {
            out.days.push((day, Action::New));
            continue;
        };
        match judge(
            stored.row_hash.as_deref(),
            &day.row_hash,
            older_than.is_some(),
        ) {
            Stored::Edited => out.edited_since_import.push(edited_hold("day", &day.date)),
            Stored::Unchanged => out.daily_steps.unchanged += 1,
            Stored::Older => out.older_export.push(older_hold("day", &day.date)),
            Stored::Differs if stored.time_zone.as_deref() != Some(&day.time_zone) => {
                out.other_time_zone.push(TimeZoneHold {
                    date: day.date.clone(),
                    stored_time_zone: stored.time_zone.clone(),
                    exported_time_zone: day.time_zone.clone(),
                });
            }
            Stored::Differs => out.days.push((day, Action::Changed)),
        }
    }

    let workouts = unique(
        &export.workouts,
        |w| w.uuid.as_str(),
        |w| w.row_hash.as_str(),
        &mut out.duplicates_in_file,
        &mut out.conflicts,
    );
    let mut new_workouts = Vec::new();
    for workout in workouts {
        let Some(stored) = index.workouts.get(&workout.uuid) else {
            new_workouts.push(workout);
            continue;
        };
        match judge(
            stored.row_hash.as_deref(),
            &workout.row_hash,
            older_than.is_some(),
        ) {
            Stored::Edited => out
                .edited_since_import
                .push(edited_hold("workout", &workout.uuid)),
            Stored::Unchanged => out.workout_tally.unchanged += 1,
            Stored::Older => out.older_export.push(older_hold("workout", &workout.uuid)),
            Stored::Differs => out.workouts.push((workout, Action::Changed)),
        }
    }

    new_workouts.sort_by(best_first);
    // Sorted so that a stored workout matching several new ones is found the same way each time.
    let mut kept: Vec<Session> = index.workouts.values().map(|w| w.session.clone()).collect();
    kept.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    for workout in new_workouts {
        let session = Session::of(&workout);
        let matched = (!keep_overlaps)
            .then(|| kept.iter().find(|other| session.is_same_as(other)))
            .flatten();
        if let Some(other) = matched {
            out.overlaps.push(Overlap {
                skipped: workout.uuid.clone(),
                skipped_source: workout.source_name.clone(),
                kept: other.uuid.clone(),
                kept_source: other.source.clone(),
                activity_type: workout.activity_type.clone(),
                start: workout.start_text.clone(),
            });
        } else {
            kept.push(session);
            out.workouts.push((workout, Action::New));
        }
    }
    out.overlaps
        .sort_by(|a, b| (&a.start, &a.skipped).cmp(&(&b.start, &b.skipped)));
    out
}
