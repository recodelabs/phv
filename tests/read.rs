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
    let lines = phv.list("Observation", None).await.unwrap();
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
        phv.list("Observation", Some("valueQuantity.value > 1.9"))
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        phv.list("Observation", Some("status = 'nope'"))
            .await
            .unwrap()
            .len(),
        0
    );
    // A filter that evaluates to empty does not match.
    assert_eq!(
        phv.list("Observation", Some("missing.field = 1"))
            .await
            .unwrap()
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
        phv.list("Observation", Some("((")).await.unwrap_err(),
        PhvError::Usage(_)
    ));
}

#[tokio::test]
async fn listing_an_empty_type_is_empty_not_an_error() {
    let (_d, phv) = phv().await;
    assert!(phv.list("Condition", None).await.unwrap().is_empty());
}
