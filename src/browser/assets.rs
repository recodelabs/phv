//! The stylesheet and htmx, embedded in the binary.

use axum::{
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

const STYLE: &str = include_str!("../../assets/style.css");
const HTMX: &str = include_str!("../../assets/htmx-2.0.11.min.js");

pub async fn asset(Path(file): Path<String>) -> Response {
    let (content_type, body) = match file.as_str() {
        "style.css" => ("text/css; charset=utf-8", STYLE),
        "htmx.min.js" => ("text/javascript; charset=utf-8", HTMX),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, content_type)], body).into_response()
}
