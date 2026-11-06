//! The stylesheet and htmx, embedded in the binary.

use axum::{
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

const STYLE: &str = include_str!("../../assets/style.css");
const HTMX: &str = include_str!("../../assets/htmx-2.0.11.min.js");
const D3: &str = include_str!("../../assets/d3-7.9.2.min.js");
const PLOT: &str = include_str!("../../assets/plot-0.6.19.min.js");
const ACTIVITY: &str = include_str!("../../assets/activity.js");

pub async fn asset(Path(file): Path<String>) -> Response {
    let (content_type, body) = match file.as_str() {
        "style.css" => ("text/css; charset=utf-8", STYLE),
        "htmx.min.js" => ("text/javascript; charset=utf-8", HTMX),
        "d3.min.js" => ("text/javascript; charset=utf-8", D3),
        "plot.min.js" => ("text/javascript; charset=utf-8", PLOT),
        "activity.js" => ("text/javascript; charset=utf-8", ACTIVITY),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, content_type)], body).into_response()
}
