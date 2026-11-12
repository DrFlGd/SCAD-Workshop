//! The app's catalog: every project in the open library, with the user's edits
//! applied, in the same shape as the website's data/catalog.json (so the page
//! needs no second code path), plus the projects themselves and what needs attention.

use crate::components::{self, ComponentLibrary};
use crate::library::{self, Library};
use crate::meta::{self, Layers};
use crate::project::{self, category_label};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Everything the page reads, built once per change.
#[derive(Default)]
pub struct Catalog {
    pub catalog: Value,
    /// "family/model" -> model page JSON (edits applied)
    pub models: HashMap<String, Value>,
    /// parts pack id -> data/libraries/<id>.json shape
    pub parts: HashMap<String, Value>,
    /// sha256 -> file on disk, for renders
    pub blobs: HashMap<String, PathBuf>,
    /// source id -> (derived data of the current version, folder of that version)
    pub derived: HashMap<String, (Value, PathBuf)>,
}

fn obj(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

/// The component libraries for a catalog: the copies in use (the user's library
/// project or the bundled copy, see [`components::effective`]) and every bundled one.
#[derive(Default, Clone, Copy)]
pub struct Libs<'a> {
    pub effective: &'a [ComponentLibrary],
    pub bundled: &'a [ComponentLibrary],
    pub projects: &'a [ComponentLibrary],
    pub prefer_bundled: &'a [String],
}

/// Build the catalog. `url` is how the page reaches library files ("library://localhost/").
pub fn build(lib: &Library, engine: &str, common_files: &Value, url: &str, libs: Libs) -> Catalog {
    let mut out = Catalog::default();
    // components: the modules of the libraries in use, as model pages
    let mut comp_list: Vec<Value> = vec![];
    let mut group_counts: BTreeMap<String, usize> = BTreeMap::new();
    let comp_thumbs = lib.root().join("thumbs/components");
    if lib.read_only().is_none() {
        for l in libs.effective {
            adopt_component_thumbs(&comp_thumbs, l);
        }
    }
    let url_of = |rel: &str| format!("{url}{}", rel.split('/').map(urlencode).collect::<Vec<_>>().join("/"));
    for l in libs.effective {
        for (rel, sha) in l.index["files"].as_object().into_iter().flatten() {
            if let Some(sha) = sha.as_str() {
                out.blobs.insert(sha.to_string(), l.root.join(rel));
            }
        }
        for c in components::components_of(l) {
            let d = components::component_model(l, &c);
            let key = d["key"].as_str().unwrap_or("").to_string();
            *group_counts.entry(c.group.clone()).or_default() += 1;
            let mut summary = Map::new();
            for k in ["key", "family", "family_name", "id", "name", "category", "category_label", "summary", "tags", "license", "browser", "terms", "settings", "updated", "kind"] {
                if let Some(v) = d.get(k).filter(|v| !v.is_null()) {
                    summary.insert(k.into(), v.clone());
                }
            }
            summary.insert("module".into(), json!(c.module));
            summary.insert("library".into(), json!(l.info.name));
            summary.insert("provider".into(), json!(l.provider));
            summary.insert("needs".into(), d["needs"].clone());
            // a thumbnail saved after the component was first made, by this copy of its library
            // (thumbs/components/<library>/<copy>/<module>.webp)
            let thumb = format!("{}/{}.webp", component_thumb_dir(l), c.module);
            let mut d = d;
            if comp_thumbs.join(&thumb).is_file() {
                let u = url_of(&format!("thumbs/components/{thumb}"));
                summary.insert("thumb".into(), json!(u));
                d["thumb"] = json!(u);
            }
            comp_list.push(Value::Object(summary));
            out.models.insert(key, d);
        }
    }
    let component_groups: Vec<Value> = components::GROUPS
        .iter()
        .filter_map(|(g, label)| group_counts.get(*g).map(|n| json!({ "id": g, "label": label, "count": n })))
        .collect();
    let lib_meta = lib.meta();
    let lib_defaults = lib_meta.get("metadata").and_then(Value::as_object).cloned();
    let custom_cats: Map<String, Value> = lib_meta.get("categories").and_then(Value::as_object).cloned().unwrap_or_default();
    let (mut models, mut families, mut libraries, mut sources, mut attention) = (vec![], vec![], vec![], vec![], vec![]);
    let mut profile_uses: Map<String, Value> = Map::new();
    // category -> (model keys, number of parts)
    let mut cats: Vec<(String, Vec<Value>, usize)> = vec![];
    // items the user deleted (kept out of the catalog; Library settings can bring them back)
    let mut removed_items: Vec<Value> = vec![];
    // examples made into parts (F-15) whose part isn't made yet: the app makes them in the background
    let mut examples_todo: Vec<Value> = vec![];
    // a removed category points to the one its items moved to
    let moved: HashMap<String, String> = custom_cats
        .iter()
        .filter_map(|(k, v)| v["moved_to"].as_str().filter(|t| !t.is_empty() && *t != k).map(|t| (k.clone(), t.to_string())))
        .collect();
    let canon = |c: &str| -> String {
        let mut c = if c.is_empty() || c == "parts" { "other".to_string() } else { c.to_string() };
        for _ in 0..8 {
            match moved.get(&c) {
                Some(t) => c = t.clone(),
                None => break,
            }
        }
        c
    };
    let cat_label = |c: &str| custom_cats.get(c).and_then(|x| x["label"].as_str()).map(String::from).unwrap_or_else(|| category_label(c));
    let file_url = |rel: &str| format!("{url}{}", rel.split('/').map(urlencode).collect::<Vec<_>>().join("/"));
    for id in lib.source_ids() {
        let Ok(src) = lib.source(&id) else { continue };
        let version = src["version"].as_str().unwrap_or("").to_string();
        let edits = lib.metadata(&id);
        let det_project = {
            let mut d = obj(&src["detected"]);
            if let Some(Value::Object(o)) = src.get("detected") {
                if o.get("category").is_none() {
                    d.remove("category");
                }
            }
            d
        };
        let derived = if src["kind"] == "pinned" { Some(pinned_derived(lib, &id, &out.models)) } else { lib.derived(&id, &version) };
        let dir = lib.version_dir(&src, &version).ok();
        // where its files are, as a library path: the stored copy, or your own project's folder (local/ or linked)
        let files_rel = match src["kind"].as_str() {
            Some("local" | "linked") => lib.version_dir(&src, &version).ok().and_then(|d| crate::api::folder_rel(lib, &id, &src, &d)),
            _ => None,
        }
        .unwrap_or_else(|| format!("sources/{id}/files/{version}"));
        // a chosen picture (milestone 6): a thumbnail, a picture in its files (None when this version
        // hasn't got it: the render is shown), or one you added
        let pic = |icon: &str| -> Option<String> {
            if let Some(p) = icon.strip_prefix("file:") {
                let rel = library::rel_inside(p).ok()?;
                return dir.as_ref().is_some_and(|d| d.join(&rel).is_file()).then(|| file_url(&format!("{files_rel}/{}", p.trim_start_matches('/'))));
            }
            if let Some(n) = icon.strip_prefix("picture:") {
                library::rel_inside(n).ok()?;
                let there = lib.source_dir(&id).is_ok_and(|d| d.join("pictures").join(n).is_file());
                return there.then(|| file_url(&format!("sources/{id}/pictures/{n}")));
            }
            Some(icon_url(icon, &id, &file_url))
        };
        let mut entry = json!({
            "id": id, "kind": src["kind"], "role": src["role"], "origin": src["origin"],
            "version": version, "version_date": src["version_date"], "added": src["added"],
            "update": src["update"], "read": derived.is_some(),
        });
        if src["kind"] == "bundled" && crate::sources::starter_on_github(&id).is_some() {
            entry["on_github"] = json!(true); // can become a GitHub project (F-1)
        }
        if src["kind"] != "bundled" && src["kind"] != "pinned" && !dir.as_ref().is_some_and(|d| d.is_dir()) {
            let message = match src["kind"].as_str().unwrap_or("") {
                "linked" => format!("The linked folder {} is missing; relink it or remove the project.", src["origin"]["path"].as_str().unwrap_or("")),
                "local" => format!("The folder {} is gone from the library (moved, renamed or deleted); put it back, or remove the project.", src["origin"]["path"].as_str().unwrap_or("")),
                _ => format!("The project's files are missing from the library (sources/{id}/files/{version}); remove the project and add it again."),
            };
            attention.push(json!({ "kind": "missing-folder", "source": id, "message": message }));
        }
        if src["update"]["state"] == "available" {
            attention.push(json!({ "kind": "update", "source": id, "message": "An update is ready to review." }));
        }
        let Some(derived) = derived else {
            entry["name"] = src["detected"]["name"].clone();
            attention.push(json!({ "kind": "unread", "source": id, "message": "Not read yet (or reading failed)." }));
            sources.push(entry);
            continue;
        };
        if project::outdated(&derived) && src["kind"] != "bundled" {
            attention.push(json!({ "kind": "reread", "source": id, "message": "Read with an older version of the app; read it again." }));
        } else if let Some(name) = libraries_changed(&derived, libs) {
            attention.push(json!({ "kind": "libraries", "source": id, "message": format!("Uses {name}, which changed (a new copy, or the other copy is now used); read it again.") }));
        }
        // the project's own resolved metadata
        let fam = &derived["family"];
        let mut det = det_project.clone();
        // a project's own description (workshop.json, or the app's description.json) says what it's called
        // and so on over what was guessed when it was added; your edits still win
        let described = derived["described_by"].as_str().is_some_and(|d| d != "reading");
        for k in ["name", "summary", "license", "authors", "tags", "category"] {
            if described && fam.get(k).is_some_and(|v| !v.is_null() && v.as_str() != Some("")) && (k != "license" || fam[k]["spdx"].as_str().is_some_and(|s| s != "NOASSERTION")) {
                det.insert(k.into(), fam[k].clone());
            } else if det.get(k).is_none_or(|v| v.is_null() || v.as_str() == Some("")) {
                if let Some(v) = fam.get(k).filter(|v| !v.is_null()) {
                    det.insert(k.into(), v.clone());
                }
            }
        }
        if let Some(c) = fam.get("category").filter(|_| det.get("category").is_none()) {
            det.insert("category".into(), c.clone());
        }
        // your own work (local/ or linked) with no license stated, and none set for the whole library, has
        // nothing to check before sharing prints (UI-31)
        let lib_license = lib_defaults.as_ref().is_some_and(|d| d.get("license").is_some_and(|l| !l.is_null()));
        if matches!(src["kind"].as_str(), Some("local" | "linked")) && !lib_license && det.get("license").is_none_or(|l| l["spdx"].as_str().is_none_or(|s| s == "NOASSERTION")) {
            // ("own" so it isn't taken for no license at all; tidying the value drops it again)
            det.insert("license".into(), json!({ "spdx": "NOASSERTION", "public_use": "ok", "own": true }));
        }
        let empty = Map::new();
        let (pvals, pfrom) = meta::resolve(&Layers {
            item: edits.get("project").and_then(Value::as_object),
            folders: vec![],
            project: None,
            detected_item: &det,
            detected_project: &empty,
            library: lib_defaults.as_ref(),
        });
        let pname = pvals.get("name").cloned().unwrap_or(json!(id));
        let pcat = canon(pvals.get("category").and_then(Value::as_str).unwrap_or("other"));
        if pvals.get("hidden").and_then(Value::as_bool) == Some(true) {
            entry["hidden"] = json!(true);
        }
        let project_thumb = edits["project"]["icon"].as_str().and_then(&pic);
        entry["name"] = pname.clone();
        entry["meta"] = Value::Object(pvals.clone());
        entry["from"] = Value::Object(pfrom.clone());
        entry["icon"] = project_thumb.clone().map(Value::String).unwrap_or(Value::Null);
        entry["models"] = json!(derived["models"].as_array().map(|m| m.len()).unwrap_or(0));
        entry["parts"] = json!(derived["parts"].as_array().map(|p| p.len()).unwrap_or(0));
        // a folder of examples (F-15): the project's page asks once what they should be
        entry["examples"] = derived["examples"].clone();
        // models rendered at their defaults (sources/<id>/checks.json), for this version only
        let checks = lib.checks(&id);
        let check_engine = checks["engine"].clone();
        // checked at an earlier version (an update accepted, a local project's files changed): its
        // models are rendered again, and their thumbnails remade, in the background
        let outdated = checks["models"].as_object().is_some_and(|m| !m.is_empty()) && checks["version"] != version.as_str();
        let checks = if checks["version"] == version.as_str() { checks["models"].as_object().cloned().unwrap_or_default() } else { Map::new() };
        let mut problems = derived["problems"].as_array().cloned().unwrap_or_default();
        for (mid, c) in &checks {
            if c["ok"] == false {
                let entry = derived["models"].as_array().into_iter().flatten().find(|m| m["id"] == mid.as_str()).and_then(|m| m["entry"].as_str()).unwrap_or(mid);
                let msg = c["message"].as_str().unwrap_or("OpenSCAD made nothing");
                problems.push(json!({ "kind": "render", "model": format!("{id}/{mid}"), "message": format!("{}: {msg}", entry.trim_start_matches('/')) }));
            }
        }
        entry["problems"] = json!(problems);
        for p in &problems {
            attention.push(json!({ "kind": p["kind"], "source": id, "model": p["model"], "message": p["message"] }));
        }
        if pvals.get("license").and_then(|l| l["public_use"].as_str()).is_some_and(|u| u != "ok") {
            attention.push(json!({ "kind": "license", "source": id, "message": "License not stated, or not cleared for sharing prints." }));
        }
        // blobs for renders
        for (sha, e) in derived["blobs"].as_object().into_iter().flatten() {
            if let Some(p) = e.as_str().and_then(|e| project::blob_path(lib, e, libs.bundled)) {
                out.blobs.insert(sha.clone(), p);
            }
        }
        let thumbs = lib.source_dir(&id).map(|d| d.join("thumbs")).ok();
        let has_thumb = |name: &str| thumbs.as_ref().is_some_and(|t| t.join(name).is_file());
        // models
        let mut model_names = vec![];
        let mut example_parts: Vec<(String, String, Value, String)> = vec![]; // (id, key, name, fingerprint)
        for d in derived["models"].as_array().into_iter().flatten() {
            let mid = d["id"].as_str().unwrap_or("").to_string();
            let key = d["key"].as_str().unwrap_or("").to_string();
            let folder = d["folder"].as_str().unwrap_or("");
            let det_item = obj(&json!({ "name": d["name"], "category": d["item_category"] }));
            let (vals, from) = meta::resolve(&Layers {
                item: edits.get("items").and_then(|i| i.get(&mid)).and_then(Value::as_object),
                folders: meta::folder_chain(&edits, folder),
                project: edits.get("project").and_then(Value::as_object),
                detected_item: &det_item,
                detected_project: &det,
                library: lib_defaults.as_ref(),
            });
            if vals.get("deleted").and_then(Value::as_bool) == Some(true) {
                removed_items.push(json!({ "source": id, "level": "item", "target": mid, "name": vals.get("name").unwrap_or(&d["name"]), "project": pname }));
                continue;
            }
            let mut detail = d.clone();
            apply(&mut detail, &vals, &custom_cats);
            if let Some(form) = vals.get("form").filter(|f| f.is_object()) {
                apply_form_overlay(&mut detail, form);
            }
            let c = canon(detail["category"].as_str().unwrap_or("other"));
            detail["category_label"] = json!(cat_label(&c));
            detail["category"] = json!(c);
            detail["family_name"] = pname.clone();
            detail["from"] = Value::Object(from.clone());
            detail["meta"] = Value::Object(vals.clone());
            detail["source_id"] = json!(id);
            detail["files_id"] = json!(project::files_id(&d["files"]));
            if has_thumb(&format!("{mid}.webp")) {
                detail["thumb"] = json!(file_url(&format!("sources/{id}/thumbs/{mid}.webp")));
            }
            if let Some(icon) = vals.get("icon").and_then(Value::as_str).and_then(&pic) {
                detail["thumb"] = json!(icon);
            }
            // defaults outside their limits (D-3), shown on the model's page too (Review 3, UI-26)
            let limits: Vec<Value> = problems.iter().filter(|p| p["kind"] == "limits" && p["model"] == key.as_str()).flat_map(|p| p["settings"].as_array().cloned().unwrap_or_default()).collect();
            if !limits.is_empty() {
                detail["limits"] = json!(limits);
            }
            if let Some(c) = checks.get(&mid) {
                detail["check"] = json!({ "ok": c["ok"], "message": c["message"], "stale": check_engine != engine });
            } else if outdated {
                detail["check"] = json!({ "outdated": true });
            }
            let mut summary = Map::new();
            for k in ["key", "family", "family_name", "id", "name", "category", "category_label", "summary", "tags", "license", "browser", "updated", "updated_from", "settings", "terms", "thumb", "source_id", "folder", "check", "files_id"] {
                if let Some(v) = detail.get(k).filter(|v| !v.is_null()) {
                    summary.insert(k.into(), v.clone());
                }
            }
            if let Some(f) = vals.get("fields") {
                summary.insert("fields".into(), f.clone());
            }
            if let Some(a) = vals.get("authors") {
                summary.insert("authors".into(), a.clone());
            }
            // your notes on it are searched too (0.4.23)
            if let Some(n) = vals.get("my_notes") {
                summary.insert("my_notes".into(), n.clone());
            }
            flags(&mut summary, &vals, &from, &id, &mut attention, json!({ "model": key }));
            for p in detail["parameters"].as_array().into_iter().flatten() {
                if let Some(profile) = p["profile"].as_str() {
                    let field = profile.split('.').next().unwrap_or(profile).to_string();
                    profile_uses.entry(field).or_insert_with(|| json!([])).as_array_mut().unwrap().push(json!({ "key": key, "param": p["name"] }));
                }
            }
            if detail["example_pending"] == true {
                // one of a folder of examples not yet asked about (F-21): its page opens, but it isn't listed
                // (or checked at its defaults) until the project's page is answered
                out.models.insert(key, detail);
                continue;
            }
            if detail["as_part"] == true {
                // one of a folder of examples made into parts (F-15): listed in the Parts Library once
                // made; its page stays, for making it at other sizes
                example_parts.push((mid.clone(), key.clone(), detail["name"].clone(), detail["files_id"].as_str().unwrap_or("").to_string()));
                out.models.insert(key, detail);
                continue;
            }
            let cat = summary.get("category").and_then(Value::as_str).unwrap_or("other").to_string();
            match cats.iter_mut().find(|(c, _, _)| *c == cat) {
                Some((_, v, _)) => v.push(json!(key)),
                None => cats.push((cat, vec![json!(key)], 0)),
            }
            model_names.push(detail["name"].clone());
            out.models.insert(key, detail);
            models.push(Value::Object(summary));
        }
        // ready-made parts: one pack per project
        let mut parts = derived["parts"].as_array().cloned().unwrap_or_default();
        // parts you made one ("latch_v2.stl" with "latch.3mf", milestone 5): metadata.json's
        // `merged: { <part>: [<parts whose files it takes>] }`, kept across updates
        merge_parts(&mut parts, &edits["merged"]);
        // (less the groups you chose to keep apart, metadata.json's `apart`, UI-35)
        let apart = edits["apart"].as_array().cloned().unwrap_or_default();
        entry["near_misses"] = json!(near_misses(&parts).into_iter().filter(|g| {
            let mut ids: Vec<&str> = g["ids"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            ids.sort();
            !apart.iter().any(|a| *a == json!(ids))
        }).collect::<Vec<_>>());
        // and the examples made into parts: sources/<id>/examples/<fingerprint>/<model id>.stl, made by the
        // app, so only those whose files changed are made again (F-20)
        let examples_rel = format!("sources/{id}/examples");
        let examples_folder = crate::scan::humanize(derived["examples"]["folder"].as_str().unwrap_or("examples").rsplit('/').next().unwrap_or("examples"));
        // the model of the file every example calls, when it has settings of its own (UI-29)
        let examples_main = derived["examples"]["main"].as_str().map(|m| m.trim_start_matches('/'));
        let main_model = derived["models"].as_array().into_iter().flatten()
            .find(|d| examples_main.is_some_and(|m| d["entry"].as_str().map(|e| e.trim_start_matches('/')) == Some(m)) && d["as_part"] != true && d["parameters"].as_array().is_some_and(|p| !p.is_empty()))
            .and_then(|d| d["key"].as_str()).map(String::from);
        for (mid, key, name, fp) in example_parts {
            let rel = format!("{fp}/{mid}.stl");
            match lib.resolve(&format!("{examples_rel}/{rel}")).ok().and_then(|p| std::fs::metadata(p).ok()) {
                Some(m) => parts.push(json!({
                    "id": mid, "name": name, "category": examples_folder, "kind": "printable", "tags": [],
                    "files": [{ "path": rel, "format": "stl", "bytes": m.len() }], "preview": rel,
                    "generator": key, "example": true, "main_model": main_model,
                })),
                // (one that didn't render isn't tried again until its defaults are checked again)
                None if checks.get(&mid).is_some_and(|c| c["ok"] == false) => {}
                None => examples_todo.push(json!({ "key": key, "source": id, "model": mid })),
            }
        }
        if !parts.is_empty() {
            let mut items = vec![];
            let mut part_cats: Vec<String> = vec![];
            for it in parts {
                let pid = it["id"].as_str().unwrap_or("").to_string();
                let folder = it["files"][0]["path"].as_str().map(|p| crate::ingest::dirname(&format!("/{p}")).trim_start_matches('/').to_string()).unwrap_or_default();
                // a made part (milestone 4) is filed under the category of the model it was made from
                let mut det_item = obj(&json!({ "name": it["name"] }));
                if let Some(c) = it["made_from"]["category"].as_str().filter(|c| !c.is_empty()) {
                    det_item.insert("category".into(), json!(c));
                }
                // and carries that project's license (what's made from a project is under its license)
                if let Some(p) = it["made_from"]["project"].as_str().filter(|p| *p != id) {
                    let edited = lib.metadata(p).get("project").and_then(|m| m.get("license")).filter(|l| l.is_object()).cloned();
                    if let Some(l) = edited.or_else(|| lib.source(p).ok().map(|s| s["detected"]["license"].clone()).filter(|l| l.is_object())) {
                        det_item.insert("license".into(), l);
                    }
                }
                let (vals, from) = meta::resolve(&Layers {
                    item: edits.get("parts").and_then(|i| i.get(&pid)).and_then(Value::as_object),
                    folders: meta::folder_chain(&edits, &folder),
                    project: edits.get("project").and_then(Value::as_object),
                    detected_item: &det_item,
                    detected_project: &det,
                    library: lib_defaults.as_ref(),
                });
                if vals.get("deleted").and_then(Value::as_bool) == Some(true) {
                    removed_items.push(json!({ "source": id, "level": "part", "target": pid, "name": vals.get("name").unwrap_or(&it["name"]), "project": pname }));
                    continue;
                }
                let mut it = it.clone();
                if let Some(n) = vals.get("name") {
                    it["name"] = n.clone();
                }
                // the library-wide category (the pack's own "category" is a subcategory: "Connectors", ...)
                let c = canon(vals.get("category").and_then(Value::as_str).unwrap_or(&pcat));
                it["category_label"] = json!(cat_label(&c));
                it["category_id"] = json!(c.clone());
                match cats.iter_mut().find(|(x, _, _)| *x == c) {
                    Some((_, _, n)) => *n += 1,
                    None => cats.push((c, vec![], 1)),
                }
                if let Some(a) = vals.get("authors") {
                    it["authors"] = a.clone();
                }
                if let Some(o) = it.as_object_mut() {
                    flags(o, &vals, &from, &id, &mut attention, json!({ "part": format!("{id}/{pid}") }));
                }
                for k in ["tags", "license", "summary"] {
                    if let Some(v) = vals.get(k) {
                        it[k] = v.clone();
                    }
                }
                if let Some(s) = vals.get("summary") {
                    it["description"] = s.clone();
                }
                it["meta"] = Value::Object(vals.clone());
                it["from"] = Value::Object(from);
                it["folder"] = json!(folder);
                let base = if it["example"] == true { &examples_rel } else { &files_rel };
                for f in it["files"].as_array_mut().into_iter().flatten() {
                    f["url"] = json!(format!("library/{base}/{}", f["path"].as_str().unwrap_or("")));
                }
                if let Some(stl) = it["preview_stl"].as_str() {
                    it["preview_url"] = json!(format!("library/sources/{id}/{stl}"));
                } else if let Some(p) = it["preview"].as_str().filter(|p| p.to_lowercase().ends_with(".stl")) {
                    it["preview_url"] = json!(format!("library/{base}/{p}"));
                } else if let Some(p) = it["preview"].as_str().filter(|p| crate::mesh::readable(p)) {
                    // a 3MF or OBJ, read by the core and handed over as STL (milestone 5)
                    it["preview_url"] = json!(format!("mesh/library/{base}/{p}"));
                }
                if has_thumb(&format!("part-{pid}.webp")) {
                    it["thumb"] = json!(file_url(&format!("sources/{id}/thumbs/part-{pid}.webp")));
                }
                if let Some(icon) = vals.get("icon").and_then(Value::as_str).and_then(&pic) {
                    it["thumb"] = json!(icon);
                }
                if let Some(c) = it["category"].as_str() {
                    if !part_cats.contains(&c.to_string()) {
                        part_cats.push(c.to_string());
                    }
                }
                items.push(it);
            }
            part_cats.sort();
            if !items.is_empty() {
                let pack = json!({
                    "id": id, "name": pname, "summary": pvals.get("summary").cloned().unwrap_or(json!("")),
                    "license": pvals.get("license").cloned().unwrap_or(json!({})), "authors": pvals.get("authors").cloned().unwrap_or(json!([])),
                    "source_url": pvals.get("origin").cloned().unwrap_or(Value::Null), "notes": derived["notes"],
                    "status": "available", "items": items, "item_count": items.len(), "categories": part_cats,
                    "source_id": id, "category": pcat, "category_label": cat_label(&pcat),
                });
                libraries.push(json!({
                    "id": id, "name": pack["name"], "summary": pack["summary"], "item_count": pack["item_count"],
                    "categories": pack["categories"], "license": pack["license"], "authors": pack["authors"], "source_id": id,
                    "category": pcat,
                }));
                out.parts.insert(id.clone(), pack);
            }
        }
        if !model_names.is_empty() {
            families.push(json!({
                "id": id, "name": pname, "status": "available", "category": pcat,
                "license": pvals.get("license").cloned().unwrap_or(json!({})), "authors": pvals.get("authors").cloned().unwrap_or(json!([])),
                "source": pvals.get("origin").cloned().unwrap_or(Value::Null), "models": model_names,
                "summary": pvals.get("summary").cloned().unwrap_or(json!("")), "source_id": id, "icon": project_thumb,
            }));
        }
        out.derived.insert(id.clone(), (derived, dir.unwrap_or_default()));
        sources.push(entry);
    }
    // project overrides ("list this item under another project"), now that every project's name is known
    // (a moved item is hidden with the project it's listed under, not the one it came from)
    let names: HashMap<String, (Value, bool)> =
        sources.iter().filter_map(|s| Some((s["id"].as_str()?.to_string(), (s["name"].clone(), s["hidden"] == true)))).collect();
    let relist = |o: &mut Map<String, Value>, id_key: &str, name_key: &str| {
        let shown = o.shift_remove("shown").is_some(); // "not hidden" set on the item itself
        let Some(t) = o.shift_remove("project_override") else { return };
        let Some((n, hidden)) = t.as_str().and_then(|t| names.get(t)) else { return };
        o.insert(name_key.into(), n.clone());
        o.insert(id_key.into(), t);
        if o.get("hidden").and_then(Value::as_str) == Some("project") {
            o.shift_remove("hidden");
        }
        if *hidden && !shown && !o.contains_key("hidden") {
            o.insert("hidden".into(), json!("project"));
        }
    };
    for m in models.iter_mut() {
        if let Some(o) = m.as_object_mut() {
            relist(o, "family", "family_name");
        }
    }
    for pack in out.parts.values_mut() {
        for it in pack["items"].as_array_mut().into_iter().flatten() {
            if let Some(o) = it.as_object_mut() {
                relist(o, "project_id", "project_name");
            }
        }
    }
    // categories: built-in, found in projects, or made by the user; a removed one points elsewhere
    let builtin = |c: &str| crate::sitebuild::CATEGORY_LABELS.iter().chain(crate::scan::EXTRA_CATEGORIES.iter()).any(|(id, _)| *id == c);
    let order = |c: &str| -> (u8, usize, String) {
        match crate::sitebuild::CATEGORY_LABELS.iter().position(|(id, _)| *id == c) {
            _ if c == "other" => (2, 0, String::new()),
            Some(p) => (0, p, String::new()),
            None => (1, 0, cat_label(c).to_lowercase()),
        }
    };
    cats.sort_by_key(|c| order(&c.0));
    let categories: Vec<Value> = cats
        .iter()
        .map(|(c, keys, parts)| {
            json!({
                "id": c,
                "label": cat_label(c),
                "icon": custom_cats.get(c).and_then(|x| x["icon"].as_str()).map(|i| icon_url(i, "", &file_url)),
                "models": keys,
                "parts": parts,
            })
        })
        .collect();
    let mut ids: Vec<String> = crate::sitebuild::CATEGORY_LABELS
        .iter()
        .chain(crate::scan::EXTRA_CATEGORIES.iter())
        .map(|(id, _)| id.to_string())
        .chain(custom_cats.keys().cloned())
        .chain(cats.iter().map(|c| c.0.clone()))
        .collect();
    ids.dedup();
    let mut seen = std::collections::HashSet::new();
    ids.retain(|c| seen.insert(c.clone()));
    ids.sort_by_key(|c| order(c));
    let count = |c: &str| cats.iter().find(|x| x.0 == c).map(|x| (x.1.len(), x.2)).unwrap_or((0, 0));
    let all_categories: Vec<Value> = ids
        .iter()
        .filter(|c| !moved.contains_key(*c))
        .map(|c| {
            let (m, p) = count(c);
            json!({ "id": c, "label": cat_label(c), "builtin": builtin(c), "models": m, "parts": p,
                    "icon": custom_cats.get(c).and_then(|x| x["icon"].as_str()).map(|i| icon_url(i, "", &file_url)) })
        })
        .collect();
    let removed_categories: Vec<Value> = ids
        .iter()
        .filter_map(|c| moved.get(c).map(|t| json!({ "id": c, "label": cat_label(c), "moved_to": t, "moved_to_label": cat_label(&canon(t)) })))
        .collect();
    // the libraries: which copy is used, and what's bundled
    let mut component_libraries: Vec<Value> = vec![];
    for l in libs.bundled.iter().chain(libs.projects.iter()) {
        let active = libs.effective.iter().any(|e| e.provider == l.provider && e.source_id == l.source_id && e.info.name == l.info.name);
        let n = l.index["components"].as_array().map(|c| c.len()).unwrap_or(0);
        component_libraries.push(json!({
            "name": l.info.name, "title": l.info.title, "summary": l.info.summary, "provider": l.provider, "source_id": l.source_id,
            "repo": l.info.repo, "commit": l.info.commit, "date": l.info.date, "license": l.info.license, "authors": l.info.authors,
            "docs": l.info.docs, "components": n, "active": active,
            "bundled_too": l.provider == "project" && libs.bundled.iter().any(|b| b.info.name.eq_ignore_ascii_case(&l.info.name)),
            "preferred_bundled": libs.prefer_bundled.iter().any(|p| p.eq_ignore_ascii_case(&l.info.name)),
        }));
    }
    out.catalog = json!({
        "components": comp_list,
        "component_groups": component_groups,
        "component_libraries": component_libraries,
        "prefer_bundled": libs.prefer_bundled,
        "engine": engine,
        "common_files": common_files,
        "models": models,
        "categories": categories,
        "category_choices": all_categories,
        "categories_removed": removed_categories,
        "removed_items": removed_items,
        "families": families,
        "libraries": libraries,
        "profile_uses": profile_uses,
        "sources": sources,
        "suggested": suggested(lib),
        "attention": attention,
        "examples_todo": examples_todo,
        "library": lib.info(),
        "library_meta": { "metadata": lib_meta.get("metadata").cloned().unwrap_or(json!({})), "categories": custom_cats },
    });
    out
}

/// "app@6cacf1b7d69a" in words, with {} where the library's name goes: "{}, the app's copy (6cacf1b)",
/// "{}, your copy (a1b2c3d)".
/// Fold the files of parts named in `merged` ({ keep: [others] }) into the part kept, which lists them.
fn merge_parts(parts: &mut Vec<Value>, merged: &Value) {
    for (keep, others) in merged.as_object().into_iter().flatten() {
        let others: Vec<&str> = others.as_array().into_iter().flatten().filter_map(Value::as_str).filter(|o| o != keep).collect();
        if !parts.iter().any(|p| p["id"] == keep.as_str()) {
            continue;
        }
        let mut files = vec![];
        let mut taken = vec![];
        parts.retain(|p| match p["id"].as_str().filter(|id| others.contains(id)) {
            Some(id) => {
                files.extend(p["files"].as_array().cloned().unwrap_or_default());
                taken.push(json!(id));
                false
            }
            None => true,
        });
        if let Some(p) = parts.iter_mut().find(|p| p["id"] == keep.as_str()) {
            if let Some(fs) = p["files"].as_array_mut() {
                fs.extend(files);
            }
            p["merged"] = json!(taken);
            if p["preview"].is_null() {
                p["preview"] = ["3mf", "stl", "obj"].iter().find_map(|x| p["files"].as_array().into_iter().flatten().find(|f| f["format"] == *x)).map(|f| f["path"].clone()).unwrap_or(Value::Null);
            }
        }
    }
}

/// Parts in one folder whose names differ only by a version or copy mark ("latch_v2", "latch (1)",
/// "latch final") and that have no format in common: likely one part, offered to be made one.
fn near_misses(parts: &[Value]) -> Vec<Value> {
    fn base(stem: &str) -> String {
        let mut s = stem.to_lowercase();
        loop {
            let t = s.trim_end_matches([' ', '_', '-', '.']).to_string();
            let word = |w: &&&str| t.ends_with(**w) && (t.len() == w.len() || !t.as_bytes()[t.len() - w.len() - 1].is_ascii_alphanumeric());
            let cut = ["final", "copy", "new"].iter().find(word).map(|w| t.len() - w.len())
                .or_else(|| {
                    // "v2", "v1.3"
                    let i = t.rfind(['v', 'V'])?;
                    let tail = &t[i + 1..];
                    (!tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '.') && (i == 0 || !t.as_bytes()[i - 1].is_ascii_alphanumeric())).then_some(i)
                })
                .or_else(|| {
                    // "(1)"
                    let open = t.strip_suffix(')')?.rfind('(')?;
                    t[open + 1..t.len() - 1].chars().all(|c| c.is_ascii_digit()).then_some(open)
                });
            match cut {
                Some(i) if i > 0 => s = t[..i].to_string(),
                _ => return t,
            }
        }
    }
    let key = |p: &Value| -> Option<(String, String)> {
        let path = p["files"][0]["path"].as_str()?;
        let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
        Some((dir.to_string(), base(file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file))))
    };
    let formats = |p: &Value| -> Vec<String> { p["files"].as_array().into_iter().flatten().filter_map(|f| f["format"].as_str().map(String::from)).collect() };
    let mut out = vec![];
    let mut seen: Vec<String> = vec![];
    for (i, a) in parts.iter().enumerate() {
        let (Some(ka), Some(ida)) = (key(a), a["id"].as_str()) else { continue };
        if seen.iter().any(|s| s == ida) || a["example"] == true {
            continue;
        }
        let mut group = vec![a];
        for b in &parts[i + 1..] {
            if b["example"] != true && key(b).as_ref() == Some(&ka) && formats(b).iter().all(|f| group.iter().all(|g| !formats(g).contains(f))) {
                group.push(b);
            }
        }
        if group.len() > 1 {
            seen.extend(group.iter().filter_map(|g| g["id"].as_str().map(String::from)));
            out.push(json!({
                "ids": group.iter().map(|g| g["id"].clone()).collect::<Vec<_>>(),
                "names": group.iter().map(|g| g["files"][0]["path"].as_str().unwrap_or("").rsplit('/').next().unwrap_or("").to_string()).collect::<Vec<_>>(),
            }));
        }
    }
    out
}

fn copy_text(copy: &str) -> String {
    let (who, ver) = copy.split_once('@').unwrap_or((copy, ""));
    let ver = &ver[..ver.len().min(7)];
    let who = if who == "app" { "{}, the app's copy" } else { "{}, your copy" };
    if ver.is_empty() { who.to_string() } else { format!("{who} ({ver})") }
}

/// Where a library's component thumbnails go in the library folder (under thumbs/components/): one
/// folder per library, one inside it per copy and version, so another copy's pictures aren't shown.
pub fn component_thumb_dir(l: &crate::components::ComponentLibrary) -> String {
    format!("{}/{}", crate::library::slug(&l.info.name), l.copy())
}

/// Thumbnails kept before 0.4.9 (thumbs/components/<library>/<module>.webp, with nothing saying
/// which copy made them) are taken as the copy in use's.
fn adopt_component_thumbs(root: &Path, l: &crate::components::ComponentLibrary) {
    let old = root.join(crate::library::slug(&l.info.name));
    let Ok(rd) = std::fs::read_dir(&old) else { return };
    let loose: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "webp")).collect();
    if loose.is_empty() {
        return;
    }
    let new = root.join(component_thumb_dir(l));
    if std::fs::create_dir_all(&new).is_err() {
        return;
    }
    for p in loose {
        let _ = std::fs::rename(&p, new.join(p.file_name().unwrap()));
    }
}

/// A project whose models include a library from somewhere else than where it would come from now
/// (the bundled copy changed with the app, or the switch between your copy and the bundled one).
fn libraries_changed(derived: &Value, libs: Libs) -> Option<String> {
    for u in derived["libraries_used"].as_array().into_iter().flatten() {
        let name = u["name"].as_str().unwrap_or("");
        let Some(now) = libs.effective.iter().find(|l| l.info.name.eq_ignore_ascii_case(name)) else {
            // not a component library (a library project without modules): only bundled copies can go stale
            if u["provider"] == "bundled" && !libs.bundled.iter().any(|b| b.info.name.eq_ignore_ascii_case(name)) {
                return Some(name.to_string());
            }
            continue;
        };
        let changed = match u["provider"].as_str() {
            Some("bundled") => now.provider != "bundled" || !now.info.commit.as_deref().unwrap_or("").starts_with(u["version"].as_str().unwrap_or("-")),
            Some(src) => now.source_id.as_deref() != Some(src),
            None => false,
        };
        if changed {
            return Some(name.to_string());
        }
    }
    None
}

/// The "Pinned components" project's models: each pin is its component's page with
/// the pin's name, defaults, hidden settings and form changes (shaped like a family
/// manifest's model entry, with "component" instead of "entrypoint").
fn pinned_derived(lib: &Library, id: &str, models: &HashMap<String, Value>) -> Value {
    let dir = lib.source_dir(id).map(|d| d.join("pins")).ok();
    let mut pins: Vec<Value> = dir
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")).filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()).collect())
        .unwrap_or_default();
    pins.sort_by(|a: &Value, b: &Value| a["created"].as_str().cmp(&b["created"].as_str()).then(a["id"].as_str().cmp(&b["id"].as_str())));
    let mut out = vec![];
    let mut problems = vec![];
    for pin in pins {
        let pid = pin["id"].as_str().unwrap_or("").to_string();
        let comp = pin["component"].as_str().unwrap_or("");
        let Some(base) = models.get(comp) else {
            problems.push(json!({ "kind": "missing", "model": format!("{id}/{pid}"), "message": format!("{} is pinned from {comp}, which isn't in any library now.", pin["name"].as_str().unwrap_or(&pid)) }));
            continue;
        };
        let mut d = base.clone();
        // pinned from another copy or version of its library than the one in use now (F-9): marked,
        // with the settings it keeps by name that the library no longer has
        let now = base["component"]["copy"].clone();
        if pin["from"]["copy"].is_string() && pin["from"]["copy"] != now {
            let names: Vec<&str> = base["parameters"].as_array().into_iter().flatten().filter_map(|p| p["name"].as_str()).collect();
            let gone: Vec<String> = ["defaults", "fixed"]
                .iter()
                .flat_map(|k| pin[*k].as_object().into_iter().flatten().map(|(n, _)| n.clone()))
                .chain(pin["hidden"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(String::from)))
                .filter(|n| !names.contains(&n.as_str()))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let library = base["component"]["library"].as_str().unwrap_or("its library");
            let desc = |c: &Value| copy_text(c.as_str().unwrap_or(""));
            let mut message = format!("Pinned with {}; {} is used now.", desc(&pin["from"]["copy"]).replace("{}", library), desc(&now).replace("{}", library));
            if !gone.is_empty() {
                message.push_str(&format!(" Settings it kept that aren't there now: {}.", gone.join(", ")));
            }
            problems.push(json!({ "kind": "pin_library", "model": format!("{id}/{pid}"), "message": format!("{}: {message}", pin["name"].as_str().unwrap_or(&pid)) }));
            d["pin_changed"] = json!({ "message": message, "gone": gone });
        } else if !pin["from"].is_object() && lib.read_only().is_none() {
            // a pin made before 0.4.9: from now on it records the copy in use
            if let Some(p) = lib.source_dir(id).ok().map(|d| d.join("pins").join(format!("{pid}.json"))) {
                let mut pin = pin.clone();
                pin["from"] = json!({ "library": base["component"]["library"], "copy": now });
                let _ = crate::config::write_json(&p, &pin);
            }
        }
        let params: Vec<Value> = d["parameters"].as_array().cloned().unwrap_or_default();
        let groups: Vec<String> = serde_json::from_value(d["groups"].clone()).unwrap_or_default();
        let (params, groups, _) = crate::ingest::apply_form(&params, &groups, &json!({}), &pin, &json!({}));
        d["parameters"] = json!(params);
        d["groups"] = json!(groups);
        d["fixed"] = pin["fixed"].clone();
        d["presets"] = pin["presets"].clone();
        d["needs"] = json!([]);
        d["key"] = json!(format!("{id}/{pid}"));
        d["family"] = json!(id);
        d["id"] = json!(pid);
        d["name"] = pin["name"].clone();
        d["kind"] = json!("pin");
        d["pinned_from"] = json!(comp);
        if let Some(c) = pin["category"].as_str() {
            d["category"] = json!(c);
            d["item_category"] = json!(c);
        }
        if let Some(sm) = pin["summary"].as_str().filter(|s| !s.is_empty()) {
            d["summary"] = json!(sm);
        }
        d["settings"] = json!(params.len());
        d["terms"] = json!(crate::ingest::setting_terms(&params));
        d["folder"] = json!("");
        out.push(d);
    }
    json!({
        "format": project::DERIVED_FORMAT, "source": id, "version": "pins",
        "family": { "id": id, "name": "Pinned components", "category": "other", "summary": "Components pinned as models", "license": { "spdx": "NOASSERTION" }, "authors": [] },
        "models": out, "parts": [], "blobs": {}, "problems": problems,
    })
}

/// The user's flags on an item, for the page: "hidden" (where it was hidden: "item",
/// "project", "folder:<path>"), "broken" ({ note, date }, also listed under Needs
/// attention) and "project_override" (resolved to a name once every project is known).
fn flags(out: &mut Map<String, Value>, vals: &Map<String, Value>, from: &Map<String, Value>, source: &str, attention: &mut Vec<Value>, what: Value) {
    match vals.get("hidden").and_then(Value::as_bool) {
        Some(true) => {
            out.insert("hidden".into(), from.get("hidden").cloned().unwrap_or(json!("item")));
        }
        Some(false) if from.get("hidden").and_then(Value::as_str) == Some("item") => {
            out.insert("shown".into(), json!(true));
        }
        _ => {}
    }
    if let Some(b) = vals.get("broken").filter(|b| !b.is_null() && b.as_bool() != Some(false)) {
        let b = if b.is_object() { b.clone() } else { json!({}) };
        let note = b["note"].as_str().filter(|n| !n.is_empty()).map(|n| format!(": {n}")).unwrap_or_default();
        let name = vals.get("name").and_then(Value::as_str).unwrap_or("");
        let mut a = json!({ "kind": "broken", "source": source, "message": format!("{name} is flagged as broken{note}") });
        for (k, v) in what.as_object().into_iter().flatten() {
            a[k] = v.clone();
        }
        attention.push(a);
        out.insert("broken".into(), b);
    }
    if let Some(t) = vals.get("project").and_then(Value::as_str).filter(|t| !t.is_empty() && *t != source) {
        out.insert("project_override".into(), json!(t));
    }
}

/// The form editor's changes (`form` on an item: a manifest-shaped `{ ui, hidden, defaults,
/// fixed, presets }`), applied with the same code the website build uses for manifests.
/// The page also gets the settings as they were, to edit the form again.
pub fn apply_form_overlay(detail: &mut Value, form: &Value) {
    let base: Vec<Value> = detail["parameters"].as_array().cloned().unwrap_or_default();
    let groups: Vec<String> = serde_json::from_value(detail["groups"].clone()).unwrap_or_default();
    let (params, new_groups, _) = crate::ingest::apply_form(&base, &groups, &json!({}), form, &json!({}));
    detail["base_parameters"] = json!(base);
    detail["base_groups"] = json!(groups);
    detail["parameters"] = json!(params);
    detail["groups"] = json!(new_groups);
    if let Some(fx) = form["fixed"].as_object() {
        let mut fixed = detail["fixed"].as_object().cloned().unwrap_or_default();
        fixed.extend(fx.clone());
        detail["fixed"] = Value::Object(fixed);
    }
    if form["presets"].is_array() {
        detail["presets"] = form["presets"].clone();
    }
    detail["form"] = form.clone();
    let visible = params.iter().filter(|p| !p.get("hidden").is_some_and(crate::ingest::truthy)).count();
    detail["settings"] = json!(visible);
    detail["terms"] = json!(crate::ingest::setting_terms(&params));
}

/// Apply resolved metadata to a model page's JSON.
fn apply(detail: &mut Value, vals: &Map<String, Value>, custom_cats: &Map<String, Value>) {
    for k in ["name", "summary", "tags", "license", "authors", "notes"] {
        if let Some(v) = vals.get(k) {
            detail[k] = v.clone();
        }
    }
    if let Some(c) = vals.get("category").and_then(Value::as_str) {
        detail["category"] = json!(c);
        detail["category_label"] = json!(custom_cats.get(c).and_then(|x| x["label"].as_str()).map(String::from).unwrap_or_else(|| category_label(c)));
    }
    if let Some(o) = vals.get("origin") {
        detail["links"] = json!({ "source": o });
    }
}

/// An icon value: "item:<model or part id>" (that item's thumbnail) or a library path.
fn icon_url(icon: &str, source: &str, file_url: &dyn Fn(&str) -> String) -> String {
    if let Some(item) = icon.strip_prefix("item:") {
        let (src, item) = item.split_once('/').unwrap_or((source, item));
        return file_url(&format!("sources/{src}/thumbs/{item}.webp"));
    }
    file_url(icon.trim_start_matches('/'))
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The blob registry as (sha, path) pairs for the renderer.
pub fn blob_list(c: &Catalog) -> Vec<(String, PathBuf)> {
    c.blobs.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Sources sorted for display.
pub fn sorted_sources(c: &Catalog) -> BTreeMap<String, Value> {
    c.catalog["sources"].as_array().into_iter().flatten().filter_map(|s| Some((s["id"].as_str()?.to_string(), s.clone()))).collect()
}


/// Projects suggested on Home and under Add project (catalog/suggested.json), each with the id of
/// the project already added from the same repository, if there is one.
fn suggested(lib: &Library) -> Value {
    let list: Value = serde_json::from_str(include_str!("../../../catalog/suggested.json")).unwrap_or(json!({}));
    let have: Vec<Value> = lib.source_ids().into_iter().filter_map(|id| lib.source(&id).ok()).collect();
    let out: Vec<Value> = list["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let u = crate::sources::parse_github_url(p["url"].as_str()?).ok()?;
            let added = have.iter().find(|s| {
                s["origin"]["owner"].as_str().is_some_and(|o| o.eq_ignore_ascii_case(&u.owner)) && s["origin"]["repo"].as_str().is_some_and(|r| r.eq_ignore_ascii_case(&u.repo))
                    || s["kind"] == "bundled" && crate::sources::starter_on_github(s["id"].as_str().unwrap_or("")).is_some_and(|c| crate::sources::parse_github_url(&c.repository).is_ok_and(|c| c.owner.eq_ignore_ascii_case(&u.owner) && c.repo.eq_ignore_ascii_case(&u.repo)))
            });
            let mut p = p.clone();
            p["added"] = added.map(|s| s["id"].clone()).unwrap_or(Value::Null);
            Some(p)
        })
        .collect();
    json!(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn near_misses_and_merging() {
        let part = |id: &str, files: &[&str]| json!({ "id": id, "files": files.iter().map(|f| json!({ "path": f, "format": f.rsplit('.').next().unwrap() })).collect::<Vec<_>>() });
        let mut parts = vec![
            part("parts-latch", &["parts/latch.3mf"]),
            part("parts-latch-v2", &["parts/latch_v2.stl"]),
            part("parts-knob", &["parts/knob.stl", "parts/knob.3mf"]),
            part("parts-knob-1", &["parts/knob (1).stl"]),
            part("parts-renew", &["parts/renew.stl"]),
            part("parts-re", &["parts/re.3mf"]),
            part("other-latch", &["other/latch.step"]),
        ];
        let near = near_misses(&parts);
        // latch.3mf + latch_v2.stl; not knob (1).stl (knob has an STL already), renew/re or another folder's latch
        assert_eq!(near, vec![json!({ "ids": ["parts-latch", "parts-latch-v2"], "names": ["latch.3mf", "latch_v2.stl"] })]);
        merge_parts(&mut parts, &json!({ "parts-latch": ["parts-latch-v2"] }));
        assert_eq!(parts.len(), 6);
        assert_eq!(parts[0]["files"].as_array().unwrap().len(), 2);
        assert_eq!(parts[0]["merged"], json!(["parts-latch-v2"]));
        assert!(near_misses(&parts).is_empty());
    }

    use super::*;

    /// The plan's check: a form edit made in the app gives the same settings as the
    /// same `ui` block in a family manifest (website build).
    #[test]
    fn form_edit_matches_a_manifest_ui_block() {
        let raw = json!({ "parameters": [
            { "name": "width", "group": "Size", "initial": 2, "caption": "Width in units", "min": 1, "max": 10, "step": 1 },
            { "name": "size", "group": "Size", "initial": [10, 20, 30], "caption": "Outer size" },
            { "name": "style", "group": "Look", "initial": "round", "options": [{ "name": "Round", "value": "round" }, { "name": "Square", "value": "square" }] },
            { "name": "label", "group": "Look", "initial": true },
            { "name": "text", "group": "Look", "initial": "A1" },
            { "name": "debug", "group": "Look", "initial": false },
        ]});
        let ui = json!({
            "width": { "label": "Width (units)", "max": 8, "step": 0.5 },
            "size": { "axes": ["width", "depth", "height"], "label": "Box size" },
            "text": { "display-condition": { "js": "label" }, "description": "Printed on the front" },
            "style": { "options": [{ "value": "round", "label": "Rounded" }, { "value": "square", "label": "Square corners" }] },
            "label": { "group": "Label" },
        });
        let form = json!({ "ui": ui, "hidden": ["debug"], "defaults": { "width": 3 } });
        let files = BTreeMap::new();
        // website: the block in the manifest
        let fam = json!({ "id": "f", "name": "F", "ui": ui });
        let m = json!({ "id": "m", "name": "M", "hidden": ["debug"], "defaults": { "width": 3 } });
        let (_, site) = crate::sitebuild::assemble_model(&fam, &m, "f/m", "/m.scad", &files, &json!({}), &raw, 0).unwrap();
        // app: the same block as a form edit on a model read without it
        let plain_fam = json!({ "id": "f", "name": "F" });
        let plain_m = json!({ "id": "m", "name": "M" });
        let (_, mut app) = crate::sitebuild::assemble_model(&plain_fam, &plain_m, "f/m", "/m.scad", &files, &json!({}), &raw, 0).unwrap();
        apply_form_overlay(&mut app, &form);
        assert_eq!(site["parameters"], app["parameters"]);
        assert_eq!(site["groups"], app["groups"]);
        assert_eq!(app["groups"], json!(["Size", "Look", "Label"]));
        assert_eq!(app["parameters"].as_array().unwrap().len(), 5);
        assert_eq!(app["parameters"][0]["default"], 3);
        assert_eq!(app["parameters"][0]["step"], 0.5);
        assert_eq!(app["parameters"][4]["name"], "text");
        assert_eq!(app["parameters"][4]["show_if"], "label");
        // the settings as they were, to edit the form again
        assert_eq!(app["base_parameters"].as_array().unwrap().len(), 6);
    }
}