//! Whole-vault consistency check. Reports problems and changes nothing.
//!
//! It takes no lock and creates no file, so it works on a read-only vault. It reads
//! `records/` itself, rather than through `Vault::lines`, so that a file the store skips
//! or cannot read is reported instead of ignored or turned into an early error.

use crate::canon::canon_resource;
use crate::error::{Issue, PhvError};
use crate::refs::{ReferenceKind, collect_refs, is_valid_id, reference_kind};
use crate::write::{Phv, resource_ref};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path};

/// The `*.ndjson` files in `records/`, as file stems, sorted. A missing folder has none.
fn record_stems(root: &Path) -> Result<Vec<String>, PhvError> {
    let entries = match fs::read_dir(root.join("records")) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut stems = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "ndjson")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            stems.push(stem.to_string());
        }
    }
    stems.sort();
    Ok(stems)
}

fn is_type_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric())
}

/// True when `url` is `sources/` followed by plain path parts, with no `..`.
fn is_plain_sources_path(url: &str) -> bool {
    Path::new(url)
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
}

impl Phv {
    pub async fn check(&self) -> Result<Vec<Issue>, PhvError> {
        let root = self.vault().root();
        let mut issues = Vec::new();
        let mut parsed: Vec<(String, Value)> = Vec::new();

        for resource_type in record_stems(root)? {
            let file = format!("records/{resource_type}.ndjson");
            if !is_type_name(&resource_type) {
                issues.push(Issue::error(
                    "PHV-FILE",
                    &file,
                    "not a resource type file name: phv ignores this file",
                ));
                continue;
            }
            if let Err(error) = self.vault().check_line_endings(&resource_type) {
                match error.issues() {
                    [] => issues.push(Issue::error("PHV-FILE", &file, error.to_string())),
                    found => issues.extend_from_slice(found),
                }
            }
            let text = match fs::read(root.join(&file)) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => text,
                    Err(_) => {
                        issues.push(Issue::error("PHV-JSON", &file, "file is not valid UTF-8"));
                        continue;
                    }
                },
                Err(e) => {
                    issues.push(Issue::error(
                        "PHV-FILE",
                        &file,
                        format!("cannot be read: {e}"),
                    ));
                    continue;
                }
            };
            let mut seen = HashSet::new();
            let mut previous: Option<String> = None;
            for (index, line) in text.lines().enumerate() {
                let location = format!("{file}:{}", index + 1);
                let resource: Value = match serde_json::from_str(line) {
                    Ok(value) => value,
                    Err(e) => {
                        issues.push(Issue::error(
                            "PHV-JSON",
                            &location,
                            format!("not valid JSON: {e}"),
                        ));
                        continue;
                    }
                };
                if resource["resourceType"] != resource_type.as_str() {
                    issues.push(Issue::error(
                        "PHV-FILE",
                        &location,
                        format!(
                            "resourceType {} does not belong in this file",
                            resource["resourceType"]
                        ),
                    ));
                    continue;
                }
                if canon_resource(&resource) != line {
                    issues.push(Issue::error(
                        "PHV-FORMAT",
                        &location,
                        "line is not in canonical form",
                    ));
                }
                let id = resource["id"].as_str().unwrap_or("").to_string();
                if !is_valid_id(&id) {
                    issues.push(Issue::error(
                        "PHV-ID",
                        &location,
                        "resource has a missing or invalid id",
                    ));
                }
                if !seen.insert(id.clone()) {
                    issues.push(Issue::error(
                        "PHV-DUPLICATE",
                        &location,
                        format!("ID {id} appears more than once"),
                    ));
                }
                if previous.as_deref().is_some_and(|p| p > id.as_str()) {
                    issues.push(Issue::error("PHV-ORDER", &location, "line is out of order"));
                }
                previous = Some(id);
                for issue in self.validator.validate(&resource).await {
                    issues.push(Issue {
                        path: format!("{location} {}", issue.path),
                        ..issue
                    });
                }
                parsed.push((location, resource));
            }
        }

        let known: HashSet<String> = parsed.iter().map(|(_, r)| resource_ref(r)).collect();
        for (location, resource) in &parsed {
            for reference in collect_refs(resource) {
                let broken = match reference_kind(&reference) {
                    ReferenceKind::External => false,
                    ReferenceKind::Local(..) => !known.contains(&reference),
                    ReferenceKind::Unresolvable | ReferenceKind::Malformed => true,
                };
                if broken {
                    issues.push(Issue::error(
                        "PHV-REF",
                        location,
                        format!("{reference} does not resolve"),
                    ));
                }
            }
            if resource["resourceType"] == "DocumentReference"
                && let Some(contents) = resource["content"].as_array()
            {
                for content in contents {
                    let Some(url) = content["attachment"]["url"].as_str() else {
                        continue;
                    };
                    if !url.starts_with("sources/") {
                        continue;
                    }
                    if !is_plain_sources_path(url) {
                        issues.push(Issue::error(
                            "PHV-SOURCE",
                            location,
                            format!("{url} is not a plain path inside sources/"),
                        ));
                    } else if !root.join(url).exists() {
                        issues.push(Issue::error(
                            "PHV-SOURCE",
                            location,
                            format!("{url} is missing"),
                        ));
                    }
                }
            }
        }
        Ok(issues)
    }
}
