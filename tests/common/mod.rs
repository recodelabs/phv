#![allow(dead_code)]

use phv::store::Vault;
use phv::write::{AddOptions, Phv};
use serde_json::{Value, json};

pub async fn phv() -> (tempfile::TempDir, Phv) {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::init(&dir.path().join("data")).unwrap();
    (dir, Phv::new(vault).await.unwrap())
}

/// Adds the synthetic person and returns `Patient/<id>`.
pub async fn add_patient(phv: &Phv) -> String {
    let report = phv
        .add(
            &json!({"resourceType":"Patient","name":[{"family":"Example","given":["Test"]}],"birthDate":"1980-01-01"}),
            &AddOptions::default(),
        )
        .await
        .unwrap();
    report.created[0].clone()
}

pub fn lab(patient: &str, value: &str) -> Value {
    serde_json::from_str(&format!(
        r#"{{"resourceType":"Observation","status":"final","code":{{"text":"Creatinine"}},
        "subject":{{"reference":"{patient}"}},"effectiveDateTime":"2026-03-14",
        "valueQuantity":{{"value":{value},"unit":"mg/dL"}}}}"#
    ))
    .unwrap()
}

pub fn records(phv: &Phv, resource_type: &str) -> String {
    std::fs::read_to_string(
        phv.vault()
            .root()
            .join(format!("records/{resource_type}.ndjson")),
    )
    .unwrap_or_default()
}
