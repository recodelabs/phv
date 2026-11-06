//! Original files and the links from records back to them.

use crate::browser::model::documents::{CATEGORIES, CATEGORY_SYSTEM, UNCATEGORIZED};
use crate::canon::{canon_resource, sha256_hex};
use crate::error::{Issue, PhvError};
use crate::store::{Change, Vault, line_id};
use crate::write::Phv;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct SourceOptions {
    /// What kind of document this is: `lab`, `receipt`, `visit-note`, `imaging` or `other`.
    pub category: Option<String>,
    /// When the file was added. The clock is read in the CLI and API layer, not here.
    pub added_at: Option<String>,
}

fn check_category(category: &str) -> Result<(), PhvError> {
    let known = category != UNCATEGORIZED && CATEGORIES.iter().any(|(code, _)| *code == category);
    if known {
        return Ok(());
    }
    Err(PhvError::Usage(format!(
        "unknown category {category:?}: use lab, receipt, visit-note, imaging or other"
    )))
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceReport {
    pub sha256: String,
    pub document_reference: String,
    pub already_imported: bool,
    /// True when the stored file was missing or damaged and was rewritten from the upload.
    pub restored: bool,
    pub warnings: Vec<Issue>,
}

/// Longest title kept, in characters.
const MAX_TITLE_CHARS: usize = 255;

/// Only the last path component of an untrusted name is kept, splitting on both `/` and `\`.
/// It is used as a title and to pick an extension, never as a path.
fn clean_name(file_name: &str) -> String {
    file_name
        .split(['/', '\\'])
        .rfind(|part| !part.is_empty() && *part != "." && *part != "..")
        .unwrap_or("source")
        .to_string()
}

/// The title: the cleaned name capped at `MAX_TITLE_CHARS` characters, on a character boundary.
fn title(name: &str) -> String {
    name.chars().take(MAX_TITLE_CHARS).collect()
}

fn extension(name: &str) -> String {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !ext.is_empty() && ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        ext
    } else {
        "bin".to_string()
    }
}

fn content_type(ext: &str) -> &'static str {
    match ext {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "heic" => "image/heic",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "ndjson" => "application/x-ndjson",
        "xml" => "application/xml",
        _ => "application/octet-stream",
    }
}

/// A source's `DocumentReference` ID: the first 32 hex characters of the file's SHA-256.
fn document_id(sha256: &str) -> String {
    sha256.chars().take(32).collect()
}

pub(crate) fn build_provenance(
    vault: &Vault,
    sha256: &str,
    targets: &[String],
    agent: &str,
    recorded: &str,
) -> Result<Value, PhvError> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(PhvError::Usage(
            "--source needs the full 64-character lowercase hex SHA-256 of the file".into(),
        ));
    }
    if agent.trim().is_empty() {
        return Err(PhvError::Usage("--agent must not be blank".into()));
    }
    let document = document_id(sha256);
    let not_found = || PhvError::NotFound(format!("source {sha256}: run phv source add first"));
    let stored = vault
        .get("DocumentReference", &document)?
        .ok_or_else(not_found)?;
    let stored: Value = serde_json::from_str(&stored).map_err(|_| {
        PhvError::Conflict(vec![Issue::error(
            "PHV-SOURCE-URL",
            format!("DocumentReference/{document}"),
            "the stored record for this source cannot be read: run phv check",
        )])
    })?;
    let matches_hash = stored["identifier"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|i| i["value"].as_str() == Some(sha256)));
    if !matches_hash {
        return Err(not_found());
    }
    let mut sorted: Vec<&String> = targets.iter().collect();
    sorted.sort();
    let joined: Vec<&str> = sorted.iter().map(|s| s.as_str()).collect();
    let id = sha256_hex(format!("{sha256}\n{}\n{agent}", joined.join("\n")).as_bytes());
    Ok(json!({
        "resourceType": "Provenance",
        "id": &id[..32],
        "target": sorted.iter().map(|t| json!({"reference": t})).collect::<Vec<_>>(),
        "recorded": recorded,
        "agent": [{"who": {"display": agent}}],
        "entity": [{"role": "source", "what": {"reference": format!("DocumentReference/{document}")}}]
    }))
}

impl Phv {
    pub async fn source_add(
        &self,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<SourceReport, PhvError> {
        self.source_add_with(file_name, bytes, &SourceOptions::default())
            .await
    }

    /// Adding a file that is already stored changes nothing, so it keeps its first category.
    pub async fn source_add_with(
        &self,
        file_name: &str,
        bytes: &[u8],
        options: &SourceOptions,
    ) -> Result<SourceReport, PhvError> {
        if let Some(category) = &options.category {
            check_category(category)?;
        }
        let _lock = self.vault().lock_async().await?;
        let sha256 = sha256_hex(bytes);
        let id = document_id(&sha256);
        let document_reference = format!("DocumentReference/{id}");
        if let Some(stored) = self.vault().get("DocumentReference", &id)? {
            let restored = self.restore_if_lost(&document_reference, &stored, &sha256, bytes)?;
            return Ok(SourceReport {
                sha256,
                document_reference,
                already_imported: true,
                restored,
                warnings: Vec::new(),
            });
        }

        let name = clean_name(file_name);
        let ext = extension(&name);
        let title = title(&name);
        let relative = format!("sources/{sha256}.{ext}");
        let mut resource = json!({
            "resourceType": "DocumentReference",
            "id": id,
            "status": "current",
            "identifier": [{"system": "urn:phv:sha256", "value": sha256}],
            "content": [{"attachment": {
                "contentType": content_type(&ext),
                "url": relative,
                "title": title
            }}]
        });
        let patient = self
            .vault()
            .lines("Patient")?
            .first()
            .and_then(|l| line_id(l).map(str::to_string));
        if let Some(patient) = patient {
            resource["subject"] = json!({"reference": format!("Patient/{patient}")});
        }
        if let Some(category) = &options.category {
            resource["category"] =
                json!([{"coding": [{"system": CATEGORY_SYSTEM, "code": category}]}]);
        }
        if let Some(added_at) = &options.added_at {
            resource["date"] = json!(added_at);
        }

        let issues = self.validator.validate(&resource).await;
        let (errors, warnings): (Vec<Issue>, Vec<Issue>) =
            issues.into_iter().partition(Issue::is_error);
        if !errors.is_empty() {
            return Err(PhvError::Invalid(errors));
        }

        self.vault()
            .write_atomic(&self.vault().root().join(&relative), bytes)?;
        self.vault().apply(&[Change::Upsert {
            resource_type: "DocumentReference".into(),
            id: id.clone(),
            line: canon_resource(&resource),
        }])?;
        Ok(SourceReport {
            sha256,
            document_reference,
            already_imported: false,
            restored: false,
            warnings,
        })
    }

    /// Rewrites the stored file when it is missing or its hash differs from this upload.
    /// The record's `url` is untrusted, so only this file's own `sources/<sha256>.<ext>` is ever written.
    fn restore_if_lost(
        &self,
        reference: &str,
        stored: &str,
        sha256: &str,
        bytes: &[u8],
    ) -> Result<bool, PhvError> {
        let refuse = |message: &str| {
            PhvError::Conflict(vec![Issue::error("PHV-SOURCE-URL", reference, message)])
        };
        let record: Value = serde_json::from_str(stored)
            .map_err(|_| refuse("the stored record is not valid JSON"))?;
        let url = record["content"][0]["attachment"]["url"]
            .as_str()
            .ok_or_else(|| refuse("the stored record has no attachment url"))?;
        // `source_add` only ever writes `sources/<sha256>.<ext>`, so nothing else is followed.
        let own_name = url
            .strip_prefix(&format!("sources/{sha256}."))
            .is_some_and(|ext| !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_alphanumeric()));
        if !own_name {
            return Err(refuse(
                "the stored attachment url is not sources/<sha256>.<extension> for this file",
            ));
        }
        let path = self.vault().root().join(url);
        // A symlink, FIFO or folder is never read: it counts as lost and is replaced.
        let intact = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_file() => sha256_hex(&std::fs::read(&path)?) == sha256,
            Ok(_) => false,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        if intact {
            return Ok(false);
        }
        self.vault().write_atomic(&path, bytes)?;
        Ok(true)
    }
}
