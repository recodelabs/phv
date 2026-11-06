use phv::Severity;
use phv::validate::{Validator, uncovered_required_bindings};
use serde_json::{Value, json};

const LAB: &str = r#"{"resourceType":"Observation","id":"lab-1","status":"final",
"category":[{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/observation-category","code":"laboratory"}]}],
"code":{"coding":[{"system":"http://loinc.org","code":"2160-0","display":"Creatinine"}]},
"subject":{"reference":"Patient/p1"},"effectiveDateTime":"2026-03-14",
"valueQuantity":{"value":1.50,"unit":"mg/dL","system":"http://unitsofmeasure.org","code":"mg/dL"}}"#;

fn lab() -> Value {
    serde_json::from_str(LAB).unwrap()
}

async fn errors(resource: &Value) -> Vec<String> {
    Validator::new()
        .await
        .unwrap()
        .validate(resource)
        .await
        .into_iter()
        .filter(|i| i.is_error())
        .map(|i| i.to_string())
        .collect()
}

#[tokio::test]
async fn a_valid_lab_result_has_no_errors() {
    assert_eq!(errors(&lab()).await, Vec::<String>::new());
}

macro_rules! rejects {
    ($name:ident, $mutate:expr, $expected:expr) => {
        #[tokio::test]
        async fn $name() {
            let mut resource = lab();
            let mutate: fn(&mut Value) = $mutate;
            mutate(&mut resource);
            let found = errors(&resource).await;
            assert!(
                found.iter().any(|e| e.contains($expected)),
                "wanted {}, got {found:?}",
                $expected
            );
        }
    };
}

rejects!(
    missing_status,
    |r| {
        r.as_object_mut().unwrap().remove("status");
    },
    "status"
);
rejects!(
    missing_code,
    |r| {
        r.as_object_mut().unwrap().remove("code");
    },
    "code"
);
rejects!(
    bad_required_code,
    |r| r["status"] = json!("banana"),
    "banana"
);
rejects!(unknown_element, |r| r["foo"] = json!("bar"), "foo");
rejects!(
    string_for_decimal,
    |r| r["valueQuantity"]["value"] = json!("1.50"),
    "decimal"
);
rejects!(
    two_choices,
    |r| r["valueString"] = json!("high"),
    "PHV-CHOICE"
);
rejects!(
    invariant_obs_6,
    |r| r["dataAbsentReason"] = json!({"text":"lost"}),
    "obs-6"
);
rejects!(
    impossible_date,
    |r| r["effectiveDateTime"] = json!("2026-13-45"),
    "2026-13-45"
);
rejects!(bad_id, |r| r["id"] = json!("bad id!"), "PHV-ID");
rejects!(empty_array, |r| r["category"] = json!([]), "category");

#[tokio::test]
async fn two_choices_nested_in_a_component_are_found() {
    let mut r = lab();
    r["component"] = json!([{"code":{"text":"x"},"valueString":"a","valueInteger":1}]);
    assert!(errors(&r).await.iter().any(|e| e.contains("PHV-CHOICE")));
}

/// One valid resource per type the first skills use, and the required field that breaks it.
fn typed_cases() -> Vec<(Value, &'static str)> {
    let subject = json!({"reference": "Patient/p1"});
    let code = json!({"text": "x"});
    vec![
        (
            json!({"resourceType":"Patient","birthDate":"1980-01-01","gender":"unknown"}),
            "gender",
        ),
        (
            json!({"resourceType":"DiagnosticReport","status":"final","code":code,"subject":subject}),
            "code",
        ),
        (
            json!({"resourceType":"Condition","code":code,"subject":subject}),
            "subject",
        ),
        (
            json!({"resourceType":"MedicationStatement","status":"active","medicationCodeableConcept":code,"subject":subject}),
            "status",
        ),
        (
            json!({"resourceType":"AllergyIntolerance","clinicalStatus":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/allergyintolerance-clinical","code":"active"}]},"code":code,"patient":subject}),
            "patient",
        ),
        (
            json!({"resourceType":"Immunization","status":"completed","vaccineCode":code,"patient":subject,"occurrenceDateTime":"2026-01-01"}),
            "vaccineCode",
        ),
        (
            json!({"resourceType":"DocumentReference","status":"current","content":[{"attachment":{"url":"sources/a.pdf"}}]}),
            "content",
        ),
        (
            json!({"resourceType":"Provenance","target":[subject],"recorded":"2026-01-01T00:00:00Z","agent":[{"who":{"display":"x"}}]}),
            "recorded",
        ),
    ]
}

#[tokio::test]
async fn each_first_skill_resource_type_accepts_valid_and_rejects_broken() {
    for (valid, field) in typed_cases() {
        let name = valid["resourceType"].clone();
        assert_eq!(
            errors(&valid).await,
            Vec::<String>::new(),
            "{name} should be valid"
        );

        let mut broken = valid.clone();
        if field == "gender" {
            broken["gender"] = json!("banana");
        } else {
            broken.as_object_mut().unwrap().remove(field);
        }
        assert!(
            errors(&broken)
                .await
                .iter()
                .any(|e| e.contains(field) || e.contains("banana")),
            "{name} without {field} should fail"
        );
    }
}

#[tokio::test]
async fn unknown_or_non_resource_types_are_rejected() {
    assert!(
        errors(&json!({"resourceType":"Foo"}))
            .await
            .iter()
            .any(|e| e.contains("PHV-TYPE"))
    );
    assert!(
        errors(&json!({"resourceType":"Quantity"}))
            .await
            .iter()
            .any(|e| e.contains("PHV-TYPE"))
    );
    assert!(
        errors(&json!({"status":"final"}))
            .await
            .iter()
            .any(|e| e.contains("PHV-TYPE"))
    );
}

#[tokio::test]
async fn upstream_tim_5_bug_is_a_warning_not_an_error() {
    // Valid FHIR that octofhir-fhirpath 0.5.8 fails with a type error. When this test
    // fails because no warning remains, the bug is fixed upstream: remove the workaround.
    let m = json!({"resourceType":"MedicationStatement","status":"active",
        "medicationCodeableConcept":{"text":"m"},"subject":{"reference":"Patient/p"},
        "dosage":[{"timing":{"repeat":{"count":1,"periodUnit":"d"}}}]});
    let issues = Validator::new().await.unwrap().validate(&m).await;
    assert!(
        issues.iter().all(|i| i.severity == Severity::Warning),
        "{issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.message.contains("evaluation failed")),
        "{issues:?}"
    );
}

#[tokio::test]
async fn real_timing_failures_are_still_errors() {
    let m: Value = serde_json::from_str(
        r#"{"resourceType":"MedicationStatement","status":"active",
        "medicationCodeableConcept":{"text":"m"},"subject":{"reference":"Patient/p"},
        "dosage":[{"timing":{"repeat":{"frequency":1,"period":-1,"periodUnit":"d"}}}]}"#,
    )
    .unwrap();
    assert!(errors(&m).await.iter().any(|e| e.contains("tim-5")));
}

#[tokio::test]
async fn hospital_records_with_unknown_profiles_and_extensions_are_accepted() {
    let patient = json!({"resourceType":"Patient",
        "meta":{"profile":["http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient|6.1.0"]},
        "extension":[{"url":"http://hl7.org/fhir/us/core/StructureDefinition/us-core-race",
            "extension":[{"url":"text","valueString":"Example"}]}],
        "name":[{"family":"Example"}],"birthDate":"1980-01-01"});
    assert_eq!(errors(&patient).await, Vec::<String>::new());

    let mut r = lab();
    r["meta"] = json!({"profile":["http://hl7.org/fhir/us/core/StructureDefinition/us-core-observation-lab"]});
    r["extension"] = json!([{"url":"http://example.org/fhir/unknown","valueString":"x"}]);
    r["code"]["extension"] = json!([{"url":"http://example.org/fhir/nested","valueCode":"a"}]);
    r["_status"] =
        json!({"extension":[{"url":"http://example.org/fhir/primitive","valueString":"x"}]});
    assert_eq!(errors(&r).await, Vec::<String>::new());
}

#[tokio::test]
async fn a_modifier_extension_is_a_warning_at_any_depth() {
    // It can change what a record means, and phv cannot interpret it.
    let mut r = lab();
    r["modifierExtension"] = json!([{"url":"http://example.org/fhir/mod","valueBoolean":true}]);
    let issues = Validator::new().await.unwrap().validate(&r).await;
    assert!(
        issues
            .iter()
            .any(|i| i.code == "PHV-MODIFIER" && i.severity == Severity::Warning),
        "{issues:?}"
    );

    let mut nested = lab();
    nested["component"] = json!([{"code":{"text":"x"},"modifierExtension":[{"url":"http://example.org/fhir/mod","valueBoolean":true}]}]);
    let issues = Validator::new().await.unwrap().validate(&nested).await;
    assert!(
        issues.iter().any(|i| i.code == "PHV-MODIFIER"),
        "{issues:?}"
    );

    let clean = Validator::new().await.unwrap().validate(&lab()).await;
    assert!(clean.iter().all(|i| i.code != "PHV-MODIFIER"), "{clean:?}");
}

#[tokio::test]
async fn results_are_in_a_stable_order() {
    let mut r = lab();
    r["foo"] = json!(1);
    r["bar"] = json!(1);
    r.as_object_mut().unwrap().remove("status");
    let v = Validator::new().await.unwrap();
    assert_eq!(v.validate(&r).await, v.validate(&r).await);
}

#[test]
fn only_the_known_required_bindings_are_uncovered() {
    let uncovered = uncovered_required_bindings();
    println!("required bindings without local codes: {uncovered:#?}");
    let known = [
        "http://hl7.org/fhir/ValueSet/currencies|4.0.1",
        "http://hl7.org/fhir/ValueSet/mimetypes|4.0.1",
        "http://hl7.org/fhir/ValueSet/parent-relationship-codes|4.0.1",
        "http://hl7.org/fhir/ValueSet/sibling-relationship-codes|4.0.1",
        "http://hl7.org/fhir/ValueSet/timezones|4.0.1",
        "http://hl7.org/fhir/ValueSet/ucum-units|4.0.1",
        "http://loinc.org/vs/LL379-9|4.0.1",
    ];
    let unexpected: Vec<_> = uncovered
        .iter()
        .filter(|u| !known.contains(&u.as_str()))
        .collect();
    assert!(unexpected.is_empty(), "newly uncovered: {unexpected:?}");
}
