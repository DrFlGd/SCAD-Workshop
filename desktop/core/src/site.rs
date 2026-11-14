//! The app's own files (`site/` beside the window's files, made by
//! tools/build_desktop.py): `data/catalog.json` names the OpenSCAD version and the
//! files every render gets (a fontconfig file and the fonts), stored by content
//! hash in `fs/<sha256>`.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub struct SiteDir {
    root: PathBuf,
}

impl SiteDir {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if !root.join("data/catalog.json").is_file() {
            bail!("{} doesn't look like a built site (no data/catalog.json)", root.display());
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a site-relative path ("data/catalog.json"), refusing anything that
    /// would leave the site folder.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let rel = rel.trim_start_matches('/');
        let p = Path::new(rel);
        if rel.is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
            bail!("invalid site path: {rel}");
        }
        Ok(self.root.join(p))
    }

    pub fn read(&self, rel: &str) -> Result<Vec<u8>> {
        let path = self.resolve(rel)?;
        std::fs::read(&path).with_context(|| format!("couldn't read {rel}"))
    }

    pub fn json(&self, rel: &str) -> Result<Value> {
        serde_json::from_slice(&self.read(rel)?).with_context(|| format!("{rel} is not valid JSON"))
    }

    /// Path of a content-addressed source file.
    pub fn blob(&self, sha: &str) -> Result<PathBuf> {
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("invalid file hash {sha}");
        }
        Ok(self.root.join("fs").join(sha))
    }

    pub fn catalog(&self) -> Result<Value> {
        self.json("data/catalog.json")
    }
}
