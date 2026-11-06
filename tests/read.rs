mod common;

use common::{add_patient, lab, phv};
use phv::PhvError;
use phv::write::AddOptions;

#[tokio::test]
async fn get_returns_the_stored_line_exactly() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let reference = phv
        .add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap()
        .created[0]
        .clone();
    let id = reference.split_once('/').unwrap().1;
    let line = phv.get("Observation", id).unwrap();
    assert!(line.contains(r#""value":1.50"#), "{line}");
}

#[tokio::test]
async fn get_reports_a_missing_resource() {
    let (_d, phv) = phv().await;
    assert!(matches!(
        phv.get("Observation", "nope").unwrap_err(),
        PhvError::NotFound(_)
    ));
}

#[tokio::test]
async fn list_without_a_filter_returns_everything_in_id_order() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    for value in ["1.50", "2.00", "3.25"] {
        phv.add(&lab(&patient, value), &AddOptions::default())
            .await
            .unwrap();
    }
    let lines = phv.search("Observation", None).await.unwrap().lines;
    assert_eq!(lines.len(), 3);
    let mut sorted = lines.clone();
    sorted.sort();
    assert_eq!(lines, sorted);
}

#[tokio::test]
async fn list_filters_with_fhirpath() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    for value in ["1.50", "2.00", "3.25"] {
        phv.add(&lab(&patient, value), &AddOptions::default())
            .await
            .unwrap();
    }
    assert_eq!(
        phv.search("Observation", Some("valueQuantity.value > 1.9"))
            .await
            .unwrap()
            .lines
            .len(),
        2
    );
    assert_eq!(
        phv.search("Observation", Some("status = 'nope'"))
            .await
            .unwrap()
            .lines
            .len(),
        0
    );
    // A filter that evaluates to empty does not match.
    assert_eq!(
        phv.search("Observation", Some("missing.field = 1"))
            .await
            .unwrap()
            .lines
            .len(),
        0
    );
}

#[tokio::test]
async fn a_bad_expression_is_a_usage_error() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    phv.add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    assert!(matches!(
        phv.search("Observation", Some("((")).await.unwrap_err(),
        PhvError::Usage(_)
    ));
}

#[tokio::test]
async fn listing_an_empty_type_is_empty_not_an_error() {
    let (_d, phv) = phv().await;
    assert!(
        phv.search("Condition", None)
            .await
            .unwrap()
            .lines
            .is_empty()
    );
}

#[tokio::test]
async fn unknown_or_abstract_types_are_usage_errors() {
    let (_d, phv) = phv().await;
    for bad in ["Observaton", "observation", "Quantity", "DomainResource"] {
        assert!(
            matches!(phv.get(bad, "x").unwrap_err(), PhvError::Usage(_)),
            "get {bad}"
        );
        assert!(
            matches!(phv.search(bad, None).await.unwrap_err(), PhvError::Usage(_)),
            "search {bad}"
        );
    }
    // A valid type with no file is an empty list, and a missing record is NotFound.
    assert!(
        phv.search("Condition", None)
            .await
            .unwrap()
            .lines
            .is_empty()
    );
    assert!(matches!(
        phv.get("Condition", "x").unwrap_err(),
        PhvError::NotFound(_)
    ));
}

#[tokio::test]
async fn an_evaluation_error_names_the_record() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let mut observation = lab(&patient, "1.50");
    observation["code"] = serde_json::json!({"coding":[
        {"system":"http://loinc.org","code":"2160-0"},
        {"system":"http://loinc.org","code":"38483-4"}
    ]});
    let reference = phv
        .add(&observation, &AddOptions::default())
        .await
        .unwrap()
        .created[0]
        .clone();
    let err = phv
        .search("Observation", Some("code.coding.code.single() = '2160-0'"))
        .await
        .unwrap_err();
    assert!(matches!(err, PhvError::Usage(_)), "{err}");
    assert!(err.to_string().contains(&reference), "{err}");
}

#[tokio::test]
async fn an_unreadable_line_names_its_line_number() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    phv.add(&lab(&patient, "1.50"), &AddOptions::default())
        .await
        .unwrap();
    let path = phv.vault().root().join("records/Observation.ndjson");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push('\n');
    std::fs::write(&path, &text).unwrap();
    let err = phv
        .search("Observation", Some("status = 'final'"))
        .await
        .unwrap_err();
    assert!(matches!(err, PhvError::Usage(_)), "{err}");
    let message = err.to_string();
    assert!(message.contains("line 2"), "{message}");
    assert!(message.contains("run phv check"), "{message}");

    text.truncate(text.len() - 1);
    text.push_str("{\"resourceType\":\"Observation\",\"id\":\"cut\n");
    std::fs::write(&path, &text).unwrap();
    let err = phv
        .search("Observation", Some("status = 'final'"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("line 2"), "{err}");
}

async fn three_labs(phv: &phv::write::Phv) -> String {
    let patient = add_patient(phv).await;
    for value in ["1.50", "2.00", "3.25"] {
        phv.add(&lab(&patient, value), &AddOptions::default())
            .await
            .unwrap();
    }
    patient
}

#[tokio::test]
async fn a_filter_that_answers_every_record_has_no_warning() {
    let (_d, phv) = phv().await;
    three_labs(&phv).await;
    let result = phv
        .search("Observation", Some("valueQuantity.value > 1.9"))
        .await
        .unwrap();
    assert_eq!(
        (result.lines.len(), result.unanswered, result.total),
        (2, 0, 3)
    );
    assert!(result.warning().is_none());
    let all = phv.search("Observation", None).await.unwrap();
    assert_eq!((all.lines.len(), all.unanswered, all.total), (3, 0, 3));
    assert!(all.warning().is_none());
}

#[tokio::test]
async fn a_typo_in_an_element_name_is_reported() {
    let (_d, phv) = phv().await;
    three_labs(&phv).await;
    let result = phv
        .search("Observation", Some("valueQuantty.value > 1"))
        .await
        .unwrap();
    assert!(result.lines.is_empty());
    assert_eq!((result.unanswered, result.total), (3, 3));
    let warning = result.warning().unwrap();
    assert!(warning.contains("3 of 3 records"), "{warning}");
    assert!(warning.contains("no true or false answer"), "{warning}");
    assert!(warning.contains("check element names"), "{warning}");
    assert!(warning.contains(".toDate()"), "{warning}");
    assert!(warning.is_ascii() && !warning.contains('\n'), "{warning}");
}

#[tokio::test]
async fn a_non_boolean_filter_is_reported() {
    let (_d, phv) = phv().await;
    three_labs(&phv).await;
    let result = phv.search("Observation", Some("status")).await.unwrap();
    assert!(result.lines.is_empty());
    assert_eq!((result.unanswered, result.total), (3, 3));
}

#[tokio::test]
async fn a_filter_on_an_element_some_records_lack_gives_a_partial_count() {
    let (_d, phv) = phv().await;
    let patient = three_labs(&phv).await;
    let mut text = lab(&patient, "9");
    text.as_object_mut().unwrap().remove("valueQuantity");
    text["valueString"] = serde_json::json!("positive");
    phv.add(&text, &AddOptions::default()).await.unwrap();
    let result = phv
        .search("Observation", Some("valueQuantity.value > 1.9"))
        .await
        .unwrap();
    assert_eq!(result.lines.len(), 2);
    assert_eq!((result.unanswered, result.total), (1, 4));
    assert!(
        result
            .warning()
            .unwrap()
            .starts_with("1 of 4 records gave no true or false answer")
    );
}

// Pins an upstream bug in octofhir-fhirpath 0.5.8: `>=` and `<=` against a date literal give
// no answer, so the record is left out and counted as unanswered. `.toDate()` works, also for
// date-times. If an upgrade fixes this, the first assertions fail, which tells us to update the
// docs and drop the workaround advice.
#[tokio::test]
async fn known_limit_date_comparison_needs_todate() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    let mut dateonly = lab(&patient, "1.50");
    phv.add(&dateonly, &AddOptions::default()).await.unwrap();
    dateonly["effectiveDateTime"] = serde_json::json!("2026-03-14T10:00:00Z");
    phv.add(&dateonly, &AddOptions::default()).await.unwrap();

    for op in [">=", "<="] {
        let bare = phv
            .search(
                "Observation",
                Some(&format!("effectiveDateTime {op} @2026-03-14")),
            )
            .await
            .unwrap();
        assert!(bare.lines.is_empty(), "bare {op} now works upstream");
        assert!(bare.unanswered >= 1, "bare {op} now answers upstream");

        let fixed = phv
            .search(
                "Observation",
                Some(&format!("effectiveDateTime.toDate() {op} @2026-03-14")),
            )
            .await
            .unwrap();
        assert_eq!(fixed.lines.len(), 2, "toDate {op}");
        assert_eq!(fixed.unanswered, 0, "toDate {op}");
    }
}
