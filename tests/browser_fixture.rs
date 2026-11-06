//! The sample vault must look like a real one, or the other browser tests prove little.

mod browser_common;

use browser_common::{copy_fixture, fixture_path};
use phv::canon::{canon_resource, parse_json};
use phv::store::{Vault, line_id};
use phv::write::Phv;

#[test]
fn every_line_is_canonical_and_sorted() {
    let vault = Vault::open(&fixture_path()).unwrap();
    for resource_type in vault.resource_types().unwrap() {
        let lines = vault.lines(&resource_type).unwrap();
        for line in &lines {
            assert_eq!(*line, canon_resource(&parse_json(line).unwrap()));
        }
        let ids: Vec<_> = lines.iter().map(|l| line_id(l).unwrap()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "{resource_type} is out of order");
    }
}

#[test]
fn source_files_are_named_for_their_hash() {
    for entry in std::fs::read_dir(fixture_path().join("sources")).unwrap() {
        let path = entry.unwrap().path();
        let hash = phv::canon::sha256_hex(&std::fs::read(&path).unwrap());
        assert_eq!(path.file_stem().unwrap().to_str().unwrap(), hash);
    }
}

#[tokio::test]
async fn every_record_passes_validation() {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture(dir.path());
    let vault = Vault::open(dir.path()).unwrap();
    let phv = Phv::new(vault.clone()).await.unwrap();
    // A vault holds one Patient, so the sample's own Patient must be checked against an empty one.
    let empty = Phv::new(Vault::init(&dir.path().join("empty")).unwrap())
        .await
        .unwrap();
    let mut problems = Vec::new();
    for resource_type in vault.resource_types().unwrap() {
        for line in vault.lines(&resource_type).unwrap() {
            let resource = parse_json(&line).unwrap();
            let checker = if resource_type == "Patient" {
                &empty
            } else {
                &phv
            };
            let issues = checker.validate_submission(&resource).await.unwrap();
            for issue in issues.iter().filter(|i| i.is_error()) {
                problems.push(format!("{}: {}", resource["id"], issue.message));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
