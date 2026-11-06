//! Turns a submitted resource or Bundle into final resources with assigned IDs.

use crate::canon::{canon_value, derive_id, strip_volatile};
use crate::error::PhvError;
use crate::refs::{collect_refs, for_each_ref};
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
    let items = input
        .get("entry")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or_else(|| PhvError::Usage("a Bundle must have a non-empty entry array".into()))?;
    items
        .iter()
        .map(|entry| {
            let resource = entry
                .get("resource")
                .ok_or_else(|| PhvError::Usage("a Bundle entry has no resource".into()))?;
            if resource.get("resourceType").and_then(Value::as_str) == Some("Bundle") {
                return Err(PhvError::Usage(
                    "a Bundle inside a Bundle cannot be added".into(),
                ));
            }
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
/// ID, working up from resources that reference nothing else in the submission. A
/// reference that cannot be resolved this way, because it is in or leads to a circular
/// reference, is hashed as `Type/#`. Every entry is hashed exactly once. Two different
/// resources that end up with one ID are refused.
pub fn prepare(input: &Value) -> Result<Vec<Value>, PhvError> {
    let entries = entries(input)?;

    let types = entries
        .iter()
        .map(|entry| resource_type(&entry.resource))
        .collect::<Result<Vec<_>, _>>()?;

    // Every name a resource can be referred to by inside this submission.
    let mut local: HashMap<String, usize> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let mut names = Vec::new();
        if let Some(url) = &entry.full_url {
            names.push(url.clone());
        }
        if let Some(id) = entry.resource.get("id").and_then(Value::as_str) {
            names.push(format!("{}/{id}", types[i]));
        }
        for name in names {
            match local.get(&name) {
                Some(&other)
                    if other != i
                        && canon_value(&entries[other].resource)
                            != canon_value(&entry.resource) =>
                {
                    return Err(PhvError::Usage(format!(
                        "two different resources in the submission are both named {name}"
                    )));
                }
                Some(_) => {}
                None => {
                    local.insert(name, i);
                }
            }
        }
    }

    // Which entries each entry refers to, and who refers back.
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    let mut waiting = vec![0usize; entries.len()];
    for (i, entry) in entries.iter().enumerate() {
        let mut targets: Vec<usize> = Vec::new();
        for reference in collect_refs(&entry.resource) {
            if let Some(&target) = local.get(&reference)
                && !targets.contains(&target)
            {
                targets.push(target);
            }
        }
        waiting[i] = targets.len();
        for target in targets {
            dependents[target].push(i);
        }
    }

    // Hashes entry `i`, with local references replaced by `Type/<id>` for resolved
    // targets and `Type/#` for the others.
    let hash = |i: usize, ids: &[Option<String>]| {
        let mut tokenized = entries[i].resource.clone();
        for_each_ref(&mut tokenized, &mut |s| {
            if let Some(&target) = local.get(s.as_str()) {
                *s = match &ids[target] {
                    Some(id) => format!("{}/{id}", types[target]),
                    None => format!("{}/#", types[target]),
                };
            }
        });
        derive_id(&tokenized)
    };

    // Peel from the leaves: an entry is hashed once all its targets have an ID.
    let mut ids: Vec<Option<String>> = vec![None; entries.len()];
    let mut ready: Vec<usize> = (0..entries.len()).filter(|&i| waiting[i] == 0).collect();
    while let Some(i) = ready.pop() {
        ids[i] = Some(hash(i, &ids));
        for &dependent in &dependents[i] {
            waiting[dependent] -= 1;
            if waiting[dependent] == 0 {
                ready.push(dependent);
            }
        }
    }
    // What is left is in or leads to a cycle. Hash it against the resolved IDs only.
    let stuck: Vec<(usize, String)> = (0..entries.len())
        .filter(|&i| ids[i].is_none())
        .map(|i| (i, hash(i, &ids)))
        .collect();
    for (i, id) in stuck {
        ids[i] = Some(id);
    }
    let ids: Vec<String> = ids.into_iter().flatten().collect();

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
