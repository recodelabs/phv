//! Finding FHIR references inside a resource.

use serde_json::Value;

pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

/// Runs `f` on every string value of a key named `reference`.
pub fn for_each_ref(value: &mut Value, f: &mut impl FnMut(&mut String)) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if key == "reference"
                    && let Value::String(s) = child
                {
                    f(s);
                    continue;
                }
                for_each_ref(child, f);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| for_each_ref(item, f)),
        _ => {}
    }
}

pub fn collect_refs(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let mut copy = value.clone();
    for_each_ref(&mut copy, &mut |s| found.push(s.clone()));
    found
}

/// `Some((type, id))` for a relative `Type/id` reference, otherwise `None`.
pub fn local_target(reference: &str) -> Option<(&str, &str)> {
    let (resource_type, id) = reference.split_once('/')?;
    let type_ok = resource_type
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
        && resource_type.chars().all(|c| c.is_ascii_alphabetic());
    (type_ok && is_valid_id(id)).then_some((resource_type, id))
}
