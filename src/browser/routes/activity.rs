//! The Activity page.

use super::{App, Chrome, not_found, render, run};
use crate::browser::model::activity::{
    Bucket, Crumb, Period, StepSummary, TypeRow, Workout, buckets_of_distance, buckets_of_steps,
    in_period, step_days, summary, type_rows, workouts,
};
use askama::Template;
use axum::{
    Router,
    extract::{Path, Query, State},
    response::Response,
    routing::get,
};
use serde::Deserialize;

/// Older workouts stay reachable by picking a year or month.
const WORKOUTS_SHOWN: usize = 100;

struct ActivityChip {
    query: String,
    label: String,
    count: usize,
    on: bool,
}

#[derive(Template)]
#[template(path = "activity.html")]
struct ActivityPage {
    chrome: Chrome,
    empty: bool,
    title: String,
    crumbs: Vec<Crumb>,
    summary: StepSummary,
    unusable: usize,
    duplicate_days: usize,
    step_heading: &'static str,
    steps: Vec<Bucket>,
    types: Vec<TypeRow>,
    type_query: String,
    selected: Option<String>,
    over_time: Vec<Bucket>,
    over_time_heading: &'static str,
    chips: Vec<ActivityChip>,
    workouts: Vec<Workout>,
    workout_total: usize,
}

#[derive(Deserialize)]
struct ActivityQuery {
    year: Option<String>,
    month: Option<String>,
    #[serde(rename = "type")]
    activity: Option<String>,
}

fn with_prefix(query: &str) -> String {
    if query.is_empty() {
        String::new()
    } else {
        format!("{query}&")
    }
}

async fn page(
    State(app): State<App>,
    Path(vault): Path<String>,
    Query(query): Query<ActivityQuery>,
) -> Response {
    run(app, vault, "activity", move |cx| {
        let filled = |value: &Option<String>| value.clone().filter(|v| !v.is_empty());
        let (year, month) = (filled(&query.year), filled(&query.month));
        let period = Period::parse(year.as_deref(), month.as_deref())
            .ok_or_else(|| not_found("No such period."))?;
        let selected = filled(&query.activity);

        let observations = cx.records.of_type("Observation")?;
        let steps = step_days(&observations);
        let all = workouts(&observations);
        let types = type_rows(&all, &period);
        let base = period.query();

        let mut shown: Vec<Workout> = in_period(&all, &period)
            .into_iter()
            .filter(|w| selected.as_deref().is_none_or(|s| w.activity == s))
            .cloned()
            .collect();
        let workout_total = shown.len();
        shown.truncate(WORKOUTS_SHOWN);

        let mut chips = vec![ActivityChip {
            query: base.clone(),
            label: "All".to_string(),
            count: types.iter().map(|t| t.count).sum(),
            on: selected.is_none(),
        }];
        chips.extend(types.iter().map(|t| ActivityChip {
            query: format!("{}type={}", with_prefix(&base), t.name_url),
            label: t.name.clone(),
            count: t.count,
            on: selected.as_deref() == Some(t.name.as_str()),
        }));

        let over_time = match (&selected, &period) {
            (Some(_), Period::Month(..)) | (None, _) => Vec::new(),
            (Some(activity), _) => buckets_of_distance(&all, activity, &period),
        };
        let over_time = if over_time.iter().any(|b| b.value.is_some()) {
            over_time
        } else {
            Vec::new()
        };

        render(ActivityPage {
            chrome: cx.chrome()?,
            empty: steps.days.is_empty() && all.is_empty(),
            title: period.title(),
            crumbs: period.crumbs(),
            summary: summary(&steps.days, &period),
            unusable: steps.unusable,
            duplicate_days: steps.duplicate_days,
            step_heading: match period {
                Period::All => "Year",
                Period::Year(_) => "Month",
                Period::Month(..) => "Day",
            },
            steps: buckets_of_steps(&steps.days, &period),
            type_query: with_prefix(&base),
            types,
            over_time_heading: if matches!(period, Period::All) {
                "Year"
            } else {
                "Month"
            },
            over_time,
            selected,
            chips,
            workouts: shown,
            workout_total,
        })
    })
    .await
}

pub fn routes() -> Router<App> {
    Router::new().route("/v/{vault}/activity", get(page))
}
