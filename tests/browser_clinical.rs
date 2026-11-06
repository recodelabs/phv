mod browser_common;

use browser_common::{app, fixture, get};
use phv::browser::model::clinical::{active_now, allergies, conditions, medicines, vaccines};
use serde_json::json;

#[test]
fn conditions_leave_out_entered_in_error() {
    let rows = conditions(&fixture("Condition"));
    assert!(!rows.iter().any(|r| r.id == "condition-error"));
    assert_eq!(rows.len(), 3);
}

#[test]
fn active_now_includes_records_with_no_status_and_says_so() {
    let active = active_now(&conditions(&fixture("Condition")));
    let names: Vec<(&str, &str)> = active
        .iter()
        .map(|r| (r.name.as_str(), r.status.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("Hypothyroidism", "Status not recorded"),
            ("Asthma", "Active")
        ]
    );
}

#[test]
fn allergies_show_reactions_and_a_missing_status_never_hides_one() {
    let rows = allergies(&fixture("AllergyIntolerance"));
    let penicillin = rows.iter().find(|r| r.name == "Penicillin").unwrap();
    assert_eq!(penicillin.detail.as_deref(), Some("Hives"));
    // FHIR requires this status, but a hand-merged file could still lack it.
    let no_status = [json!({"id": "a", "code": {"text": "Latex"}})];
    let active = active_now(&allergies(&no_status));
    assert_eq!(active[0].status, "Status not recorded");
}

#[test]
fn medicine_names_fall_back_in_order() {
    let medications = fixture("Medication");
    let rows = medicines(
        &fixture("MedicationStatement"),
        &fixture("MedicationRequest"),
        &medications,
    );
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Amoxicillin 500 mg", "Levothyroxine 50 mcg"]);

    let by_display =
        [json!({"id": "a", "status": "active", "medicationReference": {"display": "Shown"}})];
    assert_eq!(medicines(&by_display, &[], &[])[0].name, "Shown");
    let unnamed = [json!({"id": "b", "status": "active"})];
    assert_eq!(medicines(&unnamed, &[], &[])[0].name, "Unnamed medicine");
}

#[test]
fn only_active_medicines_are_active_now() {
    let rows = medicines(
        &fixture("MedicationStatement"),
        &fixture("MedicationRequest"),
        &fixture("Medication"),
    );
    let active = active_now(&rows);
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "Levothyroxine 50 mcg");
}

#[test]
fn vaccines_not_given_say_so_and_keep_a_written_date() {
    let rows = vaccines(&fixture("Immunization"));
    let mmr = rows.iter().find(|r| r.name == "MMR").unwrap();
    assert_eq!(mmr.status, "Not given");
    assert_eq!(mmr.date, "Spring 2020");
    assert!(
        active_now(&rows).is_empty(),
        "vaccines are never active now"
    );
}

#[tokio::test]
async fn each_list_page_shows_its_records() {
    let app = app();
    for (uri, expected) in [
        ("/v/vault/conditions", "Asthma"),
        ("/v/vault/medicines", "Three times a day for 7 days"),
        ("/v/vault/allergies", "Hives"),
        ("/v/vault/vaccines", "Not given"),
    ] {
        let html = get(&app, uri).await.text();
        assert!(html.contains(expected), "{uri}");
    }
}

#[tokio::test]
async fn an_empty_list_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let vault = phv::store::Vault::init(&dir.path().join("v")).unwrap();
    let app = phv::browser::router(phv::browser::Vaults::single(vault.root().to_path_buf()));
    assert!(
        get(&app, "/v/vault/conditions")
            .await
            .text()
            .contains("No conditions yet.")
    );
    assert!(
        get(&app, "/v/vault/vitals")
            .await
            .text()
            .contains("No vitals yet.")
    );
    assert!(
        get(&app, "/v/vault/labs")
            .await
            .text()
            .contains("No labs yet.")
    );
    assert!(
        get(&app, "/v/vault/documents")
            .await
            .text()
            .contains("No documents yet.")
    );
}

#[test]
fn a_refuted_diagnosis_is_never_active_now() {
    let rows = conditions(&[json!({
        "id": "c", "code": {"text": "Ruled out"},
        "clinicalStatus": {"coding": [{"code": "active"}]},
        "verificationStatus": {"coding": [{"code": "refuted"}]}
    })]);
    assert_eq!(rows.len(), 1, "it stays on the Conditions page");
    assert!(active_now(&rows).is_empty());
}

#[test]
fn recurrence_relapse_and_status_text_count_as_active() {
    for status in [
        json!({"coding": [{"code": "recurrence"}]}),
        json!({"coding": [{"code": "relapse"}]}),
        json!({"text": "Active"}),
    ] {
        let rows = conditions(&[json!({"id": "c", "clinicalStatus": status})]);
        assert_eq!(active_now(&rows).len(), 1, "{status}");
    }
}
