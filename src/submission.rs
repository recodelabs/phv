//! Turns a submitted resource or Bundle into final resources with assigned IDs.

use crate::canon::{canon_value, derive_id, strip_volatile};
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
/// The result is sorted by type and ID, with identical duplicates removed.
///
/// A reference to another resource in the submission is hashed as that resource's own
/// ID, refined round by round until the IDs stop changing or for as many rounds as there
/// are entries, which bounds circular references. Two different resources that end up
/// with one ID are refused.
pub fn prepare(input: &Value) -> Result<Vec<Value>, PhvError> {
    let entries = entries(input)?;

    let types = entries
        .iter()
        .map(|entry| resource_type(&entry.resource))
        .collect::<Result<Vec<_>, _>>()?;

    // Every name a resource can be referred to by inside this submission.
    let mut local: HashMap<String, usize> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        if let Some(url) = &entry.full_url {
            local.insert(url.clone(), i);
        }
        if let Some(id) = entry.resource.get("id").and_then(Value::as_str) {
            local.insert(format!("{}/{id}", types[i]), i);
        }
    }

    // Round 0 hashes local references as `Type/#`; each later round uses the
    // target's ID from the round before.
    let mut ids: Vec<String> = Vec::new();
    for round in 0..=entries.len() {
        let next: Vec<String> = entries
            .iter()
            .map(|entry| {
                let mut tokenized = entry.resource.clone();
                for_each_ref(&mut tokenized, &mut |s| {
                    if let Some(&target) = local.get(s.as_str()) {
                        *s = if round == 0 {
                            format!("{}/#", types[target])
                        } else {
                            format!("{}/{}", types[target], ids[target])
                        };
                    }
                });
                derive_id(&tokenized)
            })
            .collect();
        let settled = next == ids;
        ids = next;
        if settled {
            break;
        }
    }

    let mut new_refs: HashMap<String, String> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let new_ref = format!("{}/{}", types[i], ids[i]);
        if let Some(url) = &entry.full_url {
            new_refs.insert(url.clone(), new_ref.clone());
        }
        if let Some(old_id) = entry.resource.get("id").and_then(Value::as_str) {
            new_refs.insert(format!("{}/{old_id}", types[i]), new_ref);
        }
    }

    let mut out = Vec::with_capacity(entries.len());
    for (entry, new_id) in entries.into_iter().zip(ids) {
        let mut resource = entry.resource;
        strip_volatile(&mut resource);
        resource["id"] = json!(new_id);
        for_each_ref(&mut resource, &mut |s| {
            if let Some(new_ref) = new_refs.get(s.as_str()) {
                *s = new_ref.clone();
            }
        });
        out.push(resource);
    }

    let key = |r: &Value| {
        (
            r["resourceType"].as_str().unwrap_or("").to_string(),
            r["id"].as_str().unwrap_or("").to_string(),
        )
    };
    out.sort_by_key(key);
    let mut unique: Vec<Value> = Vec::with_capacity(out.len());
    for resource in out {
        if let Some(last) = unique.last()
            && key(last) == key(&resource)
        {
            if canon_value(last) != canon_value(&resource) {
                let (resource_type, id) = key(&resource);
                return Err(PhvError::Usage(format!(
                    "two different resources would both become {resource_type}/{id}"
                )));
            }
            continue;
        }
        unique.push(resource);
    }
    Ok(unique)
}
