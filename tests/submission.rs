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

fn find<'a>(out: &'a [Value], resource_type: &str, pred: impl Fn(&Value) -> bool) -> &'a Value {
    out.iter()
        .find(|r| r["resourceType"] == resource_type && pred(r))
        .unwrap()
}

fn obs(name: &str, text: &str) -> Value {
    json!({"fullUrl": format!("urn:uuid:{name}"), "resource": {
        "resourceType": "Observation", "status": "final", "code": {"text": text}
    }})
}

fn report(name: &str, target: &str) -> Value {
    json!({"fullUrl": format!("urn:uuid:{name}"), "resource": {
        "resourceType": "DiagnosticReport", "status": "final",
        "code": {"text": "Panel"},
        "result": [{"reference": format!("urn:uuid:{target}")}]
    }})
}

fn bundle(entries: Vec<Value>) -> Value {
    json!({"resourceType": "Bundle", "type": "collection", "entry": entries})
}

#[test]
fn reports_that_differ_only_in_their_target_stay_separate() {
    let b = bundle(vec![
        report("r1", "o1"),
        report("r2", "o2"),
        obs("o1", "Sodium"),
        obs("o2", "Potassium"),
    ]);
    let out = prepare(&b).unwrap();
    assert_eq!(out.len(), 4);
    let reports: Vec<&Value> = out
        .iter()
        .filter(|r| r["resourceType"] == "DiagnosticReport")
        .collect();
    assert_eq!(reports.len(), 2);
    assert_ne!(reports[0]["id"], reports[1]["id"]);
    // Each report points at the Observation that was originally linked to it.
    let sodium = find(&out, "Observation", |o| o["code"]["text"] == "Sodium");
    let potassium = find(&out, "Observation", |o| o["code"]["text"] == "Potassium");
    let refs: Vec<String> = out
        .iter()
        .filter(|r| r["resourceType"] == "DiagnosticReport")
        .map(|r| r["result"][0]["reference"].as_str().unwrap().to_string())
        .collect();
    assert!(refs.contains(&format!("Observation/{}", sodium["id"].as_str().unwrap())));
    assert!(refs.contains(&format!(
        "Observation/{}",
        potassium["id"].as_str().unwrap()
    )));
}

#[test]
fn a_difference_two_levels_down_keeps_reports_separate() {
    let panel = |name: &str, member: &str| {
        json!({"fullUrl": format!("urn:uuid:{name}"), "resource": {
            "resourceType": "Observation", "status": "final", "code": {"text": "Panel"},
            "hasMember": [{"reference": format!("urn:uuid:{member}")}]
        }})
    };
    let report_via = |name: &str, panel: &str| {
        json!({"fullUrl": format!("urn:uuid:{name}"), "resource": {
            "resourceType": "DiagnosticReport", "status": "final",
            "code": {"text": "Report"},
            "result": [{"reference": format!("urn:uuid:{panel}")}]
        }})
    };
    let b = bundle(vec![
        report_via("r1", "p1"),
        report_via("r2", "p2"),
        panel("p1", "o1"),
        panel("p2", "o2"),
        obs("o1", "Sodium"),
        obs("o2", "Potassium"),
    ]);
    let out = prepare(&b).unwrap();
    assert_eq!(out.len(), 6);
    let ids: std::collections::HashSet<_> = out.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 6);
    // Following the references from each report reaches a different leaf.
    let leaf_of = |report: &Value| -> String {
        let p = report["result"][0]["reference"].as_str().unwrap();
        let (_, pid) = local_target(p).unwrap();
        let panel = find(&out, "Observation", |o| o["id"] == pid);
        let m = panel["hasMember"][0]["reference"].as_str().unwrap();
        let (_, mid) = local_target(m).unwrap();
        find(&out, "Observation", |o| o["id"] == mid)["code"]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let mut leaves: Vec<String> = out
        .iter()
        .filter(|r| r["resourceType"] == "DiagnosticReport")
        .map(leaf_of)
        .collect();
    leaves.sort();
    assert_eq!(leaves, vec!["Potassium", "Sodium"]);
}

#[test]
fn entries_with_identical_targets_collapse() {
    let b = bundle(vec![
        report("r1", "o1"),
        report("r2", "o2"),
        obs("o1", "Sodium"),
        obs("o2", "Sodium"),
    ]);
    let out = prepare(&b).unwrap();
    assert_eq!(out.len(), 2);
    let report = find(&out, "DiagnosticReport", |_| true);
    let o = find(&out, "Observation", |_| true);
    assert_eq!(
        report["result"][0]["reference"],
        format!("Observation/{}", o["id"].as_str().unwrap())
    );
}

fn circular(a: &str, b: &str) -> Value {
    bundle(vec![
        json!({"fullUrl": format!("urn:uuid:{a}"), "resource": {
            "resourceType": "Observation", "status": "final", "code": {"text": "A"},
            "hasMember": [{"reference": format!("urn:uuid:{b}")}]
        }}),
        json!({"fullUrl": format!("urn:uuid:{b}"), "resource": {
            "resourceType": "Observation", "status": "final", "code": {"text": "B"},
            "hasMember": [{"reference": format!("urn:uuid:{a}")}]
        }}),
    ])
}

#[test]
fn circular_references_terminate_and_ignore_temporary_names() {
    let x = prepare(&circular("one", "two")).unwrap();
    let y = prepare(&circular("zzz", "yyy")).unwrap();
    assert_eq!(x.len(), 2);
    assert_ne!(x[0]["id"], x[1]["id"]);
    assert_eq!(x, y);
}

#[test]
fn entry_order_does_not_change_ids() {
    let forward = bundle(vec![
        report("r1", "o1"),
        report("r2", "o2"),
        obs("o1", "Sodium"),
        obs("o2", "Potassium"),
    ]);
    let mut reversed = forward.clone();
    reversed["entry"].as_array_mut().unwrap().reverse();
    assert_eq!(prepare(&forward).unwrap(), prepare(&reversed).unwrap());
    assert_eq!(
        prepare(&circular("a", "b")).unwrap(),
        prepare(&{
            let mut c = circular("a", "b");
            c["entry"].as_array_mut().unwrap().reverse();
            c
        })
        .unwrap()
    );
}
