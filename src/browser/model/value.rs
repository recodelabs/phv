//! Reading FHIR values for display, always as stored.

use serde_json::Value;
use std::cmp::Ordering;

pub const OBSERVATION_CATEGORY: &str = "http://terminology.hl7.org/CodeSystem/observation-category";
pub const DIAGNOSTIC_SERVICE: &str = "http://terminology.hl7.org/CodeSystem/v2-0074";
pub const LOINC: &str = "http://loinc.org";
const SYSTOLIC: &str = "8480-6";
const DIASTOLIC: &str = "8462-4";

pub fn id_of(resource: &Value) -> String {
    resource["id"].as_str().unwrap_or("").to_string()
}

/// `text`, then the first coding's `display`, then its `code`.
pub fn concept_name(concept: &Value) -> Option<String> {
    let coding = &concept["coding"][0];
    [&concept["text"], &coding["display"], &coding["code"]]
        .into_iter()
        .find_map(|v| v.as_str())
        .map(str::to_string)
}

/// Whether a CodeableConcept, or an array of them, has this code in this system.
pub fn has_code(concepts: &Value, system: &str, code: &str) -> bool {
    let matches = |concept: &Value| {
        concept["coding"].as_array().is_some_and(|codings| {
            codings
                .iter()
                .any(|c| c["system"] == system && c["code"] == code)
        })
    };
    match concepts {
        Value::Array(items) => items.iter().any(matches),
        other => matches(other),
    }
}

pub fn code_in_system(concept: &Value, system: &str) -> Option<String> {
    concept["coding"]
        .as_array()?
        .iter()
        .find(|c| c["system"] == system)
        .and_then(|c| c["code"].as_str())
        .map(str::to_string)
}

/// The number exactly as written, so `1.50` stays `1.50`.
pub fn number_text(value: &Value) -> Option<String> {
    value.as_number().map(ToString::to_string)
}

/// Comparator, value and unit as stored, such as `<0.5 mg/dL`.
pub fn quantity_text(quantity: &Value) -> Option<String> {
    let number = number_text(&quantity["value"])?;
    let comparator = quantity["comparator"].as_str().unwrap_or("");
    let unit = quantity["unit"].as_str().or(quantity["code"].as_str());
    Some(match unit {
        Some(unit) => format!("{comparator}{number} {unit}"),
        None => format!("{comparator}{number}"),
    })
}

/// The text of a resource's or component's `value[x]`, for the types phv knows how to show.
pub fn value_text(holder: &Value) -> Option<String> {
    if let Some(text) = quantity_text(&holder["valueQuantity"]) {
        return Some(text);
    }
    if let Some(name) = concept_name(&holder["valueCodeableConcept"]) {
        return Some(name);
    }
    for key in ["valueString", "valueDateTime", "valueTime"] {
        if let Some(text) = holder[key].as_str() {
            return Some(text.to_string());
        }
    }
    if let Some(flag) = holder["valueBoolean"].as_bool() {
        return Some(if flag { "Yes" } else { "No" }.to_string());
    }
    number_text(&holder["valueInteger"])
}

fn is_pressure_part(component: &Value) -> bool {
    has_code(&component["code"], LOINC, SYSTOLIC) || has_code(&component["code"], LOINC, DIASTOLIC)
}

fn pressure_reading(quantity: &Value) -> Option<String> {
    let comparator = quantity["comparator"].as_str().unwrap_or("");
    Some(format!("{comparator}{}", number_text(&quantity["value"])?))
}

fn blood_pressure(components: &[Value]) -> Option<String> {
    let find = |code: &str| {
        components
            .iter()
            .find(|c| has_code(&c["code"], LOINC, code))
            .map(|c| &c["valueQuantity"])
    };
    let systolic = find(SYSTOLIC)?;
    let top = pressure_reading(systolic)?;
    let bottom = pressure_reading(find(DIASTOLIC)?)?;
    let unit = systolic["unit"].as_str().or(systolic["code"].as_str());
    Some(match unit {
        Some(unit) => format!("{top}/{bottom} {unit}"),
        None => format!("{top}/{bottom}"),
    })
}

/// What an Observation says, one line per value. Empty means "See FHIR".
pub fn result_lines(observation: &Value) -> Vec<String> {
    let mut lines: Vec<String> = value_text(observation).into_iter().collect();
    let Some(components) = observation["component"].as_array() else {
        return lines;
    };
    let pressure = blood_pressure(components);
    let has_pressure = pressure.is_some();
    lines.extend(pressure);
    for component in components {
        if has_pressure && is_pressure_part(component) {
            continue;
        }
        let name = concept_name(&component["code"]).unwrap_or_else(|| "Unnamed".into());
        let value = value_text(component).unwrap_or_else(|| "See FHIR".into());
        lines.push(format!("{name}: {value}"));
    }
    lines
}

/// `2026-10-14` as `14 Oct 2026`. A time stays as written, with no time zone conversion.
pub fn friendly_date(written: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (date, time) = match written.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (written, None),
    };
    let parts: Vec<&str> = date.split('-').collect();
    let month = |text: &str| {
        text.parse::<usize>()
            .ok()
            .and_then(|m| m.checked_sub(1))
            .and_then(|m| MONTHS.get(m))
    };
    let friendly = match parts.as_slice() {
        [year] if year.len() == 4 => return written.to_string(),
        [year, m] if month(m).is_some() => format!("{} {year}", month(m).unwrap()),
        [year, m, day] if month(m).is_some() => {
            format!(
                "{} {} {year}",
                day.trim_start_matches('0'),
                month(m).unwrap()
            )
        }
        _ => return written.to_string(),
    };
    match time {
        Some(time) => format!("{friendly} {time}"),
        None => friendly,
    }
}

pub fn date_or_none(date: Option<&str>) -> String {
    date.map_or_else(|| "No date".to_string(), friendly_date)
}

/// Rows that sort newest first, with undated rows last and `id` breaking ties.
pub trait Dated {
    fn sort_date(&self) -> Option<&str>;
    fn sort_id(&self) -> &str;
}

/// Sorts by the date's text. Times on one day with different offsets may be out of order.
pub fn newest_first<T: Dated>(rows: &mut [T]) {
    rows.sort_by(|a, b| {
        let by_date = match (a.sort_date(), b.sort_date()) {
            (Some(x), Some(y)) => y.cmp(x),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        by_date.then_with(|| a.sort_id().cmp(b.sort_id()))
    });
}

/// Percent-encodes a value for one URL path or query segment.
pub fn encode_segment(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// First letter uppercase, for showing a status code such as `active`.
pub fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
