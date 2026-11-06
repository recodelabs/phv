mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{add_patient, lab, phv};
use http_body_util::BodyExt;
use phv::api::router;
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

async fn send(app: axum::Router, method: &str, uri: &str, body: Vec<u8>) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8747")
        .body(Body::from(body))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    (
        status,
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

async fn setup() -> (tempfile::TempDir, axum::Router, String) {
    let (dir, phv) = phv().await;
    let patient = add_patient(&phv).await;
    (dir, router(Arc::new(phv)), patient)
}

#[tokio::test]
async fn post_get_list_put_delete_work() {
    let (_d, app, patient) = setup().await;
    let body = serde_json::to_vec(&lab(&patient, "1.50")).unwrap();

    let (status, out) = send(app.clone(), "POST", "/resources", body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let reference = json(&out)["created"][0].as_str().unwrap().to_string();

    let (status, out) = send(
        app.clone(),
        "GET",
        &format!("/resources/{reference}"),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&out).contains(r#""value":1.50"#));

    let (status, out) = send(
        app.clone(),
        "GET",
        "/resources/Observation?where=status%20%3D%20%27final%27",
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(String::from_utf8_lossy(&out).lines().count(), 1);

    let changed = serde_json::to_vec(&lab(&patient, "1.75")).unwrap();
    let (status, out) = send(
        app.clone(),
        "PUT",
        &format!("/resources/{reference}"),
        changed,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&out)["updated"][0], reference.as_str());

    let (status, _) = send(
        app.clone(),
        "DELETE",
        &format!("/resources/{reference}"),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, out) = send(app, "GET", &format!("/resources/{reference}"), vec![]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&out)["resourceType"], "OperationOutcome");
}

#[tokio::test]
async fn a_wrongly_cased_type_cannot_delete_or_replace_a_record() {
    let (_d, app, patient) = setup().await;
    let id = patient.split_once('/').unwrap().1;
    let (status, _) = send(
        app.clone(),
        "DELETE",
        &format!("/resources/patient/{id}"),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body = serde_json::to_vec(&serde_json::json!({"resourceType":"Patient"})).unwrap();
    let (status, _) = send(
        app.clone(),
        "PUT",
        &format!("/resources/patient/{id}"),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(app, "GET", &format!("/resources/{patient}"), vec![]).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn errors_are_operation_outcomes_with_the_right_status() {
    let (_d, app, patient) = setup().await;

    let (status, out) = send(app.clone(), "POST", "/resources", b"{not json".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&out)["resourceType"], "OperationOutcome");

    let duplicate = br#"{"resourceType":"Patient","resourceType":"Observation"}"#.to_vec();
    let (status, _) = send(app.clone(), "POST", "/resources", duplicate).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut invalid = lab(&patient, "1");
    invalid.as_object_mut().unwrap().remove("status");
    let (status, out) = send(
        app.clone(),
        "POST",
        "/resources",
        serde_json::to_vec(&invalid).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        json(&out)["issue"][0]["details"]["text"]
            .as_str()
            .unwrap()
            .contains("status")
    );

    let broken = serde_json::to_vec(&lab("Patient/none", "1")).unwrap();
    let (status, _) = send(app, "POST", "/resources", broken).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_filter_with_no_answers_warns_in_a_header() {
    let (_d, app, _patient) = setup().await;
    let request = Request::builder()
        .uri("/resources/Patient?where=brithDate%20%3D%20%401980-01-01")
        .header("host", "127.0.0.1:8747")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("x-phv-warning").is_some());
}

#[tokio::test]
async fn validate_does_not_write() {
    let (_d, app, patient) = setup().await;
    let body = serde_json::to_vec(&lab(&patient, "1.50")).unwrap();
    let (status, out) = send(app.clone(), "POST", "/validate", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&out)["resourceType"], "OperationOutcome");
    let (_, listed) = send(app, "GET", "/resources/Observation", vec![]).await;
    assert!(listed.is_empty());
}

#[tokio::test]
async fn sources_can_be_uploaded_and_linked() {
    let (_d, app, patient) = setup().await;
    let (status, out) = send(
        app.clone(),
        "POST",
        "/sources?filename=lab.pdf",
        b"%PDF synthetic".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let sha = json(&out)["sha256"].as_str().unwrap().to_string();

    let body = serde_json::to_vec(&lab(&patient, "1.50")).unwrap();
    let uri = format!("/resources?source={sha}&agent=test&recorded_at=2026-11-06T12:00:00Z");
    let (status, out) = send(app, "POST", &uri, body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&out)["created"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn cli_and_api_write_identical_files_for_the_same_input() {
    let (_a, via_api) = phv().await;
    let patient = add_patient(&via_api).await;
    let (_b, other) = phv().await;
    // The same Patient content gets the same ID, so the two vaults match.
    add_patient(&other).await;
    let body = serde_json::to_vec(&lab(&patient, "1.50")).unwrap();
    let root = other.vault().root().to_path_buf();

    let via_api = Arc::new(via_api);
    let (status, _) = send(router(via_api.clone()), "POST", "/resources", body.clone()).await;
    assert_eq!(status, StatusCode::OK);

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(["--vault", root.to_str().unwrap(), "add", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), &body).unwrap();
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());

    let read = |dir: &std::path::Path| {
        std::fs::read_to_string(dir.join("records/Observation.ndjson")).unwrap()
    };
    assert_eq!(read(via_api.vault().root()), read(&root));
}

#[tokio::test]
async fn path_traversal_in_a_type_is_refused() {
    let (_d, app, _p) = setup().await;
    let (status, _) = send(app, "GET", "/resources/..%2F..%2Fetc", vec![]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

async fn with_headers(headers: &[(&str, &str)]) -> StatusCode {
    let (_d, app, _p) = setup().await;
    let mut builder = Request::builder().uri("/resources/Patient");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    app.oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn only_local_host_and_origin_headers_are_allowed() {
    assert_eq!(
        with_headers(&[("host", "127.0.0.1:8747")]).await,
        StatusCode::OK
    );
    assert_eq!(
        with_headers(&[("host", "localhost:8747")]).await,
        StatusCode::OK
    );
    assert_eq!(
        with_headers(&[
            ("host", "localhost:8747"),
            ("origin", "http://localhost:3000")
        ])
        .await,
        StatusCode::OK
    );
    assert_eq!(
        with_headers(&[("host", "evil.example")]).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        with_headers(&[
            ("host", "localhost:8747"),
            ("origin", "https://evil.example")
        ])
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(with_headers(&[]).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn stray_provenance_options_are_refused_and_write_nothing() {
    let (_d, app, patient) = setup().await;
    let body = serde_json::to_vec(&lab(&patient, "1.50")).unwrap();
    for uri in [
        "/resources?agent=x",
        "/resources?recorded_at=2026-11-06T12:00:00Z",
    ] {
        let (status, out) = send(app.clone(), "POST", uri, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
        assert_eq!(json(&out)["resourceType"], "OperationOutcome");
    }
    let (_, listed) = send(app, "GET", "/resources/Observation", vec![]).await;
    assert!(listed.is_empty());
}

async fn assert_outcome(app: axum::Router, method: &str, uri: &str, expected: StatusCode) {
    let (status, out) = send(app, method, uri, vec![]).await;
    assert_eq!(status, expected, "{method} {uri}");
    assert_eq!(
        json(&out)["resourceType"],
        "OperationOutcome",
        "{method} {uri}"
    );
}

#[tokio::test]
async fn framework_errors_are_operation_outcomes_too() {
    let (_d, app, _p) = setup().await;
    let bad = StatusCode::BAD_REQUEST;
    assert_outcome(app.clone(), "POST", "/sources", bad).await;
    assert_outcome(app.clone(), "POST", "/resources?dry_run=1", bad).await;
    assert_outcome(app.clone(), "PUT", "/resources/Patient/x?dry_run=1", bad).await;
    assert_outcome(
        app.clone(),
        "PATCH",
        "/resources",
        StatusCode::METHOD_NOT_ALLOWED,
    )
    .await;
    assert_outcome(
        app.clone(),
        "DELETE",
        "/validate",
        StatusCode::METHOD_NOT_ALLOWED,
    )
    .await;
    assert_outcome(app, "GET", "/nowhere", StatusCode::NOT_FOUND).await;
}

async fn with_origin(host: &str, origin: Option<&str>) -> StatusCode {
    let (_d, app, _p) = setup().await;
    let mut builder = Request::builder()
        .uri("/resources/Patient")
        .header("host", host);
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    app.oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn host_and_origin_are_parsed_strictly() {
    let forbidden = StatusCode::FORBIDDEN;
    for host in [
        "localhost:80@evil.com",
        "localhost:",
        "localhost:80a",
        "localhost:80:81",
        "127.0.0.1.evil.com",
        "evil.com#localhost:80",
        "user@localhost",
    ] {
        assert_eq!(with_origin(host, None).await, forbidden, "host {host}");
    }
    for origin in [
        "http://localhost:80@evil.com",
        "http://localhost:80a",
        "http://localhost.evil.com",
        "http://127.0.0.1:@evil.com",
    ] {
        assert_eq!(
            with_origin("localhost:8747", Some(origin)).await,
            forbidden,
            "origin {origin}"
        );
    }
    assert_eq!(with_origin("localhost", None).await, StatusCode::OK);
    assert_eq!(
        with_origin("127.0.0.1:8747", Some("http://127.0.0.1:3000")).await,
        StatusCode::OK
    );
}
