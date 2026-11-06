use phv::browser::model::value::{
    Dated, concept_name, encode_segment, friendly_date, newest_first, quantity_text, result_lines,
};
use serde_json::{Value, json};

fn parsed(text: &str) -> Value {
    phv::canon::parse_json(text).unwrap()
}

#[test]
fn numbers_keep_their_written_form() {
    let quantity = parsed(r#"{"value":1.50,"unit":"mg/dL"}"#);
    assert_eq!(quantity_text(&quantity).unwrap(), "1.50 mg/dL");
}

#[test]
fn comparators_are_kept() {
    let quantity = parsed(r#"{"value":0.5,"comparator":"<","unit":"mg/L"}"#);
    assert_eq!(quantity_text(&quantity).unwrap(), "<0.5 mg/L");
}

#[test]
fn a_quantity_without_a_value_has_no_text() {
    assert_eq!(quantity_text(&json!({"unit": "kg"})), None);
}

#[test]
fn names_prefer_text_then_display_then_code() {
    assert_eq!(
        concept_name(&json!({"text": "T", "coding": [{"display": "D", "code": "C"}]})).unwrap(),
        "T"
    );
    assert_eq!(
        concept_name(&json!({"coding": [{"display": "D", "code": "C"}]})).unwrap(),
        "D"
    );
    assert_eq!(
        concept_name(&json!({"coding": [{"code": "C"}]})).unwrap(),
        "C"
    );
    assert_eq!(concept_name(&json!({})), None);
}

#[test]
fn blood_pressure_with_both_components_is_one_line() {
    let observation = parsed(
        r#"{"component":[
        {"code":{"coding":[{"system":"http://loinc.org","code":"8480-6"}]},"valueQuantity":{"value":118,"unit":"mmHg"}},
        {"code":{"coding":[{"system":"http://loinc.org","code":"8462-4"}]},"valueQuantity":{"value":76,"unit":"mmHg"}}
    ]}"#,
    );
    assert_eq!(result_lines(&observation), ["118/76 mmHg"]);
}

#[test]
fn other_components_get_a_line_each() {
    let observation = json!({"component": [
        {"code": {"text": "Left"}, "valueString": "Clear"},
        {"code": {"text": "Right"}}
    ]});
    assert_eq!(
        result_lines(&observation),
        ["Left: Clear", "Right: See FHIR"]
    );
}

#[test]
fn an_unknown_value_type_gives_no_lines() {
    assert!(result_lines(&json!({"valueSampledData": {}})).is_empty());
}

#[test]
fn dates_stay_as_written_with_no_zone_conversion() {
    assert_eq!(friendly_date("2026-10-04"), "4 Oct 2026");
    assert_eq!(friendly_date("2026-10"), "Oct 2026");
    assert_eq!(friendly_date("2026"), "2026");
    assert_eq!(
        friendly_date("2026-10-04T23:30:00-04:00"),
        "4 Oct 2026 23:30:00-04:00"
    );
    assert_eq!(friendly_date("soon"), "soon");
    assert_eq!(friendly_date("2026-13-01"), "2026-13-01");
}

struct Row(Option<&'static str>, &'static str);

impl Dated for Row {
    fn sort_date(&self) -> Option<&str> {
        self.0
    }
    fn sort_id(&self) -> &str {
        self.1
    }
}

#[test]
fn newest_comes_first_and_undated_last() {
    let mut rows = [
        Row(None, "z"),
        Row(Some("2025-01-01"), "b"),
        Row(Some("2026-01-01"), "c"),
        Row(Some("2025-01-01"), "a"),
    ];
    newest_first(&mut rows);
    let ids: Vec<&str> = rows.iter().map(|r| r.1).collect();
    assert_eq!(ids, ["c", "a", "b", "z"]);
}

#[test]
fn segments_are_percent_encoded() {
    assert_eq!(encode_segment("text:A/B c"), "text%3AA%2FB%20c");
    assert_eq!(encode_segment("a-b.c_d~1"), "a-b.c_d~1");
}

#[test]
fn blood_pressure_keeps_comparators_the_unit_code_and_other_components() {
    let observation = parsed(
        r#"{"component":[
        {"code":{"coding":[{"system":"http://loinc.org","code":"8480-6"}]},"valueQuantity":{"value":90,"comparator":"<","code":"mm[Hg]"}},
        {"code":{"coding":[{"system":"http://loinc.org","code":"8462-4"}]},"valueQuantity":{"value":60,"code":"mm[Hg]"}},
        {"code":{"text":"Mean pressure"},"valueQuantity":{"value":70,"unit":"mmHg"}}
    ]}"#,
    );
    assert_eq!(
        result_lines(&observation),
        ["<90/60 mm[Hg]", "Mean pressure: 70 mmHg"]
    );
}
