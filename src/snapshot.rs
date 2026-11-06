//! Read-only, patient-agnostic capture. Cooperates with every phv writer's lock.
//! No directory, lock file, registry entry or vault content is created or changed.
use crate::{
    canon::{parse_json, sha256_hex},
    error::PhvError,
    store::Vault,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path},
    time::{Duration, Instant, SystemTime},
};

const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_TOTAL: usize = 64 * 1024 * 1024;

fn read(root: &Path, relative: &str, budget: &mut usize) -> Result<Vec<u8>, PhvError> {
    let path = root.join(relative);
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.len() > MAX_FILE || !fs::canonicalize(&path)?.starts_with(root) {
        return Err(PhvError::Usage(format!(
            "snapshot refuses non-regular, escaped or oversized file: {relative}"
        )));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)?;
    *budget += bytes.len();
    if bytes.len() as u64 > MAX_FILE || *budget > MAX_TOTAL {
        return Err(PhvError::Usage("snapshot exceeds its size limit".into()));
    }
    Ok(bytes)
}

pub fn capture(vault: &Vault) -> Result<Value, PhvError> {
    let root = fs::canonicalize(vault.root())?;
    // Opening read-only is deliberate: never create or truncate the writer's lock.
    let lock = File::open(root.join(".phv/lock")).map_err(|_| {
        PhvError::Usage(
            "snapshot needs an existing .phv/lock from a vault write; import records first".into(),
        )
    })?;
    let started = Instant::now();
    while lock.try_lock_shared().is_err() {
        if started.elapsed() > Duration::from_secs(2) {
            return Err(PhvError::Usage("vault is busy; retry the snapshot".into()));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut budget = 0;
    let mut files = BTreeMap::new();
    let config = read(&root, "phv.toml", &mut budget)?;
    files.insert("phv.toml".to_string(), sha256_hex(&config));
    let mut records = Vec::new();
    let mut sources = BTreeMap::new();
    let mut types = Vec::new();
    for entry in fs::read_dir(root.join("records"))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".ndjson") {
            types.push(name);
        }
    }
    types.sort();
    for name in types {
        let relative = format!("records/{name}");
        let bytes = read(&root, &relative, &mut budget)?;
        files.insert(relative.clone(), sha256_hex(&bytes));
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| PhvError::Usage(format!("{relative} is not UTF-8")))?;
        for (index, line) in text.lines().enumerate() {
            let resource = parse_json(line)
                .map_err(|e| PhvError::Usage(format!("{relative}:{}: {e}", index + 1)))?;
            if resource["resourceType"]
                .as_str()
                .map(|t| format!("{t}.ndjson"))
                != Some(name.clone())
            {
                return Err(PhvError::Usage(format!(
                    "wrong resource type in {relative}"
                )));
            }
            if resource["resourceType"] == "DocumentReference" {
                for content in resource["content"].as_array().into_iter().flatten() {
                    if let Some(url) = content["attachment"]["url"].as_str() {
                        let path = Path::new(url);
                        if !url.starts_with("sources/")
                            || !path.components().all(|c| matches!(c, Component::Normal(_)))
                        {
                            return Err(PhvError::Usage(
                                "snapshot requires local source paths inside sources/".into(),
                            ));
                        }
                        if !sources.contains_key(url) {
                            let source = read(&root, url, &mut budget)?;
                            let hash = sha256_hex(&source);
                            if path.file_stem().and_then(|s| s.to_str()) != Some(hash.as_str()) {
                                return Err(PhvError::Usage(format!(
                                    "source hash mismatch: {url}"
                                )));
                            }
                            files.insert(url.to_string(), hash.clone());
                            sources.insert(url.to_string(), json!({"sha256":hash, "text": if content["attachment"]["contentType"] == "text/plain" { std::str::from_utf8(&source).ok() } else { None }}));
                        }
                    }
                }
            }
            records.push(json!({"resource":resource,"sha256":sha256_hex(line.as_bytes()),"locator":format!("{relative}:{}",index+1)}));
        }
    }
    let binding = sha256_hex(root.to_string_lossy().as_bytes());
    let id = sha256_hex(
        serde_json::to_string(&json!({"vaultBinding":binding,"files":files}))
            .expect("snapshot metadata serializes")
            .as_bytes(),
    );
    Ok(
        json!({"schemaVersion":1,"adapterVersion":"phv-snapshot-1","id":format!("vault-{id}"),"vaultBinding":binding,"capturedAt":humantime::format_rfc3339_seconds(SystemTime::now()).to_string(),"files":files,"records":records,"sources":sources}),
    )
}
