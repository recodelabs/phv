mod browser_common;

use axum::http::StatusCode;
use browser_common::{app, copy_fixture, fixture, fixture_path, get, send};
use phv::browser::model::documents::{chips, documents, is_sha256, source_file, source_of};
use phv::browser::{Vaults, router};
use serde_json::json;
use std::fs;

const LAB: &str = "1bb0ff96726fa961b697cbb7a3c9cf60d8b28c451054a3d044ba9095d4a0d412";
const RECEIPT: &str = "82f7258fe85a893e067ec1c27ac161dafd511455dce49426a6d973d4e5ce4479";

fn rows() -> Vec<phv::browser::model::documents::DocRow> {
    documents(
        &fixture("DocumentReference"),
        &fixture("Provenance"),
        &fixture_path(),
    )
}

#[test]
fn documents_are_newest_first_with_undated_last() {
    let ids: Vec<String> = rows().into_iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        ["document-lab", "document-receipt", "document-no-file"]
    );
}

#[test]
fn a_document_shows_its_category_file_and_record_count() {
    let rows = rows();
    assert_eq!(rows[0].title, "metabolic-panel.pdf");
    assert_eq!(rows[0].category_label, "Lab");
    assert_eq!(rows[0].record_count, 4);
    assert!(rows[0].has_file);
    assert_eq!(rows[1].category_label, "Receipt");
    assert_eq!(rows[1].record_count, 0);
}

#[test]
fn no_category_or_a_foreign_one_counts_as_uncategorized() {
    let foreign = [json!({
        "id": "d", "status": "current",
        "category": [{"coding": [{"system": "http://hospital.example", "code": "lab"}]}]
    })];
    let rows = documents(&foreign, &[], &fixture_path());
    assert_eq!(rows[0].category, "uncategorized");
    assert_eq!(rows[0].title, "Untitled document");
    assert_eq!(rows[0].date, "No date");
    assert!(!rows[0].has_file);
}

#[test]
fn chips_count_only_categories_in_use() {
    let chips = chips(&rows(), Some("receipt"));
    let shown: Vec<(&str, usize, bool)> = chips
        .iter()
        .map(|c| (c.label.as_str(), c.count, c.on))
        .collect();
    assert_eq!(
        shown,
        [
            ("All", 3, false),
            ("Lab", 1, false),
            ("Receipt", 1, true),
            ("Uncategorized", 1, false)
        ]
    );
}

#[test]
fn a_record_finds_its_source_through_provenance() {
    let provenance = fixture("Provenance");
    let source = source_of("Observation/lab-glucose-2026", &provenance).unwrap();
    assert_eq!(source.document_id, "document-lab");
    assert_eq!(source.agent.as_deref(), Some("phv-add-lab"));
    assert_eq!(source_of("Observation/vital-hr-2026", &provenance), None);
}

#[test]
fn hashes_must_be_exactly_64_lowercase_hex_characters() {
    assert!(is_sha256(LAB));
    assert!(!is_sha256(&LAB.to_uppercase()));
    assert!(!is_sha256(&LAB[..63]));
    assert!(!is_sha256("../../../../etc/passwd"));
}

#[cfg(unix)]
#[test]
fn a_symlink_in_sources_does_not_count() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    let outside = dir.path().join("outside.txt");
    fs::write(&outside, "secret").unwrap();
    let hash = "a".repeat(64);
    std::os::unix::fs::symlink(&outside, dir.path().join(format!("sources/{hash}.txt"))).unwrap();
    assert_eq!(source_file(dir.path(), &hash), None);
}

#[tokio::test]
async fn the_catalog_links_files_and_filters_by_category() {
    let app = app();
    let all = get(&app, "/v/vault/documents").await.text();
    assert!(all.contains(&format!("/v/vault/sources/{LAB}")));
    assert!(all.contains("No file stored"));
    let filtered = get(&app, "/v/vault/documents?category=receipt")
        .await
        .text();
    assert!(filtered.contains("pharmacy-receipt.txt"));
    assert!(!filtered.contains("metabolic-panel.pdf"));
}

#[tokio::test]
async fn a_category_returns_a_partial_for_htmx() {
    let reply = send(
        &app(),
        "GET",
        "/v/vault/documents?category=lab",
        &[("hx-request", "true")],
    )
    .await;
    assert!(!reply.text().contains("<html"));
    assert!(reply.text().contains("metabolic-panel.pdf"));
}

#[tokio::test]
async fn a_pdf_opens_in_the_browser_without_a_sandbox() {
    let reply = get(&app(), &format!("/v/vault/sources/{LAB}")).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header("content-type"), "application/pdf");
    assert_eq!(reply.header("content-disposition"), "inline");
    assert_eq!(
        reply.header("content-security-policy"),
        "frame-ancestors 'none'"
    );
    assert_eq!(reply.header("cache-control"), "no-store");
    assert_eq!(reply.header("x-content-type-options"), "nosniff");
    assert!(reply.bytes.starts_with(b"%PDF"));
}

#[tokio::test]
async fn plain_text_opens_sandboxed() {
    let reply = get(&app(), &format!("/v/vault/sources/{RECEIPT}")).await;
    assert_eq!(reply.header("content-type"), "text/plain; charset=utf-8");
    assert_eq!(reply.header("content-security-policy"), "sandbox");
}

#[tokio::test]
async fn html_and_svg_download_instead_of_opening() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    for (hash, extension) in [("b".repeat(64), "html"), ("c".repeat(64), "svg")] {
        fs::write(
            dir.path().join(format!("sources/{hash}.{extension}")),
            "<script>alert(1)</script>",
        )
        .unwrap();
        let app = router(Vaults::single(dir.path().to_path_buf()));
        let reply = get(&app, &format!("/v/vault/sources/{hash}")).await;
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.header("content-type"), "application/octet-stream");
        assert!(
            reply
                .header("content-disposition")
                .starts_with("attachment")
        );
        assert_eq!(reply.header("content-security-policy"), "sandbox");
    }
}

#[tokio::test]
async fn bad_or_unknown_hashes_are_404() {
    let app = app();
    for uri in [
        "/v/vault/sources/nope".to_string(),
        "/v/vault/sources/..%2F..%2Fphv.toml".to_string(),
        format!("/v/vault/sources/{}", "d".repeat(64)),
    ] {
        assert_eq!(get(&app, &uri).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
}
