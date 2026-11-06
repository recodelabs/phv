//! Local REST API, bound to 127.0.0.1 only.

use crate::error::{Issue, PhvError};
use crate::write::{AddOptions, Phv};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{net::Ipv4Addr, sync::Arc};

type AppState = Arc<Phv>;

const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

fn issue_json(issue: &Issue) -> Value {
    json!({
        "severity": if issue.is_error() { "error" } else { "warning" },
        "code": "invalid",
        "details": {"text": format!("{} {}", issue.code, issue.message)},
        "expression": [issue.path],
    })
}

fn outcome(status: StatusCode, issues: Vec<Value>) -> Response {
    let body = json!({"resourceType": "OperationOutcome", "issue": issues});
    (
        status,
        [(header::CONTENT_TYPE, "application/fhir+json")],
        body.to_string(),
    )
        .into_response()
}

fn simple(status: StatusCode, code: &str, text: &str) -> Response {
    outcome(
        status,
        vec![json!({"severity": "error", "code": code, "details": {"text": text}})],
    )
}

impl IntoResponse for PhvError {
    fn into_response(self) -> Response {
        match &self {
            PhvError::Usage(m) => simple(StatusCode::BAD_REQUEST, "invalid", m),
            PhvError::NotFound(_) => simple(StatusCode::NOT_FOUND, "not-found", &self.to_string()),
            PhvError::Invalid(i) => outcome(
                StatusCode::UNPROCESSABLE_ENTITY,
                i.iter().map(issue_json).collect(),
            ),
            PhvError::Conflict(i) => {
                outcome(StatusCode::CONFLICT, i.iter().map(issue_json).collect())
            }
            PhvError::Io(e) => simple(
                StatusCode::INTERNAL_SERVER_ERROR,
                "exception",
                &e.to_string(),
            ),
        }
    }
}

fn parse(body: &[u8]) -> Result<Value, PhvError> {
    let text = std::str::from_utf8(body)
        .map_err(|e| PhvError::Usage(format!("body is not UTF-8: {e}")))?;
    crate::canon::parse_json(text).map_err(|e| PhvError::Usage(format!("body: {e}")))
}

async fn validate(State(phv): State<AppState>, body: Bytes) -> Result<Response, PhvError> {
    let issues = phv.validate_submission(&parse(&body)?).await?;
    Ok(outcome(
        StatusCode::OK,
        issues.iter().map(issue_json).collect(),
    ))
}

#[derive(Deserialize)]
struct AddQuery {
    source: Option<String>,
    agent: Option<String>,
    recorded_at: Option<String>,
    #[serde(default)]
    dry_run: bool,
}

async fn add(
    State(phv): State<AppState>,
    Query(q): Query<AddQuery>,
    body: Bytes,
) -> Result<Response, PhvError> {
    // A caller's recorded_at passes through as given, so a stray one is refused by the library.
    let recorded_at = q.recorded_at.clone().or_else(|| {
        q.source
            .as_ref()
            .map(|_| humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string())
    });
    let options = AddOptions {
        source: q.source,
        agent: q.agent,
        recorded_at,
        dry_run: q.dry_run,
    };
    Ok(Json(phv.add(&parse(&body)?, &options).await?).into_response())
}

fn ndjson(text: String) -> Response {
    ([(header::CONTENT_TYPE, "application/x-ndjson")], text).into_response()
}

async fn read_one(
    State(phv): State<AppState>,
    Path((resource_type, id)): Path<(String, String)>,
) -> Result<Response, PhvError> {
    Ok((
        [(header::CONTENT_TYPE, "application/fhir+json")],
        phv.get(&resource_type, &id)?,
    )
        .into_response())
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(rename = "where")]
    filter: Option<String>,
}

async fn list(
    State(phv): State<AppState>,
    Path(resource_type): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Response, PhvError> {
    let result = phv.search(&resource_type, q.filter.as_deref()).await?;
    let mut text = result.lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    let mut response = ndjson(text);
    if let Some(warning) = result.warning()
        && let Ok(value) = header::HeaderValue::from_str(&warning)
    {
        response.headers_mut().insert("x-phv-warning", value);
    }
    Ok(response)
}

#[derive(Deserialize)]
struct ReplaceQuery {
    #[serde(default)]
    dry_run: bool,
}

async fn replace(
    State(phv): State<AppState>,
    Path((resource_type, id)): Path<(String, String)>,
    Query(q): Query<ReplaceQuery>,
    body: Bytes,
) -> Result<Response, PhvError> {
    Ok(Json(
        phv.replace(&resource_type, &id, &parse(&body)?, q.dry_run)
            .await?,
    )
    .into_response())
}

async fn remove(
    State(phv): State<AppState>,
    Path((resource_type, id)): Path<(String, String)>,
) -> Result<Response, PhvError> {
    phv.remove(&resource_type, &id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
struct SourceQuery {
    filename: String,
}

async fn add_source(
    State(phv): State<AppState>,
    Query(q): Query<SourceQuery>,
    body: Bytes,
) -> Result<Response, PhvError> {
    Ok(Json(phv.source_add(&q.filename, &body).await?).into_response())
}

/// Exactly `127.0.0.1` or `localhost`, with an optional all-digit port.
fn is_local(host: &str) -> bool {
    let (name, port) = match host.split_once(':') {
        Some((name, port)) => (name, Some(port)),
        None => (host, None),
    };
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    port_ok && (name == "127.0.0.1" || name == "localhost")
}

/// Stops a web page in the person's browser from reaching the API.
async fn local_only(request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(is_local);
    let origin_ok = match headers.get(header::ORIGIN) {
        None => true,
        Some(origin) => origin
            .to_str()
            .ok()
            .and_then(|o| o.strip_prefix("http://"))
            .is_some_and(is_local),
    };
    if host_ok && origin_ok {
        next.run(request).await
    } else {
        simple(
            StatusCode::FORBIDDEN,
            "forbidden",
            "this API only accepts local requests",
        )
    }
}

const MAX_ERROR_BODY_BYTES: usize = 4096;

/// Rewrites the framework's plain-text errors (bad query, body too large, no such route or
/// method) as an OperationOutcome with the same status.
async fn errors_as_outcomes(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let status = response.status();
    let is_fhir = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/fhir+json"));
    if !(status.is_client_error() || status.is_server_error()) || is_fhir {
        return response;
    }
    let bytes = axum::body::to_bytes(response.into_body(), MAX_ERROR_BODY_BYTES)
        .await
        .unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes).trim().to_string();
    let text = if text.is_empty() {
        status.canonical_reason().unwrap_or("error").to_string()
    } else {
        text
    };
    simple(status, "invalid", &text)
}

async fn no_such_route() -> Response {
    simple(StatusCode::NOT_FOUND, "not-found", "no such route")
}

pub fn router(phv: AppState) -> Router {
    Router::new()
        .route("/validate", post(validate))
        .route("/resources", post(add))
        .route("/resources/{type}", get(list))
        .route(
            "/resources/{type}/{id}",
            get(read_one).put(replace).delete(remove),
        )
        .route("/sources", post(add_source))
        .fallback(no_such_route)
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .layer(middleware::from_fn(local_only))
        .layer(middleware::from_fn(errors_as_outcomes))
        .with_state(phv)
}

pub async fn serve(phv: Phv, port: u16) -> Result<(), PhvError> {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    println!("phv API on http://127.0.0.1:{port}");
    axum::serve(listener, router(Arc::new(phv))).await?;
    Ok(())
}
