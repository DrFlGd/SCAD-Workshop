//! What the app keeps on this computer (the OS's per-user config and data
//! folders), as opposed to the portable library folder: which library is open,
//! recently opened libraries, preferences (view settings, the printer profile,
//! open tabs) and the GitHub token.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The user's home folder.
pub fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `~/SCAD Workshop`; a folder from before the rename (`~/Claude Grid Workshop`) keeps being used.
pub fn default_library_path() -> PathBuf {
    let new = home_dir().join("SCAD Workshop");
    let old = home_dir().join("Claude Grid Workshop");
    if !new.exists() && old.join("settings").is_dir() {
        return old;
    }
    new
}

/// App config, in the OS's per-user config folder.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// The open library folder. (Phase 1 called it the workspace.)
    #[serde(default, alias = "workspace")]
    pub library: Option<PathBuf>,
    /// Libraries opened before, newest first.
    #[serde(default)]
    pub recent_libraries: Vec<PathBuf>,
    /// Renders at once; None = all cores but one.
    #[serde(default)]
    pub concurrency: Option<usize>,
    /// Folder of the last "Save" (the dialog opens there next time).
    #[serde(default)]
    pub last_save_dir: Option<PathBuf>,
    /// For GitHub's API (raises the hourly limit; needed for private repositories).
    #[serde(default)]
    pub github_token: Option<String>,
}

impl AppConfig {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomic(path, &serde_json::to_vec_pretty(self)?)
    }

    pub fn library_path(&self) -> PathBuf {
        self.library.clone().unwrap_or_else(default_library_path)
    }

    /// Remember `dir` as the open library (and at the top of the recent list).
    pub fn set_library(&mut self, dir: &Path) {
        self.library = Some(dir.to_path_buf());
        self.recent_libraries.retain(|p| p != dir);
        self.recent_libraries.insert(0, dir.to_path_buf());
        self.recent_libraries.truncate(8);
    }

    /// The token from the config, or $WORKSHOP_GITHUB_TOKEN / $GITHUB_TOKEN.
    pub fn github_token(&self) -> Option<String> {
        self.github_token
            .clone()
            .filter(|t| !t.trim().is_empty())
            .or_else(|| std::env::var("WORKSHOP_GITHUB_TOKEN").ok())
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
            .filter(|t| !t.trim().is_empty())
    }
}

/// Preferences the page stores (a JSON object), kept with the app.
pub struct Prefs {
    path: PathBuf,
}

impl Prefs {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn exists(&self) -> bool {
        self.path.is_file()
    }
    pub fn get(&self) -> Value {
        read_json_object(&self.path)
    }
    pub fn set(&self, prefs: &Value) -> Result<()> {
        if !prefs.is_object() {
            bail!("preferences must be an object");
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomic(&self.path, &serde_json::to_vec_pretty(prefs)?)
    }
}

/// A JSON object from a file, or {} if it's missing or unreadable.
pub fn read_json_object(path: &Path) -> Value {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Default::default()))
}

/// Write via a temporary file and rename, so a crash never leaves half a file.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("couldn't create {}", parent.display()))?;
    }
    let tmp = path.with_extension(format!("tmp-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
    std::fs::write(&tmp, data).with_context(|| format!("couldn't write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("couldn't write {}", path.display()))?;
    Ok(())
}

/// Pretty JSON with sorted keys and a final newline, so files in a library kept
/// in git or a sync tool change only where something changed. A library file (format.rs)
/// gets its format number, and one written by a newer version isn't written over.
pub fn write_json(path: &Path, v: &Value) -> Result<()> {
    let mut v = std::borrow::Cow::Borrowed(v);
    if let Some(kind) = crate::format::kind_of(path) {
        if std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).is_some_and(|old| crate::format::too_new(kind, &old)) {
            anyhow::bail!("{} was made by a newer version of SCAD Workshop, so this version doesn't change it. Update the app to change it.", path.display());
        }
        crate::format::upgrade(kind, v.to_mut(), Some(path));
    }
    let mut bytes = serde_json::to_vec_pretty(&sorted(&v))?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

/// A copy with object keys in sorted order (at every level).
pub fn sorted(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            Value::Object(keys.into_iter().map(|k| (k.clone(), sorted(&m[k]))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}
