//! Reads scan the NDJSON file for one resource type.

use crate::error::PhvError;
use crate::store::line_id;
use crate::write::Phv;
use octofhir_fhirpath::{
    Collection, EvaluationContext, FhirPathEngine, FhirPathValue, create_function_registry,
};
use octofhir_fhirschema::{EmbeddedSchemaProvider, FhirVersion, get_schemas};
use serde_json::Value;
use std::sync::Arc;

/// The outcome of a filtered read.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub lines: Vec<String>,
    /// Records that gave no true or false answer and were left out.
    pub unanswered: usize,
    pub total: usize,
}

impl SearchResult {
    /// A one-line, ASCII warning when some records were left out without an answer.
    pub fn warning(&self) -> Option<String> {
        if self.unanswered == 0 {
            return None;
        }
        Some(format!(
            "{} of {} records gave no true or false answer and were left out: check element names, and use .toDate() to compare dates",
            self.unanswered, self.total
        ))
    }
}

fn check_resource_type(resource_type: &str) -> Result<(), PhvError> {
    let known = get_schemas(FhirVersion::R4)
        .get(resource_type)
        .is_some_and(|s| s.kind == "resource" && s.abstract_type != Some(true));
    if known {
        Ok(())
    } else {
        Err(PhvError::Usage(format!(
            "{resource_type:?} is not a FHIR R4 resource type (names are case-sensitive, such as Observation)"
        )))
    }
}

impl Phv {
    /// The stored line for one resource.
    pub fn get(&self, resource_type: &str, id: &str) -> Result<String, PhvError> {
        check_resource_type(resource_type)?;
        self.vault()
            .get(resource_type, id)?
            .ok_or_else(|| PhvError::NotFound(format!("{resource_type}/{id}")))
    }

    /// Stored lines for a type, in ID order, with unanswered records left out silently.
    /// Prefer `search`, which reports them.
    pub async fn list(
        &self,
        resource_type: &str,
        filter: Option<&str>,
    ) -> Result<Vec<String>, PhvError> {
        Ok(self.search(resource_type, filter).await?.lines)
    }

    /// Stored lines for a type, in ID order. `filter` keeps resources where a FHIRPath
    /// expression evaluates to exactly `true`. A record whose result is not exactly one
    /// boolean is left out and counted as unanswered.
    pub async fn search(
        &self,
        resource_type: &str,
        filter: Option<&str>,
    ) -> Result<SearchResult, PhvError> {
        check_resource_type(resource_type)?;
        let lines = self.vault().lines(resource_type)?;
        let total = lines.len();
        let Some(expression) = filter else {
            return Ok(SearchResult {
                lines,
                unanswered: 0,
                total,
            });
        };

        let model = Arc::new(EmbeddedSchemaProvider::r4());
        let engine = FhirPathEngine::new(Arc::new(create_function_registry()), model.clone())
            .await
            .map_err(|e| PhvError::Io(std::io::Error::other(e.to_string())))?;

        // Parse errors must surface even when there are no lines to evaluate.
        let empty = EvaluationContext::new(
            Collection::single(FhirPathValue::resource(
                serde_json::json!({"resourceType":"Basic"}),
            )),
            model.clone(),
            None,
            None,
            None,
        );
        engine
            .evaluate(expression, &empty)
            .await
            .map_err(|e| PhvError::Usage(format!("invalid FHIRPath expression: {e}")))?;

        let mut kept = Vec::new();
        let mut unanswered = 0;
        for (index, line) in lines.into_iter().enumerate() {
            let number = index + 1;
            let resource: Value = serde_json::from_str(&line).map_err(|e| {
                PhvError::Usage(format!(
                    "unreadable line {number} in {resource_type}: {e}: run phv check"
                ))
            })?;
            let context = EvaluationContext::new(
                Collection::single(FhirPathValue::resource(resource)),
                model.clone(),
                None,
                None,
                None,
            );
            let result = engine.evaluate(expression, &context).await.map_err(|e| {
                let record = line_id(&line)
                    .map(|id| format!("{resource_type}/{id}"))
                    .unwrap_or_else(|| format!("line {number} of {resource_type}"));
                PhvError::Usage(format!("FHIRPath evaluation failed on {record}: {e}"))
            })?;
            let answer = if result.value.len() == 1 {
                result.value.first().and_then(|v| v.as_boolean())
            } else {
                None
            };
            match answer {
                Some(true) => kept.push(line),
                Some(false) => {}
                None => unanswered += 1,
            }
        }
        Ok(SearchResult {
            lines: kept,
            unanswered,
            total,
        })
    }
}
