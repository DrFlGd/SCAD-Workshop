//! The catalog part of the website build (tools/build_site.py calls these through
//! `workshop-cli site-prepare` / `site-finish`): for every model in
//! catalog/families, find its files, store them by content hash, and turn
//! OpenSCAD's settings export into the page's model JSON. The model assembly
//! ([`assemble_model`]) is also what the desktop app's library ingest uses, with a
//! family manifest it writes for each project it adds.

use crate::ingest::{self, Resolver};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CATEGORY_LABELS: [(&str, &str); 5] =
    [("gridfinity", "Gridfinity"), ("carrying", "Boxes & Baskets"), ("labels", "Labels"), ("wall", "Wall Storage"), ("other", "Other")];

pub fn category_label(id: &str) -> String {
    CATEGORY_LABELS.iter().find(|(c, _)| *c == id).map(|(_, l)| l.to_string()).unwrap_or_else(|| id.to_string())
}

/// Content-addressed files under <out>/fs/<sha256>.
pub struct Store {
    dir: PathBuf,
    pub bytes: u64,
}

impl Store {
    pub fn new(out: &Path) -> Result<Self> {
        let dir = out.join("fs");
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir, bytes: 0 })
    }
    pub fn put(&mut self, data: &[u8]) -> Result<String> {
        let sha = hex::encode(Sha256::digest(data));
        let p = self.dir.join(&sha);
        if !p.exists() {
            std::fs::write(&p, data)?;
            self.bytes += data.len() as u64;
        }
        Ok(sha)
    }
}

/// SCAD text with Windows line endings normalised: OpenSCAD's Customizer can't
/// read dropdown lists otherwise (the files on disk stay untouched).
pub fn packaged_bytes(path: &Path) -> Result<Vec<u8>> {
    let data = std::fs::read(path).with_context(|| format!("couldn't read {}", path.display()))?;
    let scad = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("scad"));
    Ok(if scad { replace_crlf(&data) } else { data })
}

pub fn replace_crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\r' && data.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

/// One model waiting for its settings to be read.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedModel {
    pub key: String,
    pub family_file: String,
    pub model_id: String,
    pub entry: String,
    pub files: BTreeMap<String, String>,
    pub editor: Value,
    pub browser: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Plan {
    pub common_files: BTreeMap<String, String>,
    pub families: Vec<Value>,
    pub models: Vec<PlannedModel>,
    pub problems: Vec<String>,
}

pub struct PrepareOptions {
    pub public: bool,
    pub hide_blocked: bool,
    /// Include "browser": false models (the desktop build).
    pub desktop: bool,
}

/// The common files every model gets: a fontconfig file and the bundled fonts.
pub fn common_files(repo: &Path, store: &mut Store) -> Result<BTreeMap<String, String>> {
    let mut common = BTreeMap::new();
    common.insert(
        "/fonts/fonts.conf".to_string(),
        store.put(b"<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n<fontconfig><dir>/fonts</dir><cachedir>/tmp/fontconfig</cachedir></fontconfig>\n")?,
    );
    let mut fonts: Vec<PathBuf> = std::fs::read_dir(repo.join("assets/fonts"))
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "ttf")).collect())
        .unwrap_or_default();
    fonts.sort();
    for f in fonts {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        common.insert(format!("/fonts/{name}"), store.put(&std::fs::read(&f)?)?);
    }
    Ok(common)
}

/// Read catalog/families, store every model's files in <out>/fs and write the
/// provisional data the settings export needs (catalog with common files, model stubs).
pub fn prepare(repo: &Path, out: &Path, opts: &PrepareOptions) -> Result<Plan> {
    let mut store = Store::new(out)?;
    let common = common_files(repo, &mut store)?;
    let mut fam_files: Vec<PathBuf> = std::fs::read_dir(repo.join("catalog/families"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    fam_files.sort();
    let (mut families, mut models, mut problems) = (vec![], vec![], vec![]);
    for ff in fam_files {
        let fam: Value = serde_json::from_slice(&std::fs::read(&ff)?).with_context(|| format!("{} isn't valid JSON", ff.display()))?;
        if fam["status"] == "disabled" {
            continue;
        }
        let public_use = fam["license"]["public_use"].as_str().unwrap_or("ok");
        if (opts.public && public_use != "ok") || (opts.hide_blocked && public_use == "blocked") {
            continue;
        }
        let mut entry = family_entry(&fam);
        match crate::docs::family_docs(repo, &fam, out) {
            Ok(d) => entry["docs"] = json!(d),
            Err(e) => problems.push(format!("{}: README: {e:#}", fam["id"].as_str().unwrap_or(""))),
        }
        families.push(entry);
        let editor_cfg = match fam["editor_toml"].as_str() {
            Some(p) => ingest::load_editor_toml(&repo.join(p))?,
            None => json!({}),
        };
        let libs: Vec<PathBuf> = fam["engine"]["library_paths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(|p| repo.join(p)))
            .collect();
        let resolver = Resolver { root: repo.to_path_buf(), libraries: libs };
        for m in fam["models"].as_array().into_iter().flatten() {
            let Some(entrypoint) = m["entrypoint"].as_str().filter(|e| !e.is_empty()) else { continue };
            if m["status"] != "available" {
                continue;
            }
            let browser = m["browser"] != Value::Bool(false);
            if !browser && !opts.desktop {
                continue; // these crash the WebAssembly engine: desktop app only
            }
            let key = format!("{}/{}", fam["id"].as_str().unwrap_or(""), m["id"].as_str().unwrap_or(""));
            let (files, missing) = ingest::collect_files(&resolver, entrypoint);
            if !missing.is_empty() {
                problems.push(format!("{key}: unresolved {missing:?}"));
            }
            let mut fmap = BTreeMap::new();
            for (v, h) in &files {
                fmap.insert(v.clone(), store.put(&packaged_bytes(h)?)?);
            }
            let model_file = m["editor_model"].as_str().map(String::from).unwrap_or_else(|| {
                Path::new(entrypoint).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            });
            models.push(PlannedModel {
                key,
                family_file: ff.file_name().unwrap().to_string_lossy().into_owned(),
                model_id: m["id"].as_str().unwrap_or("").to_string(),
                entry: format!("/{entrypoint}"),
                files: fmap,
                editor: ingest::editor_model_meta(&editor_cfg, &model_file),
                browser,
            });
        }
    }
    let data = out.join("data/models");
    std::fs::create_dir_all(&data)?;
    std::fs::write(out.join("data/catalog.json"), serde_json::to_vec(&json!({ "common_files": common, "models": [] }))?)?;
    for m in &models {
        let stub = json!({ "entry": m.entry, "files": m.files, "parameters": [] });
        std::fs::write(data.join(format!("{}.json", m.key.replace('/', "--"))), serde_json::to_vec(&stub)?)?;
    }
    Ok(Plan { common_files: common, families, models, problems })
}

/// The catalog's list entry for a family.
pub fn family_entry(fam: &Value) -> Value {
    let source = fam["source"]["repository"]
        .as_str()
        .map(|s| json!(s))
        .or_else(|| fam["links"].as_object().and_then(|l| l.values().next().cloned()))
        .unwrap_or(Value::Null);
    let models: Vec<Value> = fam["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["status"] == "available")
        .map(|m| m["name"].clone())
        .collect();
    json!({
        "id": fam["id"], "name": fam["name"], "status": fam["status"], "category": fam["category"],
        "license": or_obj(&fam["license"]), "authors": or_arr(&fam["authors"]),
        "source": source, "models": models,
    })
}

fn or_obj(v: &Value) -> Value {
    if v.is_null() { json!({}) } else { v.clone() }
}
fn or_arr(v: &Value) -> Value {
    if v.is_null() { json!([]) } else { v.clone() }
}

/// Everything a model page needs, from a family manifest, one of its models,
/// the model's files and OpenSCAD's settings export. Returns (catalog summary, model detail).
#[allow(clippy::too_many_arguments)]
pub fn assemble_model(
    fam: &Value,
    m: &Value,
    key: &str,
    entry: &str,
    files: &BTreeMap<String, String>,
    editor: &Value,
    raw: &Value,
    input_bytes: u64,
) -> Result<(Value, Value)> {
    let (mut params, mut groups) = ingest::convert_params(raw);
    // site-declared settings for values OpenSCAD's Customizer can't expose
    // (computed with an expression in the file); the page passes these with -D
    for extra in m["extra_params"].as_array().into_iter().flatten() {
        let mut e = Map::new();
        e.insert("type".into(), json!("number"));
        e.insert("widget".into(), json!("number"));
        e.insert("description".into(), Value::Null);
        for (k, v) in extra.as_object().into_iter().flatten() {
            e.insert(k.clone(), v.clone());
        }
        e.insert("define".into(), json!(true));
        let after = e.shift_remove("after");
        let at = after.as_ref().and_then(|a| params.iter().position(|p| &p["name"] == a));
        let group = e.get("group").and_then(Value::as_str).unwrap_or("Parameters").to_string();
        params.insert(at.map(|i| i + 1).unwrap_or(params.len()), Value::Object(e));
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    let (params, groups, tabs) = ingest::apply_form(&params, &groups, fam, m, editor);
    let category = fam["category"].as_str().unwrap_or("other");
    let mut summary = Map::new();
    summary.insert("key".into(), json!(key));
    summary.insert("family".into(), fam["id"].clone());
    summary.insert("family_name".into(), fam["name"].clone());
    summary.insert("id".into(), m["id"].clone());
    summary.insert("name".into(), m["name"].clone());
    summary.insert("category".into(), json!(category));
    summary.insert(
        "category_label".into(),
        json!(fam["category_label"].as_str().map(String::from).unwrap_or_else(|| category_label(category))),
    );
    summary.insert("summary".into(), json!(fam["summary"].as_str().unwrap_or("")));
    summary.insert("tags".into(), or_arr(&fam["tags"]));
    summary.insert("license".into(), or_obj(&fam["license"]));
    if m["browser"] == Value::Bool(false) {
        summary.insert("browser".into(), json!(false));
    }
    // when the pinned upstream version was made, or when supplied files were added
    let src = &fam["source"];
    if let Some(d) = src["commit_date"].as_str() {
        summary.insert("updated".into(), json!(d.chars().take(10).collect::<String>()));
        summary.insert("updated_from".into(), json!("upstream"));
    } else if let Some(d) = src["supplied_by"].as_str().and_then(find_date) {
        summary.insert("updated".into(), json!(d));
        summary.insert("updated_from".into(), json!("supplied"));
    }
    let visible = params.iter().filter(|p| !p.get("hidden").is_some_and(ingest::truthy)).count();
    summary.insert("settings".into(), json!(visible));
    summary.insert("terms".into(), json!(ingest::setting_terms(&params)));
    let mut detail = summary.clone();
    detail.insert("authors".into(), or_arr(&fam["authors"]));
    detail.insert("links".into(), or_obj(&fam["links"]));
    detail.insert("source".into(), fam["source"].clone());
    detail.insert("notes".into(), m["notes"].clone());
    detail.insert("part_parameter".into(), m["part_parameter"].clone());
    detail.insert(
        "description_html".into(),
        ingest::clean_html(editor["description-extra-html"].as_str()).map(Value::String).unwrap_or(Value::Null),
    );
    detail.insert("entry".into(), json!(entry));
    detail.insert("files".into(), serde_json::to_value(files)?);
    detail.insert("fixed".into(), or_obj(&m["fixed"]));
    detail.insert("groups".into(), json!(groups));
    detail.insert("tabs".into(), tabs);
    detail.insert("parameters".into(), Value::Array(params));
    detail.insert("presets".into(), or_arr(&m["presets"]));
    detail.insert("input_bytes".into(), json!(input_bytes));
    Ok((Value::Object(summary), Value::Object(detail)))
}

/// The first YYYY-MM-DD in a string.
pub fn find_date(s: &str) -> Option<String> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(9)).find_map(|i| {
        let w = &b[i..i + 10];
        let ok = w.iter().enumerate().all(|(j, c)| if j == 4 || j == 7 { *c == b'-' } else { c.is_ascii_digit() });
        ok.then(|| String::from_utf8_lossy(w).into_owned())
    })
}

/// Turn the settings exports into model JSON and the final catalog (libraries are
/// added afterwards by build_site.py). Returns the problems found.
pub fn finish(repo: &Path, out: &Path, plan: &Plan, raw: &Map<String, Value>, engine_version: &str) -> Result<Vec<String>> {
    let mut problems = plan.problems.clone();
    let mut listing: Vec<Value> = vec![];
    let mut profile_uses: Map<String, Value> = Map::new();
    let mut fam_cache: BTreeMap<String, Value> = BTreeMap::new();
    for pm in &plan.models {
        let fam = match fam_cache.get(&pm.family_file) {
            Some(f) => f.clone(),
            None => {
                let f: Value = serde_json::from_slice(&std::fs::read(repo.join("catalog/families").join(&pm.family_file))?)?;
                fam_cache.insert(pm.family_file.clone(), f.clone());
                f
            }
        };
        let m = fam["models"].as_array().into_iter().flatten().find(|m| m["id"] == pm.model_id).cloned().unwrap_or(json!({}));
        let r = raw.get(&pm.key).cloned().unwrap_or(json!({ "error": "no settings export" }));
        if let Some(e) = r.get("error") {
            problems.push(format!("{}: parameter export failed\n{}", pm.key, e.as_str().unwrap_or(&e.to_string())));
            continue;
        }
        let input_bytes: u64 = pm.files.values().filter_map(|s| std::fs::metadata(out.join("fs").join(s)).ok()).map(|m| m.len()).sum();
        let (summary, detail) = assemble_model(&fam, &m, &pm.key, &pm.entry, &pm.files, &pm.editor, &r, input_bytes)?;
        std::fs::write(out.join("data/models").join(format!("{}.json", pm.key.replace('/', "--"))), serde_json::to_vec(&detail)?)?;
        for p in detail["parameters"].as_array().into_iter().flatten() {
            if let Some(profile) = p["profile"].as_str() {
                let field = profile.split('.').next().unwrap_or(profile).to_string();
                let slot = profile_uses.entry(field).or_insert_with(|| json!([]));
                slot.as_array_mut().unwrap().push(json!({ "key": pm.key, "param": p["name"] }));
            }
        }
        listing.push(summary);
    }
    let mut cats: Vec<(String, Vec<Value>)> = vec![];
    for s in &listing {
        let c = s["category"].as_str().unwrap_or("other").to_string();
        match cats.iter_mut().find(|(id, _)| *id == c) {
            Some((_, v)) => v.push(s["key"].clone()),
            None => cats.push((c, vec![s["key"].clone()])),
        }
    }
    let order = |c: &str| CATEGORY_LABELS.iter().position(|(id, _)| *id == c).unwrap_or(99);
    cats.sort_by_key(|c| order(&c.0)); // stable: unknown categories keep their first-seen order
    let catalog = json!({
        "engine": engine_version,
        "common_files": plan.common_files,
        "models": listing,
        "categories": cats.iter().map(|(c, keys)| json!({ "id": c, "label": category_label(c), "models": keys })).collect::<Vec<_>>(),
        "families": plan.families,
        "libraries": [],
        "profile_uses": profile_uses,
    });
    std::fs::write(out.join("data/catalog.json"), serde_json::to_vec(&catalog)?)?;
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_line_endings() {
        assert_eq!(find_date("owner, 2026-10-03 ZIP").as_deref(), Some("2026-10-03"));
        assert_eq!(find_date("no date"), None);
        assert_eq!(replace_crlf(b"a\r\nb\rc\r\n"), b"a\nb\rc\n");
    }
}
