use phv::refs::{collect_refs, local_target};
use phv::submission::prepare;
use serde_json::{Value, json};

fn parse(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

fn lab_bundle(uuid: &str) -> Value {
    json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {"fullUrl": format!("urn:uuid:{uuid}-1"), "resource": {
                "resourceType": "DiagnosticReport", "status": "final",
                "code": {"text": "Metabolic panel"},
                "result": [{"reference": format!("urn:uuid:{uuid}-2")}]
            }},
            {"fullUrl": format!("urn:uuid:{uuid}-2"), "resource": {
                "resourceType": "Observation", "status": "final",
                "code": {"text": "Creatinine"}
            }}
        ]
    })
}

#[test]
fn a_single_resource_gets_a_derived_id_and_loses_volatile_meta() {
    let r = parse(
        r#"{"resourceType":"Observation","id":"made-up","status":"final","meta":{"lastUpdated":"2026-01-01"}}"#,
    );
    let out = prepare(&r).unwrap();
    assert_eq!(out.len(), 1);
    assert_ne!(out[0]["id"], "made-up");
    assert_eq!(out[0]["id"].as_str().unwrap().len(), 32);
    assert!(out[0].get("meta").is_none());
}

#[test]
fn references_in_a_bundle_point_at_the_new_ids() {
    let out = prepare(&lab_bundle("aaa")).unwrap();
    assert_eq!(out.len(), 2);
    let report = out
        .iter()
        .find(|r| r["resourceType"] == "DiagnosticReport")
        .unwrap();
    let obs = out
        .iter()
        .find(|r| r["resourceType"] == "Observation")
        .unwrap();
    assert_eq!(
        report["result"][0]["reference"],
        format!("Observation/{}", obs["id"].as_str().unwrap())
    );
}

#[test]
fn ids_do_not_depend_on_temporary_names() {
    let a = prepare(&lab_bundle("aaa")).unwrap();
    let b = prepare(&lab_bundle("bbb")).unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_temporary_id_works_as_a_local_name() {
    let bundle = json!({"resourceType":"Bundle","type":"collection","entry":[
        {"resource":{"resourceType":"Observation","id":"tmp","status":"final"}},
        {"resource":{"resourceType":"DiagnosticReport","status":"final","result":[{"reference":"Observation/tmp"}]}}
    ]});
    let out = prepare(&bundle).unwrap();
    let report = out
        .iter()
        .find(|r| r["resourceType"] == "DiagnosticReport")
        .unwrap();
    assert_ne!(report["result"][0]["reference"], "Observation/tmp");
}

#[test]
fn references_to_the_vault_are_left_alone() {
    let r = json!({"resourceType":"Observation","status":"final","subject":{"reference":"Patient/abc"}});
    assert_eq!(
        prepare(&r).unwrap()[0]["subject"]["reference"],
        "Patient/abc"
    );
}

#[test]
fn identical_resources_collapse_into_one() {
    let same = json!({"resourceType":"Observation","status":"final"});
    let bundle = json!({"resourceType":"Bundle","type":"collection","entry":[{"resource":same},{"resource":same}]});
    assert_eq!(prepare(&bundle).unwrap().len(), 1);
}

#[test]
fn other_bundle_types_and_non_resources_are_refused() {
    assert!(prepare(&json!({"resourceType":"Bundle","type":"document","entry":[]})).is_err());
    assert!(prepare(&json!({"status":"final"})).is_err());
    assert!(
        prepare(&json!({"resourceType":"Bundle","type":"collection","entry":[{"fullUrl":"x"}]}))
            .is_err()
    );
}

#[test]
fn decimals_survive_preparation() {
    let r =
        parse(r#"{"resourceType":"Observation","status":"final","valueQuantity":{"value":1.50}}"#);
    let line = phv::canon::canon_resource(&prepare(&r).unwrap()[0]);
    assert!(line.contains(r#""value":1.50"#), "{line}");
}

#[test]
fn local_target_classifies_references() {
    assert_eq!(
        local_target("Patient/abc-1.2"),
        Some(("Patient", "abc-1.2"))
    );
    assert_eq!(local_target("http://example.org/Patient/abc"), None);
    assert_eq!(local_target("#contained"), None);
    assert_eq!(local_target("urn:uuid:123"), None);
    assert_eq!(local_target("Patient/abc/_history/1"), None);
}

#[test]
fn collect_refs_finds_nested_references() {
    let r = json!({"subject":{"reference":"Patient/a"},"result":[{"reference":"Observation/b"}],"reference":5});
    let mut found = collect_refs(&r);
    found.sort();
    assert_eq!(found, vec!["Observation/b", "Patient/a"]);
}
