//! Library modules as forms: the "Components" section (docs/DESKTOP_PLAN.md,
//! section 2). Reading a library gives an index of its usable modules (what
//! they're called, their arguments, docs and examples); a component's model
//! page is made from that index entry, and rendering it writes a small file
//! that includes the library and calls the module with the values set.
//!
//! Three ways of reading a library:
//! - BOSL2-style doc comments (`// Module:`, `// Function&Module:`, with
//!   Synopsis, Topics, Arguments and Examples): only documented modules count;
//! - NopSCADlib-style `//!` comments on the module line: only those modules;
//! - anything else: every public module, with the comment above it.

use crate::ingest::{self, dirname, join, normpath};
use crate::scan;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Version of the index format (bump to read libraries again). 2: `listed` (choices from the docs).
/// 3: a module's own list of types (`threaded_inserts`) before its file's.
pub const FORMAT: u64 = 3;

/// Files a library keeps (as tools/fetch_libraries.py): OpenSCAD files and the data files they import.
const KEEP_EXT: [&str; 8] = ["scad", "stl", "dxf", "svg", "dat", "json", "csv", "off"];
const SKIP_TOP: [&str; 9] = ["tests", "test", "docs", "gallery", "examples", "scripts", "tutorials", "images", ".github"];

/// Topic groups in the Components section, in menu order.
pub const GROUPS: [(&str, &str); 16] = [
    ("gears", "Gears & pulleys"),
    ("threads", "Threads"),
    ("fasteners", "Screws, nuts & fasteners"),
    ("bearings", "Bearings"),
    ("hinges", "Hinges & joints"),
    ("motors", "Motors"),
    ("electronics", "Electronics"),
    ("enclosures", "Boxes & enclosures"),
    ("shapes3d", "3D shapes"),
    ("shapes2d", "2D shapes"),
    ("rounding", "Rounding & masks"),
    ("paths", "Paths & sweeps"),
    ("textures", "Textures & patterns"),
    ("text", "Text"),
    ("hardware", "Hardware"),
    ("other", "Other"),
];

pub fn group_label(id: &str) -> &'static str {
    GROUPS.iter().find(|(g, _)| *g == id).map(|(_, l)| *l).unwrap_or("Other")
}

/// The Parametric Models category a pinned component starts in.
pub fn model_category(group: &str) -> &'static str {
    match group {
        "gears" | "threads" | "bearings" | "hinges" | "motors" => "mechanical",
        "fasteners" => "fasteners",
        "enclosures" => "enclosures",
        "text" => "labels",
        _ => "other",
    }
}

// ------------------------------------------------------------------ source scanning

/// The text with comments replaced by spaces (newlines kept) and strings left as they are,
/// so byte offsets match the original.
fn mask_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    out[i] = b' ';
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    if b[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
                for o in out.iter_mut().take((i + 2).min(b.len())).skip(i) {
                    *o = b' ';
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Index just past the bracket matching the one at `open` (strings skipped).
fn matching(s: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open;
    while i < s.len() {
        match s[i] {
            b'"' => {
                i += 1;
                while i < s.len() && s[i] != b'"' {
                    if s[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Split at top-level `sep` (outside brackets and strings).
pub fn split_top(s: &str, sep: u8) -> Vec<String> {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(s[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(s[start..].to_string());
    out
}

/// `name` or `name = expr` -> (name, expr). None if it isn't an argument.
fn split_arg(a: &str) -> Option<(String, Option<String>)> {
    let a = a.trim();
    if a.is_empty() {
        return None;
    }
    let b = a.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$') {
        i += 1;
    }
    let name = &a[..i];
    let rest = a[i..].trim_start();
    if name.is_empty() || name.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    if rest.is_empty() {
        return Some((name.to_string(), None));
    }
    let rest = rest.strip_prefix('=')?;
    if rest.starts_with('=') {
        return None;
    }
    Some((name.to_string(), Some(rest.trim().to_string())))
}

/// A module definition found in a file.
#[derive(Debug, Clone)]
pub struct ModuleDef {
    pub name: String,
    pub args: Vec<(String, Option<String>)>,
    pub line: usize,
    /// The body calls children() unconditionally and makes nothing itself (an operator).
    pub needs_children: bool,
    /// The body calls children() unconditionally (with or without geometry of its own).
    pub uses_children: bool,
    /// Primitives in the body: (2D ones, 3D ones).
    pub prims: (bool, bool),
    /// Draws only in OpenSCAD's preview (`if ($preview)` with no else): nothing to export.
    pub preview_only: bool,
    /// `//!` on the module line (NopSCADlib), else the comment right above it.
    pub doc_bang: Option<String>,
    pub doc_above: Option<String>,
}

fn prims(body: &str) -> (bool, bool) {
    let has = |w: &str| {
        body.match_indices(w).any(|(i, _)| {
            let before = body[..i].chars().last();
            let after = body[i + w.len()..].trim_start();
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_') && after.starts_with('(')
        })
    };
    let two = ["polygon", "square", "circle", "text"].iter().any(|w| has(w));
    let three = ["cube", "cylinder", "sphere", "polyhedron", "linear_extrude", "rotate_extrude", "import", "surface", "minkowski", "hull"]
        .iter()
        .any(|w| has(w));
    (two, three)
}

/// Top-level module definitions in a file.
pub fn find_modules(text: &str) -> Vec<ModuleDef> {
    let masked = mask_comments(text);
    let b = masked.as_bytes();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    let mut depth = 0i32;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'{' => depth += 1,
            b'}' => depth = (depth - 1).max(0),
            b'm' if depth == 0 && masked[i..].starts_with("module") && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) => {
                let after = &masked[i + 6..];
                if !after.starts_with(|c: char| c.is_whitespace()) {
                    i += 1;
                    continue;
                }
                let name_start = i + 6 + (after.len() - after.trim_start().len());
                let name: String = masked[name_start..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let mut j = name_start + name.len();
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if name.is_empty() || j >= b.len() || b[j] != b'(' {
                    i += 1;
                    continue;
                }
                let Some(close) = matching(b, j) else { break };
                let args: Vec<(String, Option<String>)> = split_top(&masked[j + 1..close - 1], b',').iter().filter_map(|a| split_arg(a)).collect();
                let mut k = close;
                while k < b.len() && b[k].is_ascii_whitespace() {
                    k += 1;
                }
                let (body, end) = if k < b.len() && b[k] == b'{' {
                    let e = matching(b, k).unwrap_or(b.len());
                    (&masked[k..e], e)
                } else {
                    // `module m() statement;`
                    let e = masked[k..].find(';').map(|p| k + p + 1).unwrap_or(b.len());
                    (&masked[k..e], e)
                };
                let line = masked[..i].matches('\n').count();
                let pr = prims(body);
                // an operator: passes its children through and makes nothing of its own
                let uses_children = body.contains("children(") && !body.contains("$children");
                let needs_children = uses_children && !pr.0 && !pr.1;
                let mod_line = lines.get(line).copied().unwrap_or("");
                let doc_bang = mod_line.find("//!").map(|p| mod_line[p + 3..].trim().to_string()).filter(|s| !s.is_empty());
                let doc_above = comment_above(&lines, line);
                let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
                let preview_only = compact.contains("if($preview)") && !compact.contains("else");
                out.push(ModuleDef { name, args, line: line + 1, needs_children, uses_children, prims: pr, preview_only, doc_bang, doc_above });
                i = end;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The comment block right above a line (`//` lines or one `/* */` block), as text.
fn comment_above(lines: &[&str], line: usize) -> Option<String> {
    let mut i = line;
    let mut got: Vec<String> = vec![];
    if i > 0 && lines[i - 1].trim_end().ends_with("*/") {
        let mut block = vec![];
        while i > 0 {
            i -= 1;
            let l = lines[i];
            block.push(l);
            if l.contains("/*") {
                break;
            }
        }
        block.reverse();
        let text = block.join("\n");
        let text = text.trim().trim_start_matches("/*").trim_end_matches("*/");
        got = text.lines().map(|l| l.trim().trim_start_matches('*').trim().to_string()).collect();
    } else {
        while i > 0 && lines[i - 1].trim_start().starts_with("//") {
            i -= 1;
            got.push(lines[i].trim_start().trim_start_matches('/').trim_start_matches('!').trim().to_string());
        }
        got.reverse();
    }
    let text = got.into_iter().filter(|l| !l.chars().all(|c| "-=*/ ".contains(c)) || l.is_empty()).collect::<Vec<_>>().join("\n");
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

// ------------------------------------------------------------------ settings from a model's calls

/// Whether a file is only includes and calls (what `lift_call_values` works on): a quick look
/// before reading the files it includes.
pub fn only_calls(text: &str) -> bool {
    let masked = mask_comments(text);
    let mut rest = String::with_capacity(masked.len());
    for line in masked.split('\n') {
        let t = line.trim();
        if !((t.starts_with("include") || t.starts_with("use")) && t.contains('<')) {
            rest.push_str(line);
            rest.push('\n');
        }
    }
    !rest.contains('{')
        && !rest.contains('}')
        && split_top(&rest, b';').iter().all(|st| {
            let st = st.trim_start();
            let name_end = st.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(st.len());
            st.trim().is_empty() || (name_end > 0 && st[name_end..].trim_start().starts_with('('))
        })
}

/// A model file whose values are typed into its module calls (`spur_gear(modul=1, tooth_number=30);`,
/// like the Gears project's examples) gives OpenSCAD's Customizer nothing to list. This returns a copy
/// with each literal argument lifted into a top-level setting after the includes (named after the
/// argument, described by the library's own comment for it where there is one), which OpenSCAD then
/// lists as usual; the calls use the settings. `source_rel` names the original in the copy's header;
/// `libs` are the texts of the files it includes (for argument names, descriptions and their globals),
/// `hints` the project's other files. A setting borrows the description and choice list the library
/// gives its own Customizer variable for the argument (see `borrowed_args`), and a number written
/// as a fraction (`height=1/3`) is lifted too, as the matching choice when there's a list.
/// None when the file has settings of its own, blocks or definitions, or nothing to lift.
pub fn lift_call_values(text: &str, source_rel: &str, libs: &[String], hints: &[String]) -> Option<String> {
    let masked = mask_comments(text);
    // includes and uses stay as they are, first
    let mut head: Vec<String> = vec![];
    let mut rest = String::with_capacity(masked.len());
    for (line, orig) in masked.split('\n').zip(text.split('\n')) {
        let t = line.trim();
        if (t.starts_with("include") || t.starts_with("use")) && t.contains('<') && t.trim_end_matches(';').trim_end().ends_with('>') {
            head.push(orig.trim().to_string());
            rest.push_str(&" ".repeat(line.len()));
        } else {
            rest.push_str(line);
        }
        rest.push('\n');
    }
    if rest.contains('{') || rest.contains('}') {
        return None; // definitions or blocks: not a plain list of calls
    }
    // the modules the included files define, and their top-level variables (which only `include`
    // brings into the file: a setting of the same name would change the library's, so it's renamed)
    let defs: Vec<ModuleDef> = libs.iter().flat_map(|t| find_modules(t)).collect();
    let mut globals: BTreeSet<String> = BTreeSet::new();
    let included = head.iter().any(|h| h.starts_with("include"));
    for t in libs.iter().filter(|_| included) {
        for l in mask_comments(t).lines() {
            if let Some((n, v)) = l.split_once('=') {
                let n = n.trim_end();
                if !l.starts_with(' ') && !l.starts_with('\t') && !v.starts_with('=') && !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    globals.insert(n.to_string());
                }
            }
        }
    }
    struct Lifted {
        var: String,
        value: String,
        doc: Option<String>,
        note: Option<String>,
    }
    let texts: Vec<&str> = libs.iter().chain(hints).map(String::as_str).collect();
    let mut borrowed: BTreeMap<String, BTreeMap<String, Borrowed>> = BTreeMap::new();
    let mut blocks: Vec<(String, Vec<Lifted>)> = vec![];
    let mut calls: Vec<String> = vec![];
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for stmt in split_top(&rest, b';') {
        let st = stmt.trim();
        if st.is_empty() {
            continue;
        }
        // a chain of calls: translate([80, 0, 0]) planetary_gear(...)
        let b = st.as_bytes();
        let mut i = 0;
        let mut parts: Vec<(String, usize, usize)> = vec![]; // (name, args start, args end)
        while i < b.len() {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= b.len() {
                break;
            }
            let s0 = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let name = &st[s0..i];
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if name.is_empty() || name.as_bytes()[0].is_ascii_digit() || b.get(i) != Some(&b'(') {
                return None; // an assignment (settings of its own) or something else
            }
            let end = matching(b, i)?; // just past the ")"
            parts.push((name.to_string(), i + 1, end - 1));
            i = end;
        }
        let (module, a0, a1) = parts.last()?.clone();
        let n = {
            let c = count.entry(module.clone()).or_insert(0);
            *c += 1;
            *c
        };
        let def = defs.iter().find(|d| d.name == module);
        let docs = def.map(arg_descriptions).unwrap_or_default();
        let lent = borrowed.entry(module.clone()).or_insert_with(|| borrowed_args(&texts, &module));
        let mut lifted = vec![];
        let mut args_out = vec![];
        for (k, a) in split_top(&st[a0..a1], b',').into_iter().enumerate() {
            let a = a.trim();
            if a.is_empty() {
                continue;
            }
            let (name, value) = match split_arg(a) {
                Some((nm, Some(v))) => (Some(nm), v),
                _ => (def.and_then(|d| d.args.get(k)).map(|x| x.0.clone()), a.to_string()),
            };
            // a literal, or a number written as arithmetic (`1/3`)
            let literal = parse_literal(&value).filter(|v| !v.is_null()).or_else(|| const_number(&value).map(|x| ingest::int_if_whole(&json!(x))));
            match (name, literal) {
                (Some(nm), Some(lit)) if !nm.starts_with('$') => {
                    let mut var = if n > 1 { format!("{nm}_{n}") } else { nm.clone() };
                    if globals.contains(&var) || used.contains(&var) {
                        var = format!("{module}_{var}");
                    }
                    used.insert(var.clone());
                    args_out.push(format!("{nm}={var}"));
                    let lent = lent.get(&nm);
                    let fit = lent.and_then(|b| b.note.as_ref()).and_then(|(note, lv)| fit_note(note, lv, &lit, &value));
                    let (value, note) = match fit {
                        Some((v, n)) => (v, Some(n)),
                        None if parse_literal(&value).is_some() => (value, None),
                        None => (to_scad(&lit), None),
                    };
                    let doc = docs.get(&nm).cloned().or_else(|| lent.and_then(|b| b.doc.clone()));
                    lifted.push(Lifted { var, value, doc, note });
                }
                (name, _) => match name {
                    Some(nm) if split_arg(a).is_some_and(|x| x.1.is_some()) => args_out.push(format!("{nm}={value}")),
                    _ => args_out.push(a.to_string()),
                },
            }
        }
        let prefix = &st[..parts.last().map(|_| a0 - 1 - module.len()).unwrap_or(0)];
        calls.push(format!("{}{module}({});", prefix.trim_start(), args_out.join(", ")));
        if !lifted.is_empty() {
            let title = if count[&module] > 1 || n > 1 { format!("{} {n}", humanize_module(&module)) } else { humanize_module(&module) };
            blocks.push((title, lifted));
        }
    }
    if blocks.is_empty() {
        return None;
    }
    // a module called more than once: number its first block too
    let mut out = format!(
        "// Made by SCAD Workshop from {source_rel}: the values typed into its module calls are settings\n// here, so the form can change them. The original file is unchanged.\n\n"
    );
    for h in &head {
        out.push_str(h);
        out.push('\n');
    }
    for (title, lifted) in &blocks {
        let title = if count.iter().any(|(m, c)| *c > 1 && *title == humanize_module(m)) { format!("{title} 1") } else { title.clone() };
        out.push_str(&format!("\n/* [{title}] */\n"));
        for l in lifted {
            if let Some(d) = &l.doc {
                out.push_str(&format!("// {}\n", d.replace('\n', " ")));
            }
            match &l.note {
                Some(n) => out.push_str(&format!("{} = {}; // [{n}]\n", l.var, l.value)),
                None => out.push_str(&format!("{} = {};\n", l.var, l.value)),
            }
        }
    }
    out.push_str("\n/* [Hidden] */\n\n");
    for c in &calls {
        out.push_str(c);
        out.push('\n');
    }
    Some(out)
}

/// `name = description` lines in a module's comment (the Gears library's style), by argument name.
fn arg_descriptions(def: &ModuleDef) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let Some(doc) = def.doc_above.as_deref() else { return out };
    let names: BTreeSet<&str> = def.args.iter().map(|a| a.0.as_str()).collect();
    let mut current: Option<String> = None;
    for line in doc.lines() {
        let t = line.trim();
        match t.split_once('=') {
            Some((n, d)) if names.contains(n.trim()) && !d.starts_with('=') => {
                current = Some(n.trim().to_string());
                out.insert(n.trim().to_string(), d.trim().to_string());
            }
            _ => {
                if let Some(c) = &current {
                    if !t.is_empty() {
                        let e = out.get_mut(c).unwrap();
                        e.push(' ');
                        e.push_str(t);
                    }
                }
            }
        }
    }
    out
}

/// A top-level Customizer variable: its comment, its `// [...]` note (without the brackets), its value.
struct DocVar {
    doc: Option<String>,
    note: Option<String>,
    value: String,
}

/// The single-line top-level assignments of a file that have a comment above or a `// [...]` note.
fn documented_vars(text: &str) -> BTreeMap<String, DocVar> {
    let masked = mask_comments(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = BTreeMap::new();
    let mut depth = 0i32;
    for (i, (m, orig)) in masked.split('\n').zip(text.split('\n')).enumerate() {
        let top = depth == 0;
        let mut in_str = false;
        let mut esc = false;
        for c in m.bytes() {
            match c {
                _ if esc => esc = false,
                b'\\' if in_str => esc = true,
                b'"' => in_str = !in_str,
                b'{' | b'(' | b'[' if !in_str => depth += 1,
                b'}' | b')' | b']' if !in_str => depth -= 1,
                _ => {}
            }
        }
        if !top {
            continue;
        }
        let Some((name, rest)) = m.split_once('=') else { continue };
        let name = name.trim();
        if rest.starts_with('=') || name.is_empty() || name.as_bytes()[0].is_ascii_digit() || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
            continue;
        }
        let Some(semi) = rest.rfind(';') else { continue };
        let value = rest[..semi].trim().to_string();
        let after = &orig[(m.len() - rest.len() + semi + 1).min(orig.len())..];
        let note = after
            .trim()
            .strip_prefix("//")
            .map(str::trim)
            .and_then(|n| n.strip_prefix('[')?.strip_suffix(']'))
            .map(|n| n.trim().to_string());
        // a section header right above (`/* [General] */`) isn't a description
        let doc = comment_above(&lines, i).filter(|d| !(d.starts_with('[') && d.ends_with(']')));
        if doc.is_some() || note.is_some() {
            out.insert(name.to_string(), DocVar { doc, note, value });
        }
    }
    out
}

/// What a library says about one argument of a module.
#[derive(Debug, Clone, Default, PartialEq)]
struct Borrowed {
    doc: Option<String>,
    /// The Customizer note of the variable passed straight to the argument (`width=block_width`),
    /// with that variable's own value (so the note is only used for a value of the same type).
    note: Option<(String, String)>,
}

/// Descriptions and choice lists for a module's arguments, from calls in `texts` that pass a
/// documented top-level variable to them: LEGO.scad's own `block(width=block_width, type=block_type,
/// reinforcement=(use_reinforcement=="yes"), ...)` under `// Width of the block, in studs` and
/// `block_type = "brick"; // [brick:Brick, tile:Tile, ...]`. A variable passed straight gives its
/// description and its note; an expression of one variable gives its description only. The first
/// text that says something about an argument wins.
fn borrowed_args(texts: &[&str], module: &str) -> BTreeMap<String, Borrowed> {
    let mut out: BTreeMap<String, Borrowed> = BTreeMap::new();
    for text in texts {
        if !text.contains(module) {
            continue;
        }
        let masked = mask_comments(text);
        let b = masked.as_bytes();
        let mut vars: Option<BTreeMap<String, DocVar>> = None;
        let mut from = 0;
        while let Some(off) = masked[from..].find(module) {
            let at = from + off;
            from = at + module.len();
            let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
            if at > 0 && ident(b[at - 1]) || b.get(from).is_some_and(|&c| ident(c)) {
                continue;
            }
            let before = masked[..at].trim_end();
            if before.ends_with("module") || before.ends_with("function") {
                continue;
            }
            let mut open = from;
            while open < b.len() && b[open].is_ascii_whitespace() {
                open += 1;
            }
            if b.get(open) != Some(&b'(') {
                continue;
            }
            let Some(end) = matching(b, open) else { continue };
            let vars = vars.get_or_insert_with(|| documented_vars(text));
            if vars.is_empty() {
                break;
            }
            for a in split_top(&masked[open + 1..end - 1], b',') {
                let Some((name, Some(expr))) = split_arg(&a) else { continue };
                let expr = expr.trim();
                let got = if let Some(v) = vars.get(expr) {
                    Borrowed { doc: v.doc.clone(), note: v.note.clone().map(|n| (n, v.value.clone())) }
                } else {
                    // the variables the expression reads (outside its strings)
                    let (mut code, mut in_str, mut esc) = (String::new(), false, false);
                    for c in expr.chars() {
                        match c {
                            _ if esc => esc = false,
                            '\\' if in_str => esc = true,
                            '"' => in_str = !in_str,
                            c if !in_str => code.push(c),
                            _ => {}
                        }
                        if in_str {
                            code.push(' ');
                        }
                    }
                    let mut names: BTreeSet<String> = BTreeSet::new();
                    for t in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                        if vars.contains_key(t) {
                            names.insert(t.to_string());
                        }
                    }
                    if names.len() != 1 {
                        continue;
                    }
                    let v = &vars[names.iter().next().unwrap()];
                    Borrowed { doc: v.doc.clone(), note: None }
                };
                let e = out.entry(name).or_default();
                if e.doc.is_none() {
                    e.doc = got.doc;
                }
                if e.note.is_none() {
                    e.note = got.note;
                }
            }
            from = end;
        }
    }
    out
}

/// A number written as arithmetic of numbers (`1/3`, `2+(2/3)`, `-0.5*4`); None for anything else.
fn const_number(s: &str) -> Option<f64> {
    fn expr(b: &[u8], i: &mut usize) -> Option<f64> {
        let mut v = term(b, i)?;
        loop {
            skip(b, i);
            match b.get(*i) {
                Some(b'+') => {
                    *i += 1;
                    v += term(b, i)?;
                }
                Some(b'-') => {
                    *i += 1;
                    v -= term(b, i)?;
                }
                _ => return Some(v),
            }
        }
    }
    fn term(b: &[u8], i: &mut usize) -> Option<f64> {
        let mut v = factor(b, i)?;
        loop {
            skip(b, i);
            match b.get(*i) {
                Some(b'*') => {
                    *i += 1;
                    v *= factor(b, i)?;
                }
                Some(b'/') => {
                    *i += 1;
                    v /= factor(b, i)?;
                }
                _ => return Some(v),
            }
        }
    }
    fn factor(b: &[u8], i: &mut usize) -> Option<f64> {
        skip(b, i);
        match b.get(*i)? {
            b'-' => {
                *i += 1;
                Some(-factor(b, i)?)
            }
            b'+' => {
                *i += 1;
                factor(b, i)
            }
            b'(' => {
                *i += 1;
                let v = expr(b, i)?;
                skip(b, i);
                (b.get(*i) == Some(&b')')).then(|| *i += 1)?;
                Some(v)
            }
            _ => {
                let s = *i;
                while *i < b.len() && (b[*i].is_ascii_digit() || b[*i] == b'.' || ((b[*i] == b'e' || b[*i] == b'E') && *i > s)) {
                    *i += 1;
                    if matches!(b[*i - 1], b'e' | b'E') && matches!(b.get(*i), Some(b'-') | Some(b'+')) {
                        *i += 1;
                    }
                }
                std::str::from_utf8(&b[s..*i]).ok()?.parse::<f64>().ok()
            }
        }
    }
    fn skip(b: &[u8], i: &mut usize) {
        while *i < b.len() && b[*i].is_ascii_whitespace() {
            *i += 1;
        }
    }
    let b = s.trim().as_bytes();
    let mut i = 0;
    let v = expr(b, &mut i)?;
    skip(b, &mut i);
    (i == b.len() && v.is_finite()).then_some(v)
}

/// A lifted value's Customizer note, from the library's note for the same argument: a choice list
/// keeps the value when it's one of the choices (a number written as `1/3` takes the choice's own
/// spelling, `.33333333333`), else gets it as one more choice, labelled as the file wrote it; a
/// range is kept when the value is inside it. Returns (the value to write, the note). None when
/// the note doesn't fit the value (another type: `reinforcement=true` for `"yes"`/`"no"`).
fn fit_note(note: &str, lib_value: &str, value: &Value, written: &str) -> Option<(String, String)> {
    let kind = |v: &Value| match v {
        Value::Number(_) => 1,
        Value::String(_) => 2,
        Value::Array(_) => 3,
        _ => 0,
    };
    let lib = parse_literal(lib_value)?;
    if kind(&lib) == 0 || kind(&lib) != kind(value) {
        return None;
    }
    let num = |s: &str| {
        let s = s.trim();
        s.parse::<f64>().ok().or_else(|| s.strip_prefix('.').and_then(|r| format!("0.{r}").parse().ok()))
    };
    let parts: Vec<&str> = note.split(':').collect();
    if !note.contains(',') && parts.len() <= 3 && parts.iter().all(|p| num(p).is_some()) {
        // a slider: [max], [min:max], [min:step:max]
        let (lo, hi) = (if parts.len() == 1 { None } else { num(parts[0]) }, num(parts[parts.len() - 1])?);
        let fits = match value {
            Value::Number(n) => n.as_f64().is_some_and(|x| lo.is_none_or(|lo| x >= lo) && x <= hi),
            _ => true,
        };
        return fits.then(|| (to_scad(value), note.to_string()));
    }
    if matches!(value, Value::Array(_)) {
        return None;
    }
    let items: Vec<(String, Option<String>)> = split_top(note, b',')
        .iter()
        .map(|it| match it.split_once(':') {
            Some((v, l)) => (v.trim().to_string(), Some(l.trim().to_string())),
            None => (it.trim().to_string(), None),
        })
        .collect();
    let unquote = |s: &str| s.trim_matches('"').to_string();
    match value {
        Value::Number(n) => {
            let x = n.as_f64()?;
            let vals: Option<Vec<f64>> = items.iter().map(|(v, _)| num(v)).collect();
            let vals = vals?;
            let close = |a: f64| (a - x).abs() <= 1e-9 * x.abs().max(1.0);
            if let Some(k) = vals.iter().position(|a| close(*a)) {
                return Some((items[k].0.clone(), note.to_string()));
            }
            // one more choice, in order
            let at = vals.iter().position(|a| *a > x).unwrap_or(vals.len());
            let label: String = written.chars().filter(|c| !c.is_whitespace()).collect();
            let mut list: Vec<String> = items.iter().map(|(v, l)| l.as_ref().map(|l| format!("{v}:{l}")).unwrap_or_else(|| v.clone())).collect();
            list.insert(at, format!("{}:{label}", to_scad(value)));
            Some((to_scad(value), list.join(", ")))
        }
        Value::String(s) => {
            if items.iter().any(|(v, _)| unquote(v) == *s) {
                return Some((to_scad(value), note.to_string()));
            }
            let plain = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || " _.-".contains(c));
            plain.then(|| (to_scad(value), format!("{note}, {s}:{s}")))
        }
        _ => None,
    }
}

// ------------------------------------------------------------------ BOSL2-style docs

#[derive(Debug, Default, Clone)]
struct Section {
    name: String,
    opts: String,
    title: String,
    body: Vec<String>,
}

/// The sections of one `//` doc block: "Synopsis", "Arguments", "Example"...
fn sections(block: &[&str]) -> Vec<Section> {
    let mut out: Vec<Section> = vec![];
    for l in block {
        let t = l.strip_prefix("//").unwrap_or(l);
        if let Some(rest) = t.strip_prefix(' ').filter(|r| !r.starts_with(' ') && !r.is_empty()) {
            // "// Name(opts): title"
            let head_end = rest.find(':');
            if let Some(he) = head_end {
                let head = &rest[..he];
                let (name, opts) = match head.find('(') {
                    Some(p) if head.ends_with(')') => (&head[..p], &head[p + 1..head.len() - 1]),
                    _ => (head, ""),
                };
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '&') && name.starts_with(|c: char| c.is_uppercase()) {
                    out.push(Section { name: name.trim().to_string(), opts: opts.to_string(), title: rest[he + 1..].trim().to_string(), body: vec![] });
                    continue;
                }
            }
        }
        if let Some(s) = out.last_mut() {
            // body lines: "//   text" (indented), or "//" (blank)
            let body = t.strip_prefix("   ").or_else(|| t.strip_prefix("  ")).unwrap_or(t.trim_start());
            s.body.push(body.trim_end().to_string());
        }
    }
    out
}

fn anchor_slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect::<String>()
        .replace(' ', "-")
}

/// Strip markdown the docs use: `code`, [link](target), **bold**.
fn plain(s: &str) -> String {
    let s = s.replace('`', "").replace("**", "").replace("{{", "").replace("}}", "");
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(p) = rest.find('[') {
        out.push_str(&rest[..p]);
        let tail = &rest[p + 1..];
        match (tail.find("]("), tail.find(')')) {
            (Some(a), Some(b)) if a < b && !tail[..a].contains('[') => {
                out.push_str(&tail[..a]);
                rest = &tail[b + 1..];
            }
            _ => {
                out.push('[');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn paragraphs(lines: &[String]) -> String {
    let mut paras: Vec<String> = vec![];
    let mut cur: Vec<String> = vec![];
    for l in lines {
        let t = l.trim();
        if t.is_empty() || t == "." {
            if !cur.is_empty() {
                paras.push(cur.join(" "));
                cur.clear();
            }
        } else {
            cur.push(plain(t));
        }
    }
    if !cur.is_empty() {
        paras.push(cur.join(" "));
    }
    paras.join("\n\n")
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ArgDoc {
    pub doc: String,
    pub default_text: Option<String>,
    pub named_only: bool,
}

/// "teeth = Number of teeth.  Default: 20" lines (aliases "l / h = ..." share one), and where `---` starts the named-only ones.
fn parse_arguments(body: &[String]) -> BTreeMap<String, ArgDoc> {
    let mut out: BTreeMap<String, ArgDoc> = BTreeMap::new();
    let mut named_only = false;
    let mut current: Vec<String> = vec![];
    let push = |names: &[String], text: &str, named_only: bool, out: &mut BTreeMap<String, ArgDoc>| {
        let doc = plain(text.trim());
        let default_text = doc.find("Default:").map(|p| doc[p + 8..].trim().trim_end_matches('.').trim().to_string());
        for n in names {
            out.insert(n.clone(), ArgDoc { doc: doc.clone(), default_text: default_text.clone(), named_only });
        }
    };
    let mut text = String::new();
    for l in body {
        let t = l.trim();
        if t == "---" {
            if !current.is_empty() {
                push(&current, &text, named_only, &mut out);
                current.clear();
                text.clear();
            }
            named_only = true;
            continue;
        }
        // a new argument starts at column 0 with "name =" or "a / b ="
        let starts = !l.starts_with(' ') && t.find(" = ").is_some_and(|p| {
            t[..p].split('/').all(|n| {
                let n = n.trim();
                !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
            })
        });
        if starts {
            if !current.is_empty() {
                push(&current, &text, named_only, &mut out);
            }
            let p = t.find(" = ").unwrap();
            current = t[..p].split('/').map(|n| n.trim().to_string()).collect();
            text = t[p + 3..].to_string();
        } else if !current.is_empty() && !t.is_empty() {
            text.push(' ');
            text.push_str(t);
        }
    }
    if !current.is_empty() {
        push(&current, &text, named_only, &mut out);
    }
    out
}

/// The values a setting's doc lists as its only choices: `One of "orig", "aligned", or "icosa"`,
/// `Select "hull" or "intersect" anchor types`, `"ltr" for left to right.  "rtl" for right to left.`
/// Runs of two or more quoted values joined by commas, "or", "and" (or each followed by what it
/// means); not examples (`e.g. "M5x1" or "#8-32"`, `Default is "2B" for UTS and "6H" for ISO`),
/// and nothing when the doc leaves the list open (`"a", "b", or a number`; `Alternatively, …`).
pub fn doc_choices(doc: &str) -> Option<Vec<String>> {
    let lower = doc.to_ascii_lowercase();
    if ["alternatively", "you can also", "you can give", "can also be"].iter().any(|w| lower.contains(w)) {
        return None;
    }
    // the quoted values (byte ranges inside the quotes)
    let mut quotes: Vec<(usize, usize)> = vec![];
    let mut i = 0;
    while let Some(o) = doc[i..].find('"') {
        let open = i + o + 1;
        let Some(c) = doc[open..].find('"') else { break };
        quotes.push((open, open + c));
        i = open + c + 1;
    }
    let ok_value = |v: &str| {
        !v.is_empty() && v.len() <= 30 && v.trim() == v && v.chars().all(|ch| ch.is_ascii_alphanumeric() || "#_-+. ".contains(ch))
    };
    if quotes.iter().any(|&(a, b)| !ok_value(&doc[a..b])) {
        return None; // inch marks or quoted prose: the pairing can't be trusted
    }
    // between two values of one list: `, `, ` or `, `, and `, ` (alias `, or ` for left to right.  `
    let joiner = |t: &str| {
        let t = t.to_ascii_lowercase();
        let words: Vec<&str> = t.split(|c: char| c.is_whitespace() || ",/()".contains(c)).filter(|w| !w.is_empty()).collect();
        let plain = words.len() <= 1 && words.iter().all(|w| matches!(*w, "or" | "and" | "alias"));
        let e = t.trim_end();
        let described = t.starts_with(" for ")
            && t.len() < 80
            && (e.ends_with(',') || e.ends_with('.') || e.ends_with(';') || e.ends_with(" and") || e.ends_with(" or"));
        plain || described
    };
    // the first list that the sentence introduces ("One of …", "Can be …", "Select …", or at the
    // start of a sentence); lists mentioned in passing ("with "direct" and "reindex" methods") don't count
    const LEADS: [&str; 14] = ["one of", "can be", "may be", "must be", "either", "are", "is", "select", "choose", "set to", "between", "using", "use", "to"];
    let mut k = 0;
    while k < quotes.len() {
        let mut end = k;
        while end + 1 < quotes.len() && joiner(&doc[quotes[end].1 + 1..quotes[end + 1].0 - 1]) {
            end += 1;
        }
        let run = &quotes[k..=end];
        k = end + 1;
        if run.len() < 2 {
            continue;
        }
        let before = lower[..run[0].0 - 1].trim_end();
        if ["e.g.", "e.g.,", "such as", "as in", "like", "for example", "for example,", "default is", "default:", "default", "["]
            .iter()
            .any(|w| before.ends_with(w))
        {
            continue;
        }
        let introduced = before.is_empty()
            || before.ends_with(['.', ':', ';', ',', '('])
            || LEADS.iter().any(|w| before.ends_with(w) && !before[..before.len() - w.len()].ends_with(|c: char| c.is_ascii_alphanumeric()));
        if !introduced {
            continue;
        }
        // the rest of the sentence: "…, or a 3D point" leaves it open
        let after = &lower[run[run.len() - 1].1 + 1..];
        let sentence = &after[..after.find('.').unwrap_or(after.len())];
        if [" or a ", " or an ", " or any ", " or other "].iter().any(|w| sentence.contains(w)) {
            return None;
        }
        let mut out: Vec<String> = vec![];
        for &(a, b) in run {
            let v = &doc[a..b];
            if !out.iter().any(|o| o == v) {
                out.push(v.to_string());
            }
        }
        return Some(out);
    }
    None
}

/// Choices other modules and functions document for their arguments ({name: {argument: values}}),
/// for docs that point to them ("style = vnf_vertex_array() style to use").
fn documented_choices(texts: &BTreeMap<String, String>) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
    let mut out: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for text in texts.values() {
        let lines: Vec<&str> = text.lines().collect();
        for blk in doc_blocks(&lines) {
            let Some(args) = blk.secs.iter().find(|s| s.name == "Arguments") else { continue };
            for (arg, d) in parse_arguments(&args.body) {
                if let Some(list) = doc_choices(&d.doc) {
                    out.entry(blk.name.clone()).or_default().insert(arg, list);
                }
            }
        }
    }
    out
}

/// A setting's choices: listed in its own doc, or in the doc of the module or function it points to.
fn arg_choices(arg: &str, doc: &str, own: &str, others: &BTreeMap<String, BTreeMap<String, Vec<String>>>) -> Option<Vec<String>> {
    doc_choices(doc).or_else(|| {
        others.iter().filter(|(name, _)| name.as_str() != own).find_map(|(name, args)| {
            let list = args.get(arg)?;
            // "vnf_vertex_array() style to use", "vnf_vertex_array style"
            let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            doc.match_indices(name.as_str())
                .any(|(p, _)| {
                    let rest = &doc[p + name.len()..];
                    !word(doc[..p].chars().next_back()) && (rest.starts_with("()") || rest.starts_with(&format!(" {arg}")))
                })
                .then(|| list.clone())
        })
    })
}

/// The library's file-level "Includes:" lines (BOSL2 LibFile header).
fn file_includes(lines: &[&str]) -> Vec<String> {
    let mut out = vec![];
    let mut in_inc = false;
    for l in lines.iter().take(80) {
        let t = l.trim();
        if !t.starts_with("//") {
            if in_inc {
                break;
            }
            continue;
        }
        let body = t.trim_start_matches('/').trim();
        if body.starts_with("Includes:") {
            in_inc = true;
            continue;
        }
        if in_inc {
            if body.starts_with("include <") || body.starts_with("use <") {
                out.push(body.trim_end_matches(';').to_string());
            } else if !body.is_empty() {
                break;
            }
        }
    }
    out
}

/// Doc blocks of modules in a BOSL2-style file.
struct DocBlock {
    kind: String,
    name: String,
    secs: Vec<Section>,
}

fn doc_blocks(lines: &[&str]) -> Vec<DocBlock> {
    let mut out = vec![];
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let head = l
            .strip_prefix("// Module: ")
            .map(|r| ("Module", r))
            .or_else(|| l.strip_prefix("// Function&Module: ").map(|r| ("Function&Module", r)))
            .or_else(|| l.strip_prefix("// Function: ").map(|r| ("Function", r)));
        if let Some((kind, rest)) = head {
            let name: String = rest.trim().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let start = i;
            while i < lines.len() && lines[i].starts_with("//") {
                i += 1;
            }
            if !name.is_empty() {
                out.push(DocBlock { kind: kind.into(), name, secs: sections(&lines[start..i]) });
            }
            continue;
        }
        i += 1;
    }
    out
}

// ------------------------------------------------------------------ OpenSCAD values

/// A literal OpenSCAD value as JSON (numbers, strings, booleans, undef, vectors of them).
pub fn parse_literal(s: &str) -> Option<Value> {
    let s = s.trim();
    if s == "true" {
        return Some(json!(true));
    }
    if s == "false" {
        return Some(json!(false));
    }
    if s == "undef" {
        return Some(Value::Null);
    }
    if let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return None,
                '\\' => match chars.next()? {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    c => out.push(c),
                },
                c => out.push(c),
            }
        }
        return Some(json!(out));
    }
    if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        if inner.trim().is_empty() {
            return Some(json!([]));
        }
        if inner.contains(':') || inner.trim_start().starts_with("for") {
            return None; // ranges, comprehensions
        }
        let items: Option<Vec<Value>> = split_top(inner, b',').iter().map(|x| parse_literal(x).filter(|v| !v.is_null())).collect();
        return items.map(Value::Array);
    }
    let num = s.strip_prefix('+').unwrap_or(s);
    let ok = num.chars().all(|c| c.is_ascii_digit() || ".-+eE".contains(c)) && num.chars().any(|c| c.is_ascii_digit());
    if ok {
        if let Ok(f) = num.parse::<f64>() {
            if f.is_finite() {
                return Some(ingest::int_if_whole(&json!(f)));
            }
        }
    }
    None
}

/// A value as written in a signature or an example: a literal, or arithmetic of numbers (`1/3`,
/// `[1/2, 0, 3]`) as the number it comes to (0.4.24). None for anything that needs OpenSCAD to work out.
pub fn folded(s: &str) -> Option<Value> {
    let s = s.trim();
    if let Some(v) = parse_literal(s) {
        return Some(v);
    }
    if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        if inner.contains(':') || inner.trim_start().starts_with("for") || inner.trim().is_empty() {
            return None;
        }
        let items: Option<Vec<Value>> = split_top(inner, b',').iter().map(|x| folded(x).filter(|v| v.is_number())).collect();
        return items.map(Value::Array);
    }
    const_number(s).filter(|f| f.is_finite()).map(|f| ingest::int_if_whole(&json!(f)))
}

/// A JSON value as OpenSCAD source.
pub fn to_scad(v: &Value) -> String {
    match v {
        Value::Null => "undef".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            let f = n.as_f64().unwrap_or(0.0);
            if n.is_i64() || n.is_u64() {
                n.to_string()
            } else if f == f.trunc() && f.abs() < 1e15 {
                format!("{}", f as i64)
            } else {
                n.to_string()
            }
        }
        Value::String(s) => {
            let mut o = String::from("\"");
            for c in s.chars() {
                match c {
                    '"' => o.push_str("\\\""),
                    '\\' => o.push_str("\\\\"),
                    '\n' => o.push_str("\\n"),
                    '\t' => o.push_str("\\t"),
                    '\r' => {}
                    c => o.push(c),
                }
            }
            o.push('"');
            o
        }
        Value::Array(a) => format!("[{}]", a.iter().map(to_scad).collect::<Vec<_>>().join(", ")),
        Value::Object(_) => "undef".into(),
    }
}

/// An expression typed into a form: one expression, no statements, no file access.
pub fn check_expression(s: &str) -> Result<()> {
    let masked = mask_comments(s);
    if masked != s {
        bail!("comments aren't allowed in a value ({s})");
    }
    let mut no_strings = String::new();
    let mut in_str = false;
    let mut esc = false;
    for c in s.chars() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        }
        no_strings.push(c);
    }
    if in_str {
        bail!("a string isn't closed in {s}");
    }
    if no_strings.contains([';', '{', '}']) {
        bail!("a value can't contain ; {{ or }} ({s})");
    }
    for w in ["import", "surface", "include", "use", "module"] {
        if no_strings.split(|c: char| !c.is_alphanumeric() && c != '_').any(|t| t == w) {
            bail!("a value can't use {w} ({s})");
        }
    }
    let mut depth = 0i32;
    for c in no_strings.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth -= 1;
                if depth < 0 {
                    bail!("brackets don't match in {s}");
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        bail!("brackets don't match in {s}");
    }
    Ok(())
}

// ------------------------------------------------------------------ examples

/// One statement that is a plain call of the module: its arguments by name
/// (positional ones named from the signature, example variables filled in).
fn plain_call(stmt: &str, module: &str, arg_names: &[String], vars: &BTreeMap<String, String>) -> Option<BTreeMap<String, String>> {
    let rest = stmt.trim().strip_prefix(module)?.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let close = matching(rest.as_bytes(), 0)?;
    if !rest[close..].trim().is_empty() {
        return None; // children or more after the call
    }
    let mut out = BTreeMap::new();
    let mut pos = 0;
    for a in split_top(&rest[1..close - 1], b',') {
        let a = a.trim();
        if a.is_empty() {
            continue;
        }
        let (name, value) = match split_arg(a) {
            Some((n, Some(v))) => (n, v),
            _ => {
                let n = arg_names.get(pos)?.clone();
                pos += 1;
                (n, a.to_string())
            }
        };
        if name.starts_with('$') && !["$fn", "$fa", "$fs"].contains(&name.as_str()) {
            continue;
        }
        if !arg_names.contains(&name) && !name.starts_with('$') {
            return None;
        }
        let value = vars.get(value.trim()).cloned().unwrap_or(value);
        out.insert(name, value.trim().to_string());
    }
    Some(out)
}

/// The plain calls of the module in a doc example: one set of argument values per call.
/// Simple `name = value;` assignments before them are filled in; the example must
/// start with a plain call (after those), and other statements after it are skipped.
fn example_calls(code: &str, module: &str, arg_names: &[String]) -> Vec<BTreeMap<String, String>> {
    let masked = mask_comments(code);
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    let mut specials: BTreeMap<String, String> = BTreeMap::new();
    let mut out = vec![];
    for st in split_top(&masked, b';').iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if let Some((n, Some(v))) = split_arg(st) {
            if !out.is_empty() {
                continue;
            }
            if parse_literal(&v).is_some_and(|l| !l.is_null()) {
                if n.starts_with('$') {
                    specials.insert(n, v);
                } else {
                    vars.insert(n, v);
                }
                continue;
            }
            return vec![]; // a computed variable the call may use
        }
        match plain_call(st, module, arg_names, &vars) {
            Some(mut v) => {
                for (k, x) in &specials {
                    if ["$fn", "$fa", "$fs"].contains(&k.as_str()) {
                        v.entry(k.clone()).or_insert_with(|| x.clone());
                    }
                }
                out.push(v);
            }
            None if out.is_empty() => return vec![],
            None => {}
        }
    }
    out
}

// ------------------------------------------------------------------ reading a library

/// One usable module, as stored in the library index.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Component {
    pub module: String,
    pub file: String,
    pub line: usize,
    pub prelude: Vec<String>,
    pub fileset: usize,
    pub title: String,
    pub synopsis: String,
    pub description: String,
    pub topics: Vec<String>,
    pub group: String,
    /// 2 for a 2D shape (extruded to show and print), else 3
    pub dim: u8,
    pub doc_url: Option<String>,
    /// (name, default expression as written)
    pub args: Vec<(String, Option<String>)>,
    pub arg_docs: BTreeMap<String, ArgDoc>,
    /// the module's named anchors (BOSL2)
    pub anchors: Vec<String>,
    pub examples: Vec<Example>,
    /// choices for an argument: OpenSCAD expressions (NopSCADlib type constants)
    pub choices: BTreeMap<String, Vec<String>>,
    /// text arguments whose docs list their values (their own doc, or the doc it points to)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub listed: BTreeMap<String, Vec<String>>,
    /// its defaults don't make a shape (catalog/components.json "$no_guess"): wait for values
    #[serde(default)]
    pub hold: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Example {
    pub title: String,
    pub code: String,
    /// the example's plain calls of the module, as argument values
    pub calls: Vec<BTreeMap<String, String>>,
    /// drawn in 2D in the docs
    #[serde(default)]
    pub flat: bool,
}

/// What a library is: its name, where it came from, its license.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LibInfo {
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub docs: Option<String>,
}

fn keep_file(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.iter().any(|p| p.starts_with('.')) || (parts.len() > 1 && SKIP_TOP.contains(&parts[0])) {
        return false;
    }
    Path::new(rel).extension().and_then(|e| e.to_str()).is_some_and(|e| KEEP_EXT.contains(&e.to_lowercase().as_str()))
}

fn topic_group(t: &str) -> Option<&'static str> {
    let t = t.to_lowercase();
    let is = |k: &[&str]| k.iter().any(|k| t == *k || t.starts_with(k));
    Some(if is(&["gears"]) {
        "gears"
    } else if is(&["threading", "threads", "threaded"]) {
        "threads"
    } else if is(&["screws", "nuts", "fasteners"]) {
        "fasteners"
    } else if is(&["bearings"]) {
        "bearings"
    } else if is(&["hinges", "joiners", "joints", "snaps"]) {
        "hinges"
    } else if is(&["textures", "knurling", "patterns"]) {
        "textures"
    } else if t == "text" {
        "text"
    } else if is(&["rounding", "masking", "masks", "chamfers", "fillets"]) {
        "rounding"
    } else if is(&["shapes (2d)"]) {
        "shapes2d"
    } else if is(&["shapes (3d)"]) {
        "shapes3d"
    } else if is(&["paths", "sweep", "extrusion", "path generators", "regions", "vnf", "bezier", "nurbs"]) {
        "paths"
    } else if is(&["trusses", "cubetruss"]) {
        "shapes3d"
    } else if is(&["parts", "fdm optimized"]) {
        "hardware"
    } else {
        return None;
    })
}

/// The group for a module: its topics (BOSL2), else words in its name and summary, else its file's name.
fn group_for(topics: &[String], words: &str, file: &str, dim: u8) -> &'static str {
    // the first topic that names a group (BOSL2 lists the main one first); "Parts" only if nothing else does
    let groups: Vec<&str> = topics.iter().filter_map(|t| topic_group(t)).collect();
    if let Some(g) = groups.iter().find(|g| **g != "hardware").or(groups.first()) {
        return g;
    }
    if let Some(g) = keyword_group(words).or_else(|| keyword_group(file)) {
        return g;
    }
    if dim == 2 {
        "shapes2d"
    } else if tokens(file).iter().any(|t| t == "vitamins") {
        "hardware"
    } else {
        "shapes3d"
    }
}

fn keyword_group(words: &str) -> Option<&'static str> {
    // whole words (camelCase and snake_case split), matched at their start: "nut" isn't in "donut"
    let w = tokens(words);
    let any = |k: &[&str]| k.iter().any(|k| w.iter().any(|t| t.starts_with(k)));
    Some(if any(&["gear", "rack", "worm", "pulley", "sprocket"]) {
        "gears"
    } else if any(&["thread"]) {
        "threads"
    } else if any(&["screw", "bolt", "nut", "washer", "insert", "rivet", "nail", "fastener", "standoff", "pillar"]) {
        "fasteners"
    } else if any(&["bearing"]) {
        "bearings"
    } else if any(&["hinge", "joint", "dovetail", "snap", "joiner", "clip"]) {
        "hinges"
    } else if any(&["motor", "stepper", "nema", "servo", "extruder", "fan"]) {
        "motors"
    } else if any(&["pcb", "led", "connector", "switch", "battery", "button", "display", "jack", "header", "usb", "resistor", "capacitor", "psu", "relay", "transformer", "socket", "rocker", "fuse", "microswitch", "terminal", "veroboard", "opto", "thermistor", "pin"]) {
        "electronics"
    } else if any(&["box", "enclosure", "case", "housing"]) {
        "enclosures"
    } else if any(&["round", "fillet", "chamfer", "bevel", "mask"]) {
        "rounding"
    } else if any(&["text", "label", "font", "sign", "braille"]) {
        "text"
    } else if any(&["pattern", "texture", "knurl"]) {
        "textures"
    } else if any(&["path", "sweep", "extrude", "spline", "bezier"]) {
        "paths"
    } else {
        return None;
    })
}

/// Lowercase words of a name or text: "polyRoundExtrude" -> poly, round, extrude.
fn tokens(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn humanize_module(name: &str) -> String {
    // bevel_gear -> "Bevel gear"; ScrewThread -> "Screw thread"; M3_cap -> "M3 cap"
    let mut words: Vec<String> = vec![];
    for part in name.split('_').filter(|p| !p.is_empty()) {
        let mut cur = String::new();
        let chars: Vec<char> = part.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            let split = i > 0 && c.is_uppercase() && (chars[i - 1].is_lowercase() || chars.get(i + 1).is_some_and(|n| n.is_lowercase()) && chars[i - 1].is_uppercase());
            if split && !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            cur.push(c);
        }
        if !cur.is_empty() {
            words.push(cur);
        }
    }
    let mut out: Vec<String> = vec![];
    for (i, w) in words.iter().enumerate() {
        let keep = w.len() > 1 && w.chars().all(|c| c.is_uppercase() || c.is_ascii_digit()) || w.chars().any(|c| c.is_ascii_digit());
        let w = if keep { w.clone() } else { w.to_lowercase() };
        if i == 0 {
            let mut c = w.chars();
            out.push(c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default());
        } else {
            out.push(w);
        }
    }
    let s = out.join(" ");
    if s.is_empty() { name.to_string() } else { s }
}

/// A NopSCADlib types file's own list of its types, named like the file: `fans = [fan17x8, …];`.
fn type_list(text: &str, name: &str) -> Option<Vec<String>> {
    let masked = mask_comments(text);
    for l in masked.lines() {
        let Some((n, v)) = l.split_once('=') else { continue };
        if n.trim() != name || l.starts_with(char::is_whitespace) {
            continue;
        }
        let v = v.trim().trim_end_matches(';').trim();
        let inner = v.strip_prefix('[')?.strip_suffix(']')?;
        let items: Vec<String> = inner.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        let ident = |s: &String| s.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && s.chars().all(|c| c.is_alphanumeric() || c == '_');
        return (!items.is_empty() && items.iter().all(ident)).then_some(items);
    }
    None
}

/// NopSCADlib-style type constants in a file: `NAME = ["..."` assignments at the top level.
fn type_constants(text: &str) -> Vec<String> {
    let masked = mask_comments(text);
    let mut out = vec![];
    for l in masked.lines() {
        let t = l.trim_start();
        if l.len() != t.len() {
            continue;
        }
        if let Some((n, Some(v))) = t.split_once('=').and_then(|(n, v)| split_arg(&format!("{n}={v}"))) {
            let v = v.replace(' ', "");
            if v.starts_with("[\"") && n.chars().next().is_some_and(|c| c.is_alphabetic()) {
                out.push(n);
            }
        }
    }
    out
}

/// Read a library folder into its component index. `curated`: start values for
/// modules without examples ({ module: { arg: expression } }, catalog/components.json).
pub fn index_library(info: &LibInfo, root: &Path, curated: Option<&Value>, no_guess: Option<&Value>) -> Result<Value> {
    let name = info.name.clone();
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for f in scan::list_files(root) {
        if keep_file(&f.rel) {
            let data = std::fs::read(root.join(&f.rel)).with_context(|| format!("reading {}", f.rel))?;
            files.insert(f.rel.clone(), hex::encode(Sha256::digest(&data)));
        }
    }
    let read = |rel: &str| String::from_utf8_lossy(&std::fs::read(root.join(rel)).unwrap_or_default()).into_owned();
    let scad: Vec<&String> = files.keys().filter(|r| r.to_lowercase().ends_with(".scad")).collect();
    let texts: BTreeMap<String, String> = scad.iter().map(|r| (r.to_string(), read(r))).collect();
    let docs_style = texts.values().any(|t| t.contains("\n// Module: ") || t.contains("\n// Function&Module: "));
    let bang_style = !docs_style && texts.values().filter(|t| t.contains("//!")).count() > 3;
    let style = if docs_style { "docs" } else if bang_style { "bang" } else { "signatures" };
    let others = if docs_style { documented_choices(&texts) } else { BTreeMap::new() };
    let mut comps: Vec<Component> = vec![];
    let mut problems: Vec<String> = vec![];
    for (rel, text) in &texts {
        let defs = find_modules(text);
        let lines: Vec<&str> = text.lines().collect();
        let shape = scan::shape(text);
        let self_ref = |kw: &str| format!("{kw} <{name}/{rel}>");
        let default_prelude = || vec![if shape.geometry { self_ref("use") } else { self_ref("include") }];
        if docs_style {
            let incs: Vec<String> = file_includes(&lines)
                .into_iter()
                .map(|l| match l.split_once('<').and_then(|(kw, r)| r.split_once('/').map(|(lib, path)| (kw, lib, path))) {
                    Some((kw, lib, path)) if lib != name => format!("{kw}<{name}/{path}"),
                    _ => l,
                })
                .collect();
            for blk in doc_blocks(&lines).into_iter().filter(|b| b.kind != "Function") {
                let get = |n: &str| blk.secs.iter().find(|s| s.name == n);
                let syntags = get("SynTags").map(|s| s.title.clone()).unwrap_or_default();
                // needs children ("CHILDREN", "{ BASE; DIFF1; }") or a parent ("PARENT() show_anchors()")
                let usage_children = blk.secs.iter().filter(|s| s.name == "Usage").any(|s| {
                    s.body.iter().any(|l| l.contains("CHILDREN") || l.contains("PARENT()") || l.contains('{'))
                });
                let Some(def) = defs.iter().find(|d| d.name == blk.name) else {
                    if blk.kind == "Module" {
                        problems.push(format!("{rel}: documented module {}() has no definition", blk.name));
                    }
                    continue;
                };
                // (documented modules call children() for attachments; the docs say which need them)
                if !syntags.split(',').any(|t| t.trim() == "Geom") || usage_children || def.preview_only {
                    continue;
                }
                let arg_names: Vec<String> = def.args.iter().map(|a| a.0.clone()).collect();
                let examples: Vec<Example> = blk
                    .secs
                    .iter()
                    .filter(|s| s.name == "Example" || s.name == "Examples")
                    .map(|s| {
                        let code = s.body.join("\n").trim().to_string();
                        let calls = example_calls(&code, &blk.name, &arg_names);
                        let title = plain(&s.title).trim_end_matches('.').trim().to_string();
                        Example { title, code, calls, flat: s.opts.split(',').any(|o| o.trim() == "2D") }
                    })
                    .collect();
                let topics_first = get("Topics").and_then(|s| s.title.split(',').next().map(|t| t.trim().to_lowercase())).unwrap_or_default();
                let dim = match examples.iter().find(|e| !e.calls.is_empty()) {
                    Some(e) if e.flat => 2,
                    Some(_) => 3,
                    None if topics_first == "shapes (2d)" => 2,
                    None => 3,
                };
                let topics: Vec<String> = get("Topics").map(|s| s.title.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()).unwrap_or_default();
                let anchors: Vec<String> = get("Named Anchors")
                    .map(|s| {
                        s.body
                            .iter()
                            .filter_map(|l| {
                                let t = l.trim();
                                let q = t.strip_prefix('"')?;
                                Some(q[..q.find('"')?].to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let synopsis = get("Synopsis").map(|s| plain(&s.title)).unwrap_or_default();
                let description = get("Description").map(|s| paragraphs(&s.body)).unwrap_or_default();
                let doc_url = info.docs.as_ref().filter(|d| d.contains("BOSL2/wiki")).map(|d| {
                    format!("{}/{}#{}", d.trim_end_matches('/'), rel, anchor_slug(&format!("{}: {}()", blk.kind, blk.name)))
                });
                let group = group_for(&topics, &blk.name, rel, dim).to_string();
                let arg_docs = get("Arguments").map(|s| parse_arguments(&s.body)).unwrap_or_default();
                comps.push(Component {
                    module: blk.name.clone(),
                    file: rel.clone(),
                    line: def.line,
                    prelude: if incs.is_empty() { default_prelude() } else { incs.clone() },
                    fileset: 0,
                    title: humanize_module(&blk.name),
                    synopsis,
                    description,
                    topics,
                    group,
                    dim,
                    doc_url,
                    args: def.args.iter().filter(|a| !a.0.starts_with('_') && !a.0.starts_with('$')).cloned().collect(),
                    listed: arg_docs.iter().filter_map(|(a, d)| arg_choices(a, &d.doc, &blk.name, &others).map(|l| (a.clone(), l))).collect(),
                    arg_docs,
                    anchors,
                    examples,
                    choices: BTreeMap::new(),
                    hold: false,
                });
            }
            continue;
        }
        // NopSCADlib and plain libraries
        // the file of types beside it: insert.scad -> inserts.scad, battery.scad -> batteries.scad
        let plural = rel.strip_suffix(".scad").and_then(|s| {
            [format!("{s}s.scad"), format!("{s}es.scad"), s.strip_suffix('y').map(|b| format!("{b}ies.scad")).unwrap_or_default()]
                .into_iter()
                .find(|p| texts.contains_key(p))
        });
        // its own list of them (`inserts = [F1BM2, F1BM3, …];`), or else every type it defines
        let consts = plural
            .as_ref()
            .map(|p| {
                let stem = Path::new(p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                type_list(&texts[p], &stem).unwrap_or_else(|| type_constants(&texts[p]))
            })
            .unwrap_or_default();
        let is_nop = style == "bang";
        for def in &defs {
            // not examples, demos or tests
            let words = tokens(&def.name);
            // (without attachments, a module that calls children() works on them: an operator)
            if def.name.starts_with('_') || def.uses_children || def.preview_only || def.name == "main"
                || words.iter().any(|w| w.starts_with("example") || w.starts_with("demo") || w.starts_with("test") || w == "echo" || w.starts_with("skip"))
            {
                continue;
            }
            let doc = if is_nop { def.doc_bang.clone() } else { def.doc_above.clone().or_else(|| def.doc_bang.clone()) };
            if is_nop && doc.is_none() {
                continue;
            }
            let doc = doc.unwrap_or_default();
            // "inserts a hole in its children": nothing on its own
            if doc.to_lowercase().contains("children") {
                continue;
            }
            let dim = if def.prims.0 && !def.prims.1 { 2 } else { 3 };
            let mut prelude = vec![];
            if is_nop {
                if texts.contains_key("core.scad") {
                    prelude.push(format!("include <{name}/core.scad>"));
                }
                match &plural {
                    Some(p) => prelude.push(format!("include <{name}/{p}>")),
                    None => prelude.extend(default_prelude()),
                }
            } else {
                prelude.extend(default_prelude());
            }
            let mut choices = BTreeMap::new();
            if let Some(first) = def.args.first() {
                if first.1.is_none() && (first.0 == "type" || first.0.ends_with("_type")) {
                    // a list of its own first (`threaded_inserts` for threaded_insert), then the file's
                    let own = plural.as_ref().and_then(|p| type_list(&texts[p], &format!("{}s", def.name)));
                    let list = own.unwrap_or_else(|| consts.clone());
                    if !list.is_empty() {
                        choices.insert(first.0.clone(), list);
                    }
                }
            }
            let first_para = doc.split("\n\n").next().unwrap_or("").replace('\n', " ");
            let synopsis = first_para.split(". ").next().unwrap_or("").trim().trim_end_matches('.').to_string();
            let group = group_for(&[], &format!("{} {synopsis}", def.name), rel, dim).to_string();
            let doc_url = info.repo.as_ref().map(|r| format!("https://github.com/{r}/blob/{}/{rel}#L{}", info.commit.as_deref().unwrap_or("HEAD"), def.line));
            comps.push(Component {
                module: def.name.clone(),
                file: rel.clone(),
                line: def.line,
                prelude,
                fileset: 0,
                title: humanize_module(&def.name),
                synopsis,
                description: doc.clone(),
                topics: vec![],
                group,
                dim,
                doc_url,
                args: def.args.iter().filter(|a| !a.0.starts_with('$')).cloned().collect(),
                arg_docs: BTreeMap::new(),
                listed: BTreeMap::new(),
                anchors: vec![],
                examples: vec![],
                choices,
                hold: false,
            });
        }
    }
    // start values for modules whose docs have none
    for c in comps.iter_mut().filter(|c| c.examples.iter().all(|e| e.calls.is_empty())) {
        let Some(vals) = curated.and_then(|cur| cur.get(&c.module)).and_then(Value::as_object) else { continue };
        let call: BTreeMap<String, String> = vals.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect();
        if call.keys().all(|k| c.args.iter().any(|a| &a.0 == k)) {
            let code = format!("{}({});", c.module, call.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", "));
            c.examples.push(Example { title: "Suggested start".into(), code, calls: vec![call], flat: false });
        }
    }
    // a start value that isn't one of the file's types (a battery's contact, a motor's shaft): that list isn't its types
    for c in comps.iter_mut() {
        let Some(vals) = curated.and_then(|cur| cur.get(&c.module)).and_then(Value::as_object) else { continue };
        c.choices.retain(|arg, list| vals.get(arg).and_then(Value::as_str).is_none_or(|v| list.iter().any(|o| o == v)));
    }
    // otherwise guessed values for arguments with telling names (radius, height, ...) when that covers all the required ones
    let no_guess: Vec<String> = no_guess.and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    for c in comps.iter_mut().filter(|c| c.examples.iter().all(|e| e.calls.is_empty())) {
        c.hold = no_guess.contains(&c.module);
    }
    // (documented libraries have their own examples: no guessing there)
    for c in comps.iter_mut().filter(|c| style != "docs" && c.examples.iter().all(|e| e.calls.is_empty()) && !c.hold) {
        let required: Vec<&String> = c.args.iter().filter(|a| a.1.is_none()).map(|a| &a.0).collect();
        if required.is_empty() {
            continue;
        }
        let guesses: Option<BTreeMap<String, String>> = required.iter().map(|n| guess_value(n).map(|v| (n.to_string(), v.to_string()))).collect();
        if let Some(call) = guesses {
            let code = format!("{}({});", c.module, call.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", "));
            c.examples.push(Example { title: "Guessed start values".into(), code, calls: vec![call], flat: false });
        }
    }
    // the same module name in two files: keep the first
    let mut seen = BTreeSet::new();
    comps.retain(|c| seen.insert(c.module.clone()));
    // the files each prelude needs
    let mut filesets: Vec<Vec<String>> = vec![];
    let mut by_prelude: BTreeMap<Vec<String>, usize> = BTreeMap::new();
    for c in comps.iter_mut() {
        let idx = *by_prelude.entry(c.prelude.clone()).or_insert_with(|| {
            filesets.push(closure(&name, &c.prelude, &files, &texts));
            filesets.len() - 1
        });
        c.fileset = idx;
    }
    comps.sort_by_key(|a| a.module.to_lowercase());
    Ok(json!({
        "format": FORMAT,
        "library": name,
        "style": style,
        "files": files,
        "filesets": filesets,
        "components": comps,
        "problems": problems,
    }))
}

/// Every library file a prelude reaches through include/use/import.
fn closure(lib: &str, prelude: &[String], files: &BTreeMap<String, String>, texts: &BTreeMap<String, String>) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let mut todo: Vec<String> = vec![];
    let resolve = |base: &str, r: &str| -> Option<String> {
        let near = normpath(&join(&format!("/{base}"), r)).trim_start_matches('/').to_string();
        if files.contains_key(&near) {
            return Some(near);
        }
        let r = normpath(&format!("/{r}")).trim_start_matches('/').to_string();
        if let Some(inside) = r.strip_prefix(&format!("{lib}/")) {
            if files.contains_key(inside) {
                return Some(inside.to_string());
            }
        }
        None
    };
    for p in prelude {
        let (uses, _) = ingest::references(p);
        for u in uses {
            if let Some(r) = resolve("", &u) {
                todo.push(r);
            }
        }
    }
    while let Some(f) = todo.pop() {
        if !out.insert(f.clone()) {
            continue;
        }
        let Some(text) = texts.get(&f) else { continue };
        let base = dirname(&format!("/{f}")).trim_start_matches('/').to_string();
        let (uses, imports) = ingest::references(text);
        for r in uses.iter().chain(imports.iter()) {
            if let Some(x) = resolve(&base, r) {
                todo.push(x);
            }
        }
    }
    out.into_iter().collect()
}

// ------------------------------------------------------------------ rendering a component

/// How to call a component: its library's includes and the module, with the
/// form's values as named arguments. Sent with a render request.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Call {
    pub prelude: Vec<String>,
    pub module: String,
    /// settings passed as arguments, in order ($fa, $fs, $fn included)
    pub args: Vec<String>,
    /// settings whose values are OpenSCAD expressions, written as typed
    #[serde(default)]
    pub raw: Vec<String>,
    /// a 2D shape: extruded by this setting's value
    #[serde(default)]
    pub extrude: Option<String>,
    /// a first comment line (what made the file)
    #[serde(default)]
    pub title: Option<String>,
}

/// The OpenSCAD file for one render (and "Copy code").
pub fn call_source(call: &Call, values: &BTreeMap<String, Value>) -> Result<String> {
    let mut out = String::new();
    if let Some(t) = &call.title {
        out.push_str(&format!("// {}\n", t.replace('\n', " ")));
    }
    for p in &call.prelude {
        out.push_str(p);
        out.push('\n');
    }
    out.push('\n');
    let mut args = vec![];
    for name in &call.args {
        let Some(v) = values.get(name) else { continue };
        let text = if call.raw.contains(name) {
            match v {
                Value::Null => continue,
                Value::String(s) if s.trim().is_empty() => continue,
                Value::String(s) => {
                    check_expression(s)?;
                    s.trim().to_string()
                }
                other => to_scad(other),
            }
        } else {
            match v {
                Value::Null => continue,
                Value::Object(_) => continue,
                other => to_scad(other),
            }
        };
        if !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
            bail!("invalid setting name {name}");
        }
        args.push(format!("{name}={text}"));
    }
    if let Some(e) = &call.extrude {
        let h = values.get(e).and_then(Value::as_f64).filter(|h| *h > 0.0).unwrap_or(1.0);
        out.push_str(&format!("linear_extrude(height={})\n  ", to_scad(&ingest::int_if_whole(&json!(h)))));
    }
    let one_line = format!("{}({});", call.module, args.join(", "));
    if one_line.len() <= 100 {
        out.push_str(&one_line);
    } else {
        out.push_str(&format!("{}(\n    {}\n);", call.module, args.join(",\n    ")));
    }
    out.push('\n');
    Ok(out)
}

// ------------------------------------------------------------------ libraries the app knows

/// A library whose modules are components: bundled with the app, or a library
/// project in the user's library.
#[derive(Clone, Debug)]
pub struct ComponentLibrary {
    pub info: LibInfo,
    /// "bundled" or "project"
    pub provider: String,
    pub source_id: Option<String>,
    pub root: PathBuf,
    pub index: Value,
}

impl ComponentLibrary {
    /// "@bosl2"
    pub fn family(&self) -> String {
        format!("@{}", crate::library::slug(&self.info.name))
    }

    /// Which copy of the library this is, and its version: "app@6cacf1b7d69a" for the one shipped
    /// with the app, "<source id>@<commit>" for a library project (pins and component thumbnails
    /// record it, so a change of copy is noticed).
    pub fn copy(&self) -> String {
        let who = if self.provider == "bundled" { "app" } else { self.source_id.as_deref().unwrap_or("project") };
        let ver = self.info.commit.as_deref().unwrap_or("");
        format!("{who}@{}", &ver[..ver.len().min(12)])
    }
}

/// The libraries shipped with the app (`libs/`): their pins and indexes (made at
/// build time by `workshop-cli libs-index`, or now if missing).
pub fn bundled(libs: &Path) -> Vec<ComponentLibrary> {
    let pins: Value = std::fs::read(libs.join("libraries.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(json!({}));
    let mut out = vec![];
    for p in pins["libraries"].as_array().into_iter().flatten() {
        let Ok(info) = serde_json::from_value::<LibInfo>(p.clone()) else { continue };
        let root = libs.join(&info.name);
        if !root.is_dir() {
            continue;
        }
        let cached = libs.join("index").join(format!("{}.json", info.name));
        let index = std::fs::read(&cached)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(|v| v["format"].as_u64() == Some(FORMAT))
            .or_else(|| index_library(&info, &root, curated(libs).get(&info.name), curated(libs).get("$no_guess").and_then(|n| n.get(&info.name))).map_err(|e| eprintln!("library {}: {e:#}", info.name)).ok());
        let Some(index) = index else { continue };
        out.push(ComponentLibrary { info, provider: "bundled".into(), source_id: None, root, index });
    }
    out
}

/// A start value for a required argument, from its name (None if the name doesn't say).
fn guess_value(name: &str) -> Option<&'static str> {
    let t = tokens(name);
    let last = t.last()?.as_str();
    let has = |k: &[&str]| t.iter().any(|w| k.contains(&w.as_str()));
    Some(match last {
        "r" | "radius" | "rad" => "10",
        "d" | "dia" | "diam" | "diameter" => "20",
        "h" | "height" | "z" => "10",
        "l" | "len" | "length" => "20",
        "w" | "width" | "x" | "y" => "20",
        "depth" => "10",
        "thickness" | "thick" | "wall" => "2",
        "teeth" => "20",
        "n" | "count" | "sides" | "segments" | "number" => "6",
        "angle" | "ang" => "45",
        "pitch" => "2",
        "pos" | "position" | "offset" | "translation" => "[0, 0, 0]",
        "rot" | "rotation" | "orientation" => "[0, 0, 0]",
        _ if has(&["radius"]) => "10",
        _ if has(&["diameter", "diam"]) => "20",
        _ if has(&["height"]) => "10",
        _ if has(&["length"]) => "20",
        _ => return None,
    })
}

/// Start values for modules without examples, shipped as libs/components.json (from catalog/components.json).
pub fn curated(libs: &Path) -> Value {
    std::fs::read(libs.join("components.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(json!({}))
}

/// Write the indexes of the bundled libraries (build step).
pub fn write_bundled_indexes(libs: &Path) -> Result<Vec<(String, usize)>> {
    let pins: Value = serde_json::from_slice(&std::fs::read(libs.join("libraries.json")).context("libs/libraries.json is missing")?)?;
    std::fs::create_dir_all(libs.join("index"))?;
    let mut out = vec![];
    for p in pins["libraries"].as_array().into_iter().flatten() {
        let info: LibInfo = serde_json::from_value(p.clone())?;
        let index = index_library(&info, &libs.join(&info.name), curated(libs).get(&info.name), curated(libs).get("$no_guess").and_then(|n| n.get(&info.name)))?;
        let n = index["components"].as_array().map(|c| c.len()).unwrap_or(0);
        std::fs::write(libs.join("index").join(format!("{}.json", info.name)), serde_json::to_vec(&index)?)?;
        // its README for the side viewer, pointing to GitHub at the pinned commit
        let docs = crate::docs::library_docs(&info, &libs.join(&info.name), Some(&libs.join("docs").join(&info.name)));
        std::fs::create_dir_all(libs.join("docs").join(&info.name))?;
        std::fs::write(libs.join("docs").join(&info.name).join("docs.json"), serde_json::to_vec(&docs)?)?;
        out.push((info.name, n));
    }
    Ok(out)
}

/// Which copy of each library is used: the user's library project with the same
/// name wins, unless the library prefers the bundled copy (`prefer_bundled` in library.json).
pub fn effective(bundled: &[ComponentLibrary], projects: &[ComponentLibrary], prefer_bundled: &[String]) -> Vec<ComponentLibrary> {
    let key = |n: &str| n.to_lowercase();
    let mut out: Vec<ComponentLibrary> = vec![];
    for p in projects {
        let k = key(&p.info.name);
        let bundled_has = bundled.iter().any(|b| key(&b.info.name) == k);
        if bundled_has && prefer_bundled.iter().any(|x| key(x) == k) {
            continue;
        }
        if out.iter().any(|o| key(&o.info.name) == k) {
            continue; // two projects for one library: the first (by id) wins
        }
        let mut p = p.clone();
        // the same library from the same repository credits the same people, whichever copy (Review 3, UI-25)
        if let Some(b) = bundled.iter().find(|b| key(&b.info.name) == k) {
            let same_repo = matches!((&p.info.repo, &b.info.repo), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b));
            if same_repo && !b.info.authors.is_empty() {
                p.info.authors = b.info.authors.clone();
            }
        }
        out.push(p);
    }
    for b in bundled {
        if !out.iter().any(|o| key(&o.info.name) == key(&b.info.name)) {
            out.push(b.clone());
        }
    }
    out
}

// ------------------------------------------------------------------ a component's model page

const ANCHORS: [&str; 7] = ["CENTER", "BOTTOM", "TOP", "LEFT", "RIGHT", "FRONT", "BACK"];
const ORIENTS: [&str; 6] = ["UP", "DOWN", "LEFT", "RIGHT", "FWD", "BACK"];
pub const EXTRUDE: &str = "extrude_height";

fn looks_bool(doc: &str) -> bool {
    let d = doc.to_lowercase();
    d.starts_with("if true") || d.starts_with("if set") || d.starts_with("set to true") || d.contains("default: true") || d.contains("default: false")
}

fn looks_number(doc: &str, name: &str) -> bool {
    let d = doc.to_lowercase();
    let n = name.to_lowercase();
    let numeric_default = d.find("default:").is_some_and(|p| d[p + 8..].trim().starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.'));
    numeric_default
        || d.starts_with("number of")
        || d.starts_with("the number")
        || ["diam", "radius", "width", "height", "length", "thick", "angle", "teeth", "pitch", "depth", "count", "clearance", "gap", "wall"]
            .iter()
            .any(|k| n.contains(k) || d.starts_with(&format!("the {k}")))
            && !d.contains("vector")
            && !d.contains("list")
}

/// The model page (same shape as a project's model) for one component.
pub fn component_model(lib: &ComponentLibrary, c: &Component) -> Value {
    let family = lib.family();
    let key = format!("{family}/{}", c.module);
    // every plain call in the examples: (example title, call number, values)
    let calls: Vec<(&str, usize, &BTreeMap<String, String>)> =
        c.examples.iter().flat_map(|e| e.calls.iter().enumerate().map(move |(i, v)| (e.title.as_str(), i, v))).collect();
    let first = calls.first().map(|c| c.2);
    let in_any = |n: &str| calls.iter().any(|c| c.2.contains_key(n));
    let bosl = lib.index["style"] == "docs";
    let mut params: Vec<Value> = vec![];
    let mut raw: Vec<String> = vec![];
    let mut base: Map<String, Value> = Map::new(); // a setting's value when nothing sets it
    let mut names: Vec<String> = vec![];
    for (name, sig) in c.args.iter() {
        if name.starts_with('_') {
            continue;
        }
        let doc = c.arg_docs.get(name).cloned().unwrap_or_default();
        let sig_lit = sig.as_deref().and_then(folded);
        let ex_texts: Vec<&String> = calls.iter().filter_map(|c| c.2.get(name)).collect();
        let ex_lits: Vec<Option<Value>> = ex_texts.iter().map(|t| folded(t)).collect();
        let first_text = first.and_then(|v| v.get(name));
        let group = if bosl && ["anchor", "spin", "orient"].contains(&name.as_str()) {
            "Position"
        } else if !c.arg_docs.is_empty() && doc.named_only && !in_any(name) {
            "More settings"
        } else {
            "Settings"
        };
        let mut p = Map::new();
        p.insert("name".into(), json!(name));
        p.insert("group".into(), json!(group));
        let mut desc = doc.doc.clone();
        if desc.is_empty() {
            desc = match sig {
                Some(d) => format!("Default: {d}"),
                None => String::new(),
            };
        }
        p.insert("description".into(), if desc.is_empty() { Value::Null } else { json!(desc) });
        let placeholder = doc.default_text.clone().or_else(|| sig.clone()).map(|d| format!("default: {d}"));
        let choice = |opts: Vec<(Value, String)>, default: Value, p: &mut Map<String, Value>| {
            p.insert("type".into(), json!("expression"));
            p.insert("widget".into(), json!("dropdown"));
            let mut list: Vec<Value> = opts.into_iter().map(|(v, l)| json!({ "value": v, "label": l })).collect();
            if !list.iter().any(|o| o["value"] == default) {
                list.push(json!({ "value": default, "label": default.as_str().unwrap_or("") }));
            }
            p.insert("options".into(), Value::Array(list));
            p.insert("default".into(), default);
        };
        let ex_default = |t: Option<&String>| t.map(|s| json!(s)).unwrap_or(Value::Null);
        if bosl && name == "anchor" {
            let mut opts: Vec<(Value, String)> = vec![(Value::Null, sig.as_ref().map(|d| format!("Default ({d})")).unwrap_or_else(|| "Default".into()))];
            opts.extend(ANCHORS.iter().map(|a| (json!(a), a.to_lowercase())));
            opts.extend(c.anchors.iter().map(|a| (json!(format!("\"{a}\"")), format!("\"{a}\""))));
            choice(opts, ex_default(first_text), &mut p);
            raw.push(name.clone());
            base.insert(name.clone(), Value::Null);
        } else if bosl && name == "orient" {
            let mut opts: Vec<(Value, String)> = vec![(Value::Null, sig.as_ref().map(|d| format!("Default ({d})")).unwrap_or_else(|| "Default".into()))];
            opts.extend(ORIENTS.iter().map(|a| (json!(a), a.to_lowercase())));
            choice(opts, ex_default(first_text), &mut p);
            raw.push(name.clone());
            base.insert(name.clone(), Value::Null);
        } else if let Some(opts) = c.choices.get(name) {
            let default = first_text.map(|s| json!(s)).unwrap_or_else(|| opts.first().map(|o| json!(o)).unwrap_or(Value::Null));
            choice(opts.iter().map(|o| (json!(o), o.clone())).collect(), default.clone(), &mut p);
            raw.push(name.clone());
            base.insert(name.clone(), default);
        } else if let Some(list) = c.listed.get(name).cloned().or_else(|| doc_choices(&doc.doc)).filter(|_| {
            // a text setting whose doc lists its values ("One of \"orig\", \"aligned\" …"): a drop-down
            sig_lit.as_ref().map_or(sig.is_none(), |v| v.is_string() || v.is_null()) && ex_lits.iter().all(|v| v.as_ref().is_some_and(Value::is_string))
        }) {
            let sig_s = sig_lit.as_ref().and_then(|v| v.as_str()).map(String::from);
            let mut values = list;
            for v in sig_s.iter().cloned().chain(ex_lits.iter().flatten().filter_map(|v| v.as_str().map(String::from))) {
                if !values.contains(&v) {
                    values.push(v);
                }
            }
            let mut opts: Vec<Value> = vec![];
            if sig_s.is_none() {
                let label = match doc.default_text.as_deref().map(|d| d.trim_matches('"')).filter(|d| !d.is_empty()) {
                    Some(d) => format!("Default ({d})"),
                    None => "Default".into(),
                };
                opts.push(json!({ "value": null, "label": label }));
            }
            opts.extend(values.iter().map(|v| json!({ "value": v, "label": v })));
            let d = first_text.and_then(|t| folded(t)).or_else(|| sig_s.as_ref().map(|s| json!(s))).unwrap_or(Value::Null);
            p.insert("type".into(), json!("string"));
            p.insert("widget".into(), json!("dropdown"));
            p.insert("options".into(), Value::Array(opts));
            p.insert("default".into(), d);
            base.insert(name.clone(), sig_s.map(|s| json!(s)).unwrap_or(Value::Null));
        } else {
            // literal kinds from the signature and the examples; anything mixed is an expression
            let sig_val = sig_lit.clone().filter(|v| !v.is_null());
            let kinds: Vec<&Value> = sig_val.iter().chain(ex_lits.iter().flatten()).collect();
            let ex_all_lit = ex_lits.iter().all(Option::is_some);
            let is = |f: fn(&Value) -> bool| ex_all_lit && !kinds.is_empty() && kinds.iter().all(|v| f(v));
            let numeric_vec = |v: &Value| v.as_array().is_some_and(|a| !a.is_empty() && a.iter().all(Value::is_number));
            let sig_is_expr = sig.is_some() && sig_lit.is_none();
            if !sig_is_expr && is(Value::is_number) {
                let d = first_text.and_then(|t| folded(t)).or(sig_val.clone()).unwrap_or(Value::Null);
                p.insert("type".into(), json!("number"));
                p.insert("widget".into(), json!("number"));
                if sig_val.is_none() {
                    p.insert("optional".into(), json!(true));
                }
                p.insert("default".into(), d);
                base.insert(name.clone(), sig_val.clone().unwrap_or(Value::Null));
            } else if !sig_is_expr && is(Value::is_boolean) && sig_val.is_some() {
                let d = first_text.and_then(|t| folded(t)).or(sig_val.clone()).unwrap_or(json!(false));
                p.insert("type".into(), json!("boolean"));
                p.insert("widget".into(), json!("checkbox"));
                p.insert("default".into(), d);
                base.insert(name.clone(), sig_val.clone().unwrap_or(Value::Null));
            } else if !sig_is_expr && (is(Value::is_boolean) || sig_val.is_none() && ex_texts.is_empty() && looks_bool(&doc.doc)) {
                let d = first_text.and_then(|t| folded(t)).unwrap_or(Value::Null);
                p.insert("type".into(), json!("boolean"));
                p.insert("widget".into(), json!("dropdown"));
                p.insert("options".into(), json!([{ "value": null, "label": "Default" }, { "value": true, "label": "Yes" }, { "value": false, "label": "No" }]));
                p.insert("default".into(), d);
                base.insert(name.clone(), Value::Null);
            } else if !sig_is_expr && is(Value::is_string) {
                let d = first_text.and_then(|t| folded(t)).or(sig_val.clone()).unwrap_or(Value::Null);
                p.insert("type".into(), json!("string"));
                p.insert("widget".into(), json!("text"));
                if sig_val.is_none() {
                    p.insert("optional".into(), json!(true));
                }
                p.insert("default".into(), d);
                base.insert(name.clone(), sig_val.clone().unwrap_or(Value::Null));
            } else if !sig_is_expr && sig_val.as_ref().is_some_and(numeric_vec) && is(numeric_vec) && kinds.iter().all(|v| v.as_array().map(|a| a.len()) == sig_val.as_ref().and_then(|s| s.as_array()).map(|a| a.len())) {
                let d = first_text.and_then(|t| folded(t)).or(sig_val.clone()).unwrap_or(Value::Null);
                p.insert("type".into(), json!("number[]"));
                p.insert("widget".into(), json!("vector"));
                p.insert("default".into(), d);
                base.insert(name.clone(), sig_val.clone().unwrap_or(Value::Null));
            } else if !sig_is_expr && sig_val.is_none() && ex_texts.is_empty() && looks_number(&doc.doc, name) {
                p.insert("type".into(), json!("number"));
                p.insert("widget".into(), json!("number"));
                p.insert("optional".into(), json!(true));
                p.insert("default".into(), Value::Null);
                base.insert(name.clone(), Value::Null);
            } else {
                p.insert("type".into(), json!("expression"));
                p.insert("widget".into(), json!("text"));
                p.insert("optional".into(), json!(true));
                p.insert("default".into(), ex_default(first_text));
                raw.push(name.clone());
                base.insert(name.clone(), Value::Null);
            }
        }
        if let Some(ph) = placeholder {
            p.insert("placeholder".into(), json!(ph));
        }
        params.push(Value::Object(p));
        names.push(name.clone());
    }
    // 2D shapes are extruded to show and print
    let extrude = (c.dim == 2 && !names.iter().any(|n| n == EXTRUDE)).then(|| EXTRUDE.to_string());
    if extrude.is_some() {
        params.insert(0, json!({
            "name": EXTRUDE, "label": "Extrude to (mm)", "group": "Settings", "type": "number", "widget": "number", "default": 2, "min": 0.1,
            "description": "This is a 2D shape: it's extruded to this height for the preview and the STL (not part of the module's own settings).",
        }));
        base.insert(EXTRUDE.into(), json!(2));
    }
    for (n, label, d, desc) in [
        ("$fa", "Detail angle ($fa)", json!(6), "Smallest angle between segments of a curve, in degrees. Lower is smoother and slower. OpenSCAD's own default is 12."),
        ("$fs", "Detail size ($fs)", json!(0.5), "Smallest segment length of a curve, in mm. Lower is smoother and slower. OpenSCAD's own default is 2."),
        ("$fn", "Segments ($fn)", Value::Null, "Exact number of segments for every circle (empty: worked out from $fa and $fs)."),
    ] {
        params.push(json!({ "name": n, "label": label, "group": "Detail", "type": "number", "widget": "number", "default": d, "min": if n == "$fn" { json!(0) } else { json!(0.01) }, "optional": n == "$fn", "description": desc }));
        base.insert(n.into(), d);
        names.push(n.into());
    }
    let mut groups: Vec<String> = vec![];
    for p in &params {
        let g = p["group"].as_str().unwrap_or("Settings").to_string();
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    let order = ["Settings", "More settings", "Position", "Detail"];
    groups.sort_by_key(|g| order.iter().position(|o| o == g).unwrap_or(9));
    // presets: every plain-call example, as a complete set of values
    let mut presets = vec![];
    for (n, (title, i, values)) in calls.iter().enumerate() {
        let mut vals = base.clone();
        for p in &params {
            let name = p["name"].as_str().unwrap_or("");
            if let Some(t) = values.get(name) {
                let v = if raw.iter().any(|r| r == name) { json!(t) } else { folded(t).unwrap_or_else(|| json!(t)) };
                vals.insert(name.into(), v);
            }
        }
        let label = match (title.is_empty(), i) {
            (true, _) => format!("Example {}", n + 1),
            (false, 0) => format!("Example: {title}"),
            (false, i) => format!("Example: {title} ({})", i + 1),
        };
        presets.push(json!({ "label": label, "values": vals }));
    }
    let files: Map<String, Value> = c_files(lib, c);
    let license_spdx = lib.info.license.clone().unwrap_or_else(|| "NOASSERTION".into());
    let call = Call {
        prelude: c.prelude.clone(),
        module: c.module.clone(),
        args: names.clone(),
        raw: raw.clone(),
        extrude,
        title: Some(format!("{} {}() made with SCAD Workshop", lib.info.name, c.module)),
    };
    // with no example to start from, arguments without a default must be filled in first
    let needs: Vec<String> = if calls.is_empty() {
        c.args
            .iter()
            .filter(|(n, sig)| sig.is_none() && !n.starts_with('_'))
            .filter(|(n, _)| c.hold || params.iter().any(|p| p["name"] == n.as_str() && p["default"].is_null()))
            .map(|(n, _)| n.clone())
            .collect()
    } else {
        vec![]
    };
    // held: its defaults don't make a shape, so the settings it needs (at least the first) start empty
    let mut needs = needs;
    if c.hold && needs.is_empty() {
        needs.extend(c.args.first().map(|a| a.0.clone()));
    }
    for p in params.iter_mut().filter(|p| needs.iter().any(|n| p["name"] == n.as_str())) {
        if c.hold {
            p["default"] = Value::Null;
        }
        // a drop-down for a setting that must be filled in starts on "Choose…", not "Default"
        if p["default"].is_null() {
            if let Some(opts) = p["options"].as_array_mut() {
                match opts.iter_mut().find(|o| o["value"].is_null()) {
                    Some(o) => o["label"] = json!("Choose…"),
                    None => opts.insert(0, json!({ "value": null, "label": "Choose…" })),
                }
            }
        }
    }
    let terms = ingest::setting_terms(&params);
    let mut description_html = String::new();
    if !c.description.is_empty() {
        for para in c.description.split("\n\n") {
            description_html.push_str(&format!("<p>{}</p>", html_escape(para)));
        }
    }
    json!({
        "key": key, "family": family, "id": c.module, "name": c.title,
        "family_name": lib.info.name, "kind": "component",
        "summary": if c.synopsis.is_empty() { format!("{}() from {}", c.module, lib.info.name) } else { c.synopsis.clone() },
        "description_html": if description_html.is_empty() { Value::Null } else { json!(description_html) },
        "category": c.group, "category_label": group_label(&c.group),
        "tags": c.topics,
        "license": crate::meta::license_value(&license_spdx),
        "authors": lib.info.authors.iter().map(|a| json!({ "name": a })).collect::<Vec<_>>(),
        "source": { "repository": lib.info.repo.as_ref().map(|r| format!("https://github.com/{r}")), "commit": lib.info.commit, "commit_date": lib.info.date, "file": c.file },
        "links": { "docs": c.doc_url, "source": lib.info.repo.as_ref().map(|r| format!("https://github.com/{r}")) },
        "component": { "library": lib.info.name, "module": c.module, "file": c.file, "line": c.line, "provider": lib.provider, "source_id": lib.source_id, "copy": lib.copy(), "dim": c.dim, "doc_url": c.doc_url, "topics": c.topics },
        "call": call,
        "entry": "/component.scad",
        "files": files,
        "browser": false,
        "parameters": params,
        "groups": groups,
        "tabs": { "More settings": { "collapsed": true }, "Position": { "collapsed": true }, "Detail": { "collapsed": true } },
        "presets": presets,
        "needs": needs,
        "terms": terms,
        "settings": names.len(),
        "updated": lib.info.date,
    })
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The files a component's render needs: {"/libraries/<Name>/<path>": sha256}.
fn c_files(lib: &ComponentLibrary, c: &Component) -> Map<String, Value> {
    let set = &lib.index["filesets"][c.fileset];
    set.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let r = r.as_str()?;
            Some((format!("/libraries/{}/{r}", lib.info.name), lib.index["files"][r].clone()))
        })
        .collect()
}

/// The index entry's components.
pub fn components_of(lib: &ComponentLibrary) -> Vec<Component> {
    serde_json::from_value(lib.index["components"].clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_is_a_value() {
        assert_eq!(folded("1/4"), Some(json!(0.25)));
        assert_eq!(folded("-0.5*4"), Some(json!(-2)));
        assert_eq!(folded("[1/2, 0, 3]"), Some(json!([0.5, 0, 3])));
        assert_eq!(folded("\"a\""), Some(json!("a")));
        assert_eq!(folded("2*r"), None);
        assert_eq!(folded("[CENTER, 1]"), None);
    }

    #[test]
    fn literals() {
        assert_eq!(parse_literal("5"), Some(json!(5)));
        assert_eq!(parse_literal("-2.5"), Some(json!(-2.5)));
        assert_eq!(parse_literal("1e-3"), Some(json!(0.001)));
        assert_eq!(parse_literal("\"pitch\\\"base\""), Some(json!("pitch\"base")));
        assert_eq!(parse_literal("[1, 2,3]"), Some(json!([1, 2, 3])));
        assert_eq!(parse_literal("[\"a\", true]"), Some(json!(["a", true])));
        assert_eq!(parse_literal("undef"), Some(Value::Null));
        assert_eq!(parse_literal("UP"), None);
        assert_eq!(parse_literal("5/2"), None);
        assert_eq!(parse_literal("3-1"), None);
        assert_eq!(parse_literal("[0:10]"), None);
        assert_eq!(to_scad(&json!([1, 2.5, "a\"b", true])), "[1, 2.5, \"a\\\"b\", true]");
        assert_eq!(to_scad(&json!(3.0)), "3");
    }

    #[test]
    fn expressions_are_checked() {
        assert!(check_expression("[10, 20, 5]").is_ok());
        assert!(check_expression("BOTTOM+LEFT").is_ok());
        assert!(check_expression("\"a;b\"").is_ok());
        assert!(check_expression("1); import(\"/etc/passwd\"); cube(").is_err());
        assert!(check_expression("import(\"x.stl\")").is_err());
        assert!(check_expression("[1, 2").is_err());
        assert!(check_expression("5 // note").is_err());
    }

    #[test]
    fn modules_found() {
        let text = "// A box.\n// With walls.\nmodule box(size=[10,20,30], wall = 2, label) { cube(size); children(); }\n\
                    module _private() {}\nmodule op() { translate([1,0,0]) children(); }\nmodule nut(type, h = \"x)\") { //! Draw a nut\n  circle(3);\n}\n\
                    function f() = 1;\nmodule wrap() { if ($children) children(); cube(1); }\n";
        let m = find_modules(text);
        let names: Vec<&str> = m.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["box", "_private", "op", "nut", "wrap"]);
        assert_eq!(m[0].args, vec![("size".to_string(), Some("[10,20,30]".to_string())), ("wall".into(), Some("2".into())), ("label".into(), None)]);
        assert_eq!(m[0].doc_above.as_deref(), Some("A box.\nWith walls."));
        assert!(m[2].needs_children);
        assert!(!m[4].needs_children);
        assert_eq!(m[3].doc_bang.as_deref(), Some("Draw a nut"));
        assert_eq!(m[3].args[1], ("h".to_string(), Some("\"x)\"".to_string())));
        assert_eq!(m[3].prims, (true, false));
    }

    #[test]
    fn examples_become_values() {
        let names: Vec<String> = ["teeth", "mate_teeth", "shaft_angle", "circ_pitch"].iter().map(|s| s.to_string()).collect();
        let v = example_calls("bevel_gear(\n    circ_pitch=5, teeth=36, mate_teeth=36\n);", "bevel_gear", &names);
        assert_eq!(v[0]["circ_pitch"], "5");
        assert_eq!(v[0]["teeth"], "36");
        let v = example_calls("t1 = 16; t2 = 28; $fn=32;\nbevel_gear(t1, t2, 90, circ_pitch=5);", "bevel_gear", &names);
        assert_eq!(v[0]["teeth"], "16");
        assert_eq!(v[0]["mate_teeth"], "28");
        assert_eq!(v[0]["shaft_angle"], "90");
        assert_eq!(v[0]["$fn"], "32");
        let v = example_calls("bevel_gear(teeth=3);\nright(20) bevel_gear(teeth=4);\nbevel_gear(teeth=5);", "bevel_gear", &names);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1]["teeth"], "5");
        assert!(example_calls("color(\"red\") bevel_gear(teeth=3);", "bevel_gear", &names).is_empty());
        assert!(example_calls("bevel_gear(teeth=3) cube(1);", "bevel_gear", &names).is_empty());
        assert!(example_calls("d = 2*3; bevel_gear(teeth=d);", "bevel_gear", &names).is_empty());
        assert!(example_calls("bevel_gear(size=3);", "bevel_gear", &names).is_empty());
    }

    #[test]
    fn arguments_doc() {
        let body: Vec<String> = [
            "teeth = Number of teeth.  Default: 20",
            "l / h / length = Length.",
            "  More about length.",
            "---",
            "anchor = Translate so anchor point is at origin.  See [anchor](attachments.scad#subsection-anchor).  Default: `CENTER`",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let a = parse_arguments(&body);
        assert_eq!(a["teeth"].default_text.as_deref(), Some("20"));
        assert!(!a["teeth"].named_only);
        assert_eq!(a["h"].doc, "Length. More about length.");
        assert_eq!(a["anchor"].default_text.as_deref(), Some("CENTER"));
        assert!(a["anchor"].doc.contains("See anchor."));
        assert!(a["anchor"].named_only);
    }

    #[test]
    fn choices_from_the_docs() {
        let s = |v: &[&str]| Some(v.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        assert_eq!(
            doc_choices(r#"The style of the spheroid's construction. One of "orig", "aligned", "stagger", "octa", or "icosa".  Default: "aligned""#),
            s(&["orig", "aligned", "stagger", "octa", "icosa"])
        );
        assert_eq!(
            doc_choices(r#"Valid options are "default", "alt", "flip1", "flip2",  "min_edge", "min_area", "quincunx", "convex" and "concave"."#).map(|v| v.len()),
            Some(9)
        );
        assert_eq!(doc_choices(r#"Select "hull" or "intersect" anchor types.  Default: "hull""#), s(&["hull", "intersect"]));
        assert_eq!(doc_choices(r#"A string, "male" or "female", to specify the gender of the dovetail."#), s(&["male", "female"]));
        assert_eq!(
            doc_choices(r#"The text direction.  "ltr" for left to right.  "rtl" for right to left. "ttb" for top to bottom. "btt" for bottom to top.  Default: "ltr""#),
            s(&["ltr", "rtl", "ttb", "btt"])
        );
        assert_eq!(
            doc_choices(r#""none" for no chamfer, "all" for full chamfering, and "bot" or "bottom" for bottom chamfering.  Default: "all"."#),
            s(&["none", "all", "bot", "bottom"])
        );
        assert_eq!(
            doc_choices(r#"one of "standard" (alias "large"), "medium", "small", or "tiny"."#),
            s(&["standard", "large", "medium", "small", "tiny"])
        );
        // open-ended lists, examples, defaults, a single value: not choices
        assert_eq!(doc_choices(r#"Can be "centroid", "mean", "box" or a 3D point.  Default: "centroid""#), None);
        assert_eq!(doc_choices(r#""thin", "normal", "thick", or a thickness in mm.  Default: "normal""#), None);
        assert_eq!(doc_choices(r#"Set to "circle" for a circle hole, "D" for a D-shaped (semicircular) hole or a path to create a custom hole."#), None);
        assert_eq!(doc_choices(r#"Standard named paths are "flat", "sawtooth" and "jigsaw".  Alternatively, you can give a cutpath as a 2D path."#), None);
        assert_eq!(doc_choices(r##"nut specification, e.g. "M5x1" or "#8-32".  See screw naming."##), None);
        assert_eq!(doc_choices(r#"See tolerance. Default is "2B" for UTS and "6H" for ISO."#), None);
        assert_eq!(doc_choices(r#"vnf_vertex_array style.  Default: "min_edge""#), None);
        assert_eq!(doc_choices(r#"NPT size in inches.  1/16", 1/8", or 2".  Default: 1/2""#), None);
        // the list the sentence introduces, not values mentioned in passing
        assert_eq!(
            doc_choices(r#"sampling method to use with "direct" and "reindex" methods.  Can be "length" or "segment".  Ignored if any profile pair uses either the "distance", "fast_distance", or "tangent" methods.  Default: "length"."#),
            s(&["length", "segment"])
        );
        assert_eq!(doc_choices(r#"Change vertical center between "baseline" and "ycenter".  Default: "baseline""#), s(&["baseline", "ycenter"]));
    }

    #[test]
    fn choices_pointed_to() {
        let mut others: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        others.entry("vnf_vertex_array".into()).or_default().insert("style".into(), vec!["default".into(), "alt".into()]);
        assert_eq!(arg_choices("style", "vnf_vertex_array() style to use.  Default: \"default\"", "path_sweep", &others).map(|v| v.len()), Some(2));
        assert_eq!(arg_choices("style", "vnf_vertex_array() style to use.", "vnf_vertex_array", &others), None);
        assert_eq!(arg_choices("style", "my_vnf_vertex_array_x style", "path_sweep", &others), None);
        assert_eq!(arg_choices("method", "vnf_vertex_array() method", "path_sweep", &others), None);
        assert_eq!(arg_choices("style", "vnf_vertex_array style.  Default: \"min_edge\"", "path_sweep", &others).map(|v| v.len()), Some(2));
        // a mention isn't a pointer
        others.entry("text".into()).or_default().insert("valign".into(), vec!["top".into(), "center".into()]);
        assert_eq!(arg_choices("valign", "align text to the path using \"top\" or \"bottom\".  You can also adjust it.", "path_text", &others), None);
    }

    #[test]
    fn values_in_calls_become_settings() {
        let lib = "$fn = 50;\nclearance = 0.05;\n\n/*  Spur gear\n    modul = Height of the Tooth Tip\n    tooth_number = Number of Gear Teeth\n    optimized = Create holes; where\n        geometry allows */\nmodule spur_gear(modul, tooth_number, width, bore, pressure_angle = 20, helix_angle = 0, optimized = true, clearance = 0) {\n    cylinder(r = modul * tooth_number, h = width);\n}\n".to_string();
        let ex = "include <../gears.scad>\n\nspur_gear(modul=1, tooth_number=30, width=5, bore=4, optimized=true, clearance=0.1, $fn=20);\n\ntranslate([80, 0, 0])\nspur_gear(2, 16, width=w, bore=4);\n";
        let out = lift_call_values(ex, "examples/spur_gear.scad", &[lib], &[]).expect("lifted");
        assert!(out.contains("include <../gears.scad>\n"), "{out}");
        assert!(out.contains("/* [Spur gear 1] */\n// Height of the Tooth Tip\nmodul = 1;\n// Number of Gear Teeth\ntooth_number = 30;\nwidth = 5;\nbore = 4;\n// Create holes; where geometry allows\noptimized = true;\n"), "{out}");
        // a library global keeps its name for the library: the setting gets the module's name
        assert!(out.contains("spur_gear_clearance = 0.1;"), "{out}");
        assert!(out.contains("spur_gear(modul=modul, tooth_number=tooth_number, width=width, bore=bore, optimized=optimized, clearance=spur_gear_clearance, $fn=20);"), "{out}");
        // the second call: positional values named from the module, numbered; an expression stays
        assert!(out.contains("/* [Spur gear 2] */\n// Height of the Tooth Tip\nmodul_2 = 2;"), "{out}");
        assert!(out.contains("translate([80, 0, 0])\nspur_gear(modul=modul_2, tooth_number=tooth_number_2, width=w, bore=bore_2);"), "{out}");
        assert!(out.find("/* [Hidden] */").unwrap() < out.find("spur_gear(modul=modul").unwrap());
        // files with settings of their own, definitions or blocks are left alone
        assert!(lift_call_values("include <g.scad>\nteeth = 20;\nspur_gear(modul=1, tooth_number=teeth);\n", "a.scad", &[], &[]).is_none());
        assert!(lift_call_values("difference() { cube(10); sphere(6); }\n", "a.scad", &[], &[]).is_none());
        assert!(lift_call_values("include <g.scad>\nspur_gear(modul=m, tooth_number=t);\n", "a.scad", &[], &[]).is_none());
    }

    #[test]
    fn settings_borrow_the_librarys_own() {
        // LEGO.scad: Customizer variables passed to the module by the library's own call
        let lib = r#"/* [General] */

// Width of the block, in studs
block_width = 2;

// Height of the block. A ratio of "1" is a standard LEGO brick height; a ratio of "1/3" is a standard LEGO plate height.
block_height_ratio = 1; // [.33333333333:1/3, .5:1/2, 1:1, 1.5:1 1/2, 2:2, 3:3]

// What type of block should this be?
block_type = "brick"; // [brick:Brick, tile:Tile, wing:Wing]

// Should extra reinforcement be included?
use_reinforcement = "no"; // [no:No, yes:Yes]

// Corner radius
round_radius = 3; // [0:10]

block(
    width=block_width,
    height=block_height_ratio,
    type=block_type,
    reinforcement=(use_reinforcement=="yes"),
    stud_notches=(block_type == "wing" && use_reinforcement=="yes"),
    round_radius=round_radius
);

module block(width=1, length=2, height=1, type="brick", reinforcement=false, stud_notches=false, round_radius=0) {
    cube([width, length, height]);
}
"#
        .to_string();
        let ex = "use <../LEGO.scad>;\n\nblock(\n    width=1,\n    length=2,\n    height=1/3,\n    type=\"tile\",\n    reinforcement=true,\n    stud_notches=true,\n    round_radius=12\n);";
        let out = lift_call_values(ex, "examples/3070.scad", std::slice::from_ref(&lib), &[]).expect("lifted");
        assert!(out.contains("// Width of the block, in studs\nwidth = 1;\nlength = 2;\n"), "{out}");
        // 1/3 is the library's own 1/3 choice
        assert!(out.contains("in studs\nwidth = 1;\nlength = 2;\n// Height of the block. A ratio of \"1\" is a standard LEGO brick height; a ratio of \"1/3\" is a standard LEGO plate height.\nheight = .33333333333; // [.33333333333:1/3, .5:1/2, 1:1, 1.5:1 1/2, 2:2, 3:3]\n"), "{out}");
        assert!(out.contains("// What type of block should this be?\ntype = \"tile\"; // [brick:Brick, tile:Tile, wing:Wing]\n"), "{out}");
        // an expression of one variable: its description, not its yes/no list (the value is a boolean)
        assert!(out.contains("// Should extra reinforcement be included?\nreinforcement = true;\n"), "{out}");
        // an expression of two: nothing borrowed
        assert!(out.contains("\nstud_notches = true;\n"), "{out}");
        // outside the library's range: the description, no slider (and the library's variable of
        // the same name doesn't matter: `use` doesn't bring it in)
        assert!(out.contains("// Corner radius\nround_radius = 12;\n"), "{out}");
        assert!(out.contains("block(width=width, length=length, height=height, type=type, reinforcement=reinforcement, stud_notches=stud_notches, round_radius=round_radius);"), "{out}");
        let inc = ex.replace("use <", "include <");
        assert!(lift_call_values(&inc, "a.scad", std::slice::from_ref(&lib), &[]).unwrap().contains("// Corner radius\nblock_round_radius = 12;\n"));
        // a value that isn't one of the choices becomes one more, labelled as written
        let ex2 = "use <../LEGO.scad>;\nblock(width=2, height=2/3, type=\"round-tile\");\n";
        let out = lift_call_values(ex2, "a.scad", std::slice::from_ref(&lib), &[]).expect("lifted");
        assert!(out.contains("height = 0.6666666666666666; // [.33333333333:1/3, .5:1/2, 0.6666666666666666:2/3, 1:1, 1.5:1 1/2, 2:2, 3:3]\n"), "{out}");
        assert!(out.contains("type = \"round-tile\"; // [brick:Brick, tile:Tile, wing:Wing, round-tile:round-tile]\n"), "{out}");
        // the library's call in another of the project's files (not one the model includes)
        let bare = "module block(width=1, length=2, height=1, type=\"brick\") { cube([width, length, height]); }\n".to_string();
        let main = "use <lib.scad>;\n// Width of the block, in studs\nblock_width = 2;\nblock(width=block_width);\n".to_string();
        let out = lift_call_values("use <lib.scad>;\nblock(width=4, height=2+(2/3));\n", "b.scad", std::slice::from_ref(&bare), &[main]).expect("lifted");
        assert!(out.contains("// Width of the block, in studs\nwidth = 4;\n"), "{out}");
        // a fraction with no list to pick from: a number
        assert!(out.contains("\nheight = 2.6666666666666665;\n"), "{out}");
        assert!(only_calls("use <../LEGO.scad>;\n\nblock(\n    width=1,\n    height=1/3\n);"));
        assert!(only_calls("include <a.scad>\ntranslate([1, 0, 0]) cube(2); // x = 1;\n"));
        assert!(!only_calls("include <a.scad>\nx = 1;\ncube(x);\n"));
        assert!(!only_calls("difference() { cube(2); sphere(1); }\n"));
        assert_eq!(const_number("1/3"), Some(1.0 / 3.0));
        assert_eq!(const_number(" -2 * (1 + 0.5) "), Some(-3.0));
        assert_eq!(const_number("1e-1/2"), Some(0.05));
        assert_eq!(const_number("a/3"), None);
        assert_eq!(const_number("[1/3]"), None);
        assert_eq!(const_number("1/0"), None);
        // section headers aren't descriptions; notes need a value of the same type
        let vars = documented_vars("/* [Size] */\nw = 2; // [1:10]\nmodule m() {\n  // inside\n  x = 1;\n}\n");
        assert_eq!(vars.keys().collect::<Vec<_>>(), vec!["w"]);
        assert_eq!(vars["w"].doc, None);
        assert_eq!(vars["w"].note.as_deref(), Some("1:10"));
        assert_eq!(fit_note("no:No, yes:Yes", "\"no\"", &json!(true), "true"), None);
        assert_eq!(fit_note("false:False, true:True", "false", &json!(true), "true"), None);
        assert_eq!(fit_note("0.51:0.01:1.49", "1.00", &json!(1.2), "1.2"), Some(("1.2".into(), "0.51:0.01:1.49".into())));
    }

    #[test]
    fn source_for_a_call() {
        let call = Call {
            prelude: vec!["include <BOSL2/std.scad>".into(), "include <BOSL2/gears.scad>".into()],
            module: "bevel_gear".into(),
            args: vec!["teeth".into(), "mod".into(), "anchor".into(), "$fa".into()],
            raw: vec!["anchor".into()],
            extrude: None,
            title: None,
        };
        let mut v = BTreeMap::new();
        v.insert("teeth".into(), json!(36));
        v.insert("mod".into(), Value::Null);
        v.insert("anchor".into(), json!("BOTTOM"));
        v.insert("$fa".into(), json!(6));
        assert_eq!(call_source(&call, &v).unwrap(), "include <BOSL2/std.scad>\ninclude <BOSL2/gears.scad>\n\nbevel_gear(teeth=36, anchor=BOTTOM, $fa=6);\n");
        v.insert("anchor".into(), json!("1); import(\"x\"); (2"));
        assert!(call_source(&call, &v).is_err());
    }

    #[test]
    fn type_lists() {
        let text = "// Fans\nfan17x8  = [17, 8, 16];\nfan25x10 = [25.4, 10, 24];\n\nfans = [fan17x8, fan25x10];\n\nuse <fan.scad>\n";
        assert_eq!(type_list(text, "fans"), Some(vec!["fan17x8".to_string(), "fan25x10".into()]));
        assert_eq!(type_list(text, "inserts"), None);
        assert_eq!(type_constants("F1BM2   = [ \"F1BM2\",   4.0 ];\nX = 3;\n"), vec!["F1BM2"]);
    }

    #[test]
    fn a_modules_own_type_list() {
        // (Review 7, item 5) threaded_insert takes the file's `threaded_inserts`, not `inserts`; a start value
        // that isn't in the file's list (a battery's contact) drops the list
        let dir = std::env::temp_dir().join(format!("types-test-{}", std::process::id()));
        let files = [
            ("core.scad", "// core\n"),
            ("vitamins/insert.scad", "module insert(type) { //! Draw an insert\n cube(type[1]); }\nmodule threaded_insert(type) { //! Draw a threaded insert\n cube(type[1]); }\n"),
            ("vitamins/inserts.scad", "F1BM2 = [\"F1BM2\", 3];\nM3x8 = [\"M3x8\", 4];\ninserts = [F1BM2];\nthreaded_inserts = [M3x8];\nuse <insert.scad>\n"),
            ("vitamins/battery.scad", "module battery(type) { //! Draw a battery\n cube(type[1]); }\nmodule battery_contact(type, pos = true) { //! Draw a contact\n cube(type[1]); }\n"),
            ("vitamins/batteries.scad", "AACELL = [\"AACELL\", 5];\nbcontact = [\"bcontact\", 1];\nbatteries = [AACELL];\nuse <battery.scad>\n"),
        ];
        for (rel, text) in files {
            std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
            std::fs::write(dir.join(rel), text).unwrap();
        }
        let info: LibInfo = serde_json::from_value(json!({ "name": "Nop" })).unwrap();
        let curated = json!({ "battery_contact": { "type": "bcontact" } });
        let index = index_library(&info, &dir, Some(&curated), None).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let choices = |m: &str| index["components"].as_array().unwrap().iter().find(|c| c["module"] == m).map(|c| c["choices"].clone());
        assert_eq!(choices("insert"), Some(json!({ "type": ["F1BM2"] })));
        assert_eq!(choices("threaded_insert"), Some(json!({ "type": ["M3x8"] })));
        assert_eq!(choices("battery"), Some(json!({ "type": ["AACELL"] })));
        assert_eq!(choices("battery_contact"), Some(json!({})));
    }

    #[test]
    fn names() {
        assert_eq!(tokens("polyRoundExtrude a_triangle donutSlice"), ["poly", "round", "extrude", "a", "triangle", "donut", "slice"]);
        assert_eq!(group_for(&[], "donutSlice", "shapes.scad", 3), "shapes3d");
        assert_eq!(group_for(&[], "MetricNut", "threads.scad", 3), "fasteners");
        assert_eq!(group_for(&[], "ScrewThread", "threads.scad", 3), "threads");
        assert_eq!(group_for(&[], "RodEnd", "threads.scad", 3), "threads");
        assert_eq!(group_for(&["Gears".into(), "Parts".into()], "", "", 3), "gears");
        assert_eq!(group_for(&["Parts".into()], "", "", 3), "hardware");
        assert_eq!(humanize_module("bevel_gear"), "Bevel gear");
        assert_eq!(humanize_module("ScrewThread"), "Screw thread");
        assert_eq!(humanize_module("MetricBolt"), "Metric bolt");
        assert_eq!(humanize_module("M3_cap"), "M3 cap");
        assert_eq!(humanize_module("NEMA_motor"), "NEMA motor");
        assert_eq!(anchor_slug("Function&Module: bevel_gear()"), "functionmodule-bevel_gear");
    }
}

