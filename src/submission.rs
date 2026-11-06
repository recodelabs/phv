//! Turns a submitted resource or Bundle into final resources with assigned IDs.

use crate::canon::{derive_id, strip_volatile};
use crate::error::PhvError;
use crate::refs::for_each_ref;
use serde_json::{Value, json};
use std::collections::HashMap;

struct Entry {
    full_url: Option<String>,
    resource: Value,
}

fn resource_type(resource: &Value) -> Result<String, PhvError> {
    resource
        .get("resourceType")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| PhvError::Usage("input is not a FHIR resource: no resourceType".into()))
}

fn entries(input: &Value) -> Result<Vec<Entry>, PhvError> {
    if resource_type(input)? != "Bundle" {
        return Ok(vec![Entry {
            full_url: None,
            resource: input.clone(),
        }]);
    }
    let bundle_type = input.get("type").and_then(Value::as_str).unwrap_or("");
    if bundle_type != "transaction" && bundle_type != "collection" {
        return Err(PhvError::Usage(format!(
            "only transaction and collection Bundles can be added, not {bundle_type:?}"
        )));
    }
    let empty = Vec::new();
    let items = input
        .get("entry")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    items
        .iter()
        .map(|entry| {
            let resource = entry
                .get("resource")
                .ok_or_else(|| PhvError::Usage("a Bundle entry has no resource".into()))?;
            Ok(Entry {
                full_url: entry
                    .get("fullUrl")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                resource: resource.clone(),
            })
        })
        .collect()
}

/// Assigns IDs and rewrites references between resources in the submission.
/// The result is sorted by type and ID, with duplicates removed.
pub fn prepare(input: &Value) -> Result<Vec<Value>, PhvError> {
    let entries = entries(input)?;

    // Every name a resource can be referred to by inside this submission.
    let mut local_types: HashMap<String, String> = HashMap::new();
    for entry in &entries {
        let resource_type = resource_type(&entry.resource)?;
        if let Some(url) = &entry.full_url {
            local_types.insert(url.clone(), resource_type.clone());
        }
        if let Some(id) = entry.resource.get("id").and_then(Value::as_str) {
            local_types.insert(format!("{resource_type}/{id}"), resource_type.clone());
        }
    }

    let mut new_refs: HashMap<String, String> = HashMap::new();
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        // Local names become a fixed token, so IDs do not depend on them.
        let mut tokenized = entry.resource.clone();
        for_each_ref(&mut tokenized, &mut |s| {
            if let Some(target_type) = local_types.get(s.as_str()) {
                *s = format!("{target_type}/#");
            }
        });
        let new_id = derive_id(&tokenized);
        let resource_type = resource_type(&entry.resource)?;
        let new_ref = format!("{resource_type}/{new_id}");
        if let Some(url) = &entry.full_url {
            new_refs.insert(url.clone(), new_ref.clone());
        }
        if let Some(old_id) = entry.resource.get("id").and_then(Value::as_str) {
            new_refs.insert(format!("{resource_type}/{old_id}"), new_ref.clone());
        }
        let mut resource = entry.resource;
        strip_volatile(&mut resource);
        resource["id"] = json!(new_id);
        out.push(resource);
    }

    for resource in &mut out {
        for_each_ref(resource, &mut |s| {
            if let Some(new_ref) = new_refs.get(s.as_str()) {
                *s = new_ref.clone();
            }
        });
    }

    let key = |r: &Value| {
        (
            r["resourceType"].as_str().unwrap_or("").to_string(),
            r["id"].as_str().unwrap_or("").to_string(),
        )
    };
    out.sort_by_key(key);
    out.dedup_by_key(|r| key(r));
    Ok(out)
}
