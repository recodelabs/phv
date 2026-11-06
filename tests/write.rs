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
async fn a_wrongly_cased_type_is_a_usage_error_on_remove_and_replace_and_changes_nothing() {
    // Whatever the disk does with `patient.ndjson`, the type name is checked first.
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let obs = phv
        .add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap()
        .created[0]
        .clone();
    let patient_id = patient.split_once('/').unwrap().1;
    let obs_id = obs.split_once('/').unwrap().1;
    let before_patient = records(&phv, "Patient");
    let before_obs = records(&phv, "Observation");
    for bad in ["patient", "PATIENT", "observation", "OBSERVATION"] {
        let id = if bad.eq_ignore_ascii_case("patient") {
            patient_id
        } else {
            obs_id
        };
        assert!(
            matches!(phv.remove(bad, id).await.unwrap_err(), PhvError::Usage(_)),
            "remove {bad}"
        );
        let input = if bad.eq_ignore_ascii_case("patient") {
            json!({"resourceType":"Patient","name":[{"family":"Other"}]})
        } else {
            lab(&patient, "9")
        };
        assert!(
            matches!(
                phv.replace(bad, id, &input, false).await.unwrap_err(),
                PhvError::Usage(_)
            ),
            "replace {bad}"
        );
    }
    assert_eq!(records(&phv, "Patient"), before_patient);
    assert_eq!(records(&phv, "Observation"), before_obs);
}

#[tokio::test]
async fn a_reference_with_a_wrongly_cased_type_is_refused() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let id = patient.split_once('/').unwrap().1;
    for bad in [format!("PATIENT/{id}"), format!("patient/{id}")] {
        let err = phv
            .add(&lab(&bad, "1.50"), &AddOptions::default())
            .await
            .unwrap_err();
        assert!(
            matches!(&err, PhvError::Conflict(issues) if issues.iter().any(|i| i.code == "PHV-REF")),
            "{bad}: {err}"
        );
        assert_eq!(records(&phv, "Observation"), "", "{bad}");
    }
    let issues = phv
        .validate_submission(&lab(&format!("PATIENT/{id}"), "1"))
        .await
        .unwrap();
    assert!(issues.iter().any(|i| i.code == "PHV-REF"), "{issues:?}");
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

fn write_records(phv: &phv::write::Phv, resource_type: &str, text: &str) {
    std::fs::write(
        phv.vault()
            .root()
            .join(format!("records/{resource_type}.ndjson")),
        text,
    )
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_waiting_write_does_not_block_the_runtime() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let phv = std::sync::Arc::new(phv);
    let guard = phv.vault().lock_async().await.unwrap();
    let other = phv.clone();
    let task = tokio::spawn(async move {
        other
            .add(&lab(&patient, "1.50"), &AddOptions::default())
            .await
    });
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(!task.is_finished());
    drop(guard);
    task.await.unwrap().unwrap();
    assert_eq!(records(&phv, "Observation").lines().count(), 1);
}

#[tokio::test]
async fn remove_finds_references_written_with_spaces_or_escapes() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let id = patient.split_once('/').unwrap().1.to_string();
    for (n, reference) in [patient.clone(), patient.replace('/', "\\/")]
        .iter()
        .enumerate()
    {
        write_records(
            &phv,
            "Observation",
            &format!(
                "{{\"resourceType\":\"Observation\",\"id\":\"o1\",\"subject\":{{\"reference\": \"{reference}\"}}}}\n"
            ),
        );
        let err = phv.remove("Patient", &id).await.unwrap_err();
        assert!(matches!(err, PhvError::Conflict(_)), "{n}: {err:?}");
        assert_ne!(records(&phv, "Patient"), "");
    }
    write_records(
        &phv,
        "Observation",
        &format!(
            "{{\"resourceType\":\"Observation\",\"id\":\"o1\",\"subject\":{{\"reference\":\"{patient}\"}}}}\n"
        ),
    );
    assert!(matches!(
        phv.remove("Patient", &id).await.unwrap_err(),
        PhvError::Conflict(_)
    ));
}

#[tokio::test]
async fn remove_refuses_when_a_stored_line_cannot_be_read() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    write_records(
        &phv,
        "Observation",
        "{\"resourceType\":\"Observation\",\"id\":\"o1\",\n",
    );
    let err = phv
        .remove("Patient", patient.split_once('/').unwrap().1)
        .await
        .unwrap_err();
    assert!(matches!(err, PhvError::Conflict(_)), "{err:?}");
    let issue = &err.issues()[0];
    assert!(
        issue.path.contains("Observation.ndjson") && issue.path.contains('1'),
        "{issue:?}"
    );
}

#[tokio::test]
async fn remove_ignores_a_self_reference_but_refuses_a_cycle() {
    let (_d, phv) = phv().await;
    write_records(
        &phv,
        "Observation",
        "{\"resourceType\":\"Observation\",\"id\":\"o1\",\"hasMember\":[{\"reference\":\"Observation/o1\"}]}\n",
    );
    phv.remove("Observation", "o1").await.unwrap();
    assert_eq!(records(&phv, "Observation"), "");

    write_records(
        &phv,
        "Observation",
        concat!(
            "{\"resourceType\":\"Observation\",\"id\":\"o1\",\"hasMember\":[{\"reference\":\"Observation/o2\"}]}\n",
            "{\"resourceType\":\"Observation\",\"id\":\"o2\",\"hasMember\":[{\"reference\":\"Observation/o1\"}]}\n"
        ),
    );
    let err = phv.remove("Observation", "o1").await.unwrap_err();
    assert!(matches!(err, PhvError::Conflict(_)), "{err:?}");
    assert!(err.issues()[0].message.contains("phv replace"), "{err:?}");
}

#[tokio::test]
async fn malformed_relative_references_are_rejected() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    for bad in [
        "patient/abc",
        "Patient/john_doe",
        "Patient/abc/_history/1",
        "Patient/",
        "Patient",
        "../x/abc",
        "Patient/x y",
    ] {
        let mut r = lab(&patient, "1.50");
        r["basedOn"] = json!([{"reference": bad}]);
        let err = phv.add(&r, &AddOptions::default()).await.unwrap_err();
        assert!(matches!(err, PhvError::Conflict(_)), "{bad}: {err:?}");
        assert!(err.issues().iter().any(|i| i.code == "PHV-REF"), "{bad}");
    }
    assert_eq!(records(&phv, "Observation"), "");
    let mut r = lab(&patient, "1.50");
    r["basedOn"] = json!([{"reference":"#c1"}]);
    r["performer"] = json!([{"reference":"https://example.org/fhir/Organization/1"}]);
    r["contained"] = json!([{"resourceType":"Organization","id":"c1","name":"Example Labs"}]);
    phv.add(&r, &AddOptions::default()).await.unwrap();
}

#[tokio::test]
async fn dry_run_reports_a_write_that_would_fail() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let dry = AddOptions {
        dry_run: true,
        ..AddOptions::default()
    };

    let crlf = "{\"resourceType\":\"Observation\",\"id\":\"a\"}\r\n";
    write_records(&phv, "Observation", crlf);
    assert!(matches!(
        phv.add(&lab(&patient, "1.50"), &dry).await.unwrap_err(),
        PhvError::Conflict(_)
    ));
    assert!(matches!(
        phv.replace("Observation", "a", &lab(&patient, "1.50"), true)
            .await
            .unwrap_err(),
        PhvError::Conflict(_)
    ));
    assert_eq!(records(&phv, "Observation"), crlf);

    let unsorted = "{\"resourceType\":\"Observation\",\"id\":\"b\"}\n{\"resourceType\":\"Observation\",\"id\":\"a\"}\n";
    write_records(&phv, "Observation", unsorted);
    assert!(matches!(
        phv.add(&lab(&patient, "1.50"), &dry).await.unwrap_err(),
        PhvError::Conflict(_)
    ));
    assert!(matches!(
        phv.replace("Observation", "a", &lab(&patient, "1.50"), true)
            .await
            .unwrap_err(),
        PhvError::Conflict(_)
    ));
    assert_eq!(records(&phv, "Observation"), unsorted);
}

#[tokio::test]
async fn the_one_patient_rule_applies_only_when_a_patient_is_submitted() {
    let (_d, phv) = phv().await;
    write_records(
        &phv,
        "Patient",
        "{\"resourceType\":\"Patient\",\"id\":\"p1\"}\n{\"resourceType\":\"Patient\",\"id\":\"p2\"}\n",
    );
    phv.add(&lab("Patient/p1", "1.50"), &AddOptions::default())
        .await
        .unwrap();
    let other = json!({"resourceType":"Patient","name":[{"family":"Other"}]});
    assert!(matches!(
        phv.add(&other, &AddOptions::default()).await.unwrap_err(),
        PhvError::Conflict(_)
    ));
}

#[tokio::test]
async fn agent_or_recorded_at_without_a_source_is_refused_and_writes_nothing() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    for options in [
        AddOptions {
            agent: Some("x".into()),
            ..AddOptions::default()
        },
        AddOptions {
            recorded_at: Some("2026-11-06T12:00:00Z".into()),
            ..AddOptions::default()
        },
    ] {
        let error = phv.add(&lab(&patient, "1.50"), &options).await.unwrap_err();
        assert!(matches!(error, PhvError::Usage(_)), "{error:?}");
    }
    assert_eq!(records(&phv, "Observation"), "");
}
