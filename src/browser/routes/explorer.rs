//! All FHIR pages.

use super::{App, Chrome, Ctx, PageError, not_found, render, run};
use crate::browser::model::explorer::{
    RecordRow, RefRow, TypeCount, label_of, page_count, page_of, pretty_json, record_row,
    references,
};
use crate::browser::model::parse_resource;
use askama::Template;
use axum::{
    Router,
    extract::{Path, Query, State},
    response::Response,
    routing::get,
};
use serde::Deserialize;

#[derive(Template)]
#[template(path = "fhir_types.html")]
struct TypesPage {
    chrome: Chrome,
    types: Vec<TypeCount>,
}

#[derive(Template)]
#[template(path = "fhir_list.html")]
struct ListPage {
    chrome: Chrome,
    resource_type: String,
    total: usize,
    rows: Vec<RecordRow>,
    page: usize,
    pages: usize,
}

#[derive(Template)]
#[template(path = "fhir_record.html")]
struct RecordPage {
    chrome: Chrome,
    resource_type: String,
    id: String,
    label: String,
    json: String,
    references: Vec<RefRow>,
}

#[derive(Deserialize)]
struct PageQuery {
    page: Option<usize>,
}

fn known_type(cx: &Ctx, resource_type: &str) -> Result<(), PageError> {
    if cx
        .records
        .vault()
        .resource_types()?
        .iter()
        .any(|t| t == resource_type)
    {
        Ok(())
    } else {
        Err(not_found(format!(
            "This vault has no {resource_type} records."
        )))
    }
}

async fn types(State(app): State<App>, Path(vault): Path<String>) -> Response {
    run(app, vault, "fhir", |cx| {
        let mut types = Vec::new();
        for name in cx.records.vault().resource_types()? {
            let count = cx.records.of_type(&name)?.len();
            types.push(TypeCount { name, count });
        }
        render(TypesPage {
            chrome: cx.chrome()?,
            types,
        })
    })
    .await
}

async fn list(
    State(app): State<App>,
    Path((vault, resource_type)): Path<(String, String)>,
    Query(query): Query<PageQuery>,
) -> Response {
    run(app, vault, "fhir", move |cx| {
        known_type(cx, &resource_type)?;
        let resources = cx.records.of_type(&resource_type)?;
        let rows: Vec<RecordRow> = resources.iter().map(record_row).collect();
        let page = query.page.unwrap_or(1).max(1);
        render(ListPage {
            chrome: cx.chrome()?,
            total: rows.len(),
            pages: page_count(rows.len()),
            rows: page_of(&rows, page),
            page,
            resource_type,
        })
    })
    .await
}

async fn record(
    State(app): State<App>,
    Path((vault, resource_type, id)): Path<(String, String, String)>,
) -> Response {
    run(app, vault, "fhir", move |cx| {
        known_type(cx, &resource_type)?;
        let line = cx
            .records
            .vault()
            .get(&resource_type, &id)?
            .ok_or_else(|| not_found(format!("No {resource_type} with this id.")))?;
        let resource = parse_resource(&line, &resource_type)
            .ok_or_else(|| not_found("This record could not be read. Run phv check."))?;
        let vault_ref = cx.records.vault().clone();
        render(RecordPage {
            chrome: cx.chrome()?,
            label: label_of(&resource),
            json: pretty_json(&resource),
            references: references(&resource, |kind, target| {
                vault_ref.get(kind, target).is_ok_and(|line| line.is_some())
            }),
            resource_type,
            id,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/fhir", get(types))
        .route("/v/{vault}/fhir/{type}", get(list))
        .route("/v/{vault}/fhir/{type}/{id}", get(record))
}
