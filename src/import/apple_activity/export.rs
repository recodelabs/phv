//! Reads an activity file into plain rows. Nothing here knows about FHIR or the vault.

use super::Rejected;
use crate::canon::{canon_value, parse_json, sha256_hex};
use crate::error::PhvError;
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Number, Value};

#[derive(Debug, Clone)]
pub struct DailySteps {
    pub date: String,
    pub steps: Number,
    pub time_zone: String,
    /// SHA-256 of the source row, which says whether a stored record is still current.
    pub row_hash: String,
}

#[derive(Debug, Clone)]
pub struct Workout {
    /// Uppercase, so one workout has one key.
    pub uuid: String,
    pub activity_type: String,
    pub start_text: String,
    pub end_text: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub duration_seconds: f64,
    pub energy_kcal: Option<Number>,
    pub distance_meters: Option<Number>,
    pub source_name: String,
    pub time_zone: Option<String>,
    pub row_hash: String,
}

#[derive(Debug, Default)]
pub struct Export {
    pub days: Vec<DailySteps>,
    pub workouts: Vec<Workout>,
    pub rejected: Vec<Rejected>,
    pub failures: Vec<Value>,
    /// The summary's `through` date, which says how new the export is.
    pub through: Option<NaiveDate>,
}

fn refuse(message: impl Into<String>) -> PhvError {
    PhvError::Usage(message.into())
}

/// Reads the whole file, or refuses it when it cannot be trusted as a complete export.
/// A row with a bad value is only set aside in `rejected`.
pub fn parse(text: &str) -> Result<Export, PhvError> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut export = Export::default();
    let mut summary = None;
    let mut seen_days = 0u64;
    let mut seen_workouts = 0u64;

    for (index, line) in lines.iter().enumerate() {
        let number = index + 1;
        let row = parse_json(line).map_err(|e| {
            refuse(format!(
                "line {number} of the export is not valid JSON: {e}"
            ))
        })?;
        if row.get("v").and_then(Value::as_u64) != Some(1) {
            return Err(refuse(format!(
                "line {number} of the export is not format version 1: upgrade phv or export again"
            )));
        }
        match row.get("kind").and_then(Value::as_str) {
            Some("dailySteps") => {
                seen_days += 1;
                match daily_steps(&row) {
                    Ok(day) => export.days.push(day),
                    Err(message) => export
                        .rejected
                        .push(rejected(&row, "date", number, message)),
                }
            }
            Some("workout") => {
                seen_workouts += 1;
                match workout(&row) {
                    Ok(workout) => export.workouts.push(workout),
                    Err(message) => export
                        .rejected
                        .push(rejected(&row, "uuid", number, message)),
                }
            }
            Some("summary") => {
                if index + 1 != lines.len() {
                    return Err(refuse(format!(
                        "line {number} of the export is a summary, which belongs only on the last line: is this more than one export joined together?"
                    )));
                }
                summary = Some(row);
            }
            other => {
                return Err(refuse(format!(
                    "line {number} of the export has an unknown kind {other:?}"
                )));
            }
        }
    }

    let summary = summary.ok_or_else(|| {
        refuse("the export has no summary line, so it was cut off: export it again")
    })?;
    let counted = |name: &str| summary["counts"][name].as_u64();
    if counted("dailySteps") != Some(seen_days) || counted("workouts") != Some(seen_workouts) {
        return Err(refuse(format!(
            "the export's summary does not match its rows ({seen_days} days and {seen_workouts} workouts found): export it again"
        )));
    }
    // Absent is allowed, but then the import cannot tell an old file from a new one.
    if let Some(through) = summary.get("through") {
        let parsed = through
            .as_str()
            .filter(|t| t.len() == 10)
            .and_then(|t| NaiveDate::parse_from_str(t, "%Y-%m-%d").ok());
        export.through = Some(parsed.ok_or_else(|| {
            refuse(format!(
                "the export's summary has a through date {through} that is not a date: export it again"
            ))
        })?);
    }
    if let Some(failures) = summary["failures"].as_array() {
        export.failures = failures.clone();
    }
    Ok(export)
}

fn rejected(row: &Value, key_field: &str, number: usize, message: String) -> Rejected {
    let key = row
        .get(key_field)
        .and_then(Value::as_str)
        .map_or_else(|| format!("line {number}"), str::to_string);
    Rejected { key, message }
}

fn text_field(row: &Value, name: &str) -> Result<String, String> {
    match row.get(name).and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty() => Ok(text.to_string()),
        _ => Err(format!("{name} is missing or not text")),
    }
}

/// A time zone name, which is written as a FHIR `code` inside an extension. The vault's
/// validator does not check values inside extensions, so this is checked here.
fn time_zone(text: String) -> Result<String, String> {
    if text.chars().any(char::is_whitespace) {
        return Err(format!("{text:?} is not a time zone name"));
    }
    Ok(text)
}

/// An optional number, which must not be negative.
fn number_field(row: &Value, name: &str) -> Result<Option<Number>, String> {
    let Some(value) = row.get(name) else {
        return Ok(None);
    };
    let number = value
        .as_number()
        .ok_or_else(|| format!("{name} is not a number"))?;
    match number.as_f64() {
        Some(n) if n.is_finite() && n >= 0.0 => Ok(Some(number.clone())),
        _ => Err(format!("{name} is negative or out of range")),
    }
}

fn row_hash(row: &Value) -> String {
    sha256_hex(canon_value(row).as_bytes())
}

fn daily_steps(row: &Value) -> Result<DailySteps, String> {
    let date = text_field(row, "date")?;
    let parsed = NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok();
    if date.len() != 10 || parsed.is_none() {
        return Err(format!("{date} is not a date"));
    }
    let steps = number_field(row, "steps")?.ok_or("steps is missing")?;
    if steps.as_u64().is_none() {
        return Err("steps is not a whole number".into());
    }
    Ok(DailySteps {
        date,
        steps,
        time_zone: time_zone(text_field(row, "timeZone")?)?,
        row_hash: row_hash(row),
    })
}

fn time_field(row: &Value, name: &str) -> Result<(String, DateTime<Utc>), String> {
    let text = text_field(row, name)?;
    let time = DateTime::parse_from_rfc3339(&text)
        .map_err(|_| format!("{name} {text} is not a date and time"))?
        .with_timezone(&Utc);
    Ok((text, time))
}

fn workout(row: &Value) -> Result<Workout, String> {
    let (start_text, start) = time_field(row, "start")?;
    let (end_text, end) = time_field(row, "end")?;
    if end < start {
        return Err("the workout ends before it starts".into());
    }
    let duration = number_field(row, "durationSeconds")?.ok_or("durationSeconds is missing")?;
    Ok(Workout {
        uuid: text_field(row, "uuid")?.to_uppercase(),
        activity_type: text_field(row, "activityType")?,
        start_text,
        end_text,
        start,
        end,
        duration_seconds: duration.as_f64().ok_or("durationSeconds is out of range")?,
        // HealthKit reports 0 for a workout with no energy data, which is not a measurement.
        energy_kcal: number_field(row, "energyKcal")?.filter(|n| n.as_f64() != Some(0.0)),
        distance_meters: number_field(row, "distanceMeters")?,
        source_name: text_field(row, "sourceName")?,
        time_zone: row
            .get("timeZone")
            .map(|_| text_field(row, "timeZone").and_then(time_zone))
            .transpose()?,
        row_hash: row_hash(row),
    })
}
