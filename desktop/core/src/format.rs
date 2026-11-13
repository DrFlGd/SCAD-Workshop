//! The library's JSON files and their format numbers (docs/LIBRARY_FORMAT.md).
//!
//! One table names each kind of JSON file the app writes in a library by its path, with
//! the format this version writes. [`crate::config::write_json`] stamps that number on
//! any file it names (so no writer can forget it) and refuses to write over a file a
//! newer version wrote; the library's upgrade and Check library read the same table.
//! Derived data, `workshop.json` and `description.json` keep the numbers they always had
//! (written by their own code), so they aren't stamped here.

use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `library.json`
    Library,
    /// `sources/<id>/source.json`
    Source,
    /// `sources/<id>/metadata.json`
    Metadata,
    /// `sources/<id>/checks.json`
    Checks,
    /// `sources/pinned/pins/<id>.json`
    Pin,
    /// `collections/<id>/collection.json`
    Collection,
    /// `recipes/<id>.json` (saved settings)
    Recipe,
    /// `<name>.recipe.json` beside a made part
    PartRecipe,
    /// `trash/<entry>/trash.json`
    Trash,
}

impl Kind {
    /// The format this version writes.
    pub fn current(self) -> u64 {
        match self {
            Kind::Library => crate::library::FORMAT,
            _ => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Library => "library",
            Kind::Source => "project",
            Kind::Metadata => "project details",
            Kind::Checks => "model checks",
            Kind::Pin => "pinned component",
            Kind::Collection => "collection",
            Kind::Recipe => "saved settings",
            Kind::PartRecipe => "made part's recipe",
            Kind::Trash => "trash entry",
        }
    }
}

/// The kind of library file at `path`, by its name and the folders it's in.
pub fn kind_of(path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_str()?;
    let up = |n: usize| path.ancestors().nth(n).and_then(Path::file_name).and_then(|s| s.to_str()).unwrap_or("");
    if name.ends_with(".recipe.json") {
        return Some(Kind::PartRecipe);
    }
    match name {
        "library.json" => Some(Kind::Library),
        "source.json" if up(2) == "sources" => Some(Kind::Source),
        "metadata.json" if up(2) == "sources" => Some(Kind::Metadata),
        "checks.json" if up(2) == "sources" => Some(Kind::Checks),
        "collection.json" if up(2) == "collections" => Some(Kind::Collection),
        "trash.json" if up(2) == "trash" => Some(Kind::Trash),
        _ if name.ends_with(".json") && up(1) == "recipes" => Some(Kind::Recipe),
        _ if name.ends_with(".json") && up(1) == "pins" && up(2) == "pinned" => Some(Kind::Pin),
        _ => None,
    }
}

/// The file's format number (a missing one counts as 1; a made part's old `format: "stl"` too).
pub fn format_of(v: &Value) -> u64 {
    v["format"].as_u64().unwrap_or(1)
}

/// Written by a newer version than this one: read, but never written over.
pub fn too_new(kind: Kind, v: &Value) -> bool {
    format_of(v) > kind.current()
}

/// Bring a file up to this version's shape, in place. Returns whether anything changed.
/// `path` gives a made part's name (its file name). Unknown keys are kept.
pub fn upgrade(kind: Kind, v: &mut Value, path: Option<&Path>) -> bool {
    let Some(o) = v.as_object_mut() else { return false };
    if o.get("format").and_then(Value::as_u64).is_some_and(|f| f > kind.current()) {
        return false;
    }
    let before = Value::Object(o.clone());
    match kind {
        Kind::Recipe => recipe(v, false, None),
        Kind::PartRecipe => {
            let stem = path.and_then(|p| p.file_name()).and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".recipe.json"));
            recipe(v, true, stem);
        }
        _ => {}
    }
    // the library's own number is raised by its upgrade (library.rs), not here
    if kind != Kind::Library && v["format"].as_u64().is_none() {
        v["format"] = json!(kind.current());
    }
    *v != before
}

/// One recipe shape for saved settings and made parts (format 1, 0.4.22): saved settings'
/// `version` is `project_version`; a made part's `format` ("stl", "3mf") is `output`, its
/// `date` is `created`, and its name is its file's.
fn recipe(v: &mut Value, part: bool, stem: Option<&str>) {
    let o = v.as_object_mut().unwrap();
    if let Some(out) = o.get("format").and_then(Value::as_str).map(String::from) {
        o.shift_remove("format");
        o.entry("output").or_insert(json!(out));
    }
    if let Some(ver) = o.shift_remove("version") {
        o.entry("project_version").or_insert(ver);
    }
    if let Some(date) = o.shift_remove("date") {
        o.entry("created").or_insert(date);
    }
    o.entry("kind").or_insert(json!(if part { "part" } else { "settings" }));
    if let Some(stem) = stem {
        o.entry("name").or_insert(json!(stem));
    }
    o.entry("format").or_insert(json!(1));
}

/// A recipe as this version reads it, whichever shape it was written in.
pub fn recipe_view(mut v: Value, part: bool, stem: Option<&str>) -> Value {
    if v.is_object() && !too_new(Kind::Recipe, &v) {
        recipe(&mut v, part, stem);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_path() {
        let k = |p: &str| kind_of(Path::new(p));
        assert_eq!(k("/l/library.json"), Some(Kind::Library));
        assert_eq!(k("/l/sources/zip-a/source.json"), Some(Kind::Source));
        assert_eq!(k("/l/sources/zip-a/metadata.json"), Some(Kind::Metadata));
        assert_eq!(k("/l/sources/zip-a/checks.json"), Some(Kind::Checks));
        assert_eq!(k("/l/sources/pinned/pins/p1.json"), Some(Kind::Pin));
        assert_eq!(k("/l/collections/desk/collection.json"), Some(Kind::Collection));
        assert_eq!(k("/l/recipes/abc.json"), Some(Kind::Recipe));
        assert_eq!(k("/l/local/Made parts/Bin/Bin 2.recipe.json"), Some(Kind::PartRecipe));
        assert_eq!(k("/l/trash/2026-x/trash.json"), Some(Kind::Trash));
        // a project's own files and what's in the trash aren't library files
        assert_eq!(k("/l/trash/2026-x/source/source.json"), None);
        assert_eq!(k("/l/sources/zip-a/files/v1/metadata.json"), None);
        assert_eq!(k("/l/sources/zip-a/derived/v1.json"), None);
        assert_eq!(k("/app/prefs.json"), None);
    }

    #[test]
    fn recipes_take_one_shape() {
        let mut s = json!({ "id": "a1", "model": "p/m", "name": "A", "values": {}, "version": "v1", "created": "x" });
        assert!(upgrade(Kind::Recipe, &mut s, None));
        assert_eq!(s, json!({ "id": "a1", "model": "p/m", "name": "A", "values": {}, "project_version": "v1", "created": "x", "kind": "settings", "format": 1 }));
        assert!(!upgrade(Kind::Recipe, &mut s, None));
        let mut p = json!({ "model": "p/m", "values": {}, "project_version": "v1", "date": "d", "format": "3mf" });
        assert!(upgrade(Kind::PartRecipe, &mut p, Some(Path::new("/l/local/Made parts/M/Bin 2.recipe.json"))));
        assert_eq!(p, json!({ "model": "p/m", "values": {}, "project_version": "v1", "created": "d", "output": "3mf", "kind": "part", "name": "Bin 2", "format": 1 }));
        // newer than this version: left as it is
        let mut n = json!({ "model": "p/m", "format": 7, "date": "d" });
        assert!(!upgrade(Kind::PartRecipe, &mut n, None) && too_new(Kind::PartRecipe, &n));
        let mut c = json!({ "id": "c", "name": "C", "items": [] });
        assert!(upgrade(Kind::Collection, &mut c, None) && c["format"] == 1);
    }
}
