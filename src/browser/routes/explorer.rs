//! All FHIR pages.

use super::{App, Chrome, Ctx, PageError, not_found, render, run};
use crate::browser::model::documents::source_of;
use crate::browser::model::explorer::{
    RecordRow, RefRow, TypeCount, label_of, page_count, page_of, pretty_json, record_row,
    references, topic_of,
};
use crate::browser::model::value::{encode_segment, id_of};
use crate::refs::{collect_refs, local_target};
use askama::Template;
use axum::{
    Router,
    extract::{Path, Query, State},
    response::Response,
    routing::get,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

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

struct SourceLink {
    title: String,
    href: Option<String>,
    agent: Option<String>,
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
    topic: Option<(&'static str, String)>,
    source: Option<SourceLink>,
}

#[derive(Deserialize)]
struct PageQuery {
    page: Option<String>,
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
        let page = query
            .page
            .and_then(|page| page.parse::<usize>().ok())
            .unwrap_or(1)
            .max(1);
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

fn source_link(cx: &mut Ctx, reference: &str) -> Result<Option<SourceLink>, PageError> {
    let provenance = cx.records.of_type("Provenance")?;
    let Some(source) = source_of(reference, &provenance) else {
        return Ok(None);
    };
    let documents = cx.records.of_type("DocumentReference")?;
    let document = documents.iter().find(|d| id_of(d) == source.document_id);
    Ok(Some(SourceLink {
        title: document.map_or_else(|| source.document_id.clone(), label_of),
        href: document.map(|_| {
            format!(
                "/v/{}/fhir/DocumentReference/{}",
                cx.vault_name,
                encode_segment(&source.document_id)
            )
        }),
        agent: source.agent,
    }))
}

/// The ids of every type this record refers to, so each file is read once.
fn known_ids(cx: &mut Ctx, resource: &Value) -> HashMap<String, HashSet<String>> {
    let mut known: HashMap<String, HashSet<String>> = HashMap::new();
    for reference in collect_refs(resource) {
        let Some((kind, _)) = local_target(&reference) else {
            continue;
        };
        if !known.contains_key(kind) {
            let ids = cx.records.of_type(kind).unwrap_or_default();
            known.insert(kind.to_string(), ids.iter().map(id_of).collect());
        }
    }
    known
}

async fn record(
    State(app): State<App>,
    Path((vault, resource_type, id)): Path<(String, String, String)>,
) -> Response {
    run(app, vault, "fhir", move |cx| {
        known_type(cx, &resource_type)?;
        let resource = cx
            .records
            .of_type(&resource_type)?
            .into_iter()
            .find(|r| id_of(r) == id)
            .ok_or_else(|| not_found(format!("No {resource_type} with this id.")))?;
        let observations = cx.records.of_type("Observation")?;
        let known = known_ids(cx, &resource);
        let source = source_link(cx, &format!("{resource_type}/{id}"))?;
        render(RecordPage {
            chrome: cx.chrome()?,
            label: label_of(&resource),
            json: pretty_json(&resource),
            references: references(&resource, |kind, target| {
                known.get(kind).is_some_and(|ids| ids.contains(target))
            }),
            topic: topic_of(&resource, &observations),
            source,
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
