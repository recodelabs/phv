//! Routes, GET only. Each topic has its own file with its templates and handlers.

mod documents;
mod explorer;
mod labs;
mod vitals;

use super::guard;
use super::model::Records;
use super::model::explorer::patient_name;
use super::vaults::Vaults;
use crate::error::PhvError;
use crate::store::Vault;
use askama::Template;
use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware,
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct App {
    vaults: Arc<Vaults>,
}

pub struct NavItem {
    pub label: &'static str,
    pub href: String,
    pub on: bool,
}

pub struct SwitcherItem {
    pub name: String,
    pub href: String,
    pub available: bool,
    pub on: bool,
}

/// What every page's layout needs.
pub struct Chrome {
    pub person: String,
    pub vault: String,
    pub nav: Vec<NavItem>,
    pub switcher: Vec<SwitcherItem>,
    pub unreadable: usize,
}

const SECTIONS: [(&str, &str); 9] = [
    ("Overview", "overview"),
    ("Vitals", "vitals"),
    ("Labs", "labs"),
    ("Conditions", "conditions"),
    ("Medicines", "medicines"),
    ("Allergies", "allergies"),
    ("Vaccines", "vaccines"),
    ("Documents", "documents"),
    ("All FHIR", "fhir"),
];

/// One request's view of one vault.
pub struct Ctx {
    pub records: Records,
    pub vault_name: String,
    section: &'static str,
    app: App,
}

impl Ctx {
    /// Build this after loading records, so the unreadable count is complete.
    pub fn chrome(&mut self) -> Result<Chrome, PageError> {
        let patients = self.records.of_type("Patient")?;
        let name = &self.vault_name;
        Ok(Chrome {
            person: patients
                .first()
                .and_then(patient_name)
                .unwrap_or_else(|| name.clone()),
            vault: name.clone(),
            nav: SECTIONS
                .iter()
                .map(|(label, slug)| NavItem {
                    label,
                    href: format!("/v/{name}/{slug}"),
                    on: *slug == self.section,
                })
                .collect(),
            switcher: self.switcher(),
            unreadable: self.records.unreadable(),
        })
    }

    fn switcher(&self) -> Vec<SwitcherItem> {
        let entries = self.app.vaults.entries();
        if entries.len() < 2 {
            return Vec::new();
        }
        entries
            .iter()
            .map(|e| SwitcherItem {
                href: format!("/v/{}/{}", e.name, self.section),
                available: Vault::open(&e.path).is_ok(),
                on: e.name == self.vault_name,
                name: e.name.clone(),
            })
            .collect()
    }
}

#[derive(Debug)]
pub enum PageError {
    NotFound(String),
    Unavailable(String),
    Internal,
}

impl From<PhvError> for PageError {
    fn from(error: PhvError) -> Self {
        // No record contents reach these errors: reads fail on IO, before any parsing.
        eprintln!("phv browser: {error}");
        PageError::Internal
    }
}

pub fn not_found(what: impl Into<String>) -> PageError {
    PageError::NotFound(what.into())
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorPage {
    title: String,
    message: String,
    vaults: Vec<String>,
}

fn error_response(error: PageError, vaults: &Vaults) -> Response {
    let (status, title, message) = match error {
        PageError::NotFound(what) => (StatusCode::NOT_FOUND, "Not found".to_string(), what),
        PageError::Unavailable(why) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Vault unavailable".to_string(),
            why,
        ),
        PageError::Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Something went wrong".to_string(),
            "phv could not read this page. The server's log has the details.".to_string(),
        ),
    };
    let page = ErrorPage {
        title,
        message,
        vaults: vaults.entries().iter().map(|e| e.name.clone()).collect(),
    };
    match page.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => status.into_response(),
    }
}

/// Opens the vault and runs one page off the async threads, turning errors into pages.
pub async fn run<F>(app: App, vault_name: String, section: &'static str, page: F) -> Response
where
    F: FnOnce(&mut Ctx) -> Result<Response, PageError> + Send + 'static,
{
    let vaults = app.vaults.clone();
    let task = tokio::task::spawn_blocking(move || {
        let entry = app
            .vaults
            .get(&vault_name)
            .ok_or_else(|| not_found(format!("No vault named {vault_name}.")))?;
        let vault = Vault::open(&entry.path).map_err(|e| PageError::Unavailable(e.to_string()))?;
        let mut cx = Ctx {
            records: Records::new(vault),
            vault_name,
            section,
            app: app.clone(),
        };
        page(&mut cx)
    });
    let result = task.await.unwrap_or(Err(PageError::Internal));
    result.unwrap_or_else(|error| error_response(error, &vaults))
}

pub fn render<T: Template>(page: T) -> Result<Response, PageError> {
    match page.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(PageError::Internal),
    }
}

/// htmx asks for a partial with this header. Anything else gets the full page.
pub fn wants_partial(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

async fn home(State(app): State<App>) -> Response {
    Redirect::to(&format!("/v/{}/overview", app.vaults.default_name())).into_response()
}

async fn fallback(State(app): State<App>, request: Request) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    error_response(not_found("There is no such page."), &app.vaults)
}

pub fn router(vaults: Vaults) -> Router {
    let app = App {
        vaults: Arc::new(vaults),
    };
    Router::new()
        .route("/", get(home))
        .route("/assets/{file}", get(super::assets::asset))
        .merge(explorer::routes())
        .merge(vitals::routes())
        .merge(labs::routes())
        .merge(documents::routes())
        .fallback(fallback)
        .with_state(app)
        .layer(middleware::from_fn(guard::headers))
        .layer(middleware::from_fn(guard::local_only))
}
