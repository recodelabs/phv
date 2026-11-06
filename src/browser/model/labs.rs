//! The Labs topic: lab reports and their results.

use super::observation::{ObsRow, clinical_date, is_entered_in_error, obs_row};
use super::value::{
    DIAGNOSTIC_SERVICE, Dated, OBSERVATION_CATEGORY, concept_name, date_or_none, has_code, id_of,
    newest_first,
};
use crate::refs::local_target;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRow {
    pub id: String,
    pub raw_date: Option<String>,
    pub date: String,
    pub name: String,
    pub status: String,
    pub result_count: usize,
}

impl Dated for ReportRow {
    fn sort_date(&self) -> Option<&str> {
        self.raw_date.as_deref()
    }
    fn sort_id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportDetail {
    pub report: ReportRow,
    pub results: Vec<ObsRow>,
    /// References in the report that point at no stored Observation.
    pub missing: Vec<String>,
}

pub fn is_lab_observation(observation: &Value) -> bool {
    has_code(&observation["category"], OBSERVATION_CATEGORY, "laboratory")
}

fn by_id(observations: &[Value]) -> HashMap<String, &Value> {
    observations.iter().map(|o| (id_of(o), o)).collect()
}

fn result_ids(report: &Value) -> Vec<String> {
    report["result"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["reference"].as_str())
        .map(str::to_string)
        .collect()
}

fn observation_id(reference: &str) -> Option<&str> {
    match local_target(reference) {
        Some(("Observation", id)) => Some(id),
        _ => None,
    }
}

/// `LAB` category, or at least one result that is a `laboratory` Observation.
pub fn is_lab_report(report: &Value, observations: &[Value]) -> bool {
    if has_code(&report["category"], DIAGNOSTIC_SERVICE, "LAB") {
        return true;
    }
    let index = by_id(observations);
    result_ids(report).iter().any(|reference| {
        observation_id(reference)
            .and_then(|id| index.get(id))
            .is_some_and(|o| is_lab_observation(o))
    })
}

fn report_row(report: &Value) -> ReportRow {
    let raw_date = clinical_date(report);
    ReportRow {
        id: id_of(report),
        date: date_or_none(raw_date.as_deref()),
        raw_date,
        name: concept_name(&report["code"]).unwrap_or_else(|| "Unnamed report".into()),
        status: report["status"].as_str().unwrap_or("").to_string(),
        result_count: result_ids(report).len(),
    }
}

/// Lab reports, newest first, leaving out those entered in error.
pub fn lab_reports(reports: &[Value], observations: &[Value]) -> Vec<ReportRow> {
    let mut rows: Vec<ReportRow> = reports
        .iter()
        .filter(|r| !is_entered_in_error(r) && is_lab_report(r, observations))
        .map(report_row)
        .collect();
    newest_first(&mut rows);
    rows
}

/// One lab report with its results in the report's own order. `None` if it is not one.
pub fn report_detail(id: &str, reports: &[Value], observations: &[Value]) -> Option<ReportDetail> {
    let report = reports
        .iter()
        .find(|r| id_of(r) == id && !is_entered_in_error(r) && is_lab_report(r, observations))?;
    let index = by_id(observations);
    let mut results = Vec::new();
    let mut missing = Vec::new();
    for reference in result_ids(report) {
        match observation_id(&reference).and_then(|id| index.get(id)) {
            Some(observation) if is_entered_in_error(observation) => {}
            Some(observation) => results.push(obs_row(observation)),
            None => missing.push(reference),
        }
    }
    Some(ReportDetail {
        report: report_row(report),
        results,
        missing,
    })
}

fn referenced_by_lab_reports(reports: &[Value], observations: &[Value]) -> HashSet<String> {
    reports
        .iter()
        .filter(|r| is_lab_report(r, observations))
        .flat_map(result_ids)
        .filter_map(|reference| observation_id(&reference).map(str::to_string))
        .collect()
}

/// `laboratory` Observations in no lab report, newest first.
pub fn results_not_in_a_report(reports: &[Value], observations: &[Value]) -> Vec<ObsRow> {
    let referenced = referenced_by_lab_reports(reports, observations);
    let mut rows: Vec<ObsRow> = observations
        .iter()
        .filter(|o| is_lab_observation(o) && !is_entered_in_error(o))
        .filter(|o| !referenced.contains(&id_of(o)))
        .map(obs_row)
        .collect();
    newest_first(&mut rows);
    rows
}

/// Every result of one test across reports, newest first.
pub fn test_history(key: &str, reports: &[Value], observations: &[Value]) -> Vec<ObsRow> {
    let referenced = referenced_by_lab_reports(reports, observations);
    let mut rows: Vec<ObsRow> = observations
        .iter()
        .filter(|o| is_lab_observation(o) || referenced.contains(&id_of(o)))
        .filter(|o| !is_entered_in_error(o))
        .map(obs_row)
        .filter(|row| row.key == key)
        .collect();
    newest_first(&mut rows);
    rows
}
