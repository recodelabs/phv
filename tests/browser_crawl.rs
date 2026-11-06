//! Follows every link from the home page against a copy of the sample vault.

mod browser_common;

use axum::http::StatusCode;
use browser_common::{copy_fixture, get};
use phv::browser::{Vaults, router};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn snapshot(root: &Path, dir: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            snapshot(root, &path, into);
        } else {
            let key = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            into.insert(key, std::fs::read(&path).unwrap());
        }
    }
}

fn links(html: &str) -> Vec<String> {
    html.split(r#"href=""#)
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|href| href.starts_with('/'))
        .map(|href| href.replace("&#38;", "&"))
        .collect()
}

#[tokio::test]
async fn crawling_every_page_finds_no_dead_links_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    let mut before = BTreeMap::new();
    snapshot(dir.path(), dir.path(), &mut before);

    let app = router(Vaults::single(dir.path().to_path_buf()));
    let mut seen = BTreeSet::new();
    let mut queue = vec!["/v/vault/overview".to_string()];
    while let Some(uri) = queue.pop() {
        if !seen.insert(uri.clone()) {
            continue;
        }
        let reply = get(&app, &uri).await;
        assert_eq!(reply.status, StatusCode::OK, "{uri}");
        if reply.header("content-type").starts_with("text/html") {
            queue.extend(links(&reply.text()));
        }
    }
    assert!(seen.len() > 40, "only reached {} pages", seen.len());
    for expected in [
        "/v/vault/vitals",
        "/v/vault/labs/reports/report-2026",
        "/v/vault/documents?category=receipt",
        "/v/vault/fhir/Encounter/encounter-1",
    ] {
        assert!(seen.contains(expected), "never reached {expected}");
    }

    let mut after = BTreeMap::new();
    snapshot(dir.path(), dir.path(), &mut after);
    assert_eq!(before, after, "the vault changed");
    assert!(
        !dir.path().join(".phv").exists(),
        "the browser created .phv"
    );
}
