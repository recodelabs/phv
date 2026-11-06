mod common;

use common::{add_patient, lab, phv, records};
use phv::PhvError;
use phv::write::AddOptions;
use serde_json::{Value, json};

fn bundle(patient: &str) -> Value {
    let text = std::fs::read_to_string("tests/fixtures/lab-bundle.json").unwrap();
    serde_json::from_str(&text.replace("PATIENT_REF", patient)).unwrap()
}

#[tokio::test]
async fn add_writes_a_canonical_line_and_keeps_decimals() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let report = phv
        .add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    assert_eq!(report.created.len(), 1);
    let file = records(&phv, "Observation");
    assert!(
        file.starts_with(r#"{"resourceType":"Observation","id":""#),
        "{file}"
    );
    assert!(file.contains(r#""value":1.50"#), "{file}");
    assert!(file.ends_with("}\n"));
}

#[tokio::test]
async fn adding_the_same_content_twice_changes_nothing() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    phv.add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    let before = records(&phv, "Observation");
    let report = phv
        .add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    assert!(report.created.is_empty());
    assert_eq!(report.unchanged.len(), 1);
    assert_eq!(records(&phv, "Observation"), before);
}

#[tokio::test]
async fn a_bundle_is_written_with_working_references() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let report = phv
        .add(&bundle(&patient), &AddOptions::default())
        .await
        .unwrap();
    assert_eq!(report.created.len(), 3);
    assert_eq!(records(&phv, "Observation").lines().count(), 2);
    assert_eq!(records(&phv, "DiagnosticReport").lines().count(), 1);
    assert!(phv.check().await.unwrap().iter().all(|i| !i.is_error()));
}

#[tokio::test]
async fn one_invalid_resource_writes_nothing_at_all() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let mut b = bundle(&patient);
    b["entry"][2]["resource"]
        .as_object_mut()
        .unwrap()
        .remove("status");
    let err = phv.add(&b, &AddOptions::default()).await.unwrap_err();
    assert!(matches!(err, PhvError::Invalid(_)), "{err:?}");
    assert_eq!(records(&phv, "Observation"), "");
    assert_eq!(records(&phv, "DiagnosticReport"), "");
}

#[tokio::test]
async fn a_broken_reference_is_rejected() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let err = phv
        .add(
            &lab("Patient/does-not-exist", "1.50"),
            &AddOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, PhvError::Conflict(_)), "{err:?}");
    assert_eq!(records(&phv, "Observation"), "");
}

#[tokio::test]
async fn an_unresolved_urn_reference_is_rejected() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let mut r = lab(&patient, "1.50");
    r["basedOn"] = json!([{"reference":"urn:uuid:99999999-9999-4999-8999-999999999999"}]);
    assert!(matches!(
        phv.add(&r, &AddOptions::default()).await.unwrap_err(),
        PhvError::Conflict(_)
    ));
    r["basedOn"] = json!([{"reference":"urn:oid:1.2.3"}]);
    assert!(matches!(
        phv.add(&r, &AddOptions::default()).await.unwrap_err(),
        PhvError::Conflict(_)
    ));
}

#[tokio::test]
async fn a_display_only_reference_and_external_urls_are_allowed() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let mut r = lab(&patient, "1.50");
    r["performer"] =
        json!([{"display":"Example Labs"},{"reference":"https://example.org/fhir/Organization/1"}]);
    phv.add(&r, &AddOptions::default()).await.unwrap();
}

#[tokio::test]
async fn a_second_patient_is_rejected() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let other = json!({"resourceType":"Patient","name":[{"family":"Other"}]});
    assert!(matches!(
        phv.add(&other, &AddOptions::default()).await.unwrap_err(),
        PhvError::Conflict(_)
    ));
}

#[tokio::test]
async fn dry_run_reports_but_writes_nothing() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let options = AddOptions {
        dry_run: true,
        ..AddOptions::default()
    };
    let report = phv.add(&lab(&patient, "1.50"), &options).await.unwrap();
    assert_eq!(report.created.len(), 1);
    assert_eq!(records(&phv, "Observation"), "");
}

#[tokio::test]
async fn replace_keeps_the_id_and_changes_one_line() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let a = phv
        .add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap()
        .created[0]
        .clone();
    let b = phv
        .add(&lab(&patient, "2.00"), &AddOptions::default())
        .await
        .unwrap()
        .created[0]
        .clone();
    let id = a.split_once('/').unwrap().1;
    let untouched = phv
        .vault()
        .get("Observation", b.split_once('/').unwrap().1)
        .unwrap()
        .unwrap();

    let report = phv
        .replace("Observation", id, &lab(&patient, "1.75"), false)
        .await
        .unwrap();
    assert_eq!(report.updated, vec![a.clone()]);
    assert!(
        phv.vault()
            .get("Observation", id)
            .unwrap()
            .unwrap()
            .contains(r#""value":1.75"#)
    );
    assert_eq!(
        phv.vault()
            .get("Observation", b.split_once('/').unwrap().1)
            .unwrap()
            .unwrap(),
        untouched
    );
}

#[tokio::test]
async fn re_adding_content_that_was_replaced_is_a_conflict_not_unchanged() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let reference = phv
        .add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap()
        .created[0]
        .clone();
    let id = reference.split_once('/').unwrap().1;
    phv.replace("Observation", id, &lab(&patient, "1.75"), false)
        .await
        .unwrap();
    let before = records(&phv, "Observation");

    let err = phv
        .add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap_err();
    assert!(
        err.issues().iter().any(|i| i.code == "PHV-REPLACED"),
        "{err:?}"
    );
    assert_eq!(records(&phv, "Observation"), before);
}

#[tokio::test]
async fn known_limit_re_adding_the_new_content_creates_a_second_record() {
    // Stable IDs in the importer work will address this. Until then, correct records with replace only.
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let reference = phv
        .add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap()
        .created[0]
        .clone();
    let id = reference.split_once('/').unwrap().1;
    phv.replace("Observation", id, &lab(&patient, "1.75"), false)
        .await
        .unwrap();
    phv.add(&lab(&patient, "1.75"), &AddOptions::default())
        .await
        .unwrap();
    assert_eq!(records(&phv, "Observation").lines().count(), 2);
}

#[tokio::test]
async fn replace_refuses_a_missing_resource_or_a_different_type() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    assert!(matches!(
        phv.replace("Observation", "nope", &lab(&patient, "1"), false)
            .await
            .unwrap_err(),
        PhvError::NotFound(_)
    ));
    let id = patient.split_once('/').unwrap().1;
    assert!(matches!(
        phv.replace("Patient", id, &lab(&patient, "1"), false)
            .await
            .unwrap_err(),
        PhvError::Usage(_)
    ));
}

#[tokio::test]
async fn remove_refuses_a_referenced_resource_then_allows_it() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let obs = phv
        .add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap()
        .created[0]
        .clone();
    let patient_id = patient.split_once('/').unwrap().1;
    assert!(matches!(
        phv.remove("Patient", patient_id).await.unwrap_err(),
        PhvError::Conflict(_)
    ));

    phv.remove("Observation", obs.split_once('/').unwrap().1)
        .await
        .unwrap();
    phv.remove("Patient", patient_id).await.unwrap();
    assert_eq!(records(&phv, "Patient"), "");
}

#[tokio::test]
async fn validate_submission_reports_problems_without_writing() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let mut r = lab(&patient, "1.50");
    r.as_object_mut().unwrap().remove("status");
    let issues = phv.validate_submission(&r).await.unwrap();
    assert!(issues.iter().any(|i| i.is_error()));
    assert_eq!(records(&phv, "Observation"), "");
    assert!(
        phv.validate_submission(&lab("Patient/none", "1"))
            .await
            .unwrap()
            .iter()
            .any(|i| i.code == "PHV-REF")
    );
}
