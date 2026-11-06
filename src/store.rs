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
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| env.filter(|e| !e.is_empty()).map(PathBuf::from));
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
    // The ID must directly follow the type, so a nested "id" is never taken for the resource's.
    let (_, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(",\"id\":\"")?;
    let (id, _) = rest.split_once('"')?;
    // An escaped ID is not a plain ID, so it is treated as unreadable.
    if id.contains('\\') { None } else { Some(id) }
}

/// The text of the git config that applies to `dir`, following a `.git` file
/// (worktree or submodule) to its git directory and its `commondir` if it has one.
fn git_config_for(dir: &Path) -> Option<String> {
    let dot_git = dir.join(".git");
    let gitdir = if fs::metadata(&dot_git).ok()?.is_dir() {
        dot_git
    } else {
        let text = fs::read_to_string(&dot_git).ok()?;
        let target = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("gitdir:"))?
            .trim();
        // An absolute target replaces the base; a relative one is relative to `dir`.
        dir.join(target)
    };
    let common = match fs::read_to_string(gitdir.join("commondir")) {
        Ok(text) => gitdir.join(text.trim()),
        Err(_) => gitdir,
    };
    fs::read_to_string(common.join("config")).ok()
}

/// True for a remote URL that names the public recodelabs/phv repo, in any common form.
fn is_public_remote(value: &str) -> bool {
    let url = value.trim().to_lowercase();
    let url = url.strip_suffix('/').unwrap_or(&url);
    let url = url.strip_suffix(".git").unwrap_or(url);
    let url = url.strip_suffix('/').unwrap_or(url);
    url == "recodelabs/phv"
        || url
            .strip_suffix("recodelabs/phv")
            .is_some_and(|before| before.ends_with('/') || before.ends_with(':'))
}

/// A git config value as git reads it: quotes removed, and an unquoted `#` or `;` starts
/// a comment.
fn config_value(raw: &str) -> String {
    let mut value = String::new();
    let mut quoted = false;
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' => match chars.next() {
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some(other) => value.push(other),
                None => {}
            },
            '#' | ';' if !quoted => break,
            other => value.push(other),
        }
    }
    value.trim().to_string()
}

/// The path with symlinks resolved as far as it exists: the deepest existing ancestor is
/// canonicalized and the rest is appended, so a link into the repo is seen for what it is.
fn resolve_existing(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut current = path;
    loop {
        if let Ok(canonical) = fs::canonicalize(current) {
            return rest
                .iter()
                .rev()
                .fold(canonical, |resolved, name| resolved.join(name));
        }
        match (current.parent(), current.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                current = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn is_public_code_repo(path: &Path) -> bool {
    let path = resolve_existing(path);
    for dir in path.ancestors() {
        if let Some(config) = git_config_for(dir) {
            // Match the exact repo, so a private repo such as recodelabs/phv-data is allowed.
            return config.lines().any(|line| {
                let line = line.trim();
                let Some(key) = line.get(..3).filter(|k| k.eq_ignore_ascii_case("url")) else {
                    return false;
                };
                line[key.len()..]
                    .trim_start()
                    .strip_prefix('=')
                    .is_some_and(|value| is_public_remote(&config_value(value)))
            });
        }
    }
    false
}

/// Creates a directory and any missing parents. New folders are private (0700) on unix;
/// folders that already exist are left as they are.
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Makes a directory's entries (a new file, a rename, a removal) durable.
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// Keeps an existing `.gitignore` byte for byte and adds `.phv/` only when it is missing.
fn ensure_gitignore(root: &Path) -> std::io::Result<()> {
    let path = root.join(".gitignore");
    match fs::read(&path) {
        Ok(existing) => {
            let listed = existing.split(|b| *b == b'\n').any(|line| {
                matches!(
                    line.strip_suffix(b"\r").unwrap_or(line),
                    b".phv/" | b"/.phv/"
                )
            });
            if listed {
                return Ok(());
            }
            let mut addition = Vec::new();
            if !existing.is_empty() && !existing.ends_with(b"\n") {
                addition.push(b'\n');
            }
            addition.extend_from_slice(b".phv/\n");
            OpenOptions::new()
                .append(true)
                .open(&path)?
                .write_all(&addition)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::write(&path, ".phv/\n"),
        Err(e) => Err(e),
    }
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
        create_private_dir(&root.join("records"))?;
        create_private_dir(&root.join("sources"))?;
        create_private_dir(&root.join(".phv"))?;
        // phv.toml goes last: it is what marks the vault as made, so a failure above can be retried.
        ensure_gitignore(&root)?;
        fs::write(
            root.join("phv.toml"),
            format!("format_version = {FORMAT_VERSION}\nfhir_version = \"4.0.1\"\n"),
        )?;
        Ok(Vault { root })
    }

    pub fn open(path: &Path) -> Result<Vault, PhvError> {
        let root = std::path::absolute(path)?;
        let text = fs::read_to_string(root.join("phv.toml")).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                PhvError::Usage(format!(
                    "{} is not a phv vault: no phv.toml",
                    root.display()
                ))
            } else {
                PhvError::Io(e)
            }
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
        create_private_dir(&self.root.join(".phv"))?;
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
        let entries = match fs::read_dir(self.root.join("records")) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(types),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "ndjson")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && check_type_name(stem).is_ok()
            {
                types.push(stem.to_string());
            }
        }
        types.sort();
        Ok(types)
    }

    /// Refuses a file with carriage returns or without a final newline, because a rewrite
    /// would change the bytes of lines it should only copy. A missing file is fine.
    pub fn check_line_endings(&self, resource_type: &str) -> Result<(), PhvError> {
        let path = self.file_for(resource_type)?;
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        if bytes.contains(&b'\r') || (!bytes.is_empty() && !bytes.ends_with(b"\n")) {
            return Err(PhvError::Conflict(vec![Issue::error(
                "PHV-NEWLINE",
                format!("records/{resource_type}.ndjson"),
                "file has carriage returns or no final newline, so phv will not rewrite it: fix the line endings",
            )]));
        }
        Ok(())
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

    /// Each line's bytes, for readers that must survive one line of invalid UTF-8.
    pub fn raw_lines(&self, resource_type: &str) -> Result<Vec<Vec<u8>>, PhvError> {
        let path = self.file_for(resource_type)?;
        match fs::read(path) {
            Ok(bytes) => Ok(bytes
                .split(|b| *b == b'\n')
                .filter(|line| !line.is_empty())
                .map(<[u8]>::to_vec)
                .collect()),
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

    /// Writes bytes through a temporary file in `.phv/tmp` (same filesystem, never committed),
    /// then renames it over the target and syncs the target's directory.
    pub fn write_atomic(&self, target: &Path, bytes: &[u8]) -> Result<(), PhvError> {
        let dir = target
            .parent()
            .ok_or_else(|| PhvError::Usage("target has no parent".into()))?;
        // Git does not track empty folders, so a clone may lack the target's folder.
        create_private_dir(dir)?;
        let tmp_dir = self.root.join(".phv/tmp");
        create_private_dir(&tmp_dir)?;
        let mut temp = tempfile::NamedTempFile::new_in(tmp_dir)?;
        temp.write_all(bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(target).map_err(|e| PhvError::Io(e.error))?;
        sync_dir(dir)?;
        Ok(())
    }

    /// Holds the lock like `lock`, but waits on a blocking thread so the async runtime keeps running.
    pub async fn lock_async(&self) -> Result<File, PhvError> {
        let vault = self.clone();
        tokio::task::spawn_blocking(move || vault.lock())
            .await
            .map_err(|e| PhvError::Io(std::io::Error::other(e)))?
    }

    /// Checks that `apply` would accept these changes, without writing anything.
    pub fn check_changes(&self, changes: &[Change]) -> Result<(), PhvError> {
        self.plan(changes).map(|_| ())
    }

    fn plan(&self, changes: &[Change]) -> Result<Vec<(PathBuf, Vec<String>)>, PhvError> {
        // The caller must hand over consistent changes, so a mismatch is a programming error.
        for change in changes {
            validate_change(change)?;
        }
        let mut by_type: BTreeMap<&str, Vec<&Change>> = BTreeMap::new();
        for change in changes {
            let (Change::Upsert { resource_type, .. } | Change::Remove { resource_type, .. }) =
                change;
            by_type.entry(resource_type).or_default().push(change);
        }
        let mut pending = Vec::new();
        // Check every file before writing any of them.
        for (resource_type, changes) in by_type {
            self.check_line_endings(resource_type)?;
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
        Ok(pending)
    }

    /// Applies changes file by file. Files whose content would not change are not written.
    pub fn apply(&self, changes: &[Change]) -> Result<(), PhvError> {
        let pending = self.plan(changes)?;
        for (path, lines) in pending {
            if lines.is_empty() {
                fs::remove_file(&path)?;
                if let Some(dir) = path.parent() {
                    sync_dir(dir)?;
                }
            } else {
                let mut text = lines.join("\n");
                text.push('\n');
                self.write_atomic(&path, text.as_bytes())?;
            }
        }
        Ok(())
    }
}

fn validate_change(change: &Change) -> Result<(), PhvError> {
    let bad = |why: String| Err(PhvError::Usage(format!("invalid change: {why}")));
    match change {
        Change::Remove { resource_type, .. } => check_type_name(resource_type),
        Change::Upsert {
            resource_type,
            id,
            line,
        } => {
            check_type_name(resource_type)?;
            if line.contains(['\n', '\r']) {
                return bad(format!(
                    "the line for {resource_type}/{id} has a line break"
                ));
            }
            if !line.starts_with(&format!("{{\"resourceType\":\"{resource_type}\",")) {
                return bad(format!(
                    "the line for {resource_type}/{id} does not start with its resourceType"
                ));
            }
            if line_id(line) != Some(id.as_str()) {
                return bad(format!(
                    "the line for {resource_type}/{id} has a different or unreadable id"
                ));
            }
            Ok(())
        }
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
