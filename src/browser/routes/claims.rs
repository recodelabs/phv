//! The Claims topic.

use super::labs::original_file;
use super::{App, Chrome, not_found, render, run, wants_partial};
use crate::browser::model::claims::{
    ClaimDetail, ClaimRow, Segment, billed_text, claim_detail, claim_rows, in_year, segments,
    total_split, year_chips,
};
use crate::browser::model::documents::Chip;
use askama::Template;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Response,
    routing::get,
};
use serde::Deserialize;

#[derive(Template)]
#[template(path = "claims.html")]
struct ClaimsPage {
    chrome: Chrome,
    chips: Vec<Chip>,
    rows: Vec<ClaimRow>,
    billed: String,
    segments: Vec<Segment>,
}

#[derive(Template)]
#[template(path = "claims_list.html")]
struct ClaimsPartial {
    vault: String,
    chips: Vec<Chip>,
    rows: Vec<ClaimRow>,
    billed: String,
    segments: Vec<Segment>,
}

#[derive(Template)]
#[template(path = "claim.html")]
struct ClaimPage {
    chrome: Chrome,
    detail: ClaimDetail,
    original: Option<String>,
}

#[derive(Deserialize)]
struct YearQuery {
    year: Option<String>,
}

async fn page(
    State(app): State<App>,
    Path(vault): Path<String>,
    Query(query): Query<YearQuery>,
    headers: HeaderMap,
) -> Response {
    run(app, vault, "claims", move |cx| {
        let claims = cx.records.of_type("ExplanationOfBenefit")?;
        let organizations = cx.records.of_type("Organization")?;
        let all = claim_rows(&claims, &organizations);
        let selected = query.year.filter(|y| !y.is_empty());
        let chips = year_chips(&all, selected.as_deref());
        let rows: Vec<ClaimRow> = all
            .into_iter()
            .filter(|r| selected.as_deref().is_none_or(|y| in_year(r, y)))
            .collect();
        let total = total_split(&rows);
        let billed = billed_text(total);
        let segments = segments(total);
        if wants_partial(&headers) {
            return render(ClaimsPartial {
                vault: cx.vault_name.clone(),
                chips,
                rows,
                billed,
                segments,
            });
        }
        render(ClaimsPage {
            chrome: cx.chrome()?,
            chips,
            rows,
            billed,
            segments,
        })
    })
    .await
}

async fn claim(State(app): State<App>, Path((vault, id)): Path<(String, String)>) -> Response {
    run(app, vault, "claims", move |cx| {
        let claims = cx.records.of_type("ExplanationOfBenefit")?;
        let coverage = cx.records.of_type("Coverage")?;
        let organizations = cx.records.of_type("Organization")?;
        let practitioners = cx.records.of_type("Practitioner")?;
        let detail = claim_detail(&id, &claims, &coverage, &organizations, &practitioners)
            .ok_or_else(|| not_found("No claim with this id."))?;
        render(ClaimPage {
            original: original_file(cx, &format!("ExplanationOfBenefit/{id}"))?,
            chrome: cx.chrome()?,
            detail,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v/{vault}/claims", get(page))
        .route("/v/{vault}/claims/{id}", get(claim))
}
