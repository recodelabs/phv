//! The Activity page. One form scopes everything on it.

use super::{App, Chrome, not_found, render, run};
use crate::browser::model::activity::{
    ActivityTotals, Bucket, FilterBar, Period, StepSummary, TypeRow, Unit, Workout,
    buckets_of_distance, buckets_of_minutes, buckets_of_steps, filter_bar, in_period, step_days,
    summary, totals, type_rows, workouts,
};
use askama::Template;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, header},
    response::Response,
    routing::get,
};
use serde::Deserialize;

/// Older workouts stay reachable by picking a year or month.
const WORKOUTS_SHOWN: usize = 100;
const UNIT_COOKIE: &str = "phv_unit";

#[derive(Template)]
#[template(path = "activity.html")]
struct ActivityPage {
    chrome: Chrome,
    empty: bool,
    title: String,
    filters: FilterBar,
    unusable: usize,
    duplicate_days: usize,
    /// "Year", "Month" or "Day": the level the charts are showing.
    level: &'static str,
    has_next_level: bool,
    unit: &'static str,
    selected: Option<String>,
    summary: StepSummary,
    steps: Vec<Bucket>,
    has_steps: bool,
    types: Vec<TypeRow>,
    any_distance: bool,
    totals: ActivityTotals,
    over_time: Vec<Bucket>,
    over_time_is_distance: bool,
    workouts: Vec<Workout>,
    workout_total: usize,
}

#[derive(Deserialize)]
struct ActivityQuery {
    year: Option<String>,
    month: Option<String>,
    #[serde(rename = "type")]
    activity: Option<String>,
    unit: Option<String>,
}

fn unit_from_cookie(headers: &HeaderMap) -> Option<Unit> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == UNIT_COOKIE)
        .and_then(|(_, value)| Unit::parse(value))
}

async fn page(
    State(app): State<App>,
    Path(vault): Path<String>,
    Query(query): Query<ActivityQuery>,
    headers: HeaderMap,
) -> Response {
    let chosen = query.unit.as_deref().and_then(Unit::parse);
    let unit = chosen.or(unit_from_cookie(&headers)).unwrap_or(Unit::Km);
    let mut response = run(app, vault, "activity", move |cx| {
        let filled = |value: &Option<String>| value.clone().filter(|v| !v.is_empty());
        let (year, month) = (filled(&query.year), filled(&query.month));
        let period = Period::parse(year.as_deref(), month.as_deref())
            .ok_or_else(|| not_found("No such period."))?;
        let selected = filled(&query.activity);

        let observations = cx.records.of_type("Observation")?;
        let steps = step_days(&observations);
        let all = workouts(&observations, unit);
        let types = type_rows(&all, &period, unit);

        let shown: Vec<&Workout> = in_period(&all, &period)
            .into_iter()
            .filter(|w| selected.as_deref().is_none_or(|s| w.activity == s))
            .collect();
        let chosen_totals = totals(&shown, unit);
        let workout_total = shown.len();
        let listed: Vec<Workout> = shown
            .iter()
            .take(WORKOUTS_SHOWN)
            .map(|w| (*w).clone())
            .collect();

        let (over_time, over_time_is_distance) = match &selected {
            None => (Vec::new(), false),
            Some(activity) => {
                let distance = buckets_of_distance(&all, activity, &period, unit);
                if distance.iter().any(|b| b.value.is_some()) {
                    (distance, true)
                } else {
                    (buckets_of_minutes(&all, activity, &period), false)
                }
            }
        };
        let over_time = if over_time.iter().any(|b| b.value.is_some()) {
            over_time
        } else {
            Vec::new()
        };

        let step_buckets = buckets_of_steps(&steps.days, &period);
        render(ActivityPage {
            chrome: cx.chrome()?,
            empty: steps.days.is_empty() && all.is_empty(),
            title: period.title(),
            filters: filter_bar(&steps.days, &all, &period, selected.as_deref(), unit),
            unusable: steps.unusable,
            duplicate_days: steps.duplicate_days,
            level: match period {
                Period::All => "Year",
                Period::Year(_) => "Month",
                Period::Month(..) => "Day",
            },
            has_next_level: !matches!(period, Period::Month(..)),
            unit: unit.code(),
            selected,
            summary: summary(&steps.days, &period),
            has_steps: step_buckets.iter().any(|b| b.value.is_some()),
            steps: step_buckets,
            any_distance: types.iter().any(|t| t.chart_distance.is_some()),
            types,
            totals: chosen_totals,
            over_time,
            over_time_is_distance,
            workouts: listed,
            workout_total,
        })
    })
    .await;
    if let Some(unit) = chosen {
        let cookie = format!(
            "{UNIT_COOKIE}={}; Path=/; Max-Age=31536000; SameSite=Strict; HttpOnly",
            unit.code()
        );
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().insert(header::SET_COOKIE, value);
        }
    }
    response
}

pub fn routes() -> Router<App> {
    Router::new().route("/v/{vault}/activity", get(page))
}
