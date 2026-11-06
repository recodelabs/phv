//! The Overview page.

use super::{App, Chrome, render, run};
use crate::browser::model::overview::{Overview, Sources, overview};
use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    response::Response,
    routing::get,
};

#[derive(Template)]
#[template(path = "overview.html")]
struct OverviewPage {
    chrome: Chrome,
    view: Overview,
    record_count: usize,
    document_count: usize,
}

async fn page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "overview", |cx| {
        let patients = cx.records.of_type("Patient")?;
        let observations = cx.records.of_type("Observation")?;
        let reports = cx.records.of_type("DiagnosticReport")?;
        let conditions = cx.records.of_type("Condition")?;
        let statements = cx.records.of_type("MedicationStatement")?;
        let requests = cx.records.of_type("MedicationRequest")?;
        let medications = cx.records.of_type("Medication")?;
        let allergies = cx.records.of_type("AllergyIntolerance")?;
        let view = overview(&Sources {
            patients: &patients,
            observations: &observations,
            reports: &reports,
            conditions: &conditions,
            statements: &statements,
            requests: &requests,
            medications: &medications,
            allergies: &allergies,
        });
        let mut record_count = 0;
        for name in cx.records.vault().resource_types()? {
            record_count += cx.records.vault().lines(&name)?.len();
        }
        let document_count = cx.records.vault().lines("DocumentReference")?.len();
        render(OverviewPage {
            chrome: cx.chrome()?,
            view,
            record_count,
            document_count,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new().route("/v/{vault}/overview", get(page))
}
