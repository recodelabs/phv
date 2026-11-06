//! The Overview page: the person, latest vitals, latest lab report, what is active now.

use super::activity::{LatestSteps, latest_steps, step_days};
use super::clinical::{self, ClinicalRow};
use super::labs::{self, ReportRow};
use super::value::friendly_date;
use super::vitals::{self, Measure};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    pub birth_date: Option<String>,
    pub age: Option<String>,
    pub measures: Vec<Measure>,
    pub latest_steps: Option<LatestSteps>,
    pub latest_report: Option<ReportRow>,
    pub conditions: Vec<ClinicalRow>,
    pub medicines: Vec<ClinicalRow>,
    pub allergies: Vec<ClinicalRow>,
}

/// Input for `overview`, one slice per resource type.
pub struct Sources<'a> {
    /// Today as `YYYY-MM-DD`. The clock is read in the route, so this stays deterministic.
    pub today: &'a str,
    pub patients: &'a [Value],
    pub observations: &'a [Value],
    pub reports: &'a [Value],
    pub conditions: &'a [Value],
    pub statements: &'a [Value],
    pub requests: &'a [Value],
    pub medications: &'a [Value],
    pub allergies: &'a [Value],
}

pub fn overview(sources: &Sources) -> Overview {
    let rows = vitals::vital_rows(sources.observations);
    Overview {
        birth_date: sources
            .patients
            .first()
            .and_then(|p| p["birthDate"].as_str().map(friendly_date)),
        age: sources
            .patients
            .first()
            .and_then(|p| age_of(p, sources.today)),
        measures: vitals::measures(&rows),
        latest_steps: latest_steps(&step_days(sources.observations).days),
        latest_report: labs::lab_reports(sources.reports, sources.observations)
            .into_iter()
            .next(),
        conditions: clinical::active_now(&clinical::conditions(sources.conditions)),
        medicines: clinical::active_now(&clinical::medicines(
            sources.statements,
            sources.requests,
            sources.medications,
        )),
        allergies: clinical::active_now(&clinical::allergies(sources.allergies)),
    }
}

/// A person's age today, or `None` if they have died or their birth date makes no sense.
pub fn age_of(patient: &Value, today: &str) -> Option<String> {
    let deceased = patient["deceasedBoolean"] == true || patient["deceasedDateTime"].is_string();
    if deceased {
        return None;
    }
    age_at(patient["birthDate"].as_str()?, today)
}

fn plural(count: i64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

fn parts(date: &str) -> Option<Vec<i64>> {
    let parts: Vec<i64> = date
        .split('-')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    let valid = match parts.as_slice() {
        [_] => true,
        [_, month] => (1..=12).contains(month),
        [_, month, day] => (1..=12).contains(month) && (1..=31).contains(day),
        _ => false,
    };
    valid.then_some(parts)
}

/// Days since an arbitrary fixed date, for counting the days between two dates.
fn day_number(year: i64, month: i64, day: i64) -> i64 {
    let (y, m) = if month <= 2 {
        (year - 1, month + 12)
    } else {
        (year, month)
    };
    365 * y + y / 4 - y / 100 + y / 400 + (153 * (m - 3) + 2) / 5 + day
}

/// Days for a newborn, months up to 2, years and months up to 18, then years.
pub fn age_at(birth: &str, today: &str) -> Option<String> {
    let born = parts(birth)?;
    let now = parts(today)?;
    let [now_year, now_month, now_day] = now.as_slice() else {
        return None;
    };
    match born.as_slice() {
        // A partial date only supports whole years, and only once they are enough to mean much.
        [year] | [year, _] => {
            let mut years = now_year - year;
            if let [_, month] = born.as_slice()
                && now_month < month
            {
                years -= 1;
            }
            (years >= 2).then(|| format!("About {}", plural(years, "year")))
        }
        [year, month, day] => {
            let mut months = (now_year - year) * 12 + (now_month - month);
            if now_day < day {
                months -= 1;
            }
            let days =
                day_number(*now_year, *now_month, *now_day) - day_number(*year, *month, *day);
            if days < 0 {
                return None;
            }
            if days == 0 {
                return Some("Born today".to_string());
            }
            if months < 1 {
                return Some(plural(days, "day"));
            }
            if months < 24 {
                return Some(plural(months, "month"));
            }
            let (years, left) = (months / 12, months % 12);
            if years >= 18 || left == 0 {
                return Some(plural(years, "year"));
            }
            Some(format!(
                "{} {}",
                plural(years, "year"),
                plural(left, "month")
            ))
        }
        _ => None,
    }
}
