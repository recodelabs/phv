#![allow(dead_code)]

use serde_json::Value;
use std::path::{Path, PathBuf};

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
