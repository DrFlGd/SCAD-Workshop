//! Reading OpenSCAD projects: which files a model needs, its Customizer
//! settings, and the metadata layered on top of them (the upstream project's
//! editor.toml and the catalog's own `ui` overlays).
//!
//! Written for the website's build and kept for the desktop app's reading of
//! projects (project.rs).

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// ------------------------------------------------------------------ paths

/// posixpath.normpath for "/"-separated virtual paths.
pub fn normpath(p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let absolute = p.starts_with('/');
    let mut out: Vec<&str> = vec![];
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..");
                }
            }
            x => out.push(x),
        }
    }
    let joined = out.join("/");
    match (absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".into(),
        (false, false) => joined,
    }
}

/// posixpath.dirname
pub fn dirname(p: &str) -> &str {
    match p.rfind('/') {
        Some(0) => "/",
        Some(i) => &p[..i],
        None => "",
    }
}

/// posixpath.join for two parts.
pub fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') || a.is_empty() {
        b.to_string()
    } else if a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// Find `rel` under `base`, ignoring case where an exact match is missing, as
/// Windows and macOS do (upstream files are sometimes written with mismatched case).
pub fn find_nocase(base: &Path, rel: &str) -> Option<PathBuf> {
    let mut cur = base.to_path_buf();
    for part in rel.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                cur = cur.parent()?.to_path_buf();
                continue;
            }
            _ => {}
        }
        let direct = cur.join(part);
        if direct.exists() {
            cur = direct;
            continue;
        }
        if !cur.is_dir() {
            return None;
        }
        let lower = part.to_lowercase();
        let hit = std::fs::read_dir(&cur)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase() == lower))
            .min()?;
        cur = hit;
    }
    cur.is_file().then_some(cur)
}

/// Where a model's virtual paths live on disk: "/" is the project root, and
/// "/libraries/<x>" is looked up in each library folder in turn (OPENSCADPATH).
#[derive(Clone, Debug)]
pub struct Resolver {
    pub root: PathBuf,
    pub libraries: Vec<PathBuf>,
}

impl Resolver {
    pub fn host(&self, v: &str) -> Option<PathBuf> {
        if let Some(rel) = v.strip_prefix("/libraries/") {
            for lib in &self.libraries {
                let p = lib.join(rel);
                if p.is_file() {
                    return Some(p);
                }
            }
            return self.libraries.iter().find_map(|lib| find_nocase(lib, rel));
        }
        let rel = v.trim_start_matches('/');
        if rel.split('/').any(|c| c == "..") {
            return None; // stays inside the project
        }
        let p = self.root.join(rel);
        if p.is_file() {
            Some(p)
        } else {
            find_nocase(&self.root, rel)
        }
    }
}

fn include_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(?:include|use)\s*<([^>]+)>").unwrap())
}
fn import_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\b(?:import|surface)\s*\(\s*(?:file\s*=\s*)?"([^"]+)""#).unwrap())
}
fn block_comment_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)/\*.*?\*/").unwrap())
}

/// SCAD source with comments blanked out (line numbers kept).
pub fn strip_comments(text: &str) -> String {
    let no_blocks = block_comment_re().replace_all(text, |c: &regex::Captures| "\n".repeat(c[0].matches('\n').count()));
    no_blocks.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

/// References in one SCAD file: (include/use targets, import()/surface() files).
pub fn references(text: &str) -> (Vec<String>, Vec<String>) {
    let (mut uses, mut imports) = (vec![], vec![]);
    for line in strip_comments(text).lines() {
        if let Some(c) = include_re().captures(line) {
            uses.push(c[1].trim().to_string());
            continue;
        }
        for c in import_re().captures_iter(line) {
            imports.push(c[1].to_string());
        }
    }
    (uses, imports)
}

/// Files a model needs, by following include/use/import from its entry file as
/// OpenSCAD does: next to the including file first, then the library folders.
/// Returns {virtual path: host path} and the references that couldn't be found.
pub fn collect_files(resolver: &Resolver, entry: &str) -> (BTreeMap<String, PathBuf>, Vec<String>) {
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut missing: BTreeSet<String> = BTreeSet::new();
    let mut todo = vec![format!("/{}", entry.trim_start_matches('/'))];
    while let Some(v) = todo.pop() {
        if files.contains_key(&v) {
            continue;
        }
        let Some(h) = resolver.host(&v) else {
            missing.insert(v);
            continue;
        };
        let is_scad = h.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("scad"));
        files.insert(v.clone(), h.clone());
        if !is_scad {
            continue;
        }
        let text = String::from_utf8_lossy(&std::fs::read(&h).unwrap_or_default()).into_owned();
        let base = dirname(&v).to_string();
        let (uses, imports) = references(&text);
        for r in uses {
            let rel = normpath(&join(&base, &r));
            if resolver.host(&rel).is_some() {
                todo.push(rel);
            } else {
                todo.push(format!("/libraries/{}", normpath(&r)));
            }
        }
        for i in imports {
            let rel = normpath(&join(&base, &i));
            if resolver.host(&rel).is_some() {
                todo.push(rel);
            }
        }
    }
    (files, missing.into_iter().collect())
}

// ------------------------------------------------------------------ JSON helpers

/// Numbers compare by value (OpenSCAD writes 3 and 3.0 interchangeably).
pub fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_eq(p, q)),
        _ => a == b,
    }
}

/// 3.0 -> 3 (OpenSCAD's parameter export writes whole numbers as floats).
pub fn int_if_whole(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => {
            let f = n.as_f64().unwrap_or(0.0);
            if f.fract() == 0.0 && f.abs() < 9.0e15 {
                json!(f as i64)
            } else {
                v.clone()
            }
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| if x.is_array() { x.clone() } else { int_if_whole(x) }).collect()),
        _ => v.clone(),
    }
}

fn is_number(v: &Value) -> bool {
    v.is_number()
}

/// Python's str() for a JSON scalar (labels made from option values).
fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Null => "None".into(),
        other => other.to_string(),
    }
}

// ------------------------------------------------------------------ parameters

/// OpenSCAD's `--export-format=param` output -> the site's parameter list and group order.
pub fn convert_params(raw: &Value) -> (Vec<Value>, Vec<String>) {
    let mut params = vec![];
    let mut groups: Vec<String> = vec![];
    for p in raw.get("parameters").and_then(Value::as_array).into_iter().flatten() {
        let name = p.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let group = p.get("group").and_then(Value::as_str).filter(|g| !g.is_empty()).unwrap_or("Parameters").to_string();
        if group.to_lowercase() == "hidden" || name.starts_with('$') {
            continue; // $fn/$fa/$fs quality knobs stay at the model's defaults
        }
        let initial = p.get("initial").cloned().unwrap_or(Value::Null);
        let caption = p.get("caption").and_then(Value::as_str).map(str::trim).filter(|c| !c.is_empty());
        let mut out = Map::new();
        out.insert("name".into(), json!(name));
        out.insert("group".into(), json!(group));
        out.insert("description".into(), caption.map(|c| json!(c)).unwrap_or(Value::Null));
        out.insert("default".into(), initial.clone());
        let (ty, widget) = match &initial {
            Value::Bool(_) => ("boolean", "checkbox"),
            Value::Array(a) => (if a.iter().all(is_number) { "number[]" } else { "list" }, "vector"),
            Value::Number(_) => ("number", "number"),
            _ => ("string", "text"),
        };
        out.insert("type".into(), json!(ty));
        out.insert("widget".into(), json!(widget));
        for k in ["min", "max", "step"] {
            if let Some(v) = p.get(k).filter(|v| !v.is_null()) {
                out.insert(k.into(), v.clone());
            }
        }
        let options = p.get("options").and_then(Value::as_array).filter(|o| !o.is_empty());
        if let Some(opts) = options {
            out.insert("widget".into(), json!("dropdown"));
            let mut list: Vec<Value> = opts
                .iter()
                .map(|o| {
                    let value = o.get("value").cloned().unwrap_or(Value::Null);
                    let label = o.get("name").map(py_str).unwrap_or_else(|| py_str(&value));
                    json!({ "value": value, "label": label })
                })
                .collect();
            if list.iter().all(|o| !json_eq(&o["value"], &initial)) {
                // kept as a choice so the form can show it; reported by `limit_problems`
                list.insert(0, json!({ "value": initial, "label": py_str(&initial), "added": true }));
            }
            out.insert("options".into(), Value::Array(list));
        } else if ty == "number" && out.contains_key("max") && out.contains_key("min") {
            out.insert("widget".into(), json!("slider"));
        } else if ty == "number" && !out.contains_key("min") && !out.contains_key("max") {
            // a number without a Customizer comment: the export says step 1, but OpenSCAD's own
            // Customizer steps by the default's last decimal (1.5 -> 0.1)
            if let (Some(d), Some(s)) = (initial.as_f64(), out.get("step").and_then(Value::as_f64)) {
                if s == 1.0 && d.fract() != 0.0 {
                    out.insert("step".into(), json!(decimal_step(d)));
                }
            }
        }
        for k in ["default", "min", "max", "step"] {
            if let Some(v) = out.get(k).cloned() {
                out.insert(k.into(), int_if_whole(&v));
            }
        }
        if let Some(Value::Array(opts)) = out.get_mut("options") {
            for o in opts.iter_mut() {
                let v = o["value"].clone();
                if v.is_f64() {
                    o["value"] = int_if_whole(&v);
                }
            }
        }
        params.push(Value::Object(out));
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    (params, groups)
}

/// Choices read from the code (0.4.24): a text setting without choices of its own becomes a drop-down
/// when the files compare it with two or more different words (`style == "round"`, `"flat" != style`).
/// The empty word doesn't count, and the default stays a choice. Returns the names it changed.
pub fn choices_from_code(params: &mut [Value], texts: &[String]) -> Result<Vec<String>> {
    let code: Vec<String> = texts.iter().map(|t| strip_comments(t)).collect();
    let mut changed = vec![];
    for p in params.iter_mut() {
        let plain_text = p["type"] == "string" && p["widget"] == "text" && p.get("options").is_none_or(Value::is_null) && p["typed_in"] != true;
        let Some(name) = p["name"].as_str().filter(|_| plain_text).map(String::from) else { continue };
        let n = regex::escape(&name);
        let word = r#""((?:[^"\\]|\\.)*)""#;
        let re = Regex::new(&format!(r"(?:\b{n}\s*(?:==|!=)\s*{word})|(?:{word}\s*(?:==|!=)\s*{n}\b)"))
            .with_context(|| format!("reading {name}'s comparisons"))?;
        let mut words: Vec<String> = vec![];
        for c in code.iter().flat_map(|t| re.captures_iter(t)) {
            let w = c.get(1).or_else(|| c.get(2)).map(|m| m.as_str().to_string()).unwrap_or_default();
            if !w.is_empty() && !words.contains(&w) {
                words.push(w);
            }
        }
        if words.len() < 2 {
            continue;
        }
        if let Some(d) = p["default"].as_str().filter(|d| !d.is_empty() && !words.iter().any(|w| w == d)) {
            words.insert(0, d.to_string());
        }
        p["widget"] = json!("dropdown");
        p["options"] = json!(words.iter().map(|w| json!({ "value": w, "label": w })).collect::<Vec<_>>());
        p["choices_from"] = json!("code");
        changed.push(name);
    }
    Ok(changed)
}

/// The same step rule for settings read by an app before 0.4.6 (Review 3, F-13): a plain number whose
/// step of 1 doesn't fit its fractional default steps by its last decimal. Returns whether any changed.
pub fn fit_plain_steps(params: &mut [Value]) -> bool {
    let mut changed = false;
    for p in params.iter_mut() {
        if p["type"] != "number" || !p["min"].is_null() || !p["max"].is_null() || p["options"].is_array() {
            continue;
        }
        if let (Some(d), Some(s)) = (p["default"].as_f64(), p["step"].as_f64()) {
            if s == 1.0 && d.fract() != 0.0 {
                p["step"] = int_if_whole(&json!(decimal_step(d)));
                changed = true;
            }
        }
    }
    changed
}

/// The step of a number's last decimal: 1.5 -> 0.1, 0.25 -> 0.01.
pub fn decimal_step(v: f64) -> f64 {
    let s = format!("{v}");
    let places = s.split_once('.').map(|(_, f)| f.len().min(6)).unwrap_or(0) as i32;
    let step = 10f64.powi(-places);
    format!("{step:.*}", places as usize).parse().unwrap_or(step)
}

/// Whether `v` is a whole number of `step`s from `base` (allowing for floating point).
pub fn on_step(v: f64, base: f64, step: f64) -> bool {
    if step <= 0.0 {
        return true;
    }
    let n = (v - base) / step;
    (n - n.round()).abs() < 1e-6
}

fn show_num(v: f64) -> String {
    let s = format!("{v}");
    s.strip_suffix(".0").map(String::from).unwrap_or(s)
}

/// Settings whose default doesn't fit their own limits (below the least, above the most, off the
/// step, not one of its choices), one message each: "Width units: 1.5 isn't a step of 1 from 0".
pub fn limit_problems(params: &[Value]) -> Vec<String> {
    let mut out = vec![];
    for p in params {
        let name = p["label"].as_str().map(String::from).unwrap_or_else(|| crate::scan::humanize(p["name"].as_str().unwrap_or("")));
        if let Some(opts) = p["options"].as_array() {
            if opts.iter().any(|o| o["added"] == true) {
                out.push(format!("{name}: its default, {}, isn't one of its choices", py_str(&p["default"])));
            }
            continue;
        }
        if p["type"] != "number" && p["type"] != "number[]" {
            continue;
        }
        let values: Vec<f64> = match &p["default"] {
            Value::Number(n) => n.as_f64().into_iter().collect(),
            Value::Array(a) => a.iter().filter_map(Value::as_f64).collect(),
            _ => continue,
        };
        let (min, max, step) = (p["min"].as_f64(), p["max"].as_f64(), p["step"].as_f64());
        for v in values {
            if let Some(m) = min.filter(|m| v < *m) {
                out.push(format!("{name}: {} is below its least, {}", show_num(v), show_num(m)));
            } else if let Some(m) = max.filter(|m| v > *m) {
                out.push(format!("{name}: {} is above its most, {}", show_num(v), show_num(m)));
            } else if let Some(s) = step.filter(|s| !on_step(v, min.unwrap_or(0.0), *s)) {
                out.push(format!("{name}: {} isn't a step of {} from {}", show_num(v), show_num(s), show_num(min.unwrap_or(0.0))));
            } else {
                continue;
            }
            break;
        }
    }
    out
}

// ------------------------------------------------------------------ editor.toml

/// Parse an editor.toml (web-openscad-editor format) into JSON, keeping key order.
pub fn load_editor_toml(path: &Path) -> Result<Value> {
    if !path.is_file() {
        return Ok(Value::Object(Map::new()));
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("couldn't read {}", path.display()))?;
    let table: toml::Table = text.parse().with_context(|| format!("{} isn't valid TOML", path.display()))?;
    Ok(serde_json::to_value(table)?)
}

fn as_name_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        _ => vec![],
    }
}

/// Merge the templates (default + named) and the [[model]] entry for one model file.
/// Returns {} when the file has no entry.
pub fn editor_model_meta(cfg: &Value, model_file: &str) -> Value {
    let Some(obj) = cfg.as_object().filter(|o| !o.is_empty()) else { return json!({}) };
    let empty = Map::new();
    let templates = obj.get("model-template").and_then(Value::as_object).unwrap_or(&empty);

    fn resolve<'a>(templates: &'a Map<String, Value>, name: &str, seen: &mut Vec<String>) -> Vec<&'a Value> {
        let Some(t) = templates.get(name) else { return vec![] };
        let mut chain = vec![];
        for parent in as_name_list(t.get("template")) {
            if !seen.contains(&parent) {
                seen.push(name.to_string());
                chain.extend(resolve(templates, &parent, seen));
                seen.pop();
            }
        }
        chain.push(t);
        chain
    }

    for model in obj.get("model").and_then(Value::as_array).into_iter().flatten() {
        if model.get("file").and_then(Value::as_str) != Some(model_file) {
            continue;
        }
        let mut names = match model.get("template") {
            None => vec!["default".to_string()],
            v => as_name_list(v),
        };
        if !names.iter().any(|n| n == "default") && model.get("template").is_none() {
            names.insert(0, "default".into());
        }
        let mut layers: Vec<&Value> = vec![];
        for n in &names {
            layers.extend(resolve(templates, n, &mut vec![]));
        }
        // web-openscad-editor applies "section-" templates to models that contain the
        // referenced tab; include them generically (they only add tab metadata)
        layers.extend(templates.iter().filter(|(n, _)| n.starts_with("section-")).map(|(_, t)| t));
        layers.push(model);
        let mut merged = Map::new();
        let mut pmeta: Vec<Value> = vec![];
        let mut tmeta = Map::new();
        for layer in layers {
            for (k, v) in layer.as_object().into_iter().flatten() {
                match k.as_str() {
                    "param-metadata" => {
                        for (pattern, md) in v.as_object().into_iter().flatten() {
                            pmeta.push(json!([pattern, md]));
                        }
                    }
                    "tab-metadata" => {
                        for (tab, meta) in v.as_object().into_iter().flatten() {
                            let slot = tmeta.entry(tab.clone()).or_insert_with(|| json!({}));
                            for (mk, mv) in meta.as_object().into_iter().flatten() {
                                slot[mk] = mv.clone();
                            }
                        }
                    }
                    "file" | "template" => {}
                    _ => {
                        merged.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        merged.insert("param-metadata".into(), Value::Array(pmeta));
        merged.insert("tab-metadata".into(), Value::Object(tmeta));
        // key order as Python builds it: param-metadata and tab-metadata first
        let mut ordered = Map::new();
        ordered.insert("param-metadata".into(), merged.shift_remove("param-metadata").unwrap());
        ordered.insert("tab-metadata".into(), merged.shift_remove("tab-metadata").unwrap());
        ordered.extend(merged);
        return Value::Object(ordered);
    }
    json!({})
}

// ------------------------------------------------------------------ HTML

fn html_res() -> &'static [Regex; 3] {
    static RES: OnceLock<[Regex; 3]> = OnceLock::new();
    RES.get_or_init(|| {
        [
            Regex::new(r"(?si)<\s*(script|style|iframe|object|embed)[^>]*>.*?<\s*/\s*(?:script|style|iframe|object|embed)\s*>").unwrap(),
            Regex::new(r#"(?i)\son\w+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)"#).unwrap(),
            Regex::new(r"(?i)javascript:").unwrap(),
        ]
    })
}

/// Keep simple formatting only (the page sanitises again before showing it).
pub fn clean_html(html: Option<&str>) -> Option<String> {
    let html = html.filter(|h| !h.is_empty())?;
    let [tags, handlers, js] = html_res();
    let s = tags.replace_all(html, "");
    let s = handlers.replace_all(&s, "");
    let s = js.replace_all(&s, "");
    Some(s.trim().to_string())
}

// ------------------------------------------------------------------ metadata

/// fnmatch.fnmatchcase: `*`, `?`, `[seq]`, `[!seq]`.
pub fn fnmatch(name: &str, pattern: &str) -> bool {
    fn go(n: &[char], p: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|i| go(&n[i..], &p[1..])),
            Some('?') => !n.is_empty() && go(&n[1..], &p[1..]),
            Some('[') => {
                let Some(close) = p.iter().skip(1).position(|c| *c == ']').map(|i| i + 1) else {
                    return n.first() == Some(&'[') && go(&n[1..], &p[1..]);
                };
                let Some(c) = n.first() else { return false };
                let mut set = &p[1..close];
                let negate = set.first() == Some(&'!');
                if negate {
                    set = &set[1..];
                }
                let mut hit = false;
                let mut i = 0;
                while i < set.len() {
                    if i + 2 < set.len() && set[i + 1] == '-' {
                        hit |= set[i] <= *c && *c <= set[i + 2];
                        i += 3;
                    } else {
                        hit |= set[i] == *c;
                        i += 1;
                    }
                }
                hit != negate && go(&n[1..], &p[close + 1..])
            }
            Some(x) => n.first() == Some(x) && go(&n[1..], &p[1..]),
        }
    }
    let n: Vec<char> = name.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    go(&n, &p)
}

/// Layer site settings on the parameters: the catalog model's fixed/hidden/defaults,
/// the family and model `ui` overlays and the editor.toml metadata. Returns the
/// visible parameters and the per-group ("tab") metadata.
pub fn apply_metadata(params: &[Value], groups: &[String], fam: &Value, model: &Value, meta: &Value) -> (Vec<Value>, Value) {
    let empty = Map::new();
    let fixed = model.get("fixed").and_then(Value::as_object).unwrap_or(&empty);
    let mut hidden: BTreeSet<String> = model
        .get("hidden")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    hidden.extend(fixed.keys().cloned());
    let defaults = model.get("defaults").and_then(Value::as_object).unwrap_or(&empty);
    let mut ui: Map<String, Value> = fam.get("ui").and_then(Value::as_object).cloned().unwrap_or_default();
    for (k, v) in model.get("ui").and_then(Value::as_object).into_iter().flatten() {
        let slot = ui.entry(k.clone()).or_insert_with(|| json!({}));
        for (mk, mv) in v.as_object().into_iter().flatten() {
            slot[mk] = mv.clone();
        }
    }
    let names: BTreeSet<&str> = params.iter().filter_map(|p| p["name"].as_str()).collect();
    let pmeta: Vec<(String, Value)> = meta
        .get("param-metadata")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| Some((e.get(0)?.as_str()?.to_string(), e.get(1)?.clone())))
        .collect();
    let mut out = vec![];
    for p in params {
        let name = p["name"].as_str().unwrap_or("");
        if hidden.contains(name) {
            continue;
        }
        let mut p = p.as_object().cloned().unwrap_or_default();
        if let Some(d) = defaults.get(name) {
            p.insert("default".into(), d.clone());
        }
        let mut m = Map::new();
        for (pattern, md) in &pmeta {
            if fnmatch(name, pattern) {
                for (k, v) in md.as_object().into_iter().flatten() {
                    m.insert(k.clone(), v.clone());
                }
            }
        }
        for (k, v) in ui.get(name).and_then(Value::as_object).into_iter().flatten() {
            m.insert(k.clone(), v.clone());
        }
        if let Some(cond) = m.get("display-condition").and_then(Value::as_object) {
            if cond.get("fixed") == Some(&Value::Bool(false)) {
                p.insert("hidden".into(), json!(true));
            } else if let Some(js) = cond.get("js").filter(|j| j.as_str().is_some_and(|s| !s.is_empty())) {
                p.insert("show_if".into(), js.clone());
            }
        }
        if let Some(h) = m.get("help-link").filter(|v| truthy(v)) {
            p.insert("help_link".into(), h.clone());
        }
        if let Some(h) = m.get("description-html").filter(|v| truthy(v)) {
            p.insert("description_html".into(), clean_html(h.as_str()).map(Value::String).unwrap_or(Value::Null));
        }
        if let Some(pr) = m.get("presets").and_then(Value::as_object) {
            if let Some(values) = pr.get("values").and_then(Value::as_object).filter(|v| !v.is_empty()) {
                let label = pr.get("text").cloned().unwrap_or(json!("Presets"));
                let vals: Vec<Value> = values.iter().map(|(k, v)| json!({ "label": k, "value": v })).collect();
                p.insert("presets".into(), json!({ "label": label, "values": vals }));
            }
        }
        for k in ["label", "description", "unit", "advanced", "axes", "min", "max", "step", "profile", "group", "placeholder"] {
            if let Some(v) = m.get(k) {
                p.insert(k.into(), v.clone());
            }
        }
        if m.get("options").and_then(Value::as_array).is_some_and(Vec::is_empty) {
            // no choices: typed in, even where the file or its code gave some (0.4.24)
            p.shift_remove("options");
            p.shift_remove("choices_from");
            let number = p.get("type").and_then(Value::as_str) == Some("number");
            p.insert("widget".into(), json!(if number { "number" } else { "text" }));
            p.insert("typed_in".into(), json!(true));
        } else if let Some(opts) = m.get("options").and_then(Value::as_array) {
            p.insert("widget".into(), json!("dropdown"));
            let list: Vec<Value> = opts
                .iter()
                .map(|o| if o.is_object() { o.clone() } else { json!({ "value": o, "label": py_str(o) }) })
                .collect();
            let default = p.get("default").cloned().unwrap_or(Value::Null);
            if !list.iter().any(|o| json_eq(&o["value"], &default)) {
                if let Some(first) = list.first() {
                    p.insert("default".into(), first["value"].clone());
                }
            }
            p.insert("options".into(), Value::Array(list));
        }
        out.push(Value::Object(p));
    }
    let mut tabs = Map::new();
    for (tab, tm) in meta.get("tab-metadata").and_then(Value::as_object).into_iter().flatten() {
        if !groups.contains(tab) {
            continue;
        }
        let mut t = Map::new();
        if let Some(c) = tm.get("collapsed") {
            t.insert("collapsed".into(), json!(truthy(c)));
        }
        if let Some(c) = tm.get("control-boolean").and_then(Value::as_str).filter(|c| names.contains(c)) {
            t.insert("control".into(), json!(c));
        }
        for (src, dst) in [("help-link", "help_link"), ("description-html", "description_html"), ("description-collapsed-html", "description_collapsed_html")] {
            if let Some(v) = tm.get(src).filter(|v| truthy(v)) {
                let val = if dst.ends_with("html") { clean_html(v.as_str()).map(Value::String).unwrap_or(Value::Null) } else { v.clone() };
                t.insert(dst.into(), val);
            }
        }
        tabs.insert(tab.clone(), Value::Object(t));
    }
    (out, Value::Object(tabs))
}

/// [`apply_metadata`], then the groups that still have settings, in their order, with any
/// group a `ui` entry moved a setting to added after them. Returns (parameters, groups, tabs).
pub fn apply_form(params: &[Value], groups: &[String], fam: &Value, model: &Value, meta: &Value) -> (Vec<Value>, Vec<String>, Value) {
    let (params, tabs) = apply_metadata(params, groups, fam, model, meta);
    let mut out: Vec<String> = groups.iter().filter(|g| params.iter().any(|p| p["group"] == g.as_str())).cloned().collect();
    for p in &params {
        if let Some(g) = p["group"].as_str() {
            if !out.iter().any(|x| x == g) {
                out.push(g.to_string());
            }
        }
    }
    (params, out, tabs)
}

/// Python truthiness for JSON values.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Search terms from the visible settings ("tooth count" finds the generator that has it).
pub fn setting_terms(params: &[Value]) -> String {
    let mut terms: Vec<String> = vec![];
    for p in params.iter().filter(|p| !p.get("hidden").is_some_and(truthy)) {
        let name = p["name"].as_str().unwrap_or("").replace('_', " ");
        for t in [p.get("label").and_then(Value::as_str).map(String::from), Some(name)].into_iter().flatten() {
            if !t.is_empty() && !terms.contains(&t.to_lowercase()) {
                terms.push(t.to_lowercase());
            }
        }
    }
    let joined = terms.join(" | ");
    joined.chars().take(4000).collect()
}

#[cfg(test)]
mod tests {

    #[test]
    fn old_plain_steps_fit_their_defaults() {
        let mut ps = vec![
            json!({ "name": "Width_Units", "type": "number", "default": 1.5, "step": 1 }),
            json!({ "name": "Count", "type": "number", "default": 3, "step": 1 }),
            json!({ "name": "Size", "type": "number", "default": 2.5, "min": 0, "max": 10, "step": 1 }),
        ];
        assert!(fit_plain_steps(&mut ps));
        assert_eq!((ps[0]["step"].as_f64(), ps[1]["step"].as_f64(), ps[2]["step"].as_f64()), (Some(0.1), Some(1.0), Some(1.0)));
        assert!(limit_problems(&ps[..2]).is_empty());
    }
    use super::*;

    #[test]
    fn normpath_like_python() {
        assert_eq!(normpath("/a/b/../c"), "/a/c");
        assert_eq!(normpath("/../x"), "/x");
        assert_eq!(normpath("../x/./y"), "../x/y");
        assert_eq!(normpath("a//b/"), "a/b");
        assert_eq!(normpath(""), ".");
        assert_eq!(join("/vendor/x", "lib.scad"), "/vendor/x/lib.scad");
        assert_eq!(dirname("/vendor/x/a.scad"), "/vendor/x");
        assert_eq!(dirname("/a.scad"), "/");
    }

    #[test]
    fn fnmatch_patterns() {
        assert!(fnmatch("magnet_d", "magnet_*"));
        assert!(fnmatch("a1", "a?"));
        assert!(fnmatch("b", "[abc]"));
        assert!(!fnmatch("d", "[abc]"));
        assert!(fnmatch("d", "[!abc]"));
        assert!(fnmatch("x5", "x[0-9]"));
        assert!(!fnmatch("Magnet", "magnet"));
    }

    #[test]
    fn references_skip_comments() {
        let src = "include <a.scad>\n// use <b.scad>\n/* use <c.scad>\n */ use <d.scad>\nx = import(\"m.stl\"); // import(\"n.stl\")\n";
        let (uses, imports) = references(src);
        assert_eq!(uses, vec!["a.scad", "d.scad"]); // code after a block comment on the same line counts
        assert_eq!(imports, vec!["m.stl"]);
    }

    #[test]
    fn params_convert() {
        let raw = json!({"parameters": [
            {"name": "w", "group": "Size", "initial": 3.0, "min": 1.0, "max": 10.0, "caption": " Width "},
            {"name": "$fn", "initial": 32},
            {"name": "style", "group": "Size", "initial": "a", "options": [{"name": "B", "value": "b"}]},
            {"name": "v", "initial": [1.0, 2.5]},
            {"name": "secret", "group": "Hidden", "initial": 1}
        ]});
        let (p, g) = convert_params(&raw);
        assert_eq!(g, vec!["Size", "Parameters"]);
        assert_eq!(p[0]["default"], json!(3));
        assert_eq!(p[0]["widget"], json!("slider"));
        assert_eq!(p[0]["description"], json!("Width"));
        assert_eq!(p[1]["options"][0], json!({"value": "a", "label": "a", "added": true}));
        assert_eq!(p[2]["type"], json!("number[]"));
        assert_eq!(p[2]["default"], json!([1, 2.5]));
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn choices_read_from_the_code() {
        let mut p = vec![
            json!({ "name": "style", "type": "string", "widget": "text", "default": "round" }),
            json!({ "name": "label", "type": "string", "widget": "text", "default": "Hi" }),
            json!({ "name": "shape", "type": "string", "widget": "text", "default": "a" }),
            json!({ "name": "lid", "type": "string", "widget": "dropdown", "default": "x", "options": [{ "value": "x", "label": "x" }] }),
        ];
        let code = "if (style == \"square\") cube(1);\nelse if (\"hex\" != style) sphere(1);\n// style == \"commented\"\n\
                    if (label == \"\") echo(); my_style = \"no\" == my_style;\nx = shape == \"b\" ? 1 : 2;";
        let changed = choices_from_code(&mut p, &[code.to_string()]).unwrap();
        assert_eq!(changed, vec!["style"]);
        assert_eq!(p[0]["options"], json!([{ "value": "round", "label": "round" }, { "value": "square", "label": "square" }, { "value": "hex", "label": "hex" }]));
        assert_eq!(p[0]["widget"], "dropdown");
        assert_eq!(p[1]["widget"], "text"); // only the empty word: no choices
        assert_eq!(p[2]["widget"], "text"); // one word besides its default
        assert_eq!(p[3]["options"].as_array().unwrap().len(), 1); // its own choices stay
    }

    /// tests/settings/: models read wrongly before, with what reading them should give (0.4.24)
    #[test]
    fn settings_edge_cases() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/settings");
        let assign = Regex::new(r#"(?m)^([A-Za-z_]\w*)\s*=\s*"([^"]*)"\s*;"#).unwrap();
        let mut n = 0;
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "scad") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let expect: Value = serde_json::from_str(&std::fs::read_to_string(path.with_extension("expect.json")).unwrap()).unwrap();
            // the text settings OpenSCAD's list would give: top-level words
            let mut params: Vec<Value> = assign.captures_iter(&strip_comments(&text))
                .map(|c| json!({ "name": &c[1], "type": "string", "widget": "text", "default": &c[2] })).collect();
            choices_from_code(&mut params, &[text]).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            for (k, want) in expect["choices"].as_object().unwrap() {
                let p = params.iter().find(|p| p["name"] == k.as_str()).unwrap_or_else(|| panic!("{name}: no setting {k}"));
                let got: Vec<&Value> = p["options"].as_array().map(|o| o.iter().map(|o| &o["value"]).collect()).unwrap_or_default();
                assert_eq!(json!(got), *want, "{name}: {k}'s choices");
            }
            for k in expect["typed"].as_array().unwrap() {
                let p = params.iter().find(|p| p["name"] == *k).unwrap_or_else(|| panic!("{name}: no setting {k}"));
                assert_eq!(p["widget"], "text", "{name}: {k} should stay typed in");
            }
            n += 1;
        }
        assert!(n >= 3, "the edge cases weren't found in {}", dir.display());
    }

    #[test]
    fn defaults_fit_their_limits() {
        // Gridfinity Anylid: `Width_Units = 1.5;` with no comment comes out of the export as step 1
        let raw = json!({"parameters": [
            {"name": "Width_Units", "initial": 1.5, "step": 1},
            {"name": "Thickness_mm", "initial": 0.25, "step": 1},
            {"name": "count", "initial": 3, "step": 1},
            {"name": "w", "initial": 12.0, "min": 0.0, "max": 10.0, "step": 1.0},
            {"name": "h", "initial": 2.5, "min": 0.0, "max": 10.0, "step": 1.0},
            {"name": "angle", "initial": 0.3, "min": 0.0, "max": 1.0, "step": 0.1},
            {"name": "style", "initial": "c", "options": [{"name": "A", "value": "a"}]}
        ]});
        let (p, _) = convert_params(&raw);
        assert_eq!(p[0]["step"], json!(0.1));
        assert_eq!(p[1]["step"], json!(0.01));
        assert_eq!(p[2]["step"], json!(1));
        let probs = limit_problems(&p);
        assert_eq!(probs, vec![
            "W: 12 is above its most, 10",
            "H: 2.5 isn't a step of 1 from 0",
            "Style: its default, c, isn't one of its choices",
        ]);
        assert_eq!(decimal_step(2.125), 0.001);
    }

    #[test]
    fn html_cleaned() {
        assert_eq!(clean_html(Some("<b onclick='x()'>hi</b><script>evil()</script>")).as_deref(), Some("<b>hi</b>"));
        assert_eq!(clean_html(Some("")), None);
    }
}
