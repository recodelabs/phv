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
        let resource_type = resource
            .get("resourceType")
            .and_then(Value::as_str)
            .unwrap_or("");
        let Some(schema) = schemas()
            .get(resource_type)
            .filter(|s| s.kind == "resource")
        else {
            return vec![Issue::error(
                "PHV-TYPE",
                resource_type,
                format!("{resource_type:?} is not a FHIR R4 resource type"),
            )];
        };

        let result = self
            .inner
            .validate(resource, vec![resource_type.to_string()])
            .await;
        let mut issues: Vec<Issue> = result.errors.iter().map(from_error).collect();
        issues.extend(result.warnings.iter().map(|w| {
            let e = from_error(w);
            Issue::warning(&e.code, e.path, e.message)
        }));

        if let Some(id) = resource.get("id")
            && !id.as_str().is_some_and(is_valid_id)
        {
            issues.push(Issue::error(
                "PHV-ID",
                format!("{resource_type}.id"),
                format!("{id} is not a valid FHIR id"),
            ));
        }
        if let Some(elements) = &schema.elements {
            check_choices(resource, elements, resource_type, &mut issues);
        }
        if has_modifier_extension(resource) {
            issues.push(Issue::warning(
                "PHV-MODIFIER",
                resource_type,
                "this record has a modifierExtension, which can change what it means and which phv cannot interpret",
            ));
        }

        issues.sort_by(|a, b| (&a.path, &a.code, &a.message).cmp(&(&b.path, &b.code, &b.message)));
        issues.dedup();
        issues
    }
}

fn from_error(error: &ValidationError) -> Issue {
    let path = error
        .path
        .iter()
        .map(|p| p.as_str().map_or_else(|| p.to_string(), str::to_string))
        .collect::<Vec<_>>()
        .join(".");
    let message = error.message.clone().unwrap_or_default();
    // A constraint that could not be evaluated is an engine problem, not a finding about
    // the data. Example: tim-5 on Timing.repeat without a period in octofhir-fhirpath 0.5.8.
    if error.error_type == "FS1010" && message.contains("evaluation failed") {
        Issue::warning(&error.error_type, path, message)
    } else {
        Issue::error(&error.error_type, path, message)
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
    for (key, child) in object {
        let Some(element) = elements.get(key) else {
            continue;
        };
        let items: Vec<&Value> = match child {
            Value::Array(items) => items.iter().collect(),
            other => vec![other],
        };
        let child_path = format!("{path}.{key}");
        for item in items {
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
            if let Some(children) = inline.or(element.elements.as_ref()).or(by_type) {
                check_choices(item, children, &child_path, issues);
            }
        }
    }
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
