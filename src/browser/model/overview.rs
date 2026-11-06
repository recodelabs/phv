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
    pub measures: Vec<Measure>,
    pub latest_steps: Option<LatestSteps>,
    pub latest_report: Option<ReportRow>,
    pub conditions: Vec<ClinicalRow>,
    pub medicines: Vec<ClinicalRow>,
    pub allergies: Vec<ClinicalRow>,
}

/// Input for `overview`, one slice per resource type.
pub struct Sources<'a> {
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
