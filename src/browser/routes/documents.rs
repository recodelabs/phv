//! The document catalog and original files.

use super::{App, Chrome, PageError, not_found, render, run, wants_partial};
use crate::browser::model::documents::{
    Chip, DocRow, chips, documents, inline_media_type, is_sha256, source_file,
};
use askama::Template;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

#[derive(Template)]
#[template(path = "documents.html")]
struct DocumentsPage {
    chrome: Chrome,
    chips: Vec<Chip>,
    rows: Vec<DocRow>,
}

#[derive(Template)]
#[template(path = "documents_list.html")]
struct DocumentsPartial {
    vault: String,
    chips: Vec<Chip>,
    rows: Vec<DocRow>,
}

#[derive(Deserialize)]
struct CategoryQuery {
    category: Option<String>,
}

async fn page(
    State(app): State<App>,
    Path(vault): Path<String>,
    Query(query): Query<CategoryQuery>,
    headers: HeaderMap,
) -> Response {
    run(app, vault, "documents", move |cx| {
        let resources = cx.records.of_type("DocumentReference")?;
        let provenance = cx.records.of_type("Provenance")?;
        let root = cx.records.vault().root().to_path_buf();
        let all = documents(&resources, &provenance, &root);
        let selected = query.category.filter(|c| !c.is_empty());
        let chips = chips(&all, selected.as_deref());
        let rows: Vec<DocRow> = all
            .into_iter()
            .filter(|r| selected.as_deref().is_none_or(|c| r.category == c))
            .collect();
        if wants_partial(&headers) {
            return render(DocumentsPartial {
                vault: cx.vault_name.clone(),
                chips,
                rows,
            });
        }
        render(DocumentsPage {
            chrome: cx.chrome()?,
            chips,
            rows,
        })
    })
    .await
}

fn file_response(extension: &str, sha256: &str, bytes: Vec<u8>) -> Response {
    let no_store = (header::CACHE_CONTROL, "no-store".to_string());
    match inline_media_type(extension) {
        // A sandbox policy can stop browsers' built-in PDF viewers, and a PDF cannot
        // script this origin.
        Some("application/pdf") => (
            [
                (header::CONTENT_TYPE, "application/pdf".to_string()),
                (header::CONTENT_DISPOSITION, "inline".to_string()),
                (
                    header::CONTENT_SECURITY_POLICY,
                    "frame-ancestors 'none'".to_string(),
                ),
                no_store,
            ],
            bytes,
        )
            .into_response(),
        Some(media_type) => (
            [
                (header::CONTENT_TYPE, media_type.to_string()),
                (header::CONTENT_DISPOSITION, "inline".to_string()),
                (header::CONTENT_SECURITY_POLICY, "sandbox".to_string()),
                no_store,
            ],
            bytes,
        )
            .into_response(),
        None => (
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{sha256}.{extension}\""),
                ),
                (header::CONTENT_SECURITY_POLICY, "sandbox".to_string()),
                no_store,
            ],
            bytes,
        )
            .into_response(),
    }
}

async fn source(State(app): State<App>, Path((vault, sha256)): Path<(String, String)>) -> Response {
    run(app, vault, "documents", move |cx| {
        if !is_sha256(&sha256) {
            return Err(not_found("No such file."));
        }
        let path = source_file(cx.records.vault().root(), &sha256)
            .ok_or_else(|| not_found("No such file."))?;
        let bytes = std::fs::read(&path).map_err(|_| PageError::Internal)?;
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        // The extension is safe to echo: it came from a file name starting with a checked hash,
        // but it may hold anything else, so keep only plain characters.
        let extension: String = extension
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        Ok(file_response(&extension, &sha256, bytes))
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/documents", get(page))
        .route("/v/{vault}/sources/{sha256}", get(source))
}
