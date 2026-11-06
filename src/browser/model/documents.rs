//! The Documents topic and links between records and their source files.

use super::value::{Dated, date_or_none, id_of, newest_first};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub const CATEGORY_SYSTEM: &str = "urn:phv:document-category";
pub const HASH_SYSTEM: &str = "urn:phv:sha256";
pub const UNCATEGORIZED: &str = "uncategorized";

/// Category codes and their labels, in the order the filter chips show them.
pub const CATEGORIES: [(&str, &str); 6] = [
    ("lab", "Lab"),
    ("receipt", "Receipt"),
    ("visit-note", "Visit note"),
    ("imaging", "Imaging"),
    ("other", "Other"),
    (UNCATEGORIZED, "Uncategorized"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocRow {
    pub id: String,
    pub raw_date: Option<String>,
    pub date: String,
    pub title: String,
    pub category: String,
    pub category_label: String,
    pub sha256: Option<String>,
    pub has_file: bool,
    pub record_count: usize,
}

impl Dated for DocRow {
    fn sort_date(&self) -> Option<&str> {
        self.raw_date.as_deref()
    }
    fn sort_id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub code: String,
    pub label: String,
    pub count: usize,
    pub on: bool,
}

/// Where a record came from, according to Provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSource {
    pub document_id: String,
    pub agent: Option<String>,
}

fn document_entity(provenance: &Value) -> Option<String> {
    provenance["entity"]
        .as_array()?
        .iter()
        .filter_map(|e| e["what"]["reference"].as_str())
        .find_map(|r| r.strip_prefix("DocumentReference/"))
        .map(str::to_string)
}

fn targets(provenance: &Value) -> Vec<&str> {
    provenance["target"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["reference"].as_str())
        .collect()
}

/// The source of `Type/id`, from the first Provenance that names it.
pub fn source_of(reference: &str, provenance: &[Value]) -> Option<RecordSource> {
    provenance.iter().find_map(|p| {
        if !targets(p).contains(&reference) {
            return None;
        }
        Some(RecordSource {
            document_id: document_entity(p)?,
            agent: p["agent"][0]["who"]["display"].as_str().map(str::to_string),
        })
    })
}

fn records_from(document_id: &str, provenance: &[Value]) -> usize {
    provenance
        .iter()
        .filter(|p| document_entity(p).as_deref() == Some(document_id))
        .map(|p| targets(p).len())
        .sum()
}

fn category_code(document: &Value) -> String {
    let code = document["category"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|c| c["coding"].as_array().into_iter().flatten())
        .find(|c| c["system"] == CATEGORY_SYSTEM)
        .and_then(|c| c["code"].as_str());
    match code {
        Some(code) if CATEGORIES.iter().any(|(known, _)| *known == code) => code.to_string(),
        _ => UNCATEGORIZED.to_string(),
    }
}

fn category_label(code: &str) -> String {
    CATEGORIES
        .iter()
        .find(|(known, _)| *known == code)
        .map_or(code, |(_, label)| label)
        .to_string()
}

pub fn sha256_of(document: &Value) -> Option<String> {
    document["identifier"]
        .as_array()?
        .iter()
        .find(|i| i["system"] == HASH_SYSTEM)
        .and_then(|i| i["value"].as_str())
        .map(str::to_string)
}

pub fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The regular file `sources/<sha256>.<ext>`. Symlinks and anything else do not count.
pub fn source_file(vault_root: &Path, sha256: &str) -> Option<PathBuf> {
    if !is_sha256(sha256) {
        return None;
    }
    let prefix = format!("{sha256}.");
    fs::read_dir(vault_root.join("sources"))
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .find(|entry| {
            fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.file_type().is_file())
        })
        .map(|entry| entry.path())
}

/// Every document, newest first, leaving out those entered in error.
pub fn documents(resources: &[Value], provenance: &[Value], vault_root: &Path) -> Vec<DocRow> {
    let mut rows: Vec<DocRow> = resources
        .iter()
        .filter(|d| d["status"] != "entered-in-error")
        .map(|d| {
            let id = id_of(d);
            let raw_date = d["date"].as_str().map(str::to_string);
            let sha256 = sha256_of(d);
            let category = category_code(d);
            DocRow {
                date: date_or_none(raw_date.as_deref()),
                raw_date,
                title: d["content"][0]["attachment"]["title"]
                    .as_str()
                    .unwrap_or("Untitled document")
                    .to_string(),
                category_label: category_label(&category),
                category,
                has_file: sha256
                    .as_deref()
                    .is_some_and(|hash| source_file(vault_root, hash).is_some()),
                sha256,
                record_count: records_from(&id, provenance),
                id,
            }
        })
        .collect();
    newest_first(&mut rows);
    rows
}

/// Filter chips for categories that have documents, after an "All" chip.
pub fn chips(rows: &[DocRow], selected: Option<&str>) -> Vec<Chip> {
    let mut chips = vec![Chip {
        code: String::new(),
        label: "All".to_string(),
        count: rows.len(),
        on: selected.is_none(),
    }];
    for (code, label) in CATEGORIES {
        let count = rows.iter().filter(|r| r.category == code).count();
        if count > 0 {
            chips.push(Chip {
                code: code.to_string(),
                label: label.to_string(),
                count,
                on: selected == Some(code),
            });
        }
    }
    chips
}

/// Whether a file with this extension may open in the browser, and its media type.
pub fn inline_media_type(extension: &str) -> Option<&'static str> {
    match extension {
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "txt" => Some("text/plain; charset=utf-8"),
        _ => None,
    }
}
