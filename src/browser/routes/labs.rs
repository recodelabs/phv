//! Labs pages.

use super::{App, Chrome, Ctx, PageError, not_found, render, run, wants_partial};
use crate::browser::model::documents::{sha256_of, source_file, source_of};
use crate::browser::model::labs::{
    ReportDetail, ReportRow, lab_reports, report_detail, results_not_in_a_report, test_history,
};
use crate::browser::model::observation::{ObsRow, is_entered_in_error};
use crate::browser::model::value::id_of;
use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    http::HeaderMap,
    response::Response,
    routing::get,
};

#[derive(Template)]
#[template(path = "labs.html")]
struct LabsPage {
    chrome: Chrome,
    reports: Vec<ReportRow>,
    loose: Vec<ObsRow>,
}

#[derive(Template)]
#[template(path = "lab_report.html")]
struct ReportPage {
    chrome: Chrome,
    detail: ReportDetail,
    original: Option<String>,
}

#[derive(Template)]
#[template(path = "history_table.html")]
struct HistoryPartial {
    vault: String,
    rows: Vec<ObsRow>,
}

#[derive(Template)]
#[template(path = "lab_test.html")]
struct TestPage {
    chrome: Chrome,
    name: String,
    rows: Vec<ObsRow>,
}

async fn page(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "labs", |cx| {
        let reports = cx.records.of_type("DiagnosticReport")?;
        let observations = cx.records.of_type("Observation")?;
        render(LabsPage {
            chrome: cx.chrome()?,
            reports: lab_reports(&reports, &observations),
            loose: results_not_in_a_report(&reports, &observations),
        })
    })
    .await
}

/// The link to the file a record came from, when that file is still stored.
pub(super) fn original_file(cx: &mut Ctx, reference: &str) -> Result<Option<String>, PageError> {
    let provenance = cx.records.of_type("Provenance")?;
    let Some(source) = source_of(reference, &provenance) else {
        return Ok(None);
    };
    let documents = cx.records.of_type("DocumentReference")?;
    let root = cx.records.vault().root();
    Ok(documents
        .iter()
        .find(|d| id_of(d) == source.document_id && !is_entered_in_error(d))
        .and_then(sha256_of)
        .filter(|hash| source_file(root, hash).is_some())
        .map(|hash| format!("/v/{}/sources/{hash}", cx.vault_name)))
}

async fn report(State(app): State<App>, Path((vault, id)): Path<(String, String)>) -> Response {
    run(app, vault, "labs", move |cx| {
        let reports = cx.records.of_type("DiagnosticReport")?;
        let observations = cx.records.of_type("Observation")?;
        let detail = report_detail(&id, &reports, &observations)
            .ok_or_else(|| not_found("No lab report with this id."))?;
        render(ReportPage {
            original: original_file(cx, &format!("DiagnosticReport/{id}"))?,
            chrome: cx.chrome()?,
            detail,
        })
    })
    .await
}

async fn test(
    State(app): State<App>,
    Path((vault, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    run(app, vault, "labs", move |cx| {
        let reports = cx.records.of_type("DiagnosticReport")?;
        let observations = cx.records.of_type("Observation")?;
        let rows = test_history(&key, &reports, &observations);
        let name = rows
            .first()
            .ok_or_else(|| not_found("No results for this test."))?
            .name
            .clone();
        if wants_partial(&headers) {
            return render(HistoryPartial {
                vault: cx.vault_name.clone(),
                rows,
            });
        }
        render(TestPage {
            chrome: cx.chrome()?,
            name,
            rows,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/labs", get(page))
        .route("/v/{vault}/labs/reports/{id}", get(report))
        .route("/v/{vault}/labs/tests/{test}", get(test))
}
