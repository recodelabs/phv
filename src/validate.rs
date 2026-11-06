//! Base FHIR R4 validation: octofhir plus the checks octofhir does not make.

use crate::error::{Issue, PhvError};
use crate::refs::is_valid_id;
use octofhir_fhirpath::{FhirPathEngine, create_function_registry};
use octofhir_fhirschema::{
    EmbeddedSchemaProvider, FhirSchema, FhirSchemaElement, FhirValidator, FhirVersion,
    InMemoryTerminologyService, ValidationError, get_schemas,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

const REQUIRED_VALUE_SETS: &str = include_str!("../data/required-valuesets.json");

fn required_value_sets() -> BTreeMap<String, Vec<(String, String)>> {
    serde_json::from_str(REQUIRED_VALUE_SETS).expect("generated code lists are valid JSON")
}

fn schemas() -> &'static HashMap<String, FhirSchema> {
    get_schemas(FhirVersion::R4)
}

pub struct Validator {
    inner: FhirValidator,
}

impl Validator {
    pub async fn new() -> Result<Self, PhvError> {
        let engine = FhirPathEngine::new(
            Arc::new(create_function_registry()),
            Arc::new(EmbeddedSchemaProvider::r4()),
        )
        .await
        .map_err(|e| PhvError::Io(std::io::Error::other(e.to_string())))?;

        let mut terminology = InMemoryTerminologyService::new();
        for (url, codes) in required_value_sets() {
            for (code, system) in codes {
                terminology.add_code(&url, &code, Some(&system), None);
            }
        }

        let inner = FhirValidator::from_schemas(schemas().clone(), Some(Arc::new(engine)))
            .with_terminology_service(Arc::new(terminology));
        Ok(Self { inner })
    }

    /// All findings for one resource, in a stable order.
    pub async fn validate(&self, resource: &Value) -> Vec<Issue> {
        let mut issues = Vec::new();
        // Contained resources are not checked by octofhir, so each one is validated as a
        // resource of its own, with its findings under `contained[i].`.
        let mut pending = vec![(resource, String::new())];
        while let Some((current, prefix)) = pending.pop() {
            self.validate_one(current, &prefix, &mut issues).await;
            if let Some(Value::Array(contained)) = current.get("contained") {
                for (index, item) in contained.iter().enumerate() {
                    pending.push((item, format!("{prefix}contained[{index}].")));
                }
            }
        }
        if has_modifier_extension(resource) {
            issues.push(Issue::warning(
                "PHV-MODIFIER",
                resource.get("resourceType").and_then(Value::as_str).unwrap_or(""),
                "this record has a modifierExtension, which can change what it means and which phv cannot interpret",
            ));
        }

        issues.sort_by(|a, b| (&a.path, &a.code, &a.message).cmp(&(&b.path, &b.code, &b.message)));
        issues.dedup();
        issues
    }

    /// One resource without its contained resources. `prefix` is put before every path.
    async fn validate_one(&self, resource: &Value, prefix: &str, issues: &mut Vec<Issue>) {
        let Some(resource_type) = resource.get("resourceType").and_then(Value::as_str) else {
            issues.push(Issue::error(
                "PHV-TYPE",
                format!("{prefix}resourceType"),
                "resourceType is missing or not a string",
            ));
            return;
        };
        let Some(schema) = schemas()
            .get(resource_type)
            .filter(|s| s.kind == "resource" && s.abstract_type != Some(true))
        else {
            issues.push(Issue::error(
                "PHV-TYPE",
                format!("{prefix}{resource_type}"),
                format!("{resource_type:?} is not a FHIR R4 resource type"),
            ));
            return;
        };

        let result = self
            .inner
            .validate(resource, vec![resource_type.to_string()])
            .await;
        let mut found: Vec<Issue> = without_unmatched_coding_errors(resource, &result.errors)
            .into_iter()
            .map(from_error)
            .collect();
        found.extend(result.warnings.iter().map(|w| {
            let e = from_error(w);
            Issue::warning(&e.code, e.path, e.message)
        }));

        if let Some(id) = resource.get("id")
            && !id.as_str().is_some_and(is_valid_id)
        {
            found.push(Issue::error(
                "PHV-ID",
                format!("{resource_type}.id"),
                format!("{id} is not a valid FHIR id"),
            ));
        }
        if let Some(elements) = &schema.elements {
            check_choices(resource, elements, resource_type, &mut found);
        }
        issues.extend(found.into_iter().map(|mut issue| {
            issue.path = format!("{prefix}{}", issue.path);
            issue
        }));
    }
}

fn from_error(error: &ValidationError) -> Issue {
    let path = segments(error).join(".");
    let message = error.message.clone().unwrap_or_default();
    if is_known_engine_failure(error) {
        Issue::warning(&error.error_type, path, message)
    } else {
        Issue::error(&error.error_type, path, message)
    }
}

/// A constraint that could not be evaluated is an engine problem, not a finding about the
/// data. This covers only `tim-4` and `tim-5` on `Timing.repeat` with a unit and no
/// duration or period, which octofhir-fhirpath 0.5.8 fails with a type error. Any other
/// evaluation failure stays an error. Remove this when the bug is fixed upstream.
fn is_known_engine_failure(error: &ValidationError) -> bool {
    error.error_type == "FS1010"
        && matches!(error.constraint_key.as_deref(), Some("tim-4" | "tim-5"))
        && error
            .message
            .as_deref()
            .is_some_and(|m| m.contains("evaluation failed"))
}

fn segments(error: &ValidationError) -> Vec<String> {
    error
        .path
        .iter()
        .map(|p| p.as_str().map_or_else(|| p.to_string(), str::to_string))
        .collect()
}

/// FHIR needs only one coding of a `CodeableConcept` to match a required binding, but the
/// validator checks every coding. Drop an FS1012 error at `...coding[n]` when another coding
/// in the same `CodeableConcept` has a code and no FS1012 error of its own. The error stays
/// when no coding matches.
fn without_unmatched_coding_errors<'a>(
    resource: &Value,
    errors: &'a [ValidationError],
) -> Vec<&'a ValidationError> {
    let failing: BTreeSet<Vec<String>> = errors
        .iter()
        .filter(|e| e.error_type == "FS1012")
        .map(segments)
        .collect();
    errors
        .iter()
        .filter(|e| {
            !(e.error_type == "FS1012" && another_coding_matches(resource, &segments(e), &failing))
        })
        .collect()
}

fn another_coding_matches(
    resource: &Value,
    path: &[String],
    failing: &BTreeSet<Vec<String>>,
) -> bool {
    let Some((last, parent)) = path.split_last() else {
        return false;
    };
    let Some(this) = last
        .strip_prefix("coding[")
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|n| n.parse::<usize>().ok())
    else {
        return false;
    };
    // The first segment is the resource type, which is the resource itself.
    let Some(concept) = parent
        .iter()
        .skip(1)
        .try_fold(resource, |value, segment| step(value, segment))
    else {
        return false;
    };
    let Some(codings) = concept.get("coding").and_then(Value::as_array) else {
        return false;
    };
    codings.iter().enumerate().any(|(index, coding)| {
        let mut own = parent.to_vec();
        own.push(format!("coding[{index}]"));
        index != this && coding.get("code").is_some_and(Value::is_string) && !failing.contains(&own)
    })
}

/// One path segment of a validator error: `name`, `name[3]` or `name.ofType(type)`.
fn step<'a>(value: &'a Value, segment: &str) -> Option<&'a Value> {
    let (name, index) = match segment.split_once('[') {
        Some((name, rest)) => (name, Some(rest.strip_suffix(']')?.parse::<usize>().ok()?)),
        None => (segment, None),
    };
    let key = match name.split_once(".ofType(") {
        Some((stem, type_name)) => {
            let type_name = type_name.trim_end_matches(')');
            let mut chars = type_name.chars();
            let first = chars.next()?.to_uppercase();
            format!("{stem}{first}{}", chars.as_str())
        }
        None => name.to_string(),
    };
    let child = value.get(&key)?;
    match index {
        Some(index) => child.get(index),
        None => Some(child),
    }
}

fn has_modifier_extension(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(key, child)| key == "modifierExtension" || has_modifier_extension(child)),
        Value::Array(items) => items.iter().any(has_modifier_extension),
        _ => false,
    }
}

/// The validator accepts several `value[x]` variants together, so check that here.
fn check_choices(
    value: &Value,
    elements: &HashMap<String, FhirSchemaElement>,
    path: &str,
    issues: &mut Vec<Issue>,
) {
    let Some(object) = value.as_object() else {
        return;
    };
    for (stem, element) in elements {
        if let Some(choices) = &element.choices {
            let mut present: Vec<&String> =
                choices.iter().filter(|c| object.contains_key(*c)).collect();
            present.sort();
            if present.len() > 1 {
                issues.push(Issue::error(
                    "PHV-CHOICE",
                    format!("{path}.{stem}"),
                    format!("only one of {present:?} is allowed"),
                ));
            }
        }
    }
    check_extensions(object, path, issues);
    for (key, child) in object {
        if key == "extension" || key == "modifierExtension" {
            continue;
        }
        // Extensions on a primitive value are in `_name`.
        if let Some(primitive) = key.strip_prefix('_') {
            if elements.contains_key(primitive) {
                for item in as_items(child) {
                    if let Some(extended) = item.as_object() {
                        check_extensions(extended, &format!("{path}.{key}"), issues);
                    }
                }
            }
            continue;
        }
        let Some(element) = elements.get(key) else {
            continue;
        };
        let child_path = format!("{path}.{key}");
        for item in as_items(child) {
            let inline = item
                .get("resourceType")
                .and_then(Value::as_str)
                .and_then(|t| schemas().get(t))
                .and_then(|s| s.elements.as_ref());
            let by_type = element
                .type_name
                .as_deref()
                .and_then(|t| schemas().get(t))
                .and_then(|s| s.elements.as_ref());
            let by_reference = element
                .element_reference
                .as_deref()
                .and_then(referenced_elements);
            if let Some(children) = inline
                .or(element.elements.as_ref())
                .or(by_type)
                .or(by_reference)
            {
                check_choices(item, children, &child_path, issues);
            }
        }
    }
}

fn as_items(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    }
}

/// `extension` and `modifierExtension` are inherited by every element, so they are not in
/// any schema's `elements`. Check them against the `Extension` schema, nested ones included.
fn check_extensions(object: &serde_json::Map<String, Value>, path: &str, issues: &mut Vec<Issue>) {
    let Some(extension_elements) = schemas().get("Extension").and_then(|s| s.elements.as_ref())
    else {
        return;
    };
    for key in ["extension", "modifierExtension"] {
        if let Some(child) = object.get(key) {
            for item in as_items(child) {
                check_choices(item, extension_elements, &format!("{path}.{key}"), issues);
            }
        }
    }
}

/// Resolve a content reference such as `item.item` in `QuestionnaireResponse`, given as
/// `[structure url, "elements", "item", "elements", "item"]`, to the elements it points at.
fn referenced_elements(
    reference: &[String],
) -> Option<&'static HashMap<String, FhirSchemaElement>> {
    let root = reference.first()?.rsplit('/').next()?;
    let mut elements = schemas().get(root)?.elements.as_ref()?;
    for name in reference.iter().skip(2).step_by(2) {
        elements = elements.get(name)?.elements.as_ref()?;
    }
    Some(elements)
}

fn collect_required(elements: &HashMap<String, FhirSchemaElement>, found: &mut BTreeSet<String>) {
    for element in elements.values() {
        if let Some(binding) = &element.binding
            && binding.strength == "required"
            && let Some(url) = &binding.value_set
        {
            found.insert(url.clone());
        }
        if let Some(children) = &element.elements {
            collect_required(children, found);
        }
    }
}

/// Required value sets in the embedded schemas with no local code list. The validator
/// silently skips these, so the test suite prints and pins this list.
pub fn uncovered_required_bindings() -> Vec<String> {
    let mut found = BTreeSet::new();
    for schema in schemas().values() {
        if let Some(elements) = &schema.elements {
            collect_required(elements, &mut found);
        }
    }
    let known = required_value_sets();
    found
        .into_iter()
        .filter(|url| !known.contains_key(url))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(constraint_key: &str, message: &str) -> ValidationError {
        serde_json::from_value(serde_json::json!({
            "type": "FS1010", "path": ["X"], "message": message, "constraint-key": constraint_key
        }))
        .unwrap()
    }

    #[test]
    fn only_tim_4_and_tim_5_evaluation_failures_are_downgraded() {
        assert!(is_known_engine_failure(&error(
            "tim-4",
            "x evaluation failed y"
        )));
        assert!(is_known_engine_failure(&error(
            "tim-5",
            "x evaluation failed y"
        )));
        assert!(!is_known_engine_failure(&error(
            "obs-6",
            "x evaluation failed y"
        )));
        assert!(!is_known_engine_failure(&error(
            "tim-5",
            "Constraint 'tim-5' failed"
        )));
    }
}
