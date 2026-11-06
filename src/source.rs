//! Original files and the links from records back to them.

use crate::canon::{canon_resource, sha256_hex};
use crate::error::{Issue, PhvError};
use crate::store::{Change, Vault, line_id};
use crate::write::Phv;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct SourceReport {
    pub sha256: String,
    pub document_reference: String,
    pub already_imported: bool,
    pub warnings: Vec<Issue>,
}

/// Only the last path component of an untrusted name is kept. It is used as a title, never as a path.
fn clean_name(file_name: &str) -> String {
    Path::new(file_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("source")
        .to_string()
}

fn extension(file_name: &str) -> String {
    let ext = Path::new(file_name)
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
    let document = document_id(sha256);
    if vault.get("DocumentReference", &document)?.is_none() {
        return Err(PhvError::NotFound(format!(
            "source {sha256}: run phv source add first"
        )));
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
        let _lock = self.vault().lock_async().await?;
        let sha256 = sha256_hex(bytes);
        let id = document_id(&sha256);
        let document_reference = format!("DocumentReference/{id}");
        if self.vault().get("DocumentReference", &id)?.is_some() {
            return Ok(SourceReport {
                sha256,
                document_reference,
                already_imported: true,
                warnings: Vec::new(),
            });
        }

        let name = clean_name(file_name);
        let ext = extension(&name);
        let relative = format!("sources/{sha256}.{ext}");
        let mut resource = json!({
            "resourceType": "DocumentReference",
            "id": id,
            "status": "current",
            "identifier": [{"system": "urn:phv:sha256", "value": sha256}],
            "content": [{"attachment": {
                "contentType": content_type(&ext),
                "url": relative,
                "title": name
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
            warnings,
        })
    }
}
