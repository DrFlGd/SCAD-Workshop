//! Reading a project folder the user added: which `.scad` files are models (and
//! which are library code), its README and license, preset files, editor.toml,
//! and ready-made model files (STL, 3MF, STEP...) grouped one item per name.
//! The result is a family manifest (the shape of a workshop.json, with what was
//! read from the files), which [`crate::assemble::assemble_model`] turns into
//! model pages.

use crate::ingest::{self, Resolver};
use crate::library::slug;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The files a model reads: path in the project -> (file on disk, the library it came from).
pub type Collected = BTreeMap<String, (PathBuf, Option<usize>)>;

/// Folders never read (version control, tooling, dependencies).
const SKIP_DIRS: [&str; 8] = [".git", ".github", ".hg", ".svn", "node_modules", "__pycache__", ".vscode", ".idea"];
/// Model files shown as ready-made parts.
pub const MODEL_EXTS: [&str; 9] = ["stl", "3mf", "step", "stp", "obj", "amf", "off", "f3d", "shapr"];
const MAX_FILES: usize = 20_000;
const MAX_ENTRIES: usize = 250;

#[derive(Clone, Debug)]
pub struct FileInfo {
    /// Path inside the project, "/"-separated.
    pub rel: String,
    pub bytes: u64,
}

/// Every file in a project folder (skipping tool folders), sorted by path.
pub fn list_files(root: &Path) -> Vec<FileInfo> {
    let mut out = vec![];
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if ft.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push((e.path(), rel));
                }
            } else if ft.is_file() && out.len() < MAX_FILES {
                out.push(FileInfo { rel, bytes: e.metadata().map(|m| m.len()).unwrap_or(0) });
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// A fingerprint of a folder's contents (paths and file contents), to notice when
/// a project edited in place has changed. Content, not modification times, so a
/// library copied or unzipped elsewhere keeps the same versions.
pub fn fingerprint(root: &Path) -> String {
    let mut h = Sha256::new();
    for f in list_files(root) {
        h.update(f.rel.as_bytes());
        h.update([0]);
        match std::fs::read(root.join(&f.rel)) {
            Ok(data) => h.update(Sha256::digest(&data)),
            Err(_) => h.update(f.bytes.to_le_bytes()),
        }
    }
    hex::encode(h.finalize())[..12].to_string()
}

/// A quick fingerprint of a folder: file paths, sizes and modification times (no contents), to
/// tell cheaply whether anything might have changed since [`fingerprint`] was taken.
pub fn quick_fingerprint(root: &Path) -> String {
    let mut h = Sha256::new();
    for f in list_files(root) {
        h.update(f.rel.as_bytes());
        h.update([0]);
        h.update(f.bytes.to_le_bytes());
        let t = std::fs::metadata(root.join(&f.rel)).and_then(|m| m.modified()).ok();
        // to the nanosecond where the file system keeps it: an edit that keeps the size, in the same
        // second as the last check, still shows
        let nanos = t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
        h.update(nanos.to_le_bytes());
    }
    hex::encode(h.finalize())[..12].to_string()
}

fn ext(rel: &str) -> String {
    Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

// ------------------------------------------------------------------ SCAD structure

/// What a SCAD file does at top level.
#[derive(Debug, Default, PartialEq)]
pub struct Shape {
    /// Makes geometry at top level (so it renders on its own).
    pub geometry: bool,
    pub modules: usize,
    pub functions: usize,
    pub assignments: usize,
}

/// Blank out string literals, keeping their quotes and the text length.
fn blank_strings(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut esc = false;
    for c in s.chars() {
        if in_str {
            if esc {
                esc = false;
                out.push(' ');
            } else if c == '\\' {
                esc = true;
                out.push(' ');
            } else if c == '"' {
                in_str = false;
                out.push('"');
            } else {
                out.push(if c == '\n' { '\n' } else { ' ' });
            }
        } else {
            if c == '"' {
                in_str = true;
            }
            out.push(c);
        }
    }
    out
}

/// Classify the top-level statements of a SCAD file.
pub fn shape(text: &str) -> Shape {
    let src = blank_strings(&ingest::strip_comments(text));
    let mut sh = Shape::default();
    let mut depth: i32 = 0; // () [] {}
    let mut stmt = String::new();
    let flush = |stmt: &mut String, sh: &mut Shape| {
        let s = stmt.trim();
        if !s.is_empty() {
            let word: String = s.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
            let rest = s[word.len()..].trim_start();
            if (word == "include" || word == "use") && rest.starts_with('<') {
                // a reference; may share a line with the next statement
                if let Some(end) = rest.find('>') {
                    let after = rest[end + 1..].to_string();
                    stmt.clear();
                    if !after.trim().is_empty() {
                        stmt.push_str(&after);
                        return false;
                    }
                }
            } else if word == "module" {
                sh.modules += 1;
            } else if word == "function" {
                sh.functions += 1;
            } else if !word.is_empty() && rest.starts_with('=') && !rest.starts_with("==") {
                sh.assignments += 1;
            } else if (word == "echo" || word == "assert") && rest.starts_with('(') {
                // messages and checks make no geometry
            } else {
                sh.geometry = true;
            }
        }
        stmt.clear();
        true
    };
    for c in src.chars() {
        // include <...> at depth 0 ends at '>', not ';'
        if depth == 0 && c == '>' {
            let t = stmt.trim_start();
            if (t.starts_with("include") || t.starts_with("use")) && t.contains('<') {
                stmt.push(c);
                flush(&mut stmt, &mut sh);
                continue;
            }
        }
        stmt.push(c);
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            '}' => {
                depth = (depth - 1).max(0);
                if depth == 0 {
                    flush(&mut stmt, &mut sh);
                }
            }
            ';' if depth == 0 => {
                flush(&mut stmt, &mut sh);
            }
            _ => {}
        }
    }
    flush(&mut stmt, &mut sh);
    sh
}

// ------------------------------------------------------------------ README and license

/// The README's text and its first real paragraph (badges, headings and HTML skipped).
pub fn readme_summary(text: &str) -> String {
    let mut para: Vec<String> = vec![];
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        if t.starts_with('#') || t.starts_with("[![") || t.starts_with("![") || t.starts_with('<') || t.starts_with("---") || t.starts_with("===") || t.starts_with("```") {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        para.push(t.to_string());
    }
    let s = para.join(" ");
    // [text](link) -> text, drop emphasis markers
    let link = regex::Regex::new(r"\[([^\]]*)\]\([^)]*\)").unwrap();
    let s = link.replace_all(&s, "$1").replace("**", "").replace('`', "");
    let mut out: String = s.chars().take(400).collect();
    if s.chars().count() > 400 {
        out = out.rsplit_once(' ').map(|(a, _)| a.to_string()).unwrap_or(out) + "…";
    }
    out
}

/// SPDX id from a license text (the common ones).
pub fn detect_license(text: &str) -> Option<&'static str> {
    let t = text.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    let has = |s: &str| t.contains(s);
    if has("creative commons") || has("creativecommons.org") || has("cc by") || has("cc-by") {
        let nc = has("noncommercial") || has("non-commercial") || has("by-nc") || has("by nc");
        let sa = has("sharealike") || has("share-alike") || has("share alike") || has("-sa") || has(" sa ");
        let nd = has("noderivatives") || has("no derivatives") || has("-nd") || has("noderivs");
        if has("cc0") || has("public domain dedication") || has("publicdomain/zero") {
            return Some("CC0-1.0");
        }
        let v = if has("4.0") { "4.0" } else if has("3.0") { "3.0" } else if has("2.0") { "2.0" } else { "4.0" };
        let base = match (nc, sa, nd) {
            (true, true, _) => "CC-BY-NC-SA",
            (true, _, true) => "CC-BY-NC-ND",
            (true, _, _) => "CC-BY-NC",
            (false, true, _) => "CC-BY-SA",
            (false, _, true) => "CC-BY-ND",
            _ => "CC-BY",
        };
        return Some(match (base, v) {
            ("CC-BY", "4.0") => "CC-BY-4.0",
            ("CC-BY", "3.0") => "CC-BY-3.0",
            ("CC-BY-SA", "4.0") => "CC-BY-SA-4.0",
            ("CC-BY-SA", "3.0") => "CC-BY-SA-3.0",
            ("CC-BY-NC", "4.0") => "CC-BY-NC-4.0",
            ("CC-BY-NC", "3.0") => "CC-BY-NC-3.0",
            ("CC-BY-NC-SA", "4.0") => "CC-BY-NC-SA-4.0",
            ("CC-BY-NC-SA", "3.0") => "CC-BY-NC-SA-3.0",
            ("CC-BY-NC-ND", _) => "CC-BY-NC-ND-4.0",
            ("CC-BY-ND", _) => "CC-BY-ND-4.0",
            ("CC-BY-NC-SA", _) => "CC-BY-NC-SA-4.0",
            ("CC-BY-NC", _) => "CC-BY-NC-4.0",
            ("CC-BY-SA", _) => "CC-BY-SA-4.0",
            _ => "CC-BY-4.0",
        });
    }
    if has("cc0") || has("creative commons zero") {
        return Some("CC0-1.0");
    }
    if has("this is free and unencumbered software released into the public domain") {
        return Some("Unlicense");
    }
    if has("gnu lesser general public license") || has("gnu library general public license") {
        return Some(if has("version 3") { "LGPL-3.0" } else { "LGPL-2.1" });
    }
    if has("gnu affero general public license") {
        return Some("AGPL-3.0");
    }
    if has("gnu general public license") {
        return Some(if has("version 3") { "GPL-3.0" } else if has("version 2") { "GPL-2.0" } else { "GPL-3.0" });
    }
    if has("mozilla public license") {
        return Some("MPL-2.0");
    }
    if has("apache license") && (has("version 2.0") || has("2.0")) {
        return Some("Apache-2.0");
    }
    if has("permission is hereby granted, free of charge") {
        return Some("MIT");
    }
    if has("redistribution and use in source and binary forms") {
        return Some(if has("neither the name") || has("may be used to endorse") { "BSD-3-Clause" } else { "BSD-2-Clause" });
    }
    if has("mit license") {
        return Some("MIT");
    }
    None
}

/// The text of a project's top-level README, if it has one.
pub fn readme_text(root: &Path) -> Option<String> {
    let rd = std::fs::read_dir(root).ok()?;
    let mut names: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    names.sort();
    let p = names.into_iter().find(|p| {
        let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        n == "readme.md" || n == "readme.txt" || n == "readme" || n == "readme.markdown"
    })?;
    Some(std::fs::read_to_string(p).ok()?.chars().take(100_000).collect())
}

/// SPDX id from a project's top-level LICENSE/COPYING file.
pub fn survey_license(root: &Path) -> Option<String> {
    let rd = std::fs::read_dir(root).ok()?;
    let mut names: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    names.sort();
    names.into_iter().find_map(|p| {
        let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let is_license = ["license", "licence", "copying"].iter().any(|k| n == *k || n.starts_with(&format!("{k}.")));
        if !is_license {
            return None;
        }
        detect_license(&std::fs::read_to_string(&p).ok()?).map(String::from)
    })
}

/// Whether a license is fine for sharing prints ("ok") or needs a look ("review").
pub fn public_use(spdx: Option<&str>) -> &'static str {
    match spdx {
        None => "review",
        Some(s) if s.contains("-NC") => "review",
        Some(_) => "ok",
    }
}

// ------------------------------------------------------------------ categories

/// A category from keywords in the project's name, description, topics and files.
pub fn guess_category(words: &str) -> &'static str {
    let w = words.to_lowercase();
    // whole words, with the usual endings ("gears", "boxes", "threaded"), so "design"
    // doesn't count as "sign" and "showcase" not as "case"
    let any = |keys: &[&str]| {
        keys.iter().any(|k| {
            let k = regex::escape(k.trim()).replace(r"\ ", r"\s+").replace(' ', r"\s+");
            regex::Regex::new(&format!(r"\b{k}(?:s|es|ed|ing)?\b")).map(|re| re.is_match(&w)).unwrap_or(false)
        })
    };
    if any(&["gridfinity"]) {
        "gridfinity"
    } else if any(&["opengrid", "multiboard", "honeycomb", "pegboard", "skadis", "underware"]) {
        "wall"
    } else if any(&["gear", "bearing", "hinge", "spring", "pulley", "linkage", "servo", "cam", "sprocket", "rack"]) {
        "mechanical"
    } else if any(&["screw", "bolt", "nut", "thread", "fastener", "insert", "washer", "rivet"]) {
        "fasteners"
    } else if any(&["label", "sign", "nameplate", "lettering"]) {
        "labels"
    } else if any(&["enclosure", "case", "project box", "electronics box", "housing"]) {
        "enclosures"
    } else if any(&["box", "bin", "tray", "basket", "drawer", "organizer", "organiser", "storage", "container"]) {
        "carrying"
    } else {
        "other"
    }
}

pub const EXTRA_CATEGORIES: [(&str, &str); 4] =
    [("mechanical", "Mechanical"), ("fasteners", "Fasteners"), ("enclosures", "Enclosures"), ("libraries", "Libraries")];

/// "my_bin-v2" -> "My bin v2"
pub fn humanize(stem: &str) -> String {
    let s: String = stem.replace(['_', '-'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

// ------------------------------------------------------------------ the scan

/// A library folder other projects can include from (`<BOSL2/std.scad>` or `<threads.scad>`).
#[derive(Clone, Debug)]
pub struct LibraryDir {
    /// Folder name used in includes ("BOSL2"); the folder's files are also reachable directly.
    pub name: String,
    pub path: PathBuf,
}

/// What a project folder holds.
#[derive(Debug, Default)]
pub struct Found {
    /// .scad files that render on their own: (path, has a preset file).
    pub entries: Vec<String>,
    /// .scad files that only define modules/functions.
    pub library_files: usize,
    pub readme: Option<(String, String)>,
    pub license: Option<(String, Option<&'static str>)>,
    pub editor_toml: Option<String>,
    /// Ready-made parts: (folder, name stem) -> files.
    pub parts: Vec<Value>,
    pub problems: Vec<String>,
    pub scad_files: usize,
    /// A folder of examples (Review 3, F-15): `{ folder, main, count }`, see [`examples_folder`].
    pub examples: Option<Value>,
}

/// The files a SCAD file `include`s (not `use`s: `use` doesn't run top-level code), as written.
fn includes_in(text: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"^\s*include\s*<([^>]+)>").unwrap());
    ingest::strip_comments(text).lines().filter_map(|l| re.captures(l).map(|c| c[1].trim().to_string())).collect()
}

/// What deciding "model or library" needs to know about one .scad file.
struct ScadFile {
    rel: String,
    shape: Shape,
    /// Project files it `include`s (paths inside the project, as resolved from its folder).
    includes: Vec<String>,
    /// Names it assigns (anywhere: a conservative "defines").
    assigns: BTreeSet<String>,
    /// Every name it mentions.
    names: BTreeSet<String>,
}

impl ScadFile {
    fn read(rel: &str, text: &str) -> ScadFile {
        static NAME: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        static ASSIGN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let name = NAME.get_or_init(|| regex::Regex::new(r"[A-Za-z_$][A-Za-z0-9_$]*").unwrap());
        let assign = ASSIGN.get_or_init(|| regex::Regex::new(r"(?m)^\s*([A-Za-z_$][A-Za-z0-9_$]*)\s*=[^=]").unwrap());
        static UNDEF: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let undef = UNDEF.get_or_init(|| regex::Regex::new(r"is_undef\s*\(\s*[A-Za-z_$][A-Za-z0-9_$]*\s*\)").unwrap());
        let code = blank_strings(&ingest::strip_comments(text));
        let dir = ingest::dirname(&format!("/{rel}")).to_string();
        ScadFile {
            rel: rel.to_string(),
            shape: shape(text),
            includes: includes_in(text).into_iter().map(|p| ingest::normpath(&ingest::join(&dir, &p)).trim_start_matches('/').to_string()).collect(),
            assigns: assign.captures_iter(&code).map(|c| c[1].to_string()).collect(),
            // `is_undef(flag)` only asks whether a flag exists (BOSL2's `_BOSL2_STD` guard); not a setting it reads
            names: name.find_iter(&undef.replace_all(&code, " ")).map(|m| m.as_str().to_string()).collect(),
        }
    }
}

/// Which .scad files are models.
///
/// A file that makes geometry at top level is a model. So is a "front end": a file of
/// settings only (no geometry, no modules of its own) that `include`s a project file that
/// draws at top level (directly or through other includes) and sets something that file
/// reads but never sets itself. OpenSCAD runs the included file's top level with the front
/// end's settings, and the drawing file can't render without them: it's an engine, library
/// code, as is any file a front end includes on the way to it. Birdhouse Workshop is the
/// example: square, corner and polygon front ends over one shared engine that reads their
/// `design_preset` and `body_style`. A file that only overrides settings the drawing file
/// already has (GridFlock's docs/ animations over gridflock.scad) isn't a front end, and the
/// drawing file stays a model.
fn models_among(scads: &[ScadFile]) -> Vec<String> {
    let find = |p: &str| scads.iter().position(|f| f.rel.eq_ignore_ascii_case(p));
    // the project files reached from i through includes
    let closure = |i: usize| -> Vec<usize> {
        let mut seen: Vec<usize> = vec![];
        let mut todo: Vec<usize> = scads[i].includes.iter().filter_map(|p| find(p)).collect();
        while let Some(j) = todo.pop() {
            if j == i || seen.contains(&j) {
                continue;
            }
            seen.push(j);
            todo.extend(scads[j].includes.iter().filter_map(|p| find(p)));
        }
        seen
    };
    let mut model = vec![false; scads.len()];
    let mut engine = vec![false; scads.len()];
    for (i, f) in scads.iter().enumerate() {
        if f.shape.geometry {
            model[i] = true;
            continue;
        }
        if f.shape.modules > 0 || f.shape.assignments == 0 {
            continue;
        }
        let reached = closure(i);
        // drawing files it reaches that need one of its settings
        let fed: Vec<usize> = reached
            .iter()
            .copied()
            .filter(|&d| scads[d].shape.geometry)
            .filter(|&d| {
                let mut defined = scads[d].assigns.clone();
                for k in closure(d) {
                    defined.extend(scads[k].assigns.iter().cloned());
                }
                f.assigns.iter().any(|v| scads[d].names.contains(v) && !defined.contains(v))
            })
            .collect();
        if !fed.is_empty() {
            model[i] = true;
            for j in reached {
                engine[j] = true;
            }
        }
    }
    scads.iter().enumerate().filter(|(i, _)| model[*i] && !engine[*i]).map(|(_, f)| f.rel.clone()).collect()
}

/// More models than this in one folder, each only a call into the same main file, is a folder of examples.
const EXAMPLES_MIN: usize = 20;

/// A folder of examples (Review 3, F-15): one folder with more than [`EXAMPLES_MIN`] models, each of
/// which only includes or uses the same main file and calls its modules: no settings, modules or
/// functions of its own (LEGO.scad's `examples/`: a piece per file, by part number). Gears' examples
/// have settings of their own, so they're models. `{ folder, main, count }`.
fn examples_folder(entries: &[String], scads: &[ScadFile], refs: &BTreeMap<String, Vec<String>>) -> Option<Value> {
    let mut by_folder: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for e in entries {
        if let Some((dir, _)) = e.rsplit_once('/') {
            by_folder.entry(dir.to_string()).or_default().push(e);
        }
    }
    let (folder, files) = by_folder.into_iter().filter(|(_, fs)| fs.len() > EXAMPLES_MIN).max_by_key(|(_, fs)| fs.len())?;
    let mut main: Option<&String> = None;
    for e in &files {
        let f = scads.iter().find(|s| &s.rel == *e)?;
        let only_calls = f.shape.modules == 0 && f.shape.functions == 0 && f.assigns.iter().all(|a| a.starts_with('$'));
        let [r] = refs.get(*e).map(Vec::as_slice).unwrap_or(&[]) else { return None };
        if !only_calls || r.starts_with(&format!("{folder}/")) || main.is_some_and(|m| m != r) {
            return None;
        }
        main = Some(r);
    }
    Some(json!({ "folder": folder, "main": main?, "count": files.len() }))
}

/// Look through a project folder.
pub fn survey(root: &Path) -> Found {
    let files = list_files(root);
    let mut found = Found::default();
    let mut imported: Vec<String> = vec![]; // model files read by import() are inputs, not parts
    let mut scads: Vec<ScadFile> = vec![];
    let mut refs: BTreeMap<String, Vec<String>> = BTreeMap::new(); // the project files each .scad file includes or uses
    for f in &files {
        let lower = f.rel.to_lowercase();
        let name = lower.rsplit('/').next().unwrap_or(&lower).to_string();
        let top = !f.rel.contains('/');
        if top && found.readme.is_none() && (name == "readme.md" || name == "readme.txt" || name == "readme" || name == "readme.markdown") {
            let text = std::fs::read_to_string(root.join(&f.rel)).unwrap_or_default();
            found.readme = Some((f.rel.clone(), text.chars().take(100_000).collect()));
        }
        if top && found.license.is_none() && ["license", "license.md", "license.txt", "licence", "licence.md", "licence.txt", "copying", "copying.md", "copying.txt"].contains(&name.as_str()) {
            let text = std::fs::read_to_string(root.join(&f.rel)).unwrap_or_default();
            found.license = Some((f.rel.clone(), detect_license(&text)));
        }
        if name == "editor.toml" && found.editor_toml.as_ref().is_none_or(|e| e.matches('/').count() > f.rel.matches('/').count()) {
            found.editor_toml = Some(f.rel.clone());
        }
        if ext(&f.rel) == "scad" {
            found.scad_files += 1;
            let text = String::from_utf8_lossy(&std::fs::read(root.join(&f.rel)).unwrap_or_default()).into_owned();
            let dir = ingest::dirname(&format!("/{}", f.rel)).to_string();
            let (uses, imports) = ingest::references(&text);
            for imp in imports {
                imported.push(ingest::normpath(&ingest::join(&dir, &imp)).trim_start_matches('/').to_lowercase());
            }
            refs.insert(f.rel.clone(), uses.iter().map(|u| ingest::normpath(&ingest::join(&dir, u)).trim_start_matches('/').to_string()).collect());
            let in_test_dir = lower.split('/').any(|p| p == "test" || p == "tests");
            if in_test_dir {
                found.library_files += 1;
                continue;
            }
            scads.push(ScadFile::read(&f.rel, &text));
        }
    }
    let mut entries = models_among(&scads);
    found.library_files += scads.len() - entries.len();
    found.examples = examples_folder(&entries, &scads, &refs);
    if entries.len() > MAX_ENTRIES {
        found.problems.push(format!(
            "{} .scad files render on their own; only the first {MAX_ENTRIES} became models. If this is a library, mark it as one.",
            entries.len()
        ));
        entries.truncate(MAX_ENTRIES);
    }
    found.entries = entries;
    // ready-made parts: one item per folder + name, with every format
    let mut groups: BTreeMap<(String, String), Vec<&FileInfo>> = BTreeMap::new();
    for f in &files {
        let e = ext(&f.rel);
        if !MODEL_EXTS.contains(&e.as_str()) || imported.contains(&f.rel.to_lowercase()) {
            continue;
        }
        let (dir, file) = f.rel.rsplit_once('/').unwrap_or(("", &f.rel));
        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        groups.entry((dir.to_string(), stem.to_lowercase())).or_default().push(f);
    }
    let mut ids: Vec<String> = vec![];
    for ((dir, _), fs) in groups {
        let file = fs[0].rel.rsplit('/').next().unwrap_or(&fs[0].rel);
        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        let mut id = slug(&format!("{dir} {stem}"));
        if id.is_empty() {
            id = "part".into();
        }
        let base = id.clone();
        let mut n = 2;
        while ids.contains(&id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        ids.push(id.clone());
        let order = |r: &str| MODEL_EXTS.iter().position(|x| *x == ext(r)).unwrap_or(99);
        let mut fs = fs.clone();
        fs.sort_by_key(|f| order(&f.rel));
        // the 3D view's file: a 3MF (it can carry colours), then STL, then OBJ; the core hands the
        // viewer any of them as STL (mesh.rs, milestone 5)
        let preview = ["3mf", "stl", "obj"].iter().find_map(|x| fs.iter().find(|f| ext(&f.rel) == *x)).map(|f| f.rel.clone());
        let category = if dir.is_empty() { "Parts".to_string() } else { humanize(dir.rsplit('/').next().unwrap_or(&dir)) };
        // a part made from a model (milestone 4) has how it was made beside it: <name>.recipe.json
        let recipe_rel = if dir.is_empty() { format!("{stem}.recipe.json") } else { format!("{dir}/{stem}.recipe.json") };
        let recipe: Option<Value> = std::fs::read(root.join(&recipe_rel)).ok().and_then(|b| serde_json::from_slice(&b).ok()).filter(|r: &Value| r["model"].is_string())
            .map(|r| crate::format::recipe_view(r, true, Some(stem)));
        let mut part = json!({
            "id": id, "name": if recipe.is_some() { json!(stem) } else { json!(humanize(stem)) }, "category": category, "kind": "printable", "tags": [],
            "files": fs.iter().map(|f| json!({ "path": f.rel, "format": ext(&f.rel), "bytes": f.bytes })).collect::<Vec<_>>(),
            "preview": preview,
        });
        if let Some(r) = recipe {
            part["generator"] = r["model"].clone();
            part["made_from"] = r;
            part["recipe"] = json!(recipe_rel);
        }
        found.parts.push(part);
    }
    found
}

/// Resolve a project's model files: the project folder is "/", libraries are
/// "/libraries/<name>/..." (and directly under "/libraries/").
pub fn resolver(root: &Path, libs: &[LibraryDir]) -> LibResolver {
    LibResolver { inner: Resolver { root: root.to_path_buf(), libraries: vec![] }, libs: libs.to_vec() }
}

/// [`Resolver`] plus named library folders.
pub struct LibResolver {
    inner: Resolver,
    libs: Vec<LibraryDir>,
}

impl LibResolver {
    /// Host path for a virtual path, and which library it came from (None: the project).
    pub fn host(&self, v: &str) -> Option<(PathBuf, Option<usize>)> {
        if let Some(rel) = v.strip_prefix("/libraries/") {
            // "<BOSL2/std.scad>": a library by folder name
            if let Some((first, rest)) = rel.split_once('/') {
                for (i, l) in self.libs.iter().enumerate() {
                    if l.name.eq_ignore_ascii_case(first) {
                        let p = l.path.join(rest);
                        if p.is_file() {
                            return Some((p, Some(i)));
                        }
                        if let Some(p) = ingest::find_nocase(&l.path, rest) {
                            return Some((p, Some(i)));
                        }
                    }
                }
            }
            // "<threads.scad>": a file at the top of a library
            for (i, l) in self.libs.iter().enumerate() {
                let p = l.path.join(rel);
                if p.is_file() {
                    return Some((p, Some(i)));
                }
            }
            return None;
        }
        self.inner.host(v).map(|p| (p, None))
    }

    /// Like [`ingest::collect_files`], with named libraries.
    pub fn collect(&self, entry: &str) -> (Collected, Vec<String>) {
        let mut files: BTreeMap<String, (PathBuf, Option<usize>)> = BTreeMap::new();
        let mut missing: std::collections::BTreeSet<String> = Default::default();
        let mut todo = vec![format!("/{}", entry.trim_start_matches('/'))];
        while let Some(v) = todo.pop() {
            if files.contains_key(&v) {
                continue;
            }
            let Some((h, lib)) = self.host(&v) else {
                missing.insert(v);
                continue;
            };
            let scad = h.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("scad"));
            files.insert(v.clone(), (h.clone(), lib));
            if !scad {
                continue;
            }
            let text = String::from_utf8_lossy(&std::fs::read(&h).unwrap_or_default()).into_owned();
            let base = ingest::dirname(&v).to_string();
            let (uses, imports) = ingest::references(&text);
            for r in uses {
                let rel = ingest::normpath(&ingest::join(&base, &r));
                todo.push(if self.host(&rel).is_some() { rel } else { format!("/libraries/{}", ingest::normpath(&r)) });
            }
            for i in imports {
                let rel = ingest::normpath(&ingest::join(&base, &i));
                if self.host(&rel).is_some() {
                    todo.push(rel);
                }
            }
        }
        (files, missing.into_iter().collect())
    }
}

/// OpenSCAD parameter-set file next to an entry ("box.scad" -> "box.json") as presets.
pub fn presets_for(root: &Path, entry: &str) -> Vec<Value> {
    let p = root.join(entry).with_extension("json");
    let Ok(bytes) = std::fs::read(&p) else { return vec![] };
    let Ok(v) = serde_json::from_slice::<Value>(&bytes) else { return vec![] };
    v["parameterSets"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, set)| {
            // values are strings in the file; numbers, booleans and vectors parse as JSON
            let values: serde_json::Map<String, Value> = set
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, v)| {
                    let parsed = v.as_str().and_then(|s| serde_json::from_str::<Value>(s).ok()).unwrap_or_else(|| v.clone());
                    (k.clone(), parsed)
                })
                .collect();
            json!({ "label": name, "values": values })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_shapes() {
        let lib = "module a(x=1) { cube(x); }\nfunction f(x) = x * 2;\nW = 3; // [1:10]\n";
        assert_eq!(shape(lib), Shape { geometry: false, modules: 1, functions: 1, assignments: 1 });
        let model = "include <lib.scad>\nwidth = 2; // [1:5]\n/* [Hidden] */\nmodule m() { sphere(1); }\nm();\n";
        assert!(shape(model).geometry);
        let blocky = "use <x.scad> translate([0,0,1]) { cube(2); }";
        assert!(shape(blocky).geometry);
        let strings = "s = \"cube(1); module\";\n";
        assert!(!shape(strings).geometry);
        let cond = "if (show) cube(1);";
        assert!(shape(cond).geometry);
        let eq = "x == 2;";
        assert!(shape(eq).geometry);
    }

    #[test]
    fn licenses() {
        assert_eq!(detect_license("MIT License\n\nPermission is hereby granted, free of charge, to any person"), Some("MIT"));
        assert_eq!(detect_license("GNU GENERAL PUBLIC LICENSE\n Version 3, 29 June 2007"), Some("GPL-3.0"));
        assert_eq!(detect_license("GNU LESSER GENERAL PUBLIC LICENSE Version 2.1"), Some("LGPL-2.1"));
        assert_eq!(detect_license("Attribution-NonCommercial-ShareAlike 4.0 International (Creative Commons)"), Some("CC-BY-NC-SA-4.0"));
        assert_eq!(detect_license("Creative Commons Attribution 4.0 International"), Some("CC-BY-4.0"));
        assert_eq!(detect_license("BSD 2-Clause License\nRedistribution and use in source and binary forms"), Some("BSD-2-Clause"));
        assert_eq!(detect_license("Apache License\nVersion 2.0, January 2004"), Some("Apache-2.0"));
        assert_eq!(detect_license("Nothing to see"), None);
        assert_eq!(public_use(Some("CC-BY-NC-SA-4.0")), "review");
        assert_eq!(public_use(Some("MIT")), "ok");
    }

    #[test]
    fn readme_first_paragraph() {
        let md = "# Title\n[![badge](x)](y)\n\nA [gear](https://x) **library** for `OpenSCAD`.\nSecond line.\n\nMore.";
        assert_eq!(readme_summary(md), "A gear library for OpenSCAD. Second line.");
    }

    #[test]
    fn categories_and_names() {
        assert_eq!(guess_category("Parametric bevel gear generator"), "mechanical");
        assert_eq!(guess_category("gridfinity bins"), "gridfinity");
        assert_eq!(guess_category("Electronics enclosure"), "enclosures");
        // whole words: "design" isn't "sign", "showcase" isn't "case", "donut" isn't "nut"
        assert_eq!(guess_category("A parametric birdhouse design for CNC routers"), "other");
        assert_eq!(guess_category("A showcase of donut shapes"), "other");
        assert_eq!(guess_category("Threaded inserts and gears"), "mechanical");
        assert_eq!(guess_category("Storage boxes"), "carrying");
        assert_eq!(guess_category("Laser-cut signs"), "labels");
        assert_eq!(guess_category("Raspberry Pi project box"), "enclosures");
        assert_eq!(humanize("my_bin-v2"), "My bin v2");
    }

    #[test]
    fn survey_a_project() {
        let d = std::env::temp_dir().join(format!("workshop-survey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("parts")).unwrap();
        std::fs::create_dir_all(d.join("lib")).unwrap();
        std::fs::write(d.join("box.scad"), "use <lib/util.scad>\nw = 2; // [1:5]\nbox(w);\n").unwrap();
        std::fs::write(d.join("box.json"), r#"{"parameterSets":{"Wide":{"w":"4"}},"fileFormatVersion":"1"}"#).unwrap();
        std::fs::write(d.join("lib/util.scad"), "module box(w) { cube(w); }\n").unwrap();
        std::fs::write(d.join("parts/Latch.stl"), b"solid").unwrap();
        std::fs::write(d.join("parts/latch.step"), b"step").unwrap();
        std::fs::write(d.join("README.md"), "# Box\n\nA box maker.\n").unwrap();
        std::fs::write(d.join("LICENSE"), "MIT License\nPermission is hereby granted, free of charge").unwrap();
        let f = survey(&d);
        assert_eq!(f.entries, vec!["box.scad"]);
        assert_eq!(f.library_files, 1);
        assert_eq!(f.license.as_ref().unwrap().1, Some("MIT"));
        assert_eq!(f.parts.len(), 1);
        assert_eq!(f.parts[0]["files"].as_array().unwrap().len(), 2);
        assert_eq!(f.parts[0]["preview"], json!("parts/Latch.stl"));
        // one item per name; its 3D view from the 3MF, else STL, else OBJ
        std::fs::write(d.join("parts/latch.3mf"), b"3mf").unwrap();
        std::fs::write(d.join("parts/knob.obj"), b"v 0 0 0").unwrap();
        let f = survey(&d);
        assert_eq!(f.parts.len(), 2);
        let preview = |stem: &str| f.parts.iter().find(|p| p["id"].as_str().is_some_and(|i| i.contains(stem))).map(|p| p["preview"].clone());
        assert_eq!(preview("latch"), Some(json!("parts/latch.3mf")));
        assert_eq!(preview("knob"), Some(json!("parts/knob.obj")));
        assert_eq!(presets_for(&d, "box.scad"), vec![json!({"label": "Wide", "values": {"w": 4}})]);
        let r = resolver(&d, &[]);
        let (files, missing) = r.collect("box.scad");
        assert!(missing.is_empty());
        assert_eq!(files.keys().cloned().collect::<Vec<_>>(), vec!["/box.scad", "/lib/util.scad"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_of_examples() {
        // F-15: more than 20 files that only call the main file's modules (LEGO.scad's examples/)
        let d = std::env::temp_dir().join(format!("workshop-examples-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("examples")).unwrap();
        std::fs::write(d.join("blocks.scad"), "module block(w = 1, l = 2) { cube([w * 8, l * 8, 9.6]); }\n").unwrap();
        for i in 1..=21 {
            std::fs::write(d.join(format!("examples/brick-1x{i}.scad")), format!("use <../blocks.scad>;\n$fn = 24;\nblock(w = 1, l = {i});\n")).unwrap();
        }
        let f = survey(&d);
        assert_eq!(f.examples, Some(json!({ "folder": "examples", "main": "blocks.scad", "count": 21 })));
        // one with a setting of its own: they're models (Gears' examples)
        std::fs::write(d.join("examples/brick-1x1.scad"), "use <../blocks.scad>;\nlength = 3; // [1:10]\nblock(w = 1, l = length);\n").unwrap();
        assert_eq!(survey(&d).examples, None);
        // twenty aren't a folder of examples
        std::fs::remove_file(d.join("examples/brick-1x1.scad")).unwrap();
        assert_eq!(survey(&d).examples, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn made_parts_carry_their_recipe() {
        // local/Made parts/ (milestone 4): a part saved from a model has <name>.recipe.json beside it
        let d = std::env::temp_dir().join(format!("workshop-made-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Basic Bin")).unwrap();
        std::fs::write(d.join("Basic Bin/Basic Bin 3×2.stl"), b"solid").unwrap();
        std::fs::write(d.join("Basic Bin/Basic Bin 3×2.recipe.json"), r#"{"model":"gridfinity-rebuilt/bin","values":{"gridx":3}}"#).unwrap();
        std::fs::write(d.join("Basic Bin/other_part.stl"), b"solid").unwrap();
        let f = survey(&d);
        assert_eq!(f.parts.len(), 2);
        let made = f.parts.iter().find(|p| p["generator"] == "gridfinity-rebuilt/bin").unwrap();
        assert_eq!(made["name"], json!("Basic Bin 3×2")); // the name it was given, as it is
        assert_eq!(made["made_from"]["values"]["gridx"], json!(3));
        assert_eq!(made["recipe"], json!("Basic Bin/Basic Bin 3×2.recipe.json"));
        let plain = f.parts.iter().find(|p| p["generator"].is_null()).unwrap();
        assert_eq!(plain["name"], json!("Other part"));
        assert!(plain.get("made_from").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn front_ends_over_a_shared_engine() {
        // Birdhouse Workshop's layout: front ends set settings and include an engine that
        // draws at top level; one front end draws itself with the engine switched off
        let d = std::env::temp_dir().join(format!("workshop-engine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("lib")).unwrap();
        std::fs::write(d.join("src/engine.scad"), "module body(s) { cube(s); }\nauto = is_undef(autorun) ? true : autorun;\nif (auto) body(size);\n").unwrap();
        // the engine never sets `size`: only its front ends do
        std::fs::write(d.join("src/square.scad"), "size = 10; // [1:50]\ninclude <engine.scad>\n").unwrap();
        // through another include, and with the include written with a folder
        std::fs::write(d.join("src/common.scad"), "wall = 2;\ninclude <engine.scad>\n").unwrap();
        std::fs::write(d.join("tall.scad"), "size = 30;\ninclude <src/common.scad>\n").unwrap();
        std::fs::write(d.join("src/castle.scad"), "autorun = false;\nsize = 5;\ninclude <engine.scad>\nmodule keep() { body(size * 2); }\nkeep();\n").unwrap();
        // not front ends: `use` runs no top-level code; a file with modules of its own is a library;
        // a library with no geometry stays one
        std::fs::write(d.join("used.scad"), "size = 3;\nuse <src/engine.scad>\n").unwrap();
        std::fs::write(d.join("lib/shapes.scad"), "r = 1;\nmodule ball() { sphere(r); }\ninclude <../src/engine.scad>\n").unwrap();
        std::fs::write(d.join("lib/util.scad"), "module plate() { cube(1); }\n").unwrap();
        std::fs::write(d.join("config.scad"), "w = 2;\ninclude <lib/util.scad>\n").unwrap();
        // a guard flag checked with is_undef (BOSL2's std.scad over vnf.scad) isn't a setting
        std::fs::write(d.join("lib/guarded.scad"), "_G = is_undef(_STD) ? echo(\"use std\") : 0;\ncube(1);\n").unwrap();
        std::fs::write(d.join("lib/std.scad"), "_STD = true;\ninclude <guarded.scad>\n").unwrap();
        let f = survey(&d);
        assert_eq!(f.entries, vec!["lib/guarded.scad", "src/castle.scad", "src/square.scad", "tall.scad"]);
        assert_eq!(f.scad_files, 11);
        assert_eq!(f.library_files, 7);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_drawing_file_nobody_fronts_stays_a_model() {
        // an example that includes a library with a demo at top level: both draw, neither is a front end
        let d = std::env::temp_dir().join(format!("workshop-demo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("examples")).unwrap();
        std::fs::write(d.join("gear.scad"), "teeth = 10; // [5:50]\nmodule gear(n) { cylinder(r = n); }\ngear(teeth);\n").unwrap();
        std::fs::write(d.join("examples/big.scad"), "include <../gear.scad>\ngear(40);\n").unwrap();
        // settings that gear.scad already has (GridFlock's docs/ animations), and one it never reads
        std::fs::write(d.join("examples/many.scad"), "teeth = 40;\nframes = 100;\ninclude <../gear.scad>\n").unwrap();
        let f = survey(&d);
        assert_eq!(f.entries, vec!["examples/big.scad", "gear.scad"]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
