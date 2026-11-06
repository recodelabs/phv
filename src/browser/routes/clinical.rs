//! Conditions, medicines, allergies and vaccines.

use super::{App, Chrome, render, run};
use crate::browser::model::clinical::{ClinicalRow, allergies, conditions, medicines, vaccines};
use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    response::Response,
    routing::get,
};

#[derive(Template)]
#[template(path = "clinical_list.html")]
struct ListPage {
    chrome: Chrome,
    title: &'static str,
    empty: &'static str,
    show_detail: bool,
    rows: Vec<ClinicalRow>,
}

async fn conditions_page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "conditions", |cx| {
        let rows = conditions(&cx.records.of_type("Condition")?);
        render(ListPage {
            chrome: cx.chrome()?,
            title: "Conditions",
            empty: "No conditions yet.",
            show_detail: false,
            rows,
        })
    })
    .await
}

async fn medicines_page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "medicines", |cx| {
        let statements = cx.records.of_type("MedicationStatement")?;
        let requests = cx.records.of_type("MedicationRequest")?;
        let medications = cx.records.of_type("Medication")?;
        render(ListPage {
            chrome: cx.chrome()?,
            title: "Medicines",
            empty: "No medicines yet.",
            show_detail: true,
            rows: medicines(&statements, &requests, &medications),
        })
    })
    .await
}

async fn allergies_page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "allergies", |cx| {
        let rows = allergies(&cx.records.of_type("AllergyIntolerance")?);
        render(ListPage {
            chrome: cx.chrome()?,
            title: "Allergies",
            empty: "No allergies yet.",
            show_detail: true,
            rows,
        })
    })
    .await
}

async fn vaccines_page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "vaccines", |cx| {
        let rows = vaccines(&cx.records.of_type("Immunization")?);
        render(ListPage {
            chrome: cx.chrome()?,
            title: "Vaccines",
            empty: "No vaccines yet.",
            show_detail: false,
            rows,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/conditions", get(conditions_page))
        .route("/v/{vault}/medicines", get(medicines_page))
        .route("/v/{vault}/allergies", get(allergies_page))
        .route("/v/{vault}/vaccines", get(vaccines_page))
}
