//! IPS generation. All test data is synthetic.

mod common;

use common::{add_patient, phv, records};
use phv::PhvError;
use phv::ips::{IpsOptions, IpsReport};
use phv::write::{AddOptions, Phv};
use serde_json::{Value, json};

const AT: &str = "2026-11-06T12:00:00Z";
const CATEGORY: &str = "http://terminology.hl7.org/CodeSystem/observation-category";

fn options() -> IpsOptions {
    IpsOptions {
        generated_at: AT.into(),
    }
}

fn lab_category() -> Value {
    json!([{"coding":[{"system":CATEGORY,"code":"laboratory"}]}])
}

/// A vault with something for most sections, and some records IPS cannot use.
async fn add_history(phv: &Phv, patient: &str) {
    let subject = json!({"reference": patient});
    let bundle = json!({"resourceType":"Bundle","type":"transaction","entry":[
        {"fullUrl":"urn:uuid:c1","resource":{"resourceType":"Condition","subject":subject,
            "clinicalStatus":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/condition-clinical","code":"active"}]},
            "code":{"text":"Asthma"},"onsetDateTime":"2010-05-01"}},
        {"fullUrl":"urn:uuid:c2","resource":{"resourceType":"Condition","subject":subject,
            "clinicalStatus":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/condition-clinical","code":"resolved"}]},
            "code":{"text":"Appendicitis"},"onsetDateTime":"1995-03-01"}},
        {"fullUrl":"urn:uuid:c3","resource":{"resourceType":"Condition","subject":subject,
            "verificationStatus":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/condition-ver-status","code":"entered-in-error"}]},
            "code":{"text":"Wrong patient"}}},
        {"fullUrl":"urn:uuid:a1","resource":{"resourceType":"AllergyIntolerance","patient":subject,"clinicalStatus":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/allergyintolerance-clinical","code":"active"}]},
            "code":{"text":"Penicillin"},"criticality":"high"}},
        {"fullUrl":"urn:uuid:a2","resource":{"resourceType":"AllergyIntolerance","patient":subject,"clinicalStatus":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/allergyintolerance-clinical","code":"active"}]},
            "note":[{"text":"no code recorded"}]}},
        {"fullUrl":"urn:uuid:med1","resource":{"resourceType":"Medication","code":{"text":"Metformin 500 mg tablet"}}},
        {"fullUrl":"urn:uuid:m1","resource":{"resourceType":"MedicationStatement","status":"active","subject":subject,
            "medicationReference":{"reference":"urn:uuid:med1"},"effectiveDateTime":"2024-01-01",
            "dosage":[{"text":"500 mg twice daily"}]}},
        {"fullUrl":"urn:uuid:med2","resource":{"resourceType":"Medication","code":{"text":"Only used by a bad statement"}}},
        {"fullUrl":"urn:uuid:m2","resource":{"resourceType":"MedicationStatement","status":"active","subject":subject,
            "medicationReference":{"reference":"urn:uuid:med2"}}},
        {"fullUrl":"urn:uuid:i1","resource":{"resourceType":"Immunization","status":"completed","patient":subject,
            "vaccineCode":{"text":"Influenza"},"occurrenceDateTime":"2025-10-01"}},
        {"fullUrl":"urn:uuid:o1","resource":{"resourceType":"Observation","status":"final","category":lab_category(),
            "code":{"coding":[{"system":"http://loinc.org","code":"2160-0"}],"text":"Creatinine"},"subject":subject,
            "effectiveDateTime":"2026-03-14","valueQuantity":{"value":1.50,"unit":"mg/dL"}}},
        {"fullUrl":"urn:uuid:r1","resource":{"resourceType":"DiagnosticReport","status":"final","code":{"text":"Renal panel"},
            "subject":subject,"effectiveDateTime":"2026-03-14","performer":[{"display":"Example Labs"}],
            "result":[{"reference":"urn:uuid:o1"}]}},
        {"fullUrl":"urn:uuid:o2","resource":{"resourceType":"Observation","status":"final","category":lab_category(),
            "code":{"text":"Potassium"},"subject":subject,"effectiveDateTime":"2026-02-01",
            "valueQuantity":{"value":4.1,"unit":"mmol/L"}}},
        {"fullUrl":"urn:uuid:r2","resource":{"resourceType":"DiagnosticReport","status":"final","code":{"text":"Report with no lab named"},
            "subject":subject,"effectiveDateTime":"2026-02-01","result":[{"reference":"urn:uuid:o2"}]}},
        {"fullUrl":"urn:uuid:v1","resource":{"resourceType":"Observation","status":"final",
            "category":[{"coding":[{"system":CATEGORY,"code":"vital-signs"}]}],
            "code":{"coding":[{"system":"http://loinc.org","code":"29463-7"}],"text":"Body weight"},"subject":subject,
            "effectiveDateTime":"2026-06-01","valueQuantity":{"value":70,"unit":"kg","system":"http://unitsofmeasure.org","code":"kg"}}},
        {"fullUrl":"urn:uuid:s1","resource":{"resourceType":"Observation","status":"final",
            "category":[{"coding":[{"system":CATEGORY,"code":"activity"}]}],
            "code":{"coding":[{"system":"http://loinc.org","code":"41950-7"}]},"subject":subject,
            "effectiveDateTime":"2026-06-01","valueQuantity":{"value":5000,"unit":"steps"}}}
    ]});
    phv.add(&bundle, &AddOptions::default()).await.unwrap();
}

async fn ips_of(phv: &Phv) -> IpsReport {
    phv.ips(&options()).await.unwrap()
}

fn resources(report: &IpsReport) -> Vec<&Value> {
    report.bundle["entry"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["resource"])
        .collect()
}

fn find<'a>(report: &'a IpsReport, resource_type: &str, text: &str) -> Option<&'a Value> {
    resources(report)
        .into_iter()
        .find(|r| r["resourceType"] == resource_type && r.to_string().contains(text))
}

fn section<'a>(report: &'a IpsReport, code: &str) -> Option<&'a Value> {
    resources(report)[0]["section"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["code"]["coding"][0]["code"] == code)
}

fn omitted_refs(report: &IpsReport) -> Vec<String> {
    report.omitted.iter().map(|o| o.reference.clone()).collect()
}

async fn full_vault() -> (tempfile::TempDir, Phv, IpsReport) {
    let (dir, phv) = phv().await;
    let patient = add_patient(&phv).await;
    add_history(&phv, &patient).await;
    let report = ips_of(&phv).await;
    (dir, phv, report)
}

#[tokio::test]
async fn builds_a_document_with_composition_and_patient_first() {
    let (_d, _phv, report) = full_vault().await;
    let bundle = &report.bundle;
    assert_eq!(bundle["type"], "document");
    assert_eq!(bundle["timestamp"], AT);
    assert!(
        bundle["identifier"]["value"]
            .as_str()
            .unwrap()
            .starts_with("urn:uuid:")
    );
    let all = resources(&report);
    assert_eq!(all[0]["resourceType"], "Composition");
    assert_eq!(all[1]["resourceType"], "Patient");
    assert_eq!(
        all[0]["subject"]["reference"],
        bundle["entry"][1]["fullUrl"]
    );
    assert_eq!(all[0]["type"]["coding"][0]["code"], "60591-5");
    assert!(
        all[1]["meta"]["profile"]
            .as_array()
            .unwrap()
            .contains(&json!(
                "http://hl7.org/fhir/uv/ips/StructureDefinition/Patient-uv-ips"
            ))
    );
}

#[tokio::test]
async fn records_go_in_their_sections() {
    let (_d, _phv, report) = full_vault().await;
    let count = |code: &str| {
        section(&report, code)
            .and_then(|s| s["entry"].as_array())
            .map_or(0, Vec::len)
    };
    assert_eq!(count("11450-4"), 1, "active problem");
    assert_eq!(count("11348-0"), 1, "resolved problem in past illness");
    assert_eq!(count("48765-2"), 1, "allergy with a code");
    assert_eq!(count("10160-0"), 1, "statement with a date");
    assert_eq!(count("11369-6"), 1);
    assert_eq!(count("30954-2"), 2, "one report and its result");
    assert_eq!(count("8716-3"), 1);
    assert!(
        section(&report, "47519-4").is_none(),
        "empty optional section left out"
    );
    assert!(find(&report, "Condition", "Wrong patient").is_none());
    assert!(
        find(&report, "Observation", "41950-7").is_none(),
        "activity is not clinical"
    );
    assert!(
        find(&report, "Medication", "Metformin").is_some(),
        "supporting record"
    );
}

#[tokio::test]
async fn records_that_fail_their_profile_are_left_out_with_their_dependants() {
    let (_d, phv, report) = full_vault().await;
    let omitted = omitted_refs(&report);
    let id_of = |resource_type: &str, text: &str| {
        let line = records(&phv, resource_type)
            .lines()
            .find(|l| l.contains(text))
            .unwrap()
            .to_string();
        let value: Value = serde_json::from_str(&line).unwrap();
        format!("{resource_type}/{}", value["id"].as_str().unwrap())
    };
    let mut expected = vec![
        id_of("AllergyIntolerance", "no code recorded"),
        id_of("MedicationStatement", "\"status\":\"active\",\"subject\""),
        id_of("Observation", "Potassium"),
        id_of("DiagnosticReport", "no lab named"),
    ];
    // The statement without a date is the one that refers to med2.
    let med2 = id_of("Medication", "bad statement");
    let statements = records(&phv, "MedicationStatement");
    let bad = statements.lines().find(|l| l.contains(&med2)).unwrap();
    let bad: Value = serde_json::from_str(bad).unwrap();
    expected[1] = format!("MedicationStatement/{}", bad["id"].as_str().unwrap());
    expected.sort();
    assert_eq!(omitted, expected, "{:#?}", report.omitted);

    let report_entry = report
        .omitted
        .iter()
        .find(|o| o.reference.starts_with("DiagnosticReport/"))
        .unwrap();
    assert_eq!(report_entry.issues[0].code, "PHV-IPS-DEPENDS");
    assert_eq!(report_entry.section, Some("Results"));
    assert!(
        find(&report, "Medication", "bad statement").is_none(),
        "only the bad statement used it"
    );

    let allergies = section(&report, "48765-2").unwrap();
    assert!(
        allergies["text"]["div"]
            .as_str()
            .unwrap()
            .contains("1 record was left out")
    );
    assert!(report.summary().unwrap().starts_with("4 records"));
}

#[tokio::test]
async fn lab_results_take_the_performer_of_their_report_in_the_ips_only() {
    let (_d, phv, report) = full_vault().await;
    let creatinine = find(&report, "Observation", "Creatinine").unwrap();
    assert_eq!(creatinine["performer"], json!([{"display":"Example Labs"}]));
    assert!(!records(&phv, "Observation").contains("Example Labs"));
}

#[tokio::test]
async fn every_reference_points_at_an_entry() {
    let (_d, _phv, report) = full_vault().await;
    let text = report.bundle.to_string();
    let urls: Vec<&str> = report.bundle["entry"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["fullUrl"].as_str().unwrap())
        .collect();
    let mut checked = 0;
    for part in text.split("\"reference\":\"").skip(1) {
        let reference = part.split('"').next().unwrap();
        assert!(urls.contains(&reference), "{reference} is not an entry");
        checked += 1;
    }
    assert!(checked > 10);
}

#[tokio::test]
async fn required_sections_are_present_when_empty() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let report = ips_of(&phv).await;
    for code in ["11450-4", "48765-2", "10160-0"] {
        let section = section(&report, code).unwrap();
        assert_eq!(section["emptyReason"]["coding"][0]["code"], "unavailable");
        assert!(section.get("entry").is_none());
        assert!(
            section["text"]["div"]
                .as_str()
                .unwrap()
                .contains("No information is recorded")
        );
    }
    assert_eq!(
        resources(&report)[0]["section"].as_array().unwrap().len(),
        3
    );
    assert_eq!(resources(&report).len(), 2);
}

#[tokio::test]
async fn output_is_the_same_whatever_the_insert_order() {
    let (_d1, first, report) = full_vault().await;
    let (_d2, second) = phv().await;
    let patient = add_patient(&second).await;
    // Same records, added one at a time in reverse.
    let mut lines: Vec<String> = Vec::new();
    for resource_type in [
        "Medication",
        "Observation",
        "DiagnosticReport",
        "Immunization",
        "MedicationStatement",
        "AllergyIntolerance",
        "Condition",
    ] {
        lines.extend(records(&first, resource_type).lines().map(str::to_string));
    }
    // Records that refer to others must come after them, so add the plain ones first.
    let mut pending: Vec<Value> = lines
        .iter()
        .rev()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    while !pending.is_empty() {
        let before = pending.len();
        let mut next = Vec::new();
        for resource in pending {
            if second.add(&resource, &AddOptions::default()).await.is_err() {
                next.push(resource);
            }
        }
        assert!(next.len() < before, "stuck adding records");
        pending = next;
    }
    assert_eq!(
        patient,
        format!("Patient/{}", resources(&report)[1]["id"].as_str().unwrap())
    );
    let again = ips_of(&second).await;
    assert_eq!(
        serde_json::to_string(&report.bundle).unwrap(),
        serde_json::to_string(&again.bundle).unwrap()
    );
    let repeat = ips_of(&first).await;
    assert_eq!(
        serde_json::to_string(&report.bundle).unwrap(),
        serde_json::to_string(&repeat.bundle).unwrap()
    );
}

#[tokio::test]
async fn decimals_keep_their_digits() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    // From text, because json! would turn 1.50 into 1.5.
    let observation: Value = serde_json::from_str(&format!(
        r#"{{"resourceType":"Observation","status":"final",
        "category":[{{"coding":[{{"system":"{CATEGORY}","code":"laboratory"}}]}}],
        "code":{{"text":"Creatinine"}},"subject":{{"reference":"{patient}"}},
        "effectiveDateTime":"2026-03-14","performer":[{{"display":"Example Labs"}}],
        "valueQuantity":{{"value":1.50,"unit":"mg/dL"}}}}"#
    ))
    .unwrap();
    phv.add(&observation, &AddOptions::default()).await.unwrap();
    let report = ips_of(&phv).await;
    assert!(report.omitted.is_empty(), "{:#?}", report.omitted);
    let text = serde_json::to_string(&report.bundle).unwrap();
    assert!(text.contains("\"value\":1.50"));
    assert!(text.contains("1.50 mg/dL"));
}

#[tokio::test]
async fn the_vault_is_not_changed() {
    let (dir, phv) = phv().await;
    let patient = add_patient(&phv).await;
    add_history(&phv, &patient).await;
    let snapshot = |root: &std::path::Path| {
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(root.join("records"))
            .unwrap()
            .map(|e| {
                let path = e.unwrap().path();
                (path.display().to_string(), std::fs::read(&path).unwrap())
            })
            .collect();
        files.sort();
        files
    };
    let root = dir.path().join("data");
    let before = snapshot(&root);
    ips_of(&phv).await;
    assert_eq!(before, snapshot(&root));
}

#[tokio::test]
async fn no_patient_is_a_usage_error() {
    let (_d, phv) = phv().await;
    let error = phv.ips(&options()).await.unwrap_err();
    assert!(
        matches!(error, PhvError::Usage(ref m) if m.contains("no Patient")),
        "{error}"
    );
}

#[tokio::test]
async fn a_patient_without_a_birth_date_cannot_be_summarized() {
    let (_d, phv) = phv().await;
    phv.add(
        &json!({"resourceType":"Patient","name":[{"family":"Example"}]}),
        &AddOptions::default(),
    )
    .await
    .unwrap();
    let error = phv.ips(&options()).await.unwrap_err();
    let issues = error.issues();
    assert_eq!(issues[0].code, "PHV-IPS-PATIENT");
    assert!(
        issues.iter().any(|i| i.message.contains("birthDate")),
        "{issues:#?}"
    );
}

#[tokio::test]
async fn a_bad_time_is_a_usage_error() {
    let (_d, phv) = phv().await;
    add_patient(&phv).await;
    let error = phv
        .ips(&IpsOptions {
            generated_at: "2026-11-06".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(error, PhvError::Usage(_)));
}

/// Pins the octofhir finding the IPS checker drops: any extension on a Patient matches
/// "multiple slices". If this starts failing because octofhir is fixed, remove the filter.
#[tokio::test]
async fn patient_extensions_do_not_leave_the_patient_out() {
    let (_d, phv) = phv().await;
    phv.add(
        &json!({"resourceType":"Patient","name":[{"family":"Example"}],"birthDate":"1980-01-01",
            "extension":[{"url":"http://hl7.org/fhir/StructureDefinition/patient-citizenship",
                "extension":[{"url":"code","valueCodeableConcept":{"text":"Somewhere"}}]}]}),
        &AddOptions::default(),
    )
    .await
    .unwrap();
    let report = ips_of(&phv).await;
    assert!(report.omitted.is_empty());
}

/// The IPS checks mean nothing unless the profiles reject records: a lab result with no
/// performer and no report to take one from is left out for that reason.
#[tokio::test]
async fn a_lab_result_with_no_performer_is_left_out_for_that_reason() {
    let (_d, phv) = phv().await;
    let patient = add_patient(&phv).await;
    phv.add(
        &json!({"resourceType":"Observation","status":"final","category":lab_category(),
            "code":{"text":"Sodium"},"subject":{"reference":patient},"effectiveDateTime":"2026-01-01",
            "valueQuantity":{"value":140,"unit":"mmol/L"}}),
        &AddOptions::default(),
    )
    .await
    .unwrap();
    let report = ips_of(&phv).await;
    assert_eq!(report.omitted.len(), 1);
    assert!(
        report.omitted[0].issues[0].message.contains("performer"),
        "{:#?}",
        report.omitted
    );
    assert!(report.omitted[0].issues[0].path.starts_with("Observation/"));
}

/// The whole document for the fixture vault, byte for byte, so any change to the output
/// shows up in review. After an intended change, regenerate it with
/// `PHV_UPDATE_GOLDEN=1 cargo test --test ips golden`, and read the diff.
#[tokio::test]
async fn golden_document_for_the_fixture_vault() {
    let (_d, phv) = phv().await;
    let fixture = std::fs::read_to_string("tests/fixtures/ips/records.json").unwrap();
    let records = phv::canon::parse_json(&fixture).unwrap();
    phv.add(&records, &AddOptions::default()).await.unwrap();
    let report = ips_of(&phv).await;
    assert!(report.omitted.is_empty(), "{:#?}", report.omitted);
    let actual = format!(
        "{}\n",
        serde_json::to_string_pretty(&report.bundle).unwrap()
    );
    let path = "tests/fixtures/ips/expected-ips.json";
    if std::env::var_os("PHV_UPDATE_GOLDEN").is_some() {
        std::fs::write(path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(path).unwrap();
    assert!(
        actual == expected,
        "the IPS for the fixture vault changed: if that was intended, run \
         PHV_UPDATE_GOLDEN=1 cargo test --test ips golden and check the diff"
    );
}
