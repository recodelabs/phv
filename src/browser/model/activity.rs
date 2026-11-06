//! The Activity topic: daily steps and workouts from the Physical Activity IG.
//!
//! Every other topic shows values as stored. These totals and friendly units are worked out
//! here, from the vault's files on each request. The exact values stay in the FHIR view.

use super::observation::is_entered_in_error;
use super::value::{
    Dated, LOINC, OBSERVATION_CATEGORY, concept_name, date_or_none, encode_segment, friendly_date,
    has_code, id_of, newest_first, number_text, quantity_text,
};
use crate::refs::local_target;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

const STEPS: &str = "41950-7";
const PANEL: &str = "PAPanel";
const ACTIVITY_TYPE: &str = "73985-4";
const DURATION: &str = "55411-3";
const CALORIES: &str = "55424-6";
const DISTANCE: &str = "55412-1";
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const NO_DATA: &str = "No data";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Period {
    All,
    Year(String),
    Month(String, u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub label: String,
    /// The query string to link to. `None` for the page you are on.
    pub query: Option<String>,
}

impl Period {
    /// `None` when the values are not a real year and month.
    pub fn parse(year: Option<&str>, month: Option<&str>) -> Option<Period> {
        let is_year = |y: &str| y.len() == 4 && y.bytes().all(|b| b.is_ascii_digit());
        match (year, month) {
            (None, None) => Some(Period::All),
            (Some(y), None) if is_year(y) => Some(Period::Year(y.to_string())),
            (Some(y), Some(m)) if is_year(y) => {
                let month: u32 = m.parse().ok()?;
                (1..=12)
                    .contains(&month)
                    .then(|| Period::Month(y.to_string(), month))
            }
            _ => None,
        }
    }

    pub fn title(&self) -> String {
        match self {
            Period::All => "All years".to_string(),
            Period::Year(year) => year.clone(),
            Period::Month(year, month) => format!("{} {year}", MONTHS[*month as usize - 1]),
        }
    }

    pub fn query(&self) -> String {
        match self {
            Period::All => String::new(),
            Period::Year(year) => format!("year={year}"),
            Period::Month(year, month) => format!("year={year}&month={month}"),
        }
    }

    /// Whether a `YYYY-MM-DD` date, as stored, falls in this period.
    pub fn contains(&self, date: &str) -> bool {
        match self {
            Period::All => true,
            Period::Year(year) => date.starts_with(&format!("{year}-")),
            Period::Month(year, month) => date.starts_with(&format!("{year}-{month:02}-")),
        }
    }

    pub fn crumbs(&self) -> Vec<Crumb> {
        let mut crumbs = vec![Crumb {
            label: Period::All.title(),
            query: Some(String::new()),
        }];
        if let Period::Year(year) | Period::Month(year, _) = self {
            crumbs.push(Crumb {
                label: year.clone(),
                query: Some(format!("year={year}")),
            });
        }
        if let Period::Month(_, month) = self {
            crumbs.push(Crumb {
                label: MONTHS[*month as usize - 1].to_string(),
                query: None,
            });
        }
        if let Some(last) = crumbs.last_mut() {
            last.query = None;
        }
        crumbs
    }
}

/// `1234567` as `1,234,567`.
pub fn grouped(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn grouped_tenths(number: f64) -> String {
    let text = format!("{number:.1}");
    let (whole, tenth) = text.split_once('.').unwrap_or((&text, "0"));
    format!(
        "{}.{tenth}",
        grouped(whole.parse::<u64>().unwrap_or_default())
    )
}

/// `72.0` minutes as `1 h 12 min`.
pub fn friendly_duration(minutes: f64) -> String {
    let whole = minutes.round() as u64;
    match (whole / 60, whole % 60) {
        (0, min) => format!("{min} min"),
        (hours, 0) => format!("{} h", grouped(hours)),
        (hours, min) => format!("{} h {min} min", grouped(hours)),
    }
}

fn is_activity(resource: &Value) -> bool {
    has_code(&resource["category"], OBSERVATION_CATEGORY, "activity")
}

fn is_step_record(resource: &Value) -> bool {
    is_activity(resource) && has_code(&resource["code"], LOINC, STEPS)
}

fn is_panel(resource: &Value) -> bool {
    is_activity(resource)
        && resource["code"]["coding"]
            .as_array()
            .is_some_and(|codings| codings.iter().any(|c| c["code"] == PANEL))
}

/// Whether the Activity page shows this record, directly or as part of a workout.
pub fn is_shown(resource: &Value) -> bool {
    is_step_record(resource)
        || is_panel(resource)
        || (is_activity(resource)
            && [ACTIVITY_TYPE, DURATION, CALORIES, DISTANCE]
                .iter()
                .any(|code| has_code(&resource["code"], LOINC, code)))
}

/// The `YYYY-MM-DD` start of a date as written, with no time zone conversion.
fn day_of(written: &str) -> Option<String> {
    let day = written.get(..10)?;
    let bytes = day.as_bytes();
    let shaped = bytes.iter().enumerate().all(|(i, b)| match i {
        4 | 7 => *b == b'-',
        _ => b.is_ascii_digit(),
    });
    shaped.then(|| day.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepDay {
    pub date: String,
    pub steps: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepData {
    /// One entry per recorded day, oldest first. A day with no record has no entry.
    pub days: Vec<StepDay>,
    /// Days with more than one record. The largest is used, so sources never double count.
    pub duplicate_days: usize,
    /// Records with no usable date or count, which are not guessed at.
    pub unusable: usize,
}

pub fn step_days(observations: &[Value]) -> StepData {
    let mut by_day: BTreeMap<String, u64> = BTreeMap::new();
    let mut data = StepData::default();
    let records = observations
        .iter()
        .filter(|o| is_step_record(o) && !is_entered_in_error(o));
    for record in records {
        let day = record["effectiveDateTime"].as_str().and_then(day_of);
        let steps = number_text(&record["valueQuantity"]["value"]).and_then(|n| n.parse().ok());
        let (Some(day), Some(steps)) = (day, steps) else {
            data.unusable += 1;
            continue;
        };
        match by_day.get_mut(&day) {
            Some(stored) => {
                data.duplicate_days += 1;
                *stored = (*stored).max(steps);
            }
            None => {
                by_day.insert(day, steps);
            }
        }
    }
    data.days = by_day
        .into_iter()
        .map(|(date, steps)| StepDay { date, steps })
        .collect();
    data
}

/// One bar and table row: a year, a month or a day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bucket {
    pub label: String,
    /// The label in full, for the chart's tooltip.
    pub title: String,
    /// The query string that drills into this bucket, when there is data to see.
    pub query: Option<String>,
    /// The chart's number. `None` means no records, which is not zero.
    pub value: Option<String>,
    pub text: String,
}

fn days_in_month(year: &str, month: u32) -> u32 {
    let year: u32 = year.parse().unwrap_or_default();
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Sums `(date, amount)` points into buckets for a period, leaving gaps as gaps.
fn buckets(
    points: &[(String, f64)],
    period: &Period,
    show: impl Fn(f64) -> (String, String),
) -> Vec<Bucket> {
    // label, title, date prefix, drill-down query
    let slots: Vec<(String, String, String, Option<String>)> = match period {
        Period::All => {
            let years = || {
                points
                    .iter()
                    .filter_map(|(date, _)| date.get(..4)?.parse::<u32>().ok())
            };
            match (years().min(), years().max()) {
                (Some(first), Some(last)) => (first..=last)
                    .map(|y| {
                        let y = y.to_string();
                        (
                            y.clone(),
                            y.clone(),
                            format!("{y}-"),
                            Some(format!("year={y}")),
                        )
                    })
                    .collect(),
                _ => Vec::new(),
            }
        }
        Period::Year(year) => (1..=12)
            .map(|m| {
                let label = MONTHS[m as usize - 1];
                (
                    label.to_string(),
                    format!("{label} {year}"),
                    format!("{year}-{m:02}-"),
                    Some(format!("year={year}&month={m}")),
                )
            })
            .collect(),
        Period::Month(year, month) => (1..=days_in_month(year, *month))
            .map(|d| {
                (
                    d.to_string(),
                    format!("{d} {} {year}", MONTHS[*month as usize - 1]),
                    format!("{year}-{month:02}-{d:02}"),
                    None,
                )
            })
            .collect(),
    };
    slots
        .into_iter()
        .map(|(label, title, prefix, query)| {
            let matching: Vec<f64> = points
                .iter()
                .filter(|(date, _)| date.starts_with(&prefix))
                .map(|(_, amount)| *amount)
                .collect();
            if matching.is_empty() {
                return Bucket {
                    label,
                    title,
                    query: None,
                    value: None,
                    text: NO_DATA.to_string(),
                };
            }
            let (value, text) = show(matching.iter().sum());
            Bucket {
                label,
                title,
                query,
                value: Some(value),
                text,
            }
        })
        .collect()
}

pub fn buckets_of_steps(days: &[StepDay], period: &Period) -> Vec<Bucket> {
    let points: Vec<(String, f64)> = days
        .iter()
        .map(|d| (d.date.clone(), d.steps as f64))
        .collect();
    buckets(&points, period, |total| {
        let total = total as u64;
        (total.to_string(), grouped(total))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSummary {
    pub total: String,
    /// Per recorded day, so a day with no record does not lower it.
    pub average: String,
    pub days: String,
}

pub fn summary(days: &[StepDay], period: &Period) -> StepSummary {
    let shown: Vec<&StepDay> = days.iter().filter(|d| period.contains(&d.date)).collect();
    let total: u64 = shown.iter().map(|d| d.steps).sum();
    let average = if shown.is_empty() {
        0
    } else {
        (total as f64 / shown.len() as f64).round() as u64
    };
    StepSummary {
        total: grouped(total),
        average: grouped(average),
        days: grouped(shown.len() as u64),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestSteps {
    pub steps: String,
    pub date: String,
}

pub fn latest_steps(days: &[StepDay]) -> Option<LatestSteps> {
    days.last().map(|day| LatestSteps {
        steps: grouped(day.steps),
        date: friendly_date(&day.date),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workout {
    pub id: String,
    pub id_url: String,
    raw_start: Option<String>,
    /// The `YYYY-MM-DD` the workout started on, as written.
    pub day: Option<String>,
    pub date: String,
    pub activity: String,
    pub minutes: Option<f64>,
    pub meters: Option<f64>,
    pub duration: Option<String>,
    pub distance: Option<String>,
    pub calories: Option<String>,
}

impl Dated for Workout {
    fn sort_date(&self) -> Option<&str> {
        self.raw_start.as_deref()
    }
    fn sort_id(&self) -> &str {
        &self.id
    }
}

/// The amount when the unit is the expected UCUM code, and the stored text either way.
fn measure(member: Option<&&Value>, unit_code: &str) -> (Option<f64>, Option<String>) {
    let Some(quantity) = member.map(|m| &m["valueQuantity"]) else {
        return (None, None);
    };
    let amount = (quantity["code"] == unit_code)
        .then(|| quantity["value"].as_f64())
        .flatten();
    (amount, quantity_text(quantity))
}

/// Every workout panel with its measures, newest first.
pub fn workouts(observations: &[Value]) -> Vec<Workout> {
    let index: HashMap<String, &Value> = observations.iter().map(|o| (id_of(o), o)).collect();
    let mut rows: Vec<Workout> = observations
        .iter()
        .filter(|o| is_panel(o) && !is_entered_in_error(o))
        .map(|panel| {
            let members: Vec<&Value> = panel["hasMember"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| m["reference"].as_str())
                .filter_map(local_target)
                .filter(|(kind, _)| *kind == "Observation")
                .filter_map(|(_, id)| index.get(id).copied())
                .filter(|member| !is_entered_in_error(member))
                .collect();
            let member = |code: &str| members.iter().find(|m| has_code(&m["code"], LOINC, code));
            let raw_start = panel["effectivePeriod"]["start"]
                .as_str()
                .map(str::to_string);
            let day = raw_start.as_deref().and_then(day_of);
            let (minutes, duration_text) = measure(member(DURATION), "min");
            let (meters, distance_text) = measure(member(DISTANCE), "m");
            let (kcal, calories_text) = measure(member(CALORIES), "kcal");
            Workout {
                id: id_of(panel),
                id_url: encode_segment(&id_of(panel)),
                date: date_or_none(day.as_deref()),
                raw_start,
                day,
                activity: member(ACTIVITY_TYPE)
                    .and_then(|m| concept_name(&m["valueCodeableConcept"]))
                    .unwrap_or_else(|| "Workout".to_string()),
                minutes,
                meters,
                duration: minutes.map(friendly_duration).or(duration_text),
                distance: meters
                    .map(|m| format!("{:.2} km", m / 1000.0))
                    .or(distance_text),
                calories: kcal
                    .map(|k| format!("{} kcal", grouped(k.round() as u64)))
                    .or(calories_text),
            }
        })
        .collect();
    newest_first(&mut rows);
    rows
}

pub fn in_period<'a>(all: &'a [Workout], period: &Period) -> Vec<&'a Workout> {
    all.iter()
        .filter(|w| match (&w.day, period) {
            (_, Period::All) => true,
            (Some(day), period) => period.contains(day),
            (None, _) => false,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRow {
    pub name: String,
    pub name_url: String,
    pub count: usize,
    pub distance: Option<String>,
    /// Kilometers for the chart. `None` when no workout of this kind recorded a distance.
    pub chart_km: Option<String>,
    pub time: String,
}

/// Totals for each kind of workout in a period, by distance and then by count.
pub fn type_rows(all: &[Workout], period: &Period) -> Vec<TypeRow> {
    let mut totals: BTreeMap<&str, (usize, f64, f64)> = BTreeMap::new();
    for workout in in_period(all, period) {
        let entry = totals.entry(&workout.activity).or_default();
        entry.0 += 1;
        entry.1 += workout.meters.unwrap_or_default();
        entry.2 += workout.minutes.unwrap_or_default();
    }
    let mut sorted: Vec<(&str, (usize, f64, f64))> = totals.into_iter().collect();
    sorted.sort_by(|a, b| {
        b.1.1
            .total_cmp(&a.1.1)
            .then(b.1.0.cmp(&a.1.0))
            .then(a.0.cmp(b.0))
    });
    sorted
        .into_iter()
        .map(|(name, (count, meters, minutes))| {
            let km = meters / 1000.0;
            TypeRow {
                name: name.to_string(),
                name_url: encode_segment(name),
                count,
                distance: (meters > 0.0).then(|| format!("{} km", grouped_tenths(km))),
                chart_km: (meters > 0.0).then(|| format!("{km:.1}")),
                time: friendly_duration(minutes),
            }
        })
        .collect()
}

/// One kind of workout's distance over a period.
pub fn buckets_of_distance(all: &[Workout], activity: &str, period: &Period) -> Vec<Bucket> {
    let points: Vec<(String, f64)> = all
        .iter()
        .filter(|w| w.activity == activity)
        .filter_map(|w| Some((w.day.clone()?, w.meters? / 1000.0)))
        .collect();
    buckets(&points, period, |km| {
        (format!("{km:.1}"), format!("{} km", grouped_tenths(km)))
    })
}
