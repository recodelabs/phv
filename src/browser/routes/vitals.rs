//! Vitals pages.

use super::{App, Chrome, not_found, render, run, wants_partial};
use crate::browser::model::observation::ObsRow;
use crate::browser::model::vitals::{Measure, history, measures, vital_rows};
use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    http::HeaderMap,
    response::Response,
    routing::get,
};

#[derive(Template)]
#[template(path = "vitals.html")]
struct VitalsPage {
    chrome: Chrome,
    measures: Vec<Measure>,
}

#[derive(Template)]
#[template(path = "history_table.html")]
struct HistoryPartial {
    rows: Vec<ObsRow>,
}

#[derive(Template)]
#[template(path = "vital_history.html")]
struct HistoryPage {
    chrome: Chrome,
    name: String,
    rows: Vec<ObsRow>,
}

async fn page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "vitals", |cx| {
        let observations = cx.records.of_type("Observation")?;
        let rows = vital_rows(&observations);
        render(VitalsPage {
            chrome: cx.chrome()?,
            measures: measures(&rows),
        })
    })
    .await
}

async fn measure(
    State(app): State<App>,
    Path((vault, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    run(app, vault, "vitals", move |cx| {
        let observations = cx.records.of_type("Observation")?;
        let rows = history(&vital_rows(&observations), &key);
        let name = rows
            .first()
            .ok_or_else(|| not_found("No records for this measure."))?
            .name
            .clone();
        if wants_partial(&headers) {
            return render(HistoryPartial { rows });
        }
        render(HistoryPage {
            chrome: cx.chrome()?,
            name,
            rows,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/vitals", get(page))
        .route("/v/{vault}/vitals/{measure}", get(measure))
}
