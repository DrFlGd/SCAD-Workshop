//! Turning a project in the library into model pages ("reading" it): its models'
//! files, settings (OpenSCAD's own export, run natively) and metadata, saved as
//! sources/<id>/derived/<version>.json. Also packaging the website build's
//! catalog as starter-library projects that ship with the app.

use crate::ingest;
use crate::library::{self, slug, Library};
use crate::render::{RenderRequest, Renderer};
use crate::scan::{self, LibraryDir};
use crate::sitebuild;
use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Version of the derived-data format (bump to re-read projects; see `outdated`). 2 (0.4): settings
/// lifted from module calls, reference documents. 3 (0.4.1): lifted settings borrow the library's
/// descriptions and choice lists, and fractions are lifted. 4 (0.4.2): files that draw through an
/// included engine are models and the engine isn't (see `models_differ`); category keywords match
/// whole words. 5 (0.4.8): a model whose settings were lifted into a copy records the hash of the
/// project's own file (`file_bases`), which code edits are based on.
pub const DERIVED_FORMAT: u64 = 5;

/// Whether a project's derived data was made by an app that reads it differently. Formats 3 and 5
/// changed only models whose settings were lifted from module calls, so a project read before
/// needs reading again only when it has such models. Format 4: see `models_differ`.
pub fn outdated(derived: &Value) -> bool {
    let f = derived["format"].as_u64().unwrap_or(0);
    let lifted = || derived["family"]["models"].as_array().into_iter().flatten().any(|m| m["settings_from"] == "calls");
    f < 2 || (f < 5 && lifted())
}

/// Format 4 changed which files are models only for projects with front ends over an included
/// engine, so a project read before it needs reading again only when looking at its files now
/// finds other models than it has (cheap: no OpenSCAD). Checked by `source_rescan` at start-up.
pub fn models_differ(src: &Value, derived: &Value, root: &Path) -> bool {
    let mut had: Vec<String> =
        derived["family"]["models"].as_array().into_iter().flatten().filter_map(|m| m["entrypoint"].as_str().map(String::from)).collect();
    // a project added as a library has no models, whatever its files hold
    let mut now = if src["role"] == "library" { vec![] } else { scan::survey(root).entries };
    had.sort();
    now.sort();
    had != now
}

/// What milestone 3 added to reading a project, for a project read before it (starter projects
/// included, which aren't read again): the description beside its files (`workshop.json`, 0.4.7),
/// and its defaults checked against the form's limits (D-3), from what was read (Review 3, F-13).
fn fill_in(lib: &Library, id: &str, src: &Value) {
    let version = src["version"].as_str().unwrap_or("");
    // a starter project whose copy is from GitHub records where and which commit (F-17)
    if src["kind"] == "bundled" && src["origin"]["commit"].is_null() {
        if let Some((repository, commit, why)) = crate::sources::starter_upstream(id) {
            let mut s = src.clone();
            s["origin"]["repository"] = json!(repository);
            s["origin"]["commit"] = json!(commit);
            if !why.is_empty() {
                s["origin"]["not_updatable"] = json!(why);
            }
            let _ = lib.save_source(&s);
        }
    }
    let Some(mut derived) = lib.derived(id, version) else { return };
    if let Ok(dir) = lib.source_dir(id) {
        if !dir.join("workshop.json").is_file() && derived["family"]["models"].is_array() {
            let _ = crate::config::write_json(&dir.join("workshop.json"), &describe(&derived["family"], id, version));
        }
    }
    if derived["limits_checked"] == true {
        return;
    }
    let mut problems: Vec<Value> = derived["problems"].as_array().into_iter().flatten().filter(|p| p["kind"] != "limits").cloned().collect();
    // read before 0.4.6: its plain numbers get the step reading gives them now, so a default like 1.5
    // isn't reported as off its step of 1
    for m in derived["models"].as_array_mut().into_iter().flatten() {
        if let Some(ps) = m["parameters"].as_array_mut() {
            ingest::fit_plain_steps(ps);
        }
    }
    for m in derived["models"].as_array().into_iter().flatten() {
        let off = ingest::limit_problems(m["parameters"].as_array().map(Vec::as_slice).unwrap_or(&[]));
        if !off.is_empty() {
            let mid = m["id"].as_str().unwrap_or("");
            let key = m["key"].as_str().map(String::from).unwrap_or_else(|| format!("{id}/{mid}"));
            let entry = m["entry"].as_str().unwrap_or(mid).trim_start_matches('/').to_string();
            problems.push(json!({ "kind": "limits", "model": key, "settings": off, "message": format!("{entry}: {}", off.join("; ")) }));
        }
    }
    derived["problems"] = json!(problems);
    derived["limits_checked"] = json!(true);
    let _ = lib.save_derived(id, version, &derived);
}

/// The local project parts made from models are kept in (milestone 4): local/Made parts/<model>/.
pub const MADE_PARTS: &str = "Made parts";

/// What [`scan_changes`] found.
#[derive(Debug, Default)]
pub struct Changes {
    /// Projects to read again: their files changed since they were read, or they were read before
    /// format 4 and looking at their files now finds other models.
    pub read: Vec<String>,
    /// Projects whose files are gone (a stored copy deleted, a local/ folder removed or renamed,
    /// a linked folder moved).
    pub missing: Vec<String>,
}

/// Look at every project's files for changes since it was read, cheaply: names, sizes and
/// modification times first, contents only when those differ (a file touched but not changed
/// isn't a change). `local/` projects and linked folders get new versions in
/// [`sources::sync_local`], so here they're only checked for being gone. Ids in `skip` are being
/// read anyway. Records the fingerprints of projects read before they were kept.
pub fn scan_changes(lib: &Library, skip: &[String]) -> Changes {
    let mut out = Changes::default();
    for id in lib.source_ids() {
        let Ok(src) = lib.source(&id) else { continue };
        let kind = src["kind"].as_str().unwrap_or("");
        if kind != "pinned" && !skip.contains(&id) && lib.read_only().is_none() {
            fill_in(lib, &id, &src);
        }
        if kind == "bundled" || kind == "pinned" || skip.contains(&id) {
            continue;
        }
        let version = src["version"].as_str().unwrap_or("").to_string();
        let Some(dir) = lib.version_dir(&src, &version).ok() else { continue };
        if !dir.is_dir() {
            out.missing.push(id);
            continue;
        }
        if lib.read_only().is_some() {
            continue;
        }
        let Some(mut derived) = lib.derived(&id, &version) else { continue };
        // read by an app that reads it differently (see `outdated`): read again
        if outdated(&derived) {
            out.read.push(id.clone());
            continue;
        }
        if kind == "local" || kind == "linked" {
            // their changes make a new version (sync_local); only a read before format 4 is checked here
            if derived["format"].as_u64().unwrap_or(0) < 4 {
                if models_differ(&src, &derived, &dir) {
                    out.read.push(id.clone());
                } else {
                    derived["format"] = json!(DERIVED_FORMAT);
                    let _ = lib.save_derived(&id, &version, &derived);
                }
            }
            continue;
        }
        let stamp = |derived: &mut Value, quick: String, full: Option<String>| {
            derived["format"] = json!(DERIVED_FORMAT);
            derived["files_stat"] = json!(quick);
            if let Some(f) = full {
                derived["files_fp"] = json!(f);
            }
            let _ = lib.save_derived(&id, &version, derived);
        };
        let old_format = derived["format"].as_u64().unwrap_or(0) < 4;
        if old_format || derived["files_fp"].as_str().is_none() {
            // read before fingerprints were kept: read again only if its models would change
            if models_differ(&src, &derived, &dir) {
                out.read.push(id.clone());
            } else {
                stamp(&mut derived, scan::quick_fingerprint(&dir), Some(scan::fingerprint(&dir)));
            }
            continue;
        }
        let quick = scan::quick_fingerprint(&dir);
        if derived["files_stat"].as_str() == Some(quick.as_str()) {
            continue;
        }
        if derived["files_fp"].as_str() == Some(scan::fingerprint(&dir).as_str()) {
            stamp(&mut derived, quick, None); // touched, not changed
        } else {
            out.read.push(id.clone());
        }
    }
    out
}

/// A blob owner's suffix for files the app made for a version (`<source>@<version>~gen:<path>`, in
/// `sources/<source>/generated/<version>/`).
pub const GENERATED: &str = "~gen";

/// What reading a project needs to know about libraries besides the library folder:
/// the copies bundled with the app, which of them win over the user's own
/// (`prefer_bundled` in library.json), and start values for modules without examples.
#[derive(Clone, Default)]
pub struct IngestLibs {
    pub bundled: Vec<crate::components::ComponentLibrary>,
    pub prefer_bundled: Vec<String>,
    pub curated: Value,
}

impl IngestLibs {
    /// Library folders for includes, in the order they're tried: the user's library
    /// projects first, the bundled copies after them (before them for preferred ones).
    pub fn search_order(&self, lib: &Library, except: &str) -> Vec<LibrarySource> {
        let prefer = |n: &str| self.prefer_bundled.iter().any(|p| p.eq_ignore_ascii_case(n));
        let bundled = |b: &crate::components::ComponentLibrary| LibrarySource {
            dir: LibraryDir { name: b.info.name.clone(), path: b.root.clone() },
            source: format!("@bundled/{}", b.info.name),
            version: b.info.commit.as_deref().unwrap_or("").chars().take(12).collect(),
        };
        let mut out: Vec<LibrarySource> = self.bundled.iter().filter(|b| prefer(&b.info.name)).map(bundled).collect();
        out.extend(library_sources(lib).into_iter().filter(|l| l.source != except));
        out.extend(self.bundled.iter().filter(|b| !prefer(&b.info.name)).map(bundled));
        out
    }
}

/// What a library project is, for its components.
pub fn project_lib_info(src: &Value, version: &str, name: &str) -> crate::components::LibInfo {
    let det = &src["detected"];
    let repo = match (src["origin"]["owner"].as_str(), src["origin"]["repo"].as_str()) {
        (Some(o), Some(r)) => Some(format!("{o}/{r}")),
        _ => None,
    };
    let v = src["versions"].as_array().into_iter().flatten().find(|v| v["id"] == version);
    crate::components::LibInfo {
        name: name.to_string(),
        title: det["name"].as_str().unwrap_or(name).to_string(),
        summary: det["summary"].as_str().unwrap_or("").to_string(),
        commit: v.and_then(|v| v["commit"].as_str()).map(String::from),
        date: src["version_date"].as_str().map(|d| d.chars().take(10).collect()),
        license: det["license"]["spdx"].as_str().map(String::from),
        authors: det["authors"].as_array().into_iter().flatten().filter_map(|a| a["name"].as_str().or(a.as_str()).map(String::from)).collect(),
        docs: if name.eq_ignore_ascii_case("BOSL2") { Some("https://github.com/BelfrySCAD/BOSL2/wiki".into()) } else { None },
        repo,
    }
}

/// A library project that others can include from.
#[derive(Clone, Debug)]
pub struct LibrarySource {
    pub dir: LibraryDir,
    pub source: String,
    pub version: String,
}

/// The library projects (role "library") in a library, at their current versions.
pub fn library_sources(lib: &Library) -> Vec<LibrarySource> {
    let mut out = vec![];
    for id in lib.source_ids() {
        let Ok(src) = lib.source(&id) else { continue };
        if src["role"] != "library" {
            continue;
        }
        let version = src["version"].as_str().unwrap_or("").to_string();
        let Ok(path) = lib.version_dir(&src, &version) else { continue };
        let name = src["library_name"]
            .as_str()
            .map(String::from)
            .or_else(|| src["origin"]["repo"].as_str().map(String::from))
            .unwrap_or_else(|| src["detected"]["name"].as_str().unwrap_or(&id).to_string());
        out.push(LibrarySource { dir: LibraryDir { name, path }, source: id, version });
    }
    out
}

/// A model's fingerprint: what its files are (the render cache's file map, path -> sha256), so it
/// changes only when a file the model uses does (F-20), not with every edit elsewhere in its project.
pub fn files_id(files: &Value) -> String {
    hex::encode(Sha256::digest(serde_json::to_vec(files).unwrap_or_default()))[..12].to_string()
}

fn sha_file(p: &Path) -> Result<String> {
    Ok(hex::encode(Sha256::digest(std::fs::read(p).with_context(|| format!("couldn't read {}", p.display()))?)))
}

/// Model id from an entry path: "box.scad" -> "box", "examples/My Box.scad" -> "examples-my-box".
pub fn model_id(entry: &str) -> String {
    let no_ext = entry.rsplit_once('.').map(|(a, _)| a).unwrap_or(entry);
    let id = slug(no_ext);
    if id.is_empty() { "model".into() } else { id }
}

/// Read a project at one version. Returns the derived data (also saved in the library).
pub async fn ingest(
    lib: &Library,
    renderer: &Arc<Renderer>,
    common_files: &BTreeMap<String, String>,
    libs_ctx: &IngestLibs,
    src: &Value,
    version: &str,
    on_progress: impl Fn(String),
) -> Result<Value> {
    let id = src["id"].as_str().context("project has no id")?.to_string();
    let root = lib.version_dir(src, version)?;
    if !root.is_dir() {
        bail!("The files for {id} aren't there ({}).", root.display());
    }
    on_progress("Looking through the files…".into());
    let mut found = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || scan::survey(&root)).await?
    };
    let det = &src["detected"];
    let role = src["role"].as_str().unwrap_or("project");
    // the project's own description (workshop.json at its top), when it has one
    // or, for a starter project turned into a GitHub project, the app's description of it (F-1)
    let (own, own_name) = match read_workshop_json(&root) {
        Some(w) => (Some(w), "workshop.json"),
        None => {
            crate::sources::refresh_description(lib, src);
            (read_workshop_json_at(&lib.source_dir(&id)?.join("description.json")), "The app's description")
        }
    };
    let mut own_problems = vec![];
    let listed: Option<Vec<String>> = own.as_ref().and_then(|w| w["models"].as_array()).map(|ms| {
        ms.iter()
            .filter_map(|m| m["entrypoint"].as_str())
            .map(|e| e.trim_start_matches('/').to_string())
            .filter(|e| {
                let there = library::rel_inside(e).is_ok_and(|r| root.join(r).is_file());
                if !there {
                    own_problems.push(json!({ "kind": "scan", "message": format!("{own_name} lists {e}, which isn't in the project") }));
                }
                there
            })
            .collect()
    });
    // the app's description of a version whose files it doesn't fit at all (upstream moved on): read as usual
    let (own, listed) = if own_name != "workshop.json" && listed.as_ref().is_some_and(|l| l.is_empty()) {
        own_problems.clear();
        (None, None)
    } else {
        (own, listed)
    };
    // the app's description names the files it lists; any other model file, bar those it leaves out on
    // purpose, is read as usual, so a file upstream adds becomes a model (F-14). A project's own
    // workshop.json is the whole list.
    let overlay = own_name != "workshop.json" && own.as_ref().is_some_and(|w| w["left_out"].is_array());
    let listed = match (listed, overlay) {
        (Some(mut l), true) => {
            l.extend(found.entries.iter().filter(|e| !l.contains(e) && !crate::sources::left_out(own.as_ref().unwrap(), e)).cloned().collect::<Vec<_>>());
            Some(l)
        }
        (l, _) => l,
    };
    let entries: Vec<String> = if role == "library" { vec![] } else { listed.clone().unwrap_or_else(|| found.entries.clone()) };
    // a folder of examples (Review 3, F-15): listed as models until the project's page has asked, then
    // as metadata.json's `examples: { folder, as }` says: "models", "parts" (each made once at its own
    // values, listed in the Parts Library) or "leave" (left out)
    let examples = found.examples.clone().filter(|_| role != "library").map(|mut e| {
        let choice = lib.metadata(&id)["examples"].clone();
        e["as"] = if choice["folder"] == e["folder"] { choice["as"].clone() } else { json!("") };
        e
    });
    let examples_as = examples.as_ref().and_then(|e| e["as"].as_str()).unwrap_or("").to_string();
    let examples_dir = examples.as_ref().map(|e| format!("{}/", e["folder"].as_str().unwrap_or(""))).unwrap_or_default();
    let in_examples = |entry: &str| examples.is_some() && entry.trim_start_matches('/').starts_with(&examples_dir);
    let entries: Vec<String> = if examples_as == "leave" { entries.into_iter().filter(|e| !in_examples(e)).collect() } else { entries };
    let words = format!(
        "{} {} {} {}",
        det["name"].as_str().unwrap_or(""),
        det["summary"].as_str().unwrap_or(""),
        det["tags"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
        entries.join(" ")
    );
    let own_category = own.as_ref().and_then(|w| w["category"].as_str()).map(String::from);
    let category = if role == "library" { "libraries".to_string() } else { own_category.unwrap_or_else(|| scan::guess_category(&words).to_string()) };
    let category = category.as_str();
    let origin_url = det["origin"].as_str().or(src["origin"]["url"].as_str());
    let mut source_info = Map::new();
    if let Some(u) = origin_url {
        source_info.insert("repository".into(), json!(u));
    }
    if src["kind"] == "github" {
        let v = src["versions"].as_array().into_iter().flatten().find(|v| v["id"] == version);
        if let Some(v) = v {
            source_info.insert("commit".into(), v["commit"].clone());
            source_info.insert("commit_date".into(), v["date"].clone());
        }
    } else {
        source_info.insert("supplied_by".into(), json!(format!("added {}", src["version_date"].as_str().unwrap_or(""))));
    }
    let mut fam = json!({
        "id": id, "name": det["name"], "status": "available", "category": category,
        "category_label": category_label(category),
        "summary": det["summary"].as_str().unwrap_or(""), "tags": det["tags"].clone(),
        "license": det["license"].clone(), "authors": det["authors"].clone(),
        "links": origin_url.map(|u| json!({ "source": u })).unwrap_or(json!({})),
        "source": Value::Object(source_info),
        "models": [],
    });
    if let Some(w) = &own {
        for k in ["name", "summary", "tags", "license", "authors", "links", "ui"] {
            if let Some(v) = w.get(k).filter(|v| !v.is_null()) {
                fam[k] = if k == "license" { crate::meta::license_from(v) } else { v.clone() };
            }
        }
    }
    let editor_cfg = match &found.editor_toml {
        Some(p) => ingest::load_editor_toml(&root.join(p)).unwrap_or(json!({})),
        None => json!({}),
    };
    let libs = libs_ctx.search_order(lib, &id);
    let lib_dirs: Vec<LibraryDir> = libs.iter().map(|l| l.dir.clone()).collect();
    let mut problems: Vec<Value> = found.problems.iter().map(|p| json!({ "kind": "scan", "message": p })).collect();
    problems.extend(own_problems);
    let mut blobs: BTreeMap<String, String> = BTreeMap::new();
    let mut renderable: Vec<(String, PathBuf)> = vec![];
    let mut used_ids: Vec<String> = vec![];
    let mut planned = vec![];
    let mut bases: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut used: BTreeMap<String, Value> = BTreeMap::new(); // libraries the models include, by name
    let mut project_texts: Option<Vec<String>> = None;
    let mut described_used: Vec<usize> = vec![];
    for entry in &entries {
        // the first description of this file not used yet (two models can share a file, with different settings)
        let described = own.as_ref().and_then(|w| w["models"].as_array()).and_then(|ms| {
            ms.iter().enumerate().find(|(i, m)| m["entrypoint"].as_str().map(|e| e.trim_start_matches('/')) == Some(entry.as_str()) && !described_used.contains(i))
        });
        if let Some((i, _)) = described {
            described_used.push(i);
        }
        let described = described.map(|(_, m)| m);
        let mut mid = described.and_then(|m| m["id"].as_str()).filter(|i| library::valid_id(i).is_ok()).map(String::from).unwrap_or_else(|| model_id(entry));
        let base = mid.clone();
        let mut n = 2;
        while used_ids.contains(&mid) {
            mid = format!("{base}-{n}");
            n += 1;
        }
        used_ids.push(mid.clone());
        let resolver = scan::resolver(&root, &lib_dirs);
        let (files, missing) = resolver.collect(entry);
        let mut fmap = BTreeMap::new();
        let mut input_bytes = 0u64;
        for (vpath, (host, which)) in &files {
            let sha = sha_file(host)?;
            input_bytes += std::fs::metadata(host).map(|m| m.len()).unwrap_or(0);
            let (owner, base) = match which {
                Some(i) => {
                    let l = &libs[*i];
                    used.entry(l.dir.name.to_lowercase()).or_insert_with(|| {
                        json!({ "name": l.dir.name, "provider": if l.source.starts_with("@bundled/") { "bundled" } else { l.source.as_str() }, "version": l.version })
                    });
                    (format!("{}@{}", l.source, l.version), l.dir.path.clone())
                }
                None => (format!("{id}@{version}"), root.clone()),
            };
            let rel = host.strip_prefix(&base).map(|r| r.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")).unwrap_or_default();
            blobs.insert(sha.clone(), format!("{owner}:{rel}"));
            renderable.push((sha.clone(), host.clone()));
            fmap.insert(vpath.clone(), sha);
        }
        // values typed into its module calls (no settings of its own): a copy that has them as settings
        let entry_v = format!("/{}", entry.trim_start_matches('/'));
        let mut lifted = false;
        if let Some((host, _)) = files.get(&entry_v) {
            let text = String::from_utf8_lossy(&std::fs::read(host).unwrap_or_default()).into_owned();
            // (the files it includes are read only for a file that's just includes and calls)
            let calls = crate::components::only_calls(&text);
            let others: Vec<String> = if !calls { vec![] } else { files
                .iter()
                .filter(|(v, (h, _))| **v != entry_v && h.extension().is_some_and(|e| e.eq_ignore_ascii_case("scad")))
                .map(|(_, (h, _))| String::from_utf8_lossy(&std::fs::read(h).unwrap_or_default()).into_owned())
                .collect() };
            // (the project's other model files are read only for a file that has values to lift: they
            // may hold the library's own Customizer call, whose descriptions and choices the settings borrow)
            let mut copy = if calls { crate::components::lift_call_values(&text, entry, &others, &[]) } else { None };
            if copy.is_some() {
                let hints = project_texts.get_or_insert_with(|| {
                    entries
                        .iter()
                        .filter_map(|e| library::rel_inside(e).ok())
                        .map(|r| root.join(r))
                        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() < 2_000_000))
                        .filter_map(|p| std::fs::read(p).ok())
                        .map(|b| String::from_utf8_lossy(&b).into_owned())
                        .collect()
                });
                copy = crate::components::lift_call_values(&text, entry, &others, hints).or(copy);
            }
            if let Some(copy) = copy {
                let gen = lib.source_dir(&id)?.join("generated").join(version).join(library::rel_inside(entry)?);
                std::fs::create_dir_all(gen.parent().unwrap())?;
                crate::config::write_atomic(&gen, copy.as_bytes())?;
                let sha = sha_file(&gen)?;
                blobs.insert(sha.clone(), format!("{id}@{version}{GENERATED}:{entry}"));
                renderable.push((sha.clone(), gen));
                // the project's own file, which code edits are based on (the copy is the app's)
                if let Some(orig) = fmap.insert(entry_v.clone(), sha) {
                    bases.insert(mid.clone(), (entry_v.clone(), orig));
                }
                lifted = true;
            }
        }
        if !missing.is_empty() {
            problems.push(json!({
                "kind": "missing", "model": format!("{id}/{mid}"), "files": missing,
                "message": format!("{} can't find {}", entry, missing.iter().map(|m| m.trim_start_matches("/libraries/")).collect::<Vec<_>>().join(", ")),
            }));
        }
        let stem = Path::new(entry).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut model = json!({
            "id": mid, "name": scan::humanize(&stem), "entrypoint": entry, "status": "available",
            "presets": scan::presets_for(&root, entry),
        });
        // what the project's workshop.json says about it: its name, notes, form (ui, hidden, fixed), presets…
        for (k, v) in described.and_then(Value::as_object).into_iter().flatten() {
            if !["id", "entrypoint", "status"].contains(&k.as_str()) && !v.is_null() {
                model[k] = v.clone();
            }
        }
        if lifted {
            model["settings_from"] = json!("calls");
        }
        let editor = ingest::editor_model_meta(&editor_cfg, Path::new(entry).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().as_str());
        planned.push((mid, model, fmap, editor, input_bytes, entry.clone()));
    }
    renderer.add_blobs(renderable);
    // settings: several OpenSCAD processes at once (the renderer limits how many)
    let mut set = tokio::task::JoinSet::new();
    for (i, (mid, _, fmap, _, _, entry)) in planned.iter().enumerate() {
        let mut files = common_files.clone();
        files.extend(fmap.clone());
        let req = RenderRequest { model: format!("{id}/{mid}"), entry: format!("/{entry}"), files, values: Default::default(), defines: vec![], call: None, format: None, engine: None };
        let r = renderer.clone();
        set.spawn(async move { (i, r.export_params(&req).await) });
    }
    let total = planned.len();
    let mut raws: Vec<Option<Result<Value, String>>> = vec![None; total];
    let mut done = 0;
    let failure = |e: crate::render::RenderFailure| {
        let tail: Vec<&String> = e.logs.iter().filter(|l| l.starts_with("ERROR") || l.starts_with("WARNING")).take(5).collect();
        format!("{}{}", e.message, if tail.is_empty() { String::new() } else { format!(" ({})", tail.into_iter().cloned().collect::<Vec<_>>().join("; ")) })
    };
    let mut retry = vec![];
    while let Some(res) = set.join_next().await {
        let (i, r) = res?;
        done += 1;
        on_progress(format!("Reading settings: {done} of {total}"));
        match r {
            Ok(v) => raws[i] = Some(Ok(v)),
            Err(e) => {
                retry.push(i);
                raws[i] = Some(Err(failure(e)));
            }
        }
    }
    // a file whose settings couldn't be read is tried once more on its own: with several OpenSCAD
    // processes at once (and renders from the page) one can fail that reads fine alone, and a model
    // left out is lost from the project (its saved settings, its thumbnail)
    for i in retry {
        let (mid, _, fmap, _, _, entry) = &planned[i];
        let mut files = common_files.clone();
        files.extend(fmap.clone());
        let req = RenderRequest { model: format!("{id}/{mid}"), entry: format!("/{entry}"), files, values: Default::default(), defines: vec![], call: None, format: None, engine: None };
        if let Ok(v) = renderer.export_params(&req).await {
            raws[i] = Some(Ok(v));
        }
    }
    let mut models = vec![];
    for ((mid, model, fmap, editor, input_bytes, entry), raw) in planned.into_iter().zip(raws) {
        let key = format!("{id}/{mid}");
        match raw.unwrap_or_else(|| Err("not read".into())) {
            Ok(raw) => {
                fam["models"].as_array_mut().unwrap().push(model.clone());
                let (_, mut detail) = sitebuild::assemble_model(&fam, &model, &key, &format!("/{entry}"), &fmap, &editor, &raw, input_bytes)?;
                // text settings the code compares with words become drop-downs (0.4.24); if that fails, the
                // settings stay as OpenSCAD's list gave them and the project's problems say so
                let texts: Vec<String> = fmap.keys().filter(|f| !f.starts_with("/libraries/") && f.to_lowercase().ends_with(".scad"))
                    .filter_map(|f| library::rel_inside(f).ok()).map(|r| root.join(r))
                    .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() < 2_000_000))
                    .filter_map(|p| std::fs::read(p).ok()).map(|b| String::from_utf8_lossy(&b).into_owned()).collect();
                if let Some(params) = detail["parameters"].as_array_mut() {
                    let before = params.clone();
                    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ingest::choices_from_code(params, &texts)));
                    if let Some(e) = match read { Ok(Ok(_)) => None, Ok(Err(e)) => Some(format!("{e:#}")), Err(_) => Some("it stopped unexpectedly".into()) } {
                        *params = before;
                        problems.push(json!({ "kind": "reading", "model": key, "message": format!("{entry}: choices weren't read from the code ({e}); its settings are as OpenSCAD lists them") }));
                    }
                }
                detail["folder"] = json!(ingest::dirname(&format!("/{entry}")).trim_start_matches('/'));
                if examples_as == "parts" && in_examples(&entry) {
                    detail["as_part"] = json!(true); // made once at its defaults and listed as a part (F-15)
                } else if examples_as.is_empty() && in_examples(&entry) {
                    detail["example_pending"] = json!(true); // not listed or checked until the project's page is answered (F-21)
                }
                if let Some((path, orig)) = bases.get(&mid) {
                    detail["file_bases"] = json!({ path.as_str(): orig });
                }
                let off = ingest::limit_problems(detail["parameters"].as_array().map(Vec::as_slice).unwrap_or(&[]));
                if !off.is_empty() {
                    problems.push(json!({ "kind": "limits", "model": key, "settings": off, "message": format!("{entry}: {}", off.join("; ")) }));
                }
                models.push(detail);
            }
            Err(e) => problems.push(json!({ "kind": "settings", "model": key, "message": format!("{entry}: {e}") })),
        }
    }
    // Fusion 360 files with user parameters are models too, read with MitCad's reader (experimental,
    // 0.4.27; docs/DESKTOP_PLAN.md, "Fusion 360 files through MitCad"): the settings and the preview from
    // the file; generating needs MitCad installed. The file leaves the Parts Library item it was listed in.
    if role != "library" {
        on_progress("Reading Fusion 360 files…".into());
        let fusion: Vec<scan::FileInfo> = scan::list_files(&root).into_iter().filter(|f| {
            let l = f.rel.to_lowercase();
            (l.ends_with(".f3d") || l.ends_with(".f3z")) && !l.split('/').any(|p| p == "test" || p == "tests")
        }).collect();
        for f in fusion {
            let host = root.join(&f.rel);
            let entry = f.rel.clone();
            let info = match crate::f3d::read(&host) {
                Ok(i) => i,
                Err(e) => {
                    problems.push(json!({ "kind": "settings", "model": format!("{id}/{}", model_id(&entry)), "message": format!("{entry}: couldn't be read as a Fusion 360 file ({e:#})") }));
                    continue;
                }
            };
            if info.parameters.is_empty() {
                continue; // nothing to set: it stays a part
            }
            let mut mid = model_id(&entry);
            let base = mid.clone();
            let mut n = 2;
            while used_ids.contains(&mid) {
                mid = format!("{base}-{n}");
                n += 1;
            }
            used_ids.push(mid.clone());
            let key = format!("{id}/{mid}");
            let sha = sha_file(&host)?;
            blobs.insert(sha.clone(), format!("{id}@{version}:{entry}"));
            renderer.add_blobs([(sha.clone(), host.clone())]);
            let entry_v = format!("/{entry}");
            let fmap: BTreeMap<String, String> = [(entry_v.clone(), sha)].into();
            let stem = Path::new(&entry).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let model = json!({ "id": mid, "name": scan::humanize(&stem), "entrypoint": entry, "status": "available", "presets": [] });
            fam["models"].as_array_mut().unwrap().push(model.clone());
            let (_, mut detail) = sitebuild::assemble_model(&fam, &model, &key, &entry_v, &fmap, &json!({}), &crate::f3d::raw_params(&info), f.bytes)?;
            detail["engine"] = json!("mitcad");
            detail["folder"] = json!(ingest::dirname(&entry_v).trim_start_matches('/'));
            detail["fusion"] = json!({ "formulas": info.formulas, "timeline_items": info.timeline_items, "components": info.components, "writer": info.writer, "design": info.label });
            // each setting's unit (mm, deg, …): what its value goes back to MitCad with
            let units = crate::f3d::units(&info);
            for p in detail["parameters"].as_array_mut().into_iter().flatten() {
                if let Some(u) = p["name"].as_str().and_then(|n| units.get(n)) {
                    p["unit"] = json!(u);
                }
            }
            if let Some(png) = &info.preview_png {
                let thumbs = lib.source_dir(&id)?.join("thumbs");
                std::fs::create_dir_all(&thumbs)?;
                crate::config::write_atomic(&thumbs.join(format!("{mid}.png")), png)?;
            }
            models.push(detail);
            for part in found.parts.iter_mut() {
                if let Some(files) = part["files"].as_array_mut() {
                    files.retain(|x| x["path"] != entry.as_str());
                }
            }
            found.parts.retain(|p| p["files"].as_array().is_some_and(|f| !f.is_empty()));
        }
    }
    // a library project: its modules, for Components
    let components = if role == "library" {
        on_progress("Reading the library's modules…".into());
        let name = library_sources(lib).into_iter().find(|l| l.source == id).map(|l| l.dir.name).unwrap_or_else(|| id.clone());
        let info = project_lib_info(src, version, &name);
        let curated = libs_ctx.curated.get(&name).cloned();
        let no_guess = libs_ctx.curated.get("$no_guess").and_then(|n| n.get(&name)).cloned();
        let root2 = root.clone();
        match tokio::task::spawn_blocking(move || crate::components::index_library(&info, &root2, curated.as_ref(), no_guess.as_ref())).await? {
            Ok(v) => v,
            Err(e) => {
                problems.push(json!({ "kind": "scan", "message": format!("Couldn't read the library's modules: {e:#}") }));
                Value::Null
            }
        }
    } else {
        Value::Null
    };
    // reference documents: the README as HTML (kept in the library, relative to the project's
    // folder), PDFs and readme.txt as they are
    on_progress("Reading the documents…".into());
    let docs = {
        let root2 = root.clone();
        let out_dir = lib.source_dir(&id)?.join("docs").join(version);
        let prefix = format!("docs/{version}/");
        let (docs, probs) = tokio::task::spawn_blocking(move || {
            let mut d = crate::docs::find(&root2);
            let probs = crate::docs::convert_all(&root2, &mut d, &out_dir, &prefix, crate::docs::Rebase::default());
            (d, probs)
        })
        .await?;
        for p in probs {
            problems.push(json!({ "kind": "docs", "message": format!("Couldn't read a document: {p}") }));
        }
        docs
    };
    let derived = json!({
        "format": DERIVED_FORMAT,
        "docs": docs,
        "components": components,
        "libraries_used": used.into_values().collect::<Vec<_>>(),
        "engine": renderer.engine().version,
        "made": library::now(),
        "source": id, "version": version,
        "family": fam,
        "models": models,
        "parts": found.parts,
        "blobs": blobs,
        "problems": problems,
        "stats": { "scad_files": found.scad_files, "library_files": found.library_files, "entries": found.entries.len() },
        "readme": found.readme.as_ref().map(|(p, _)| p.clone()),
        "license_file": found.license.as_ref().map(|(p, _)| p.clone()),
        "editor_toml": found.editor_toml,
        // what the files were when read, for Scan for changes: names, sizes and times (cheap to
        // compare), and contents (compared only when the first differs)
        "files_stat": scan::quick_fingerprint(&root),
        "files_fp": scan::fingerprint(&root),
        "described_by": if own.is_none() { "reading" } else if own_name == "workshop.json" { "workshop.json" } else { "description.json" },
        "limits_checked": true,
        "examples": examples,
    });
    lib.save_derived(&id, version, &derived)?;
    // every version records its file count and contents (F-2); the description beside the files (F-5)
    if let Ok(mut s) = lib.source(&id) {
        let before = s.clone();
        let n = scan::list_files(&root).len();
        for v in s["versions"].as_array_mut().into_iter().flatten().filter(|v| v["id"] == version) {
            v["files"] = json!(n);
            v["content"] = derived["files_fp"].clone();
        }
        if s != before {
            lib.save_source(&s)?;
        }
    }
    let _ = crate::config::write_json(&lib.source_dir(&id)?.join("workshop.json"), &describe(&derived["family"], &id, version));
    Ok(derived)
}

/// A project's own description, `workshop.json` at the top of its files (a family manifest's shape:
/// docs/DESKTOP_PLAN.md, "Consistent projects design"), when it has a readable one.
pub fn read_workshop_json(root: &Path) -> Option<Value> {
    read_workshop_json_at(&root.join("workshop.json"))
}

fn read_workshop_json_at(path: &Path) -> Option<Value> {
    let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    v.is_object().then_some(v)
}

/// The description the app uses for a project, in workshop.json's shape: written beside its files
/// (sources/<id>/workshop.json) when it's read, so the library says what each project is without
/// the app, and into a project's own folder by Save as project file.
pub fn describe(fam: &Value, id: &str, version: &str) -> Value {
    let mut out = Map::new();
    out.insert("$schema".into(), json!("https://raw.githubusercontent.com/DrFlGd/SCAD-Workshop/main/schema/workshop.schema.json"));
    out.insert("format".into(), json!(1));
    let worth = |v: &&Value| !v.is_null() && v.as_object().is_none_or(|o| !o.is_empty()) && v.as_array().is_none_or(|a| !a.is_empty()) && v.as_str() != Some("");
    for k in ["name", "summary", "category", "tags", "license", "authors", "links", "ui"] {
        if k == "license" && fam[k]["spdx"].as_str().is_none_or(|s| s == "NOASSERTION") && fam[k]["notes"].is_null() {
            continue; // not stated: nothing to say
        }
        if let Some(v) = fam.get(k).filter(worth) {
            out.insert(k.into(), v.clone());
        }
    }
    let models: Vec<Value> = fam["models"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let mut o = Map::new();
            // every key a description can give a model, so the file says all the app reads it with (F-14)
            for (k, v) in m.as_object().into_iter().flatten() {
                if !["status", "settings_from"].contains(&k.as_str()) && worth(&v) {
                    o.insert(k.clone(), v.clone());
                }
            }
            Value::Object(o)
        })
        .collect();
    out.insert("models".into(), json!(models));
    out.insert("made_by".into(), json!(format!("SCAD Workshop {}, for {id} at {version}", env!("CARGO_PKG_VERSION"))));
    Value::Object(out)
}

pub fn category_label(id: &str) -> String {
    scan::EXTRA_CATEGORIES
        .iter()
        .find(|(c, _)| *c == id)
        .map(|(_, l)| l.to_string())
        .unwrap_or_else(|| sitebuild::category_label(id))
}

/// Where a blob entry ("<source>@<version>:<path>") is on disk.
pub fn blob_path(lib: &Library, entry: &str, bundled: &[crate::components::ComponentLibrary]) -> Option<PathBuf> {
    let (owner, rel) = entry.split_once(':')?;
    if let Some(owner) = owner.strip_suffix(GENERATED) {
        // "<source>@<version>~gen": a file the app made for that version (settings lifted from calls)
        let (source, version) = owner.split_once('@')?;
        library::valid_id(version).ok()?;
        return Some(lib.source_dir(source).ok()?.join("generated").join(version).join(library::rel_inside(rel).ok()?));
    }
    if let Some(name) = owner.strip_prefix("@bundled/") {
        // "@bundled/<Name>@<commit>": a library shipped with the app
        let name = name.split('@').next()?;
        let b = bundled.iter().find(|b| b.info.name.eq_ignore_ascii_case(name))?;
        return Some(b.root.join(library::rel_inside(rel).ok()?));
    }
    let (source, version) = owner.split_once('@')?;
    let src = lib.source(source).ok()?;
    let dir = lib.version_dir(&src, version).ok()?;
    let p = dir.join(library::rel_inside(rel).ok()?);
    Some(p)
}

// ------------------------------------------------------------------ starter library

/// Package a built site's catalog (tools/build_site.py output, with thumbnails)
/// as library projects in `out/sources/`: one per family and one per parts pack.
/// Projects whose content didn't change keep their version id between builds.
pub fn bundle_site(site: &Path, out: &Path) -> Result<Vec<String>> {
    let catalog: Value = serde_json::from_slice(&std::fs::read(site.join("data/catalog.json"))?)?;
    let made = library::now();
    let sources = out.join("sources");
    std::fs::create_dir_all(&sources)?;
    let mut ids = vec![];
    for fam in catalog["families"].as_array().into_iter().flatten() {
        let id = fam["id"].as_str().context("family without id")?.to_string();
        let keys: Vec<String> = catalog["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["family"] == id.as_str())
            .filter_map(|m| m["key"].as_str().map(String::from))
            .collect();
        if keys.is_empty() {
            continue;
        }
        let mut details = vec![];
        for k in &keys {
            let d: Value = serde_json::from_slice(&std::fs::read(site.join("data/models").join(format!("{}.json", k.replace('/', "--"))))?)?;
            details.push(d);
        }
        let mut h = Sha256::new();
        for d in &details {
            h.update(serde_json::to_vec(d)?);
        }
        let version = format!("bundle-{}", &hex::encode(h.finalize())[..12]);
        let dir = sources.join(&id);
        let files = dir.join("files").join(&version);
        let mut blobs = BTreeMap::new();
        for d in &details {
            for (vpath, sha) in d["files"].as_object().into_iter().flatten() {
                let sha = sha.as_str().unwrap_or("");
                let rel = library::rel_inside(vpath)?;
                let dst = files.join(&rel);
                if !dst.exists() {
                    std::fs::create_dir_all(dst.parent().unwrap())?;
                    std::fs::copy(site.join("fs").join(sha), &dst).with_context(|| format!("copying {vpath}"))?;
                }
                blobs.insert(sha.to_string(), format!("{id}@{version}:{}", vpath.trim_start_matches('/')));
            }
        }
        let first = &details[0];
        let mut thumbs_made = 0;
        for d in &details {
            let k = d["key"].as_str().unwrap_or("");
            let t = site.join("thumbs/gen").join(format!("{}.webp", k.replace('/', "--")));
            if t.is_file() {
                std::fs::create_dir_all(dir.join("thumbs"))?;
                std::fs::copy(&t, dir.join("thumbs").join(format!("{}.webp", d["id"].as_str().unwrap_or(""))))?;
                thumbs_made += 1;
            }
        }
        let updated = first["updated"].as_str().map(String::from);
        let origin_url = fam["source"].as_str().map(String::from).or_else(|| first["source"]["repository"].as_str().map(String::from));
        let family = json!({
            "id": id, "name": fam["name"], "status": "available", "category": first["category"],
            "category_label": first["category_label"], "summary": first["summary"], "tags": first["tags"],
            "license": fam["license"], "authors": fam["authors"], "links": first["links"], "source": first["source"],
            "models": details.iter().map(|d| json!({ "id": d["id"], "name": d["name"] })).collect::<Vec<_>>(),
        });
        // the family's README from the website build, kept like a read project's (already
        // pointing to GitHub, so not relative to the project's folder)
        let mut docs = vec![];
        for d in fam["docs"].as_array().into_iter().flatten() {
            let Some(url) = d["url"].as_str() else { continue };
            let ext = if d["kind"] == "text" { "txt" } else { "html" };
            let did = d["id"].as_str().unwrap_or("readme");
            let file = format!("docs/{version}/{did}.{ext}");
            let to = dir.join(&file);
            std::fs::create_dir_all(to.parent().unwrap())?;
            std::fs::copy(site.join(library::rel_inside(url)?), &to).with_context(|| format!("copying {url}"))?;
            docs.push(json!({ "id": did, "title": d["title"], "kind": d["kind"], "file": file, "relative": false }));
        }
        let derived = json!({
            "format": DERIVED_FORMAT, "engine": catalog["engine"], "made": made, "source": id, "version": version,
            "family": family, "models": details, "parts": [], "blobs": blobs, "problems": [],
            "stats": { "thumbnails": thumbs_made }, "docs": docs,
        });
        crate::config::write_atomic(&dir.join("derived").join(format!("{version}.json")), &serde_json::to_vec(&derived)?)?;
        let src = json!({
            "id": id, "kind": "bundled", "role": "project", "added": made,
            "origin": { "type": "app", "bundle": "scad-workshop", "url": origin_url },
            "version": version, "version_date": updated.clone().unwrap_or_else(|| made.clone()),
            "versions": [{ "id": version, "date": updated, "added": made }],
            "detected": {
                "name": fam["name"], "summary": first["summary"], "license": fam["license"], "authors": fam["authors"],
                "tags": first["tags"], "origin": origin_url, "category": first["category"],
            },
        });
        crate::config::write_json(&dir.join("source.json"), &src)?;
        ids.push(id);
    }
    for l in catalog["libraries"].as_array().into_iter().flatten() {
        let id = l["id"].as_str().context("library without id")?.to_string();
        let detail: Value = serde_json::from_slice(&std::fs::read(site.join("data/libraries").join(format!("{id}.json")))?)?;
        let mut h = Sha256::new();
        h.update(serde_json::to_vec(&detail)?);
        let version = format!("bundle-{}", &hex::encode(h.finalize())[..12]);
        let dir = sources.join(&id);
        let files = dir.join("files").join(&version);
        let mut parts = vec![];
        for it in detail["items"].as_array().into_iter().flatten() {
            let item_id = it["id"].as_str().unwrap_or("").to_string();
            let mut it = it.clone();
            for f in it["files"].as_array_mut().into_iter().flatten() {
                let path = f["path"].as_str().unwrap_or("").to_string();
                let dst = files.join(library::rel_inside(&path)?);
                if !dst.exists() {
                    std::fs::create_dir_all(dst.parent().unwrap())?;
                    std::fs::copy(site.join("parts").join(&id).join(&path), &dst).with_context(|| format!("copying {path}"))?;
                }
                f.as_object_mut().unwrap().shift_remove("url");
            }
            if let Some(pv) = it["preview_url"].as_str().map(String::from) {
                let src_pv = site.join(&pv);
                if pv.contains("/_preview/") && src_pv.is_file() {
                    std::fs::create_dir_all(dir.join("previews"))?;
                    std::fs::copy(&src_pv, dir.join("previews").join(format!("{item_id}.stl")))?;
                    it["preview_stl"] = json!(format!("previews/{item_id}.stl"));
                }
            }
            it.as_object_mut().unwrap().shift_remove("preview_url");
            it.as_object_mut().unwrap().shift_remove("thumb");
            let t = site.join("thumbs/part").join(format!("{id}--{item_id}.webp"));
            if t.is_file() {
                std::fs::create_dir_all(dir.join("thumbs"))?;
                std::fs::copy(&t, dir.join("thumbs").join(format!("part-{item_id}.webp")))?;
            }
            parts.push(it);
        }
        let category = detail["category"].as_str().unwrap_or("other");
        let family = json!({
            "id": id, "name": detail["name"], "status": "available", "category": category,
            "summary": detail["summary"], "tags": [], "license": detail["license"], "authors": detail["authors"],
            "links": detail["source_url"].as_str().map(|u| json!({ "source": u })).unwrap_or(json!({})),
            "source": { "repository": detail["source_url"] }, "models": [],
        });
        let derived = json!({
            "format": DERIVED_FORMAT, "engine": catalog["engine"], "made": made, "source": id, "version": version,
            "family": family, "models": [], "parts": parts, "blobs": {}, "problems": [],
            "notes": detail["notes"],
        });
        crate::config::write_atomic(&dir.join("derived").join(format!("{version}.json")), &serde_json::to_vec(&derived)?)?;
        let src = json!({
            "id": id, "kind": "bundled", "role": "project", "added": made,
            "origin": { "type": "app", "bundle": "scad-workshop", "url": detail["source_url"] },
            "version": version, "version_date": made, "versions": [{ "id": version, "date": made, "added": made }],
            "detected": {
                "name": detail["name"], "summary": detail["summary"], "license": detail["license"], "authors": detail["authors"],
                "tags": [], "origin": detail["source_url"], "category": category,
            },
        });
        crate::config::write_json(&dir.join("source.json"), &src)?;
        ids.push(id);
    }
    crate::config::write_json(&out.join("starter.json"), &json!({ "made": made, "engine": catalog["engine"], "sources": ids }))?;
    Ok(ids)
}

/// Copy starter projects into a library: new ones are installed (unless the user
/// removed them before), changed ones arrive as an update to accept.
/// Returns (installed, updates offered).
pub fn sync_starter(lib: &Library, starter: &Path) -> Result<(Vec<String>, Vec<String>)> {
    if lib.read_only().is_some() || !starter.join("starter.json").is_file() {
        return Ok((vec![], vec![]));
    }
    let removed: Vec<String> = lib.meta()["removed_starter"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    let (mut installed, mut updates) = (vec![], vec![]);
    let dir = starter.join("sources");
    let mut ids: Vec<String> = std::fs::read_dir(&dir).map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().map(String::from)).collect()).unwrap_or_default();
    ids.sort();
    for id in ids {
        let Ok(bundled) = serde_json::from_slice::<Value>(&std::fs::read(dir.join(&id).join("source.json")).unwrap_or_default()) else { continue };
        let version = bundled["version"].as_str().unwrap_or("").to_string();
        match lib.source(&id) {
            Err(_) => {
                if removed.contains(&id) || lib.source_dir(&id)?.exists() {
                    continue;
                }
                copy_tree(&dir.join(&id), &lib.source_dir(&id)?)?;
                installed.push(id);
            }
            Ok(mut have) => {
                if have["kind"] == "bundled" && have["version"] == version.as_str() {
                    // same version from an app before 0.4: bring in its documents
                    let target = lib.source_dir(&id)?;
                    let theirs = dir.join(&id).join("docs").join(&version);
                    if theirs.is_dir() && !target.join("docs").join(&version).is_dir() {
                        copy_tree(&theirs, &target.join("docs").join(&version))?;
                        let d = format!("derived/{version}.json");
                        copy_tree_file(&dir.join(&id).join(&d), &target.join(&d))?;
                    }
                    continue;
                }
                if have["kind"] != "bundled" || have["update"]["latest"]["id"] == version.as_str() {
                    continue;
                }
                // bring the new version in beside the current one; the user accepts it like any update
                let target = lib.source_dir(&id)?;
                copy_tree(&dir.join(&id).join("files").join(&version), &target.join("files").join(&version))?;
                copy_tree(&dir.join(&id).join("derived"), &target.join("derived"))?;
                for extra in ["thumbs", "previews", "docs"] {
                    if dir.join(&id).join(extra).is_dir() {
                        copy_tree(&dir.join(&id).join(extra), &target.join(extra))?;
                    }
                }
                have["update"] = json!({
                    "state": "available", "checked": library::now(), "from": "app",
                    "latest": { "id": version, "date": bundled["version_date"], "message": "Updated with the app" },
                });
                lib.save_source(&have)?;
                updates.push(id);
            }
        }
    }
    Ok((installed, updates))
}

fn copy_tree_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::copy(from, to).with_context(|| format!("couldn't copy {}", from.display()))?;
    Ok(())
}

/// Copy a folder tree (files overwrite).
pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for f in scan::list_files(from) {
        let dst = to.join(&f.rel);
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(from.join(&f.rel), &dst).with_context(|| format!("couldn't copy {}", f.rel))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids() {
        assert_eq!(model_id("box.scad"), "box");
        assert_eq!(model_id("examples/My Box.scad"), "examples-my-box");
    }

    #[test]
    fn scanning_for_changes() {
        let dir = std::env::temp_dir().join(format!("workshop-scan-changes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (lib, _) = Library::open(&dir.join("lib"), &crate::config::Prefs::new(dir.join("p.json"))).unwrap();
        // a project added from a ZIP, read at format 4 with fingerprints
        let files = dir.join("lib/sources/zip-box/files/zip-1");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join("box.scad"), "w = 2;\ncube(w);\n").unwrap();
        let src = json!({ "id": "zip-box", "kind": "zip", "role": "project", "version": "zip-1" });
        lib.save_source(&src).unwrap();
        let derived = |fmt: u64, fps: bool| {
            let mut d = json!({ "format": fmt, "family": { "models": [{ "entrypoint": "box.scad" }] } });
            if fps {
                d["files_stat"] = json!(scan::quick_fingerprint(&files));
                d["files_fp"] = json!(scan::fingerprint(&files));
            }
            d
        };
        lib.save_derived("zip-box", "zip-1", &derived(4, true)).unwrap();
        let c = scan_changes(&lib, &[]);
        assert!(c.read.is_empty() && c.missing.is_empty(), "{c:?}");
        // touched (same contents, new time): not a change, and the quick fingerprint is updated
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(files.join("box.scad"), "w = 2;\ncube(w);\n").unwrap();
        assert!(scan_changes(&lib, &[]).read.is_empty());
        assert_eq!(lib.derived("zip-box", "zip-1").unwrap()["files_stat"], json!(scan::quick_fingerprint(&files)));
        // edited: read again (unless it's being read anyway). Same size, so wait for a
        // new modification time: within the same second the quick look can't tell.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(files.join("box.scad"), "w = 3;\ncube(w);\n").unwrap();
        assert_eq!(scan_changes(&lib, &[]).read, vec!["zip-box"]);
        assert!(scan_changes(&lib, &["zip-box".into()]).read.is_empty());
        // read before fingerprints were kept, models unchanged: just recorded
        lib.save_derived("zip-box", "zip-1", &derived(3, false)).unwrap();
        assert!(scan_changes(&lib, &[]).read.is_empty());
        let d = lib.derived("zip-box", "zip-1").unwrap();
        assert_eq!(d["format"], json!(DERIVED_FORMAT));
        assert_eq!(d["files_fp"], json!(scan::fingerprint(&files)));
        // ... but a new model file means reading again
        lib.save_derived("zip-box", "zip-1", &derived(3, false)).unwrap();
        std::fs::write(files.join("lid.scad"), "cube(1);\n").unwrap();
        assert_eq!(scan_changes(&lib, &[]).read, vec!["zip-box"]);
        // a project added as a library has no models, so its files' models don't count
        lib.save_source(&json!({ "id": "zip-box", "kind": "zip", "role": "library", "version": "zip-1" })).unwrap();
        lib.save_derived("zip-box", "zip-1", &json!({ "format": 3, "family": { "models": [] } })).unwrap();
        assert!(scan_changes(&lib, &[]).read.is_empty());
        // files gone
        std::fs::remove_dir_all(&files).unwrap();
        assert_eq!(scan_changes(&lib, &[]).missing, vec!["zip-box"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
