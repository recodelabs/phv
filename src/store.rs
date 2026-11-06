//! The data repo on disk. Lines are stored and copied as raw text, never re-serialized.

use crate::error::{Issue, PhvError};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Upsert {
        resource_type: String,
        id: String,
        line: String,
    },
    Remove {
        resource_type: String,
        id: String,
    },
}

#[derive(Deserialize)]
struct VaultConfig {
    format_version: u32,
}

/// Named vaults, stored in `~/.config/phv/config.toml`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default)]
    pub vaults: BTreeMap<String, PathBuf>,
}

fn is_valid_vault_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

impl Registry {
    pub fn parse(text: &str) -> Result<Self, PhvError> {
        toml::from_str(text)
            .map_err(|e| PhvError::Usage(format!("invalid ~/.config/phv/config.toml: {e}")))
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("the registry serializes")
    }

    /// Adds a vault. The first one registered becomes the default.
    pub fn register(&mut self, name: &str, path: &Path) -> Result<(), PhvError> {
        if !is_valid_vault_name(name) {
            return Err(PhvError::Usage(format!(
                "invalid vault name {name:?}: use lowercase letters, numbers, - and _"
            )));
        }
        if self.vaults.contains_key(name) {
            return Err(PhvError::Usage(format!(
                "a vault named {name} already exists"
            )));
        }
        self.vaults.insert(name.to_string(), path.to_path_buf());
        if self.default.is_none() {
            self.default = Some(name.to_string());
        }
        Ok(())
    }

    pub fn use_vault(&mut self, name: &str) -> Result<(), PhvError> {
        if !self.vaults.contains_key(name) {
            return Err(PhvError::NotFound(format!(
                "vault {name}: phv vaults lists them"
            )));
        }
        self.default = Some(name.to_string());
        Ok(())
    }
}

/// Precedence: `--vault`, then `PHV_VAULT`, then `default` in the user config.
/// `--vault` and `PHV_VAULT` take a registered name or a path.
pub fn resolve_vault_path(
    flag: Option<&Path>,
    env: Option<&str>,
    config_text: Option<&str>,
) -> Result<PathBuf, PhvError> {
    let registry = config_text
        .map(Registry::parse)
        .transpose()?
        .unwrap_or_default();
    let given = flag
        .map(Path::to_path_buf)
        .or_else(|| env.map(PathBuf::from));
    if let Some(given) = given {
        let named = given.to_str().and_then(|name| registry.vaults.get(name));
        return Ok(named.cloned().unwrap_or(given));
    }
    if let Some(path) = registry
        .default
        .as_ref()
        .and_then(|name| registry.vaults.get(name))
    {
        return Ok(path.clone());
    }
    Err(PhvError::Usage(
        "no vault given: run phv setup, use --vault, or set PHV_VAULT".into(),
    ))
}

/// The ID of a stored line, read from its fixed prefix without parsing the JSON.
pub fn line_id(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("{\"resourceType\":\"")?;
    let (_, rest) = rest.split_once("\",\"id\":\"")?;
    rest.split_once('"').map(|(id, _)| id)
}

fn is_public_code_repo(path: &Path) -> bool {
    for dir in path.ancestors() {
        if let Ok(config) = fs::read_to_string(dir.join(".git/config")) {
            // Match the exact repo, so a private repo such as recodelabs/phv-data is allowed.
            return config.lines().any(|line| {
                let line = line.trim();
                line.starts_with("url")
                    && (line.ends_with("recodelabs/phv") || line.ends_with("recodelabs/phv.git"))
            });
        }
    }
    false
}

fn check_type_name(resource_type: &str) -> Result<(), PhvError> {
    if !resource_type.is_empty() && resource_type.chars().all(|c| c.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(PhvError::Usage(format!(
            "invalid resource type: {resource_type}"
        )))
    }
}

#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    pub fn init(path: &Path) -> Result<Vault, PhvError> {
        let root = std::path::absolute(path)?;
        if is_public_code_repo(&root) {
            return Err(PhvError::Usage(
                "refusing to create a vault inside the public recodelabs/phv repo: health data must stay in a separate private repo".into(),
            ));
        }
        if root.join("phv.toml").exists() {
            return Err(PhvError::Usage(format!(
                "{} is already a vault",
                root.display()
            )));
        }
        fs::create_dir_all(root.join("records"))?;
        fs::create_dir_all(root.join("sources"))?;
        fs::write(
            root.join("phv.toml"),
            format!("format_version = {FORMAT_VERSION}\nfhir_version = \"4.0.1\"\n"),
        )?;
        fs::write(root.join(".gitignore"), ".phv/\n")?;
        Ok(Vault { root })
    }

    pub fn open(path: &Path) -> Result<Vault, PhvError> {
        let root = std::path::absolute(path)?;
        let text = fs::read_to_string(root.join("phv.toml")).map_err(|_| {
            PhvError::Usage(format!(
                "{} is not a phv vault: no phv.toml",
                root.display()
            ))
        })?;
        let config: VaultConfig =
            toml::from_str(&text).map_err(|e| PhvError::Usage(format!("invalid phv.toml: {e}")))?;
        if config.format_version > FORMAT_VERSION {
            return Err(PhvError::Usage(format!(
                "vault format {} is newer than this phv supports ({FORMAT_VERSION}): upgrade phv",
                config.format_version
            )));
        }
        Ok(Vault { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Holds an exclusive advisory lock until the returned file is dropped.
    pub fn lock(&self) -> Result<File, PhvError> {
        fs::create_dir_all(self.root.join(".phv"))?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join(".phv/lock"))?;
        file.lock()?;
        Ok(file)
    }

    fn file_for(&self, resource_type: &str) -> Result<PathBuf, PhvError> {
        check_type_name(resource_type)?;
        Ok(self
            .root
            .join("records")
            .join(format!("{resource_type}.ndjson")))
    }

    /// Resource types that have a file, sorted.
    pub fn resource_types(&self) -> Result<Vec<String>, PhvError> {
        let mut types = Vec::new();
        for entry in fs::read_dir(self.root.join("records"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "ndjson") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    types.push(stem.to_string());
                }
            }
        }
        types.sort();
        Ok(types)
    }

    /// The raw lines for one resource type.
    pub fn lines(&self, resource_type: &str) -> Result<Vec<String>, PhvError> {
        let path = self.file_for(resource_type)?;
        match fs::read_to_string(path) {
            Ok(text) => Ok(text.lines().map(str::to_string).collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// The raw line for one resource, if present.
    pub fn get(&self, resource_type: &str, id: &str) -> Result<Option<String>, PhvError> {
        Ok(self
            .lines(resource_type)?
            .into_iter()
            .find(|l| line_id(l) == Some(id)))
    }

    /// Writes bytes through a temporary file in the target's directory, then renames it.
    pub fn write_atomic(&self, target: &Path, bytes: &[u8]) -> Result<(), PhvError> {
        let dir = target
            .parent()
            .ok_or_else(|| PhvError::Usage("target has no parent".into()))?;
        let mut temp = tempfile::NamedTempFile::new_in(dir)?;
        temp.write_all(bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(target).map_err(|e| PhvError::Io(e.error))?;
        Ok(())
    }

    /// Applies changes file by file. Files whose content would not change are not written.
    pub fn apply(&self, changes: &[Change]) -> Result<(), PhvError> {
        let mut by_type: BTreeMap<&str, Vec<&Change>> = BTreeMap::new();
        for change in changes {
            let (Change::Upsert { resource_type, .. } | Change::Remove { resource_type, .. }) =
                change;
            by_type.entry(resource_type).or_default().push(change);
        }
        let mut pending = Vec::new();
        // Check every file before writing any of them.
        for (resource_type, changes) in by_type {
            let original = self.lines(resource_type)?;
            ensure_sorted(resource_type, &original)?;
            let mut lines = original.clone();
            for change in changes {
                apply_one(&mut lines, change);
            }
            if lines != original {
                pending.push((self.file_for(resource_type)?, lines));
            }
        }
        for (path, lines) in pending {
            if lines.is_empty() {
                fs::remove_file(path)?;
            } else {
                let mut text = lines.join("\n");
                text.push('\n');
                self.write_atomic(&path, text.as_bytes())?;
            }
        }
        Ok(())
    }
}

fn apply_one(lines: &mut Vec<String>, change: &Change) {
    let (id, new_line) = match change {
        Change::Upsert { id, line, .. } => (id, Some(line)),
        Change::Remove { id, .. } => (id, None),
    };
    let found = lines.binary_search_by(|l| line_id(l).unwrap_or("").cmp(id.as_str()));
    match (found, new_line) {
        (Ok(i), Some(line)) => lines[i] = line.clone(),
        (Ok(i), None) => {
            lines.remove(i);
        }
        (Err(i), Some(line)) => lines.insert(i, line.clone()),
        (Err(_), None) => {}
    }
}

fn ensure_sorted(resource_type: &str, lines: &[String]) -> Result<(), PhvError> {
    let in_order = lines
        .windows(2)
        .all(|pair| match (line_id(&pair[0]), line_id(&pair[1])) {
            (Some(a), Some(b)) => a < b,
            _ => false,
        });
    let readable = lines.len() != 1 || line_id(&lines[0]).is_some();
    if in_order && readable {
        Ok(())
    } else {
        Err(PhvError::Conflict(vec![Issue::error(
            "PHV-ORDER",
            format!("records/{resource_type}.ndjson"),
            "file is out of order or has an unreadable line, so phv will not rewrite it: run phv check",
        )]))
    }
}
