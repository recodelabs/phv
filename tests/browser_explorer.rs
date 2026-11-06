//! The server's foundations and the All FHIR pages.

mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, copy_fixture, fixture_path, get, send};
use phv::browser::model::explorer::{page_count, page_of};
use phv::browser::{Vaults, router};
use std::fs;
use std::path::Path;

const CONFIG: &str = r#"
default = "maya"
[vaults]
maya = "/nowhere/maya"
ben = "/nowhere/ben"
"#;

#[test]
fn a_registered_name_opens_the_whole_registry_on_that_vault() {
    let vaults = Vaults::resolve(Some(Path::new("ben")), None, Some(CONFIG)).unwrap();
    assert_eq!(vaults.default_name(), "ben");
    assert_eq!(vaults.entries().len(), 2);
}

#[test]
fn a_path_is_the_only_entry() {
    let vaults = Vaults::resolve(None, Some("/some/path"), Some(CONFIG)).unwrap();
    assert_eq!(vaults.entries().len(), 1);
    assert_eq!(vaults.entries()[0].path, Path::new("/some/path"));
}

#[test]
fn no_flag_uses_the_registry_default() {
    let vaults = Vaults::resolve(None, None, Some(CONFIG)).unwrap();
    assert_eq!(vaults.default_name(), "maya");
}

#[test]
fn no_vault_at_all_is_an_error() {
    assert!(Vaults::resolve(None, None, None).is_err());
}

#[tokio::test]
async fn home_redirects_to_the_default_vault() {
    let reply = get(&app(), "/").await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.header("location"), "/v/vault/overview");
}

#[tokio::test]
async fn pages_send_the_security_headers() {
    let reply = get(&app(), "/v/vault/fhir").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(
        reply
            .header("content-security-policy")
            .contains("default-src 'none'")
    );
    assert!(
        reply
            .header("content-security-policy")
            .contains("script-src 'self'")
    );
    assert_eq!(reply.header("cache-control"), "no-store");
    assert_eq!(reply.header("x-content-type-options"), "nosniff");
    assert_eq!(reply.header("referrer-policy"), "no-referrer");
    assert_eq!(reply.header("vary"), "HX-Request");
}

#[tokio::test]
async fn assets_are_served_and_unknown_ones_are_not() {
    let app = app();
    let htmx = get(&app, "/assets/htmx.min.js").await;
    assert_eq!(htmx.status, StatusCode::OK);
    assert!(htmx.header("content-type").starts_with("text/javascript"));
    assert_eq!(get(&app, "/assets/style.css").await.status, StatusCode::OK);
    assert_eq!(
        get(&app, "/assets/secrets").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn htmx_is_configured_to_work_under_the_policy_and_keep_no_copies() {
    let html = get(&app(), "/v/vault/fhir").await.text();
    assert!(html.contains(r#""allowEval":false"#));
    assert!(html.contains(r#""selfRequestsOnly":true"#));
    assert!(html.contains(r#""includeIndicatorStyles":false"#));
    assert!(html.contains(r#""historyCacheSize":0"#));
}

#[tokio::test]
async fn other_hosts_and_origins_are_rejected() {
    let app = app();
    let cases: [&[(&str, &str)]; 3] = [
        &[("host", "evil.example")],
        &[("origin", "https://evil.example")],
        &[("origin", "null")],
    ];
    for headers in cases {
        let mut request = axum::http::Request::builder().uri("/v/vault/fhir");
        if !headers.iter().any(|(name, _)| *name == "host") {
            request = request.header("host", "localhost:8747");
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = tower::ServiceExt::oneshot(
            app.clone(),
            request.body(axum::body::Body::empty()).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{headers:?}");
    }
    let local = send(
        &app,
        "GET",
        "/v/vault/fhir",
        &[("origin", "http://localhost:8747")],
    )
    .await;
    assert_eq!(local.status, StatusCode::OK);
}

#[tokio::test]
async fn nothing_but_reads_is_allowed() {
    let app = app();
    for method in ["POST", "PUT", "DELETE", "PATCH"] {
        for uri in [
            "/v/vault/fhir",
            "/v/vault/fhir/Patient/patient-maya",
            "/nowhere",
        ] {
            let reply = send(&app, method, uri, &[]).await;
            assert_eq!(
                reply.status,
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {uri}"
            );
        }
    }
}

#[tokio::test]
async fn all_fhir_lists_types_with_counts() {
    let html = get(&app(), "/v/vault/fhir").await.text();
    assert!(html.contains(">Encounter</a>"));
    assert!(html.contains(">Observation</a>"));
}

#[tokio::test]
async fn a_type_lists_its_records() {
    let html = get(&app(), "/v/vault/fhir/Condition").await.text();
    assert!(html.contains("Asthma"));
    assert!(html.contains("condition-asthma"));
}

#[tokio::test]
async fn a_record_shows_its_json_exactly_and_links_references_that_resolve() {
    let html = get(&app(), "/v/vault/fhir/Observation/lab-creatinine-2026")
        .await
        .text();
    assert!(html.contains("1.50"), "{html}");
    assert!(
        html.contains(r#"href="/v/vault/fhir/Patient/patient-maya""#),
        "{html}"
    );
}

#[tokio::test]
async fn a_reference_that_does_not_resolve_is_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    fs::write(
        dir.path().join("records/Encounter.ndjson"),
        r#"{"resourceType":"Encounter","id":"e","subject":{"reference":"Patient/gone"}}
"#,
    )
    .unwrap();
    let app = router(Vaults::single(dir.path().to_path_buf()));
    let html = get(&app, "/v/vault/fhir/Encounter/e").await.text();
    assert!(html.contains("Patient/gone"));
    assert!(!html.contains("fhir/Patient/gone"));
}

#[tokio::test]
async fn unknown_vaults_types_and_ids_are_404() {
    let app = app();
    for uri in [
        "/v/nobody/fhir",
        "/v/vault/fhir/Nope",
        "/v/vault/fhir/..%2F..%2Fphv",
        "/v/vault/fhir/Patient/nobody",
        "/nowhere",
    ] {
        assert_eq!(get(&app, uri).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn record_text_is_escaped() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    fs::write(
        dir.path().join("records/Encounter.ndjson"),
        r#"{"resourceType":"Encounter","id":"e","type":[],"code":{"text":"<script>alert(1)</script>"}}
"#,
    )
    .unwrap();
    let app = router(Vaults::single(dir.path().to_path_buf()));
    for uri in ["/v/vault/fhir/Encounter", "/v/vault/fhir/Encounter/e"] {
        let html = get(&app, uri).await.text();
        assert!(!html.contains("<script>alert"), "{uri}");
        assert!(html.contains("alert(1)"), "{uri}");
    }
}

#[tokio::test]
async fn unreadable_lines_are_counted_and_the_rest_still_show() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    let path = dir.path().join("records/Condition.ndjson");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("{not json\n");
    fs::write(&path, text).unwrap();
    let app = router(Vaults::single(dir.path().to_path_buf()));
    let reply = get(&app, "/v/vault/fhir/Condition").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(
        reply.text().contains("1 records couldn&#39;t be read")
            || reply.text().contains("1 records couldn't be read")
    );
    assert!(reply.text().contains("Asthma"));
}

#[tokio::test]
async fn an_unavailable_vault_explains_why_and_others_keep_working() {
    let config = format!(
        "default = \"maya\"\n[vaults]\nmaya = {:?}\ngone = \"/nowhere/gone\"\n",
        fixture_path()
    );
    let app = router(Vaults::resolve(None, None, Some(&config)).unwrap());
    let good = get(&app, "/v/maya/fhir").await;
    assert_eq!(good.status, StatusCode::OK);
    assert!(
        good.text().contains("(unavailable)"),
        "the switcher marks it"
    );
    let bad = get(&app, "/v/gone/fhir").await;
    assert_eq!(bad.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(bad.text().contains("not a phv vault"));
    assert!(bad.text().contains(r#"href="/v/maya/overview""#));
}

#[tokio::test]
async fn the_switcher_is_hidden_with_one_vault() {
    assert!(
        !get(&app(), "/v/vault/fhir")
            .await
            .text()
            .contains("switcher")
    );
}

#[test]
fn pages_hold_two_hundred_records() {
    let items: Vec<usize> = (0..450).collect();
    assert_eq!(page_of(&items, 1).len(), 200);
    assert_eq!(page_of(&items, 3), (400..450).collect::<Vec<_>>());
    assert!(page_of(&items, 4).is_empty());
    assert_eq!(page_count(450), 3);
    assert_eq!(page_count(0), 1);
}
