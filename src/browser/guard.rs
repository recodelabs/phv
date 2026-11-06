//! Keeps other web pages away from this server, and sets the headers on every response.

use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

const PAGE_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'";

fn is_local_host(authority: &str) -> bool {
    let host = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|b| b.is_ascii_digit()))
        .map_or(authority, |(host, _)| host);
    host == "127.0.0.1" || host == "localhost"
}

fn is_local_origin(origin: &str) -> bool {
    origin.strip_prefix("http://").is_some_and(is_local_host)
}

/// Rejects a `Host` that is not local, which stops DNS rebinding, and an `Origin` from elsewhere.
pub async fn local_only(request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(is_local_host);
    let origin_ok = headers
        .get(header::ORIGIN)
        .is_none_or(|o| o.to_str().is_ok_and(is_local_origin));
    if !host_ok || !origin_ok {
        return (
            StatusCode::FORBIDDEN,
            "this server only answers local requests",
        )
            .into_response();
    }
    next.run(request).await
}

/// Adds any header a handler has not set itself. Original files set their own policy.
pub async fn headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    let defaults = [
        (header::CONTENT_SECURITY_POLICY, PAGE_POLICY),
        (header::CACHE_CONTROL, "no-store"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::VARY, "HX-Request"),
    ];
    for (name, value) in defaults {
        headers
            .entry(name)
            .or_insert(HeaderValue::from_static(value));
    }
    response
}
