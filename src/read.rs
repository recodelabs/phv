//! Reads scan the NDJSON file for one resource type.

use crate::error::PhvError;
use crate::write::Phv;
use octofhir_fhirpath::{
    Collection, EvaluationContext, FhirPathEngine, FhirPathValue, create_function_registry,
};
use octofhir_fhirschema::EmbeddedSchemaProvider;
use serde_json::Value;
use std::sync::Arc;

impl Phv {
    /// The stored line for one resource.
    pub fn get(&self, resource_type: &str, id: &str) -> Result<String, PhvError> {
        self.vault()
            .get(resource_type, id)?
            .ok_or_else(|| PhvError::NotFound(format!("{resource_type}/{id}")))
    }

    /// Stored lines for a type, in ID order. `filter` keeps resources where a FHIRPath
    /// expression evaluates to exactly `true`.
    pub async fn list(
        &self,
        resource_type: &str,
        filter: Option<&str>,
    ) -> Result<Vec<String>, PhvError> {
        let lines = self.vault().lines(resource_type)?;
        let Some(expression) = filter else {
            return Ok(lines);
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
        for line in lines {
            let resource: Value = serde_json::from_str(&line).map_err(|e| {
                PhvError::Usage(format!(
                    "unreadable line in {resource_type}: {e}: run phv check"
                ))
            })?;
            let context = EvaluationContext::new(
                Collection::single(FhirPathValue::resource(resource)),
                model.clone(),
                None,
                None,
                None,
            );
            let result = engine
                .evaluate(expression, &context)
                .await
                .map_err(|e| PhvError::Usage(format!("FHIRPath evaluation failed: {e}")))?;
            if result.value.len() == 1
                && result.value.first().and_then(|v| v.as_boolean()) == Some(true)
            {
                kept.push(line);
            }
        }
        Ok(kept)
    }
}
