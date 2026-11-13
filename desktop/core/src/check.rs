//! Check library (0.4.22, docs/LIBRARY_FORMAT.md): reads the whole library folder against
//! the format and lists what's off, as what can be repaired safely and what's only listed
//! for you to look at. Repair never changes a project's files (`files/<version>/`) or
//! anything you made; what it removes can always be made again.

use crate::format::{self, Kind};
use crate::library::{Library, README};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

/// One thing found. `fix` names the repair; None means it's only listed.
struct Finding {
    what: String,
    path: String,
    fix: Option<&'static str>,
}

/// The ids items have in the library (`gen:<project>/<model>`, `comp:@<lib>/<module>`,
/// `part:<project>/<part>`), from the catalog, to find references to nothing, and
/// `proj:<project>` for a project not read yet (its items aren't known, so nothing that
/// refers to it is listed). None skips those checks.
pub type Known = Option<HashSet<String>>;

fn refers(known: &HashSet<String>, r: &str) -> bool {
    let project = r.split_once(':').and_then(|(_, rest)| rest.split_once('/')).map(|(p, _)| p).unwrap_or("");
    known.contains(r) || known.contains(&format!("proj:{project}"))
}

fn rel(lib: &Library, p: &Path) -> String {
    lib.relative(p).unwrap_or_else(|| p.display().to_string())
}

fn dirs(p: &Path) -> Vec<std::fs::DirEntry> {
    std::fs::read_dir(p).map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).collect()).unwrap_or_default()
}

fn name(e: &std::fs::DirEntry) -> String {
    e.file_name().to_string_lossy().into_owned()
}

fn findings(lib: &Library, known: &Known) -> (usize, Vec<Finding>) {
    let mut out: Vec<Finding> = vec![];
    let mut add = |what: String, path: String, fix: Option<&'static str>| out.push(Finding { what, path, fix });
    let root = lib.root();
    for d in ["sources", "local", "collections", "recipes"] {
        if !root.join(d).is_dir() {
            add(format!("The {d} folder is missing."), d.into(), Some("folders"));
        }
    }
    if !root.join("README.txt").is_file() {
        add("The library's README is missing.".into(), "README.txt".into(), Some("readme"));
    }
    // every JSON file the app writes: readable, not newer than this version, in this version's shape
    let files = lib.json_files();
    for p in &files {
        let Some(kind) = format::kind_of(p) else { continue };
        let r = rel(lib, p);
        let v = match std::fs::read(p).ok().map(|b| serde_json::from_slice::<Value>(&b)) {
            Some(Ok(v)) if v.is_object() => v,
            _ => {
                add(format!("This {} file isn't valid JSON, so the app can't read it.", kind.name()), r, None);
                continue;
            }
        };
        if format::too_new(kind, &v) {
            add(format!("This {} was made by a newer version of SCAD Workshop (format {}), so this version only reads it.", kind.name(), format::format_of(&v)), r, None);
        } else if kind != Kind::Library && format::upgrade(kind, &mut v.clone(), Some(p)) {
            add(format!("This {} isn't in this version's format yet.", kind.name()), r, Some("upgrade"));
        }
    }
    let this_pid = std::process::id().to_string();
    for s in dirs(&lib.sources_dir()) {
        let id = name(&s);
        let sp = s.path();
        if let Some(pid) = id.strip_prefix(".adding-") {
            // left by an add that stopped part way (not one running in this app now)
            if pid != this_pid {
                add("A project that was being added when the app stopped: its unfinished copy.".into(), rel(lib, &sp), Some("temporary"));
            }
            continue;
        }
        if id.starts_with('.') {
            continue;
        }
        let Ok(src) = lib.source(&id) else {
            add("A folder in sources without its source.json, so it isn't a project the app can read.".into(), rel(lib, &sp), None);
            continue;
        };
        // content you added and pictures you added (milestone 6): their files are there
        for d in crate::content::docs(lib, &id, "").iter().filter(|d| d["missing"] == true) {
            add(format!("\"{}\", content you added, has lost its file.", d["title"].as_str().unwrap_or("?")), rel(lib, &sp.join(d["file"].as_str().unwrap_or(""))), None);
        }
        let meta = lib.metadata(&id);
        let levels = std::iter::once(&meta["project"]).chain(["items", "parts", "folders"].iter().flat_map(|k| meta[*k].as_object().into_iter().flat_map(|m| m.values())));
        for pic in levels.filter_map(|l| l["icon"].as_str()?.strip_prefix("picture:")) {
            if !sp.join("pictures").join(pic).is_file() {
                add(format!("A picture you chose ({pic}) has lost its file; the render is shown instead."), rel(lib, &sp.join("pictures")), None);
            }
        }
        let kind = src["kind"].as_str().unwrap_or("");
        let current = src["version"].as_str().unwrap_or("");
        if kind == "linked" {
            if !lib.version_dir(&src, current).is_ok_and(|d| d.is_dir()) {
                add(format!("The folder this linked project reads ({}) isn't there.", src["origin"]["path"].as_str().unwrap_or("?")), rel(lib, &sp), None);
            }
            continue;
        }
        if !matches!(kind, "github" | "zip" | "folder" | "bundled") {
            continue;
        }
        let listed: HashSet<String> = src["versions"].as_array().into_iter().flatten().filter_map(|v| v["id"].as_str().map(String::from)).collect();
        if !current.is_empty() && !sp.join("files").join(current).is_dir() {
            add(format!("The files of the version in use ({current}) are missing."), rel(lib, &sp.join("files")), None);
        }
        let mut present: HashSet<String> = HashSet::new();
        for f in dirs(&sp.join("files")) {
            let n = name(&f);
            if n.starts_with('.') && n.contains(".tmp-") {
                add("A version that was being downloaded when the app stopped: its unfinished copy.".into(), rel(lib, &f.path()), Some("temporary"));
                continue;
            }
            if !listed.contains(&n) && n != current {
                add("A version's files that the project doesn't list.".into(), rel(lib, &f.path()), None);
            }
            present.insert(n);
        }
        // what the app made from a version whose files are gone (as Clean up removes them)
        let gone = |v: &str| !present.contains(v) && v != current;
        for e in std::fs::read_dir(sp.join("derived")).into_iter().flatten().flatten() {
            let n = name(&e);
            if n.strip_suffix(".json").is_some_and(gone) {
                add("What the app read from a version whose files are gone.".into(), rel(lib, &e.path()), Some("rebuildable"));
            }
        }
        for sub in ["docs", "generated"] {
            for e in dirs(&sp.join(sub)) {
                if gone(&name(&e)) {
                    add(format!("The {} made for a version whose files are gone.", if sub == "docs" { "documents" } else { "copies" }), rel(lib, &e.path()), Some("rebuildable"));
                }
            }
        }
    }
    // made parts: each recipe beside its part
    for p in files.iter().filter(|p| format::kind_of(p) == Some(Kind::PartRecipe)) {
        let stem = p.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".recipe.json")).unwrap_or("");
        let dir = p.parent().unwrap_or(root);
        if !["stl", "3mf", "obj", "step", "stp"].iter().any(|x| dir.join(format!("{stem}.{x}")).is_file()) {
            add(format!("How \"{stem}\" was made, but its part isn't beside it."), rel(lib, p), None);
        }
    }
    if let Some(known) = known {
        let has_model = |m: &str| refers(known, &format!("gen:{m}")) || known.contains(&format!("comp:{m}"));
        for p in files.iter().filter(|p| format::kind_of(p) == Some(Kind::Recipe)) {
            let Some(r) = std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
            let m = r["model"].as_str().unwrap_or("");
            if !has_model(m) {
                add(format!("Saved settings \"{}\" for a model that isn't in the library ({m}).", r["name"].as_str().unwrap_or("?")), rel(lib, p), None);
            }
        }
        for p in files.iter().filter(|p| format::kind_of(p) == Some(Kind::Collection)) {
            let Some(c) = std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
            for e in c["items"].as_array().into_iter().flatten() {
                let r = e["ref"].as_str().unwrap_or("");
                if !refers(known, r) {
                    add(format!("\"{}\" in the collection {} refers to an item that isn't in the library ({r}).", e["name"].as_str().unwrap_or("?"), c["name"].as_str().unwrap_or("?")), rel(lib, p), None);
                }
            }
        }
        for f in lib.favourites().as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !refers(known, f) && (f.starts_with("gen:") || f.starts_with("comp:") || f.starts_with("part:")) {
                add(format!("A favourite refers to an item that isn't in the library ({f})."), "library.json".into(), None);
            }
        }
    }
    (files.len(), out)
}

/// What Check library found: `repair` (with what Repair does) and `look` (listed only),
/// with the backups the app made of files it changed.
pub fn check(lib: &Library, known: &Known) -> Value {
    let (checked, found) = findings(lib, known);
    let show = |f: &Finding| json!({ "what": f.what, "path": f.path, "fix": f.fix });
    let mut backups: Vec<String> = std::fs::read_dir(lib.root().join("backups"))
        .into_iter().flatten().flatten().map(|e| format!("backups/{}", name(&e))).filter(|n| n.ends_with(".zip")).collect();
    backups.sort();
    json!({
        "checked": checked,
        "repair": found.iter().filter(|f| f.fix.is_some()).map(show).collect::<Vec<_>>(),
        "look": found.iter().filter(|f| f.fix.is_none()).map(show).collect::<Vec<_>>(),
        "backups": backups,
        "read_only": lib.read_only(),
    })
}

/// Repair what can be repaired safely, then check again.
pub fn repair(lib: &Library, known: &Known) -> Result<Value> {
    lib.writable()?;
    let (_, found) = findings(lib, known);
    let mut repaired = 0;
    let mut upgrade = false;
    for f in &found {
        let p = lib.root().join(&f.path);
        let done = match f.fix {
            Some("folders") => std::fs::create_dir_all(&p).is_ok(),
            Some("readme") => std::fs::write(&p, README).is_ok(),
            Some("upgrade") => {
                upgrade = true;
                true
            }
            Some("temporary" | "rebuildable") if p.starts_with(lib.sources_dir()) => {
                if p.is_dir() { std::fs::remove_dir_all(&p).is_ok() } else { std::fs::remove_file(&p).is_ok() }
            }
            _ => false,
        };
        repaired += done as usize;
    }
    if upgrade {
        lib.upgrade_files("repair")?;
    }
    let mut after = check(lib, known);
    after["repaired"] = json!(repaired);
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_repairs() {
        let dir = std::env::temp_dir().join(format!("gw-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (lib, _) = Library::open(dir.join("lib")).unwrap();
        let src = lib.sources_dir();
        std::fs::create_dir_all(src.join("zip-a/files/v1")).unwrap();
        std::fs::create_dir_all(src.join("zip-a/docs/v0")).unwrap();
        std::fs::create_dir_all(src.join("zip-a/derived")).unwrap();
        std::fs::write(src.join("zip-a/derived/v0.json"), "{}").unwrap();
        std::fs::write(src.join("zip-a/derived/v1.json"), "{}").unwrap();
        crate::config::write_json(&src.join("zip-a/source.json"), &json!({ "id": "zip-a", "kind": "zip", "version": "v1", "versions": [{ "id": "v0" }, { "id": "v1" }] })).unwrap();
        std::fs::create_dir_all(src.join(".adding-1")).unwrap();
        std::fs::create_dir_all(src.join("odd")).unwrap();
        std::fs::write(lib.root().join("recipes/r1.json"), r#"{"id":"r1","model":"zip-a/box","name":"R","values":{},"version":"v1"}"#).unwrap();
        std::fs::write(lib.root().join("recipes/bad.json"), "{nope").unwrap();
        std::fs::write(lib.root().join("recipes/new.json"), r#"{"id":"new","model":"zip-a/box","format":9}"#).unwrap();
        crate::config::write_json(&lib.root().join("collections/c/collection.json"), &json!({ "id": "c", "name": "C", "items": [{ "ref": "gen:zip-a/gone", "name": "Gone" }] })).unwrap();
        let known: Known = Some(["gen:zip-a/box".to_string()].into_iter().collect());
        let c = check(&lib, &known);
        let paths = |k: &str| c[k].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let mut fixable = paths("repair");
        fixable.sort();
        assert_eq!(fixable, ["recipes/r1.json", "sources/.adding-1", "sources/zip-a/derived/v0.json", "sources/zip-a/docs/v0"]);
        let look = paths("look");
        assert!(look.contains(&"recipes/bad.json".to_string()) && look.contains(&"recipes/new.json".to_string()));
        assert!(look.contains(&"sources/odd".to_string()) && look.contains(&"collections/c/collection.json".to_string()));
        assert!(!look.iter().any(|p| p.contains("zip-a/files")));
        let after = repair(&lib, &known).unwrap();
        assert_eq!(after["repaired"], 4);
        assert!(after["repair"].as_array().unwrap().is_empty());
        assert!(src.join("zip-a/derived/v1.json").is_file() && src.join("zip-a/files/v1").is_dir() && !src.join(".adding-1").exists());
        let r1: Value = serde_json::from_slice(&std::fs::read(lib.root().join("recipes/r1.json")).unwrap()).unwrap();
        assert_eq!((r1["project_version"].as_str(), r1["format"].as_u64()), (Some("v1"), Some(1)));
        assert!(after["backups"].as_array().unwrap().iter().any(|b| b.as_str().unwrap().starts_with("backups/repair-")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
