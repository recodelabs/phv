//! The Activity tabs: Steps and Workouts. Each has one form that scopes everything below it.

use super::{App, Chrome, Ctx, PageError, not_found, render, run};
use crate::browser::model::activity::{
    ActivityTotals, Bucket, CalendarDay, Level, Period, StepSummary, StepsFilters, TypeRow, Unit,
    Workout, WorkoutsFilters, buckets_of_distance, buckets_of_minutes, buckets_of_steps, in_period,
    step_calendar, step_days, steps_filters, summary, totals, type_rows, workout_calendar,
    workouts, workouts_filters,
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
/// Past this many bars, a chart draws a line, so a bar is no longer something to click.
const MOST_BARS: usize = 150;
const UNIT_COOKIE: &str = "phv_unit";

#[derive(Template)]
#[template(path = "activity_steps.html")]
struct StepsPage {
    chrome: Chrome,
    path: String,
    heading: String,
    any_steps: bool,
    title: String,
    /// The period only, so switching tabs keeps it.
    tab_query: String,
    filters: StepsFilters,
    unusable: usize,
    duplicate_days: usize,
    level: &'static str,
    can_zoom: bool,
    summary: StepSummary,
    steps: Vec<Bucket>,
    has_steps: bool,
    calendar_heading: String,
    calendar: Vec<CalendarDay>,
}

#[derive(Template)]
#[template(path = "activity_workouts.html")]
struct WorkoutsPage {
    chrome: Chrome,
    path: String,
    heading: String,
    any_workouts: bool,
    title: String,
    tab_query: String,
    filters: WorkoutsFilters,
    level: &'static str,
    can_zoom: bool,
    unit: &'static str,
    selected: Option<String>,
    types: Vec<TypeRow>,
    any_distance: bool,
    totals: ActivityTotals,
    over_time: Vec<Bucket>,
    over_time_is_distance: bool,
    calendar_heading: String,
    calendar: Vec<CalendarDay>,
    workouts: Vec<Workout>,
    workout_total: usize,
}

#[derive(Deserialize)]
struct StepsQuery {
    year: Option<String>,
    month: Option<String>,
    by: Option<String>,
}

#[derive(Deserialize)]
struct WorkoutsQuery {
    year: Option<String>,
    month: Option<String>,
    #[serde(rename = "type")]
    activity: Option<String>,
    unit: Option<String>,
}

fn filled(value: &Option<String>) -> Option<String> {
    value.clone().filter(|v| !v.is_empty())
}

fn period_of(year: &Option<String>, month: &Option<String>) -> Result<Period, PageError> {
    let (year, month) = (filled(year), filled(month));
    Period::parse(year.as_deref(), month.as_deref()).ok_or_else(|| not_found("No such period."))
}

fn observations(cx: &mut Ctx) -> Result<Vec<serde_json::Value>, PageError> {
    Ok(cx.records.of_type("Observation")?)
}

async fn steps_page(
    State(app): State<App>,
    Path(vault): Path<String>,
    Query(query): Query<StepsQuery>,
) -> Response {
    run(app, vault, "activity", move |cx| {
        let period = period_of(&query.year, &query.month)?;
        let level = filled(&query.by)
            .as_deref()
            .and_then(Level::parse)
            .filter(|level| period.allows(*level))
            .unwrap_or(period.default_level());
        let data = step_days(&observations(cx)?);
        let steps = buckets_of_steps(&data.days, &period, level);
        let calendar_period = period.steps_calendar_period(&data.days);
        render(StepsPage {
            chrome: cx.chrome()?,
            path: format!("/v/{}/activity", cx.vault_name),
            heading: period.title(),
            any_steps: !data.days.is_empty(),
            title: period.title(),
            tab_query: period.query(),
            filters: steps_filters(&data.days, &period, level),
            unusable: data.unusable,
            duplicate_days: data.duplicate_days,
            level: level.label(),
            can_zoom: level != Level::Day && steps.len() <= MOST_BARS,
            summary: summary(&data.days, &period),
            has_steps: steps.iter().any(|b| b.value.is_some()),
            steps,
            calendar_heading: format!("Calendar · {}", calendar_period.title()),
            calendar: if matches!(period, Period::Month(..)) {
                Vec::new()
            } else {
                step_calendar(&data.days, &calendar_period)
            },
        })
    })
    .await
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

async fn workouts_page(
    State(app): State<App>,
    Path(vault): Path<String>,
    Query(query): Query<WorkoutsQuery>,
    headers: HeaderMap,
) -> Response {
    let chosen = query.unit.as_deref().and_then(Unit::parse);
    let unit = chosen.or(unit_from_cookie(&headers)).unwrap_or(Unit::Km);
    let mut response = run(app, vault, "activity", move |cx| {
        let period = period_of(&query.year, &query.month)?;
        let selected = filled(&query.activity);
        let chosen_activity = selected.clone();
        let all = workouts(&observations(cx)?, unit);
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

        render(WorkoutsPage {
            chrome: cx.chrome()?,
            path: format!("/v/{}/activity/workouts", cx.vault_name),
            heading: match &selected {
                Some(activity) => format!("{} · {activity}", period.title()),
                None => period.title(),
            },
            any_workouts: !all.is_empty(),
            title: period.title(),
            tab_query: period.query(),
            filters: workouts_filters(&all, &period, selected.as_deref(), unit),
            level: period.default_level().label(),
            can_zoom: period.default_level() != Level::Day,
            unit: unit.code(),
            selected,
            any_distance: types.iter().any(|t| t.chart_distance.is_some()),
            types,
            totals: chosen_totals,
            over_time,
            over_time_is_distance,
            calendar_heading: "Calendar".to_string(),
            calendar: if matches!(period, Period::Month(..)) {
                Vec::new()
            } else {
                workout_calendar(&all, &period, chosen_activity.as_deref())
            },
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
    Router::new()
        .route("/v/{vault}/activity", get(steps_page))
        .route("/v/{vault}/activity/workouts", get(workouts_page))
}
