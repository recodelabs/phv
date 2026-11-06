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

/// What a `reference` string points at, as far as this vault is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind<'a> {
    /// A fragment (`#id`) or an absolute URL. phv does not check these.
    External,
    /// A well-formed relative `Type/id`.
    Local(&'a str, &'a str),
    /// A `urn:` reference. It can only be resolved inside a submission.
    Unresolvable,
    /// Anything else, such as `patient/abc`, `Patient/john_doe` or `Patient/abc/_history/1`.
    Malformed,
}

pub fn reference_kind(reference: &str) -> ReferenceKind<'_> {
    if reference.starts_with('#') || reference.contains("://") {
        ReferenceKind::External
    } else if reference.starts_with("urn:") {
        ReferenceKind::Unresolvable
    } else if let Some((resource_type, id)) = local_target(reference) {
        ReferenceKind::Local(resource_type, id)
    } else {
        ReferenceKind::Malformed
    }
}
