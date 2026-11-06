//! Which vaults this server shows. Paths never come from a URL.

use crate::error::PhvError;
use crate::store::Registry;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultEntry {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vaults {
    entries: Vec<VaultEntry>,
    default: String,
}

impl Vaults {
    /// Precedence matches `resolve_vault_path`. A given name opens the whole registry on that
    /// vault. A given path that is not a registered name is the only entry.
    pub fn resolve(
        flag: Option<&Path>,
        env: Option<&str>,
        config_text: Option<&str>,
    ) -> Result<Self, PhvError> {
        let registry = config_text
            .map(Registry::parse)
            .transpose()?
            .unwrap_or_default();
        let given = flag
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .or_else(|| env.filter(|e| !e.is_empty()).map(PathBuf::from));
        let entries: Vec<VaultEntry> = registry
            .vaults
            .iter()
            .map(|(name, path)| VaultEntry {
                name: name.clone(),
                path: path.clone(),
            })
            .collect();
        if let Some(given) = given {
            if let Some(name) = given.to_str().filter(|n| registry.vaults.contains_key(*n)) {
                return Ok(Self {
                    default: name.to_string(),
                    entries,
                });
            }
            return Ok(Self::single(given));
        }
        match registry.default {
            Some(default) if registry.vaults.contains_key(&default) => {
                Ok(Self { entries, default })
            }
            _ => Err(PhvError::Usage(
                "no vault given: run phv setup, use --vault, or set PHV_VAULT".into(),
            )),
        }
    }

    pub fn single(path: PathBuf) -> Self {
        let name = "vault".to_string();
        Self {
            entries: vec![VaultEntry {
                name: name.clone(),
                path,
            }],
            default: name,
        }
    }

    pub fn entries(&self) -> &[VaultEntry] {
        &self.entries
    }

    pub fn get(&self, name: &str) -> Option<&VaultEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn default_name(&self) -> &str {
        &self.default
    }
}
