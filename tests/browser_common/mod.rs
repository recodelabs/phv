#![allow(dead_code)]

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use phv::browser::{Vaults, router};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

pub fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-vault")
}

/// Every resource of a type in the sample vault.
pub fn fixture(resource_type: &str) -> Vec<Value> {
    let path = fixture_path().join(format!("records/{resource_type}.ndjson"));
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| phv::canon::parse_json(line).unwrap())
        .collect()
}

pub fn app() -> Router {
    router(Vaults::single(fixture_path()))
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl Reply {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    pub fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .map_or("", |value| value.to_str().unwrap())
    }
}

pub async fn send(app: &Router, method: &str, uri: &str, headers: &[(&str, &str)]) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8747");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    Reply {
        status,
        headers,
        bytes,
    }
}

pub async fn get(app: &Router, uri: &str) -> Reply {
    send(app, "GET", uri, &[]).await
}

/// A private copy of the sample vault that a test can change.
pub fn copy_fixture(to: &Path) {
    copy_dir(&fixture_path(), to);
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
