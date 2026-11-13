//! "Include once": the files native OpenSCAD reads for a model, with repeated includes left out
//! where that can't change the result (Review 7, item 1).
//!
//! OpenSCAD reads an `include`d file again each time it is included, and on each assignment to a
//! name already assigned (a constants file included by twenty module files assigns its constants
//! twenty times) it works out both files' paths relative to the main file (`handle_assignment` in
//! its parser.y, `fs::relative`), which looks at every folder of both paths on disk. For a
//! Gridfinity Extended bin that is about 400,000 system calls: half a second on Linux, 13 to 18 s
//! on Windows, where each costs far more.
//!
//! So, for each file OpenSCAD reads as a whole (the model's file, and each file it `use`s), this
//! writes a copy of every file it includes next to the original (`<name>.~<id>.scad`, so paths
//! relative to the file still work), in which an include of a file already included is left out
//! when that file
//! - has nothing at its top level but assignments, `module` and `function` definitions, `include`
//!   and `use` (including it again would only assign the same things again), and
//! - defines no name (variable, module or function) that another file read with it defines too
//!   (so nothing in between can have changed what it sets).
//!
//! Includes inside a block (`module m() { include <x.scad> }`) are left alone. A file that isn't
//! understood (an include from outside the model's folder, a path that doesn't end) leaves the
//! model as it is. `WORKSHOP_INCLUDE_ONCE=0` turns this off.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

/// Where OpenSCAD looks for libraries inside a model's tree (OPENSCADPATH; see render.rs).
const LIBRARIES: &str = "libraries";

/// The file to give OpenSCAD instead of `entry` (a path inside `tree`), or None to give it
/// `entry` as it is (switched off, or something not understood).
pub fn prepare(tree: &Path, entry: &Path) -> Result<Option<PathBuf>> {
    if std::env::var("WORKSHOP_INCLUDE_ONCE").is_ok_and(|v| v == "0") {
        return Ok(None);
    }
    let mut p = Preparer { tree, files: HashMap::new(), done: HashMap::new(), busy: HashSet::new() };
    let entry = normalize(entry).context("the model's file is outside its folder")?;
    Ok(p.unit(&entry)?.map(|rel| tree.join(rel)))
}

/// A line of OpenSCAD's output with the copies' names put back (`bin.~1a2b3c4d.scad` → `bin.scad`).
pub fn clean_log(line: &str) -> std::borrow::Cow<'_, str> {
    if !line.contains(".~") {
        return line.into();
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find(".~") {
        let tail = &rest[i + 2..];
        let id_ok = tail.len() >= 8 && tail.as_bytes()[..8].iter().all(u8::is_ascii_hexdigit);
        if id_ok && tail[8..].starts_with(".scad") {
            out.push_str(&rest[..i]);
            rest = &tail[8..];
        } else {
            out.push_str(&rest[..i + 2]);
            rest = tail;
        }
    }
    out.push_str(rest);
    out.into()
}

#[derive(Debug, Clone)]
struct Directive {
    /// the whole `include <…>` or `use <…>` (byte offsets)
    span: (usize, usize),
    /// the path between `<` and `>`
    path: (usize, usize),
    include: bool,
    /// at the top level of the file, not inside a block
    top: bool,
}

#[derive(Debug, Default)]
struct Scanned {
    directives: Vec<Directive>,
    /// names defined at the top level: "v:x", "m:cup", "f:width"
    names: BTreeSet<String>,
    /// anything else at the top level (a module call, `if`, `echo`…)
    code: bool,
    /// something this doesn't follow (an include's path that doesn't end, a `}` too many)
    unsure: bool,
}

/// The text with comments, and what's inside strings, made spaces (newlines kept, so offsets and
/// lines stay where they were).
fn blank(text: &str) -> Vec<u8> {
    let mut b = text.as_bytes().to_vec();
    let space = |s: &mut [u8]| s.iter_mut().filter(|c| **c != b'\n').for_each(|c| *c = b' ');
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let end = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |k| i + k);
                space(&mut b[i..end]);
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let end = b[i + 2..].windows(2).position(|w| w == b"*/").map_or(b.len(), |k| i + 2 + k + 2);
                space(&mut b[i..end]);
                i = end;
            }
            b'"' => {
                let mut j = i + 1;
                while j < b.len() && b[j] != b'"' {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                let end = j.min(b.len());
                space(&mut b[i + 1..end]);
                i = end + 1;
            }
            _ => i += 1,
        }
    }
    b
}

fn ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

fn scan(text: &str) -> Scanned {
    let b = blank(text);
    let mut s = Scanned::default();
    let mut depth: i32 = 0;
    let mut stmt: Vec<u8> = vec![];
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if (c == b'i' || c == b'u') && (i == 0 || !ident(b[i - 1])) {
            let kw: &[u8] = if b[i..].starts_with(b"include") { b"include" } else if b[i..].starts_with(b"use") { b"use" } else { b"" };
            let mut j = i + kw.len();
            if !kw.is_empty() && b.get(j).is_none_or(|&c| !ident(c)) {
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if b.get(j) == Some(&b'<') {
                    let start = j + 1;
                    // (OpenSCAD's lexer: the path is [^\t\r\n>]*)
                    let end = b[start..].iter().position(|&c| matches!(c, b'>' | b'\n' | b'\r' | b'\t')).map(|k| start + k);
                    match end {
                        Some(e) if b[e] == b'>' => {
                            s.directives.push(Directive { span: (i, e + 1), path: (start, e), include: kw == b"include", top: depth == 0 });
                            if depth == 0 {
                                classify(&std::mem::take(&mut stmt), &mut s);
                            }
                            i = e + 1;
                            continue;
                        }
                        _ => s.unsure = true,
                    }
                }
            }
        }
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            s.unsure = true;
            depth = 0;
        }
        stmt.push(c);
        if depth == 0 && (c == b';' || c == b'}') {
            classify(&std::mem::take(&mut stmt), &mut s);
        }
        i += 1;
    }
    classify(&stmt, &mut s);
    s
}

/// One top-level statement: a definition (its name), or code.
fn classify(stmt: &[u8], s: &mut Scanned) {
    let t = String::from_utf8_lossy(stmt);
    let t = t.trim();
    if t.is_empty() || t == ";" {
        return;
    }
    let word = |t: &str| -> (String, String) {
        let end = t.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')).unwrap_or(t.len());
        (t[..end].to_string(), t[end..].trim_start().to_string())
    };
    let (w, rest) = word(t);
    if w == "module" || w == "function" {
        let (name, _) = word(&rest);
        if !name.is_empty() {
            s.names.insert(format!("{}:{name}", &w[..1]));
            return;
        }
    }
    if !w.is_empty() && !w.as_bytes()[0].is_ascii_digit() && rest.starts_with('=') && !rest.starts_with("==") {
        s.names.insert(format!("v:{w}"));
        return;
    }
    s.code = true;
}

/// A path inside the tree without `.` and `..` (None if it leaves the tree).
fn normalize(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(x) => out.push(x),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The name of a file's copy for one unit: `bin.scad` → `bin.~1a2b3c4d.scad`.
fn copy_name(base: &str, id: &str) -> String {
    format!("{}.~{id}.scad", base.strip_suffix(".scad").unwrap_or(base))
}

fn unit_id(root: &Path) -> String {
    let key = root.to_string_lossy().replace('\\', "/");
    hex::encode(&Sha256::digest(key.as_bytes())[..4])
}

struct Preparer<'a> {
    tree: &'a Path,
    /// tree-relative path → its text and what's in it (None: not readable as text)
    files: HashMap<PathBuf, Option<(String, Scanned)>>,
    /// units done: root → its copy's tree-relative path (None: left as it is)
    done: HashMap<PathBuf, Option<PathBuf>>,
    /// units being prepared (a `use` that comes back round is left as it is)
    busy: HashSet<PathBuf>,
}

impl Preparer<'_> {
    fn file(&mut self, rel: &Path) -> Option<&(String, Scanned)> {
        if !self.files.contains_key(rel) {
            let v = std::fs::read_to_string(self.tree.join(rel)).ok().map(|t| {
                let s = scan(&t);
                (t, s)
            });
            self.files.insert(rel.to_path_buf(), v);
        }
        self.files.get(rel).and_then(Option::as_ref)
    }

    /// Where OpenSCAD finds `name` from the file `from`: beside it, then in the libraries.
    fn resolve(&self, from: &Path, name: &str) -> Option<PathBuf> {
        if Path::new(name).is_absolute() {
            return None;
        }
        let dir = from.parent().unwrap_or(Path::new(""));
        [dir.join(name), Path::new(LIBRARIES).join(name)]
            .iter()
            .filter_map(|p| normalize(p))
            .find(|p| self.tree.join(p).is_file())
    }

    /// One file OpenSCAD reads as a whole (the model, or a file it `use`s): its copy, made now
    /// unless made before (None: left as it is).
    fn unit(&mut self, root: &Path) -> Result<Option<PathBuf>> {
        if let Some(d) = self.done.get(root) {
            return Ok(d.clone());
        }
        if !self.busy.insert(root.to_path_buf()) {
            return Ok(None);
        }
        let r = self.make_unit(root);
        self.busy.remove(root);
        let r = r?;
        self.done.insert(root.to_path_buf(), r.clone());
        Ok(r)
    }

    fn make_unit(&mut self, root: &Path) -> Result<Option<PathBuf>> {
        let id = unit_id(root);
        let base = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let copy = root.with_file_name(copy_name(&base, &id));
        if self.tree.join(&copy).is_file() {
            return Ok(Some(copy)); // made by an earlier render (written last, so complete)
        }
        // every file read with it (the includes at the top level, all the way down), and the files they use
        let mut order: Vec<PathBuf> = vec![];
        let mut todo = vec![root.to_path_buf()];
        let mut uses: Vec<PathBuf> = vec![];
        let mut seen: HashSet<PathBuf> = HashSet::new();
        while let Some(f) = todo.pop() {
            if !seen.insert(f.clone()) {
                continue;
            }
            let Some((text, s)) = self.file(&f) else { return Ok(None) };
            if s.unsure {
                return Ok(None);
            }
            let dirs: Vec<(bool, bool, String)> = s.directives.iter().map(|d| (d.include, d.top, text[d.path.0..d.path.1].to_string())).collect();
            order.push(f.clone());
            for (include, top, name) in dirs {
                if !top {
                    continue;
                }
                match (self.resolve(&f, &name), include) {
                    (Some(p), true) => todo.push(p),
                    (Some(p), false) => uses.push(p),
                    // an include from elsewhere (OpenSCAD's own libraries?) or missing: not followed
                    (None, true) => return Ok(None),
                    (None, false) => {}
                }
            }
        }
        // files it's safe to include once: no top-level code, and no name another of them defines
        let mut defined: HashMap<String, usize> = HashMap::new();
        for f in &order {
            for n in &self.files[f].as_ref().unwrap().1.names {
                *defined.entry(n.clone()).or_default() += 1;
            }
        }
        let once: HashSet<PathBuf> = order
            .iter()
            .filter(|f| {
                let s = &self.files[*f].as_ref().unwrap().1;
                !s.code && s.names.iter().all(|n| defined[n] == 1)
            })
            .cloned()
            .collect();
        // the files it uses first (each its own unit)
        let mut used: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
        for u in uses {
            if let std::collections::hash_map::Entry::Vacant(e) = used.entry(u.clone()) {
                e.insert(self.unit(&u)?);
            }
        }
        let mut made: HashMap<PathBuf, PathBuf> = HashMap::new();
        let mut included: HashSet<PathBuf> = HashSet::new();
        let mut stack = vec![];
        self.emit(root, &id, &once, &used, &mut included, &mut stack, &mut made)?;
        Ok(made.get(root).cloned())
    }

    /// Write `file`'s copy for unit `id` (once), returning its tree-relative path.
    #[allow(clippy::too_many_arguments)]
    fn emit(&mut self, file: &Path, id: &str, once: &HashSet<PathBuf>, used: &HashMap<PathBuf, Option<PathBuf>>,
            included: &mut HashSet<PathBuf>, stack: &mut Vec<PathBuf>, made: &mut HashMap<PathBuf, PathBuf>) -> Result<PathBuf> {
        let base = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let copy = file.with_file_name(copy_name(&base, id));
        if made.contains_key(file) {
            return Ok(copy);
        }
        let (text, s) = self.file(file).context("a file went missing")?;
        let (text, directives) = (text.clone(), s.directives.clone());
        stack.push(file.to_path_buf());
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for d in directives.iter().filter(|d| d.top) {
            let name = &text[d.path.0..d.path.1];
            let Some(target) = self.resolve(file, name) else { continue };
            // the path as written, its last part the copy's name
            let (dir, last) = name.rsplit_once(['/', '\\']).map_or(("", name), |(d, l)| (&name[..d.len() + 1], l));
            let replace = if d.include {
                if stack.contains(&target) {
                    Some(format!("include <{dir}{}>", copy_name(last, id)))
                } else if once.contains(&target) && included.contains(&target) {
                    Some(String::new())
                } else {
                    included.insert(target.clone());
                    self.emit(&target, id, once, used, included, stack, made)?;
                    Some(format!("include <{dir}{}>", copy_name(last, id)))
                }
            } else {
                // (the used file's own copy, when it has one)
                used.get(&target).cloned().flatten().map(|_| format!("use <{dir}{}>", copy_name(last, &unit_id(&target))))
            };
            if let Some(r) = replace {
                out.push_str(&text[at..d.span.0]);
                out.push_str(&r);
                at = d.span.1;
            }
        }
        out.push_str(&text[at..]);
        stack.pop();
        let dst = self.tree.join(&copy);
        let tmp = dst.with_extension(format!("tmp-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
        std::fs::write(&tmp, out).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &dst).with_context(|| format!("writing {}", dst.display()))?;
        made.insert(file.to_path_buf(), copy.clone());
        Ok(copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_top_level_statements() {
        let s = scan("include <a.scad>\nuse<lib/b.scad> // include <c.scad>\n/* use <d.scad> */ x = 1; $fn = 32;\nfunction f(a) = a == 1 ? \"include <e.scad>\" : 2;\nmodule m() { include <in.scad>\n cube(x); }\n");
        let paths: Vec<_> = s.directives.iter().map(|d| (d.include, d.top)).collect();
        assert_eq!(paths, vec![(true, true), (false, true), (true, false)]);
        assert_eq!(s.names.iter().cloned().collect::<Vec<_>>(), vec!["f:f", "m:m", "v:$fn", "v:x"]);
        assert!(!s.code && !s.unsure);
        assert!(scan("x = 1;\nif (demo) { cube(1); }\n").code);
        assert!(scan("translate([1, 0, 0]) cube(2);").code);
        assert!(scan("include <a.scad").unsure);
    }

    #[test]
    fn clean_log_puts_names_back() {
        assert_eq!(clean_log("WARNING: x in file modules/c.~0a1b2c3d.scad, line 3"), "WARNING: x in file modules/c.scad, line 3");
        assert_eq!(clean_log("a.~ b.~zz.scad"), "a.~ b.~zz.scad");
    }

    struct Tree(PathBuf);
    impl Tree {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tree(files: &[(&str, &str)]) -> Tree {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let t = Tree(std::env::temp_dir().join(format!("include-once-{}-{n}", std::process::id())));
        let _ = std::fs::remove_dir_all(t.path());
        for (p, text) in files {
            let p = t.path().join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        t
    }

    fn read(t: &Path, rel: &str) -> String {
        std::fs::read_to_string(t.join(rel)).unwrap_or_else(|_| panic!("no {rel}"))
    }

    #[test]
    fn a_constants_file_is_included_once() {
        let t = tree(&[
            ("model.scad", "include <m/consts.scad>\ninclude <m/a.scad>\ninclude <m/b.scad>\nuse <m/u.scad>\nw = 2;\nbox();\n"),
            ("m/consts.scad", "pitch = 42; // the grid\n"),
            ("m/a.scad", "include <consts.scad>\nmodule box() cube(pitch);\n"),
            ("m/b.scad", "include <consts.scad>\ninclude <demo.scad>\nfunction h() = pitch;\n"),
            ("m/demo.scad", "include <consts.scad>\nif (false) cube(1);\n"),
            ("m/u.scad", "include <consts.scad>\ninclude <consts.scad>\nmodule ub() cube(pitch);\n"),
        ]);
        let root = prepare(t.path(), Path::new("model.scad")).unwrap().unwrap();
        let id = unit_id(Path::new("model.scad"));
        assert_eq!(root, t.path().join(format!("model.~{id}.scad")));
        let uid = unit_id(Path::new("m/u.scad"));
        assert_eq!(read(t.path(), &format!("model.~{id}.scad")),
                   format!("include <m/consts.~{id}.scad>\ninclude <m/a.~{id}.scad>\ninclude <m/b.~{id}.scad>\nuse <m/u.~{uid}.scad>\nw = 2;\nbox();\n"));
        // consts.scad again: left out; demo.scad has code, so it stays (it's included once anyway)
        assert_eq!(read(t.path(), &format!("m/a.~{id}.scad")), "\nmodule box() cube(pitch);\n");
        assert_eq!(read(t.path(), &format!("m/b.~{id}.scad")), format!("\ninclude <demo.~{id}.scad>\nfunction h() = pitch;\n"));
        assert_eq!(read(t.path(), &format!("m/demo.~{id}.scad")), "\nif (false) cube(1);\n");
        // the used file is its own unit
        assert_eq!(read(t.path(), &format!("m/u.~{uid}.scad")), format!("include <consts.~{uid}.scad>\n\nmodule ub() cube(pitch);\n"));
        // done before: the same copy
        assert_eq!(prepare(t.path(), Path::new("model.scad")).unwrap().unwrap(), root);
    }

    #[test]
    fn included_again_when_it_could_matter() {
        // pitch is set by both files: including consts again resets it, so it stays
        let t = tree(&[
            ("model.scad", "include <consts.scad>\npitch = 7;\ninclude <a.scad>\n"),
            ("consts.scad", "pitch = 42;\n"),
            ("a.scad", "include <consts.scad>\nmodule box() cube(pitch);\n"),
        ]);
        prepare(t.path(), Path::new("model.scad")).unwrap().unwrap();
        let id = unit_id(Path::new("model.scad"));
        assert_eq!(read(t.path(), &format!("a.~{id}.scad")), format!("include <consts.~{id}.scad>\nmodule box() cube(pitch);\n"));
    }

    #[test]
    fn left_as_it_is_when_not_understood() {
        let t = tree(&[("model.scad", "include <MCAD/units.scad>\ncube(1);\n")]);
        assert_eq!(prepare(t.path(), Path::new("model.scad")).unwrap(), None);
        let t = tree(&[("model.scad", "include <../outside.scad>\n")]);
        assert_eq!(prepare(t.path(), Path::new("model.scad")).unwrap(), None);
        // from the libraries folder, as OpenSCAD finds it there
        let t = tree(&[("model.scad", "include <BOSL2/std.scad>\ncube(1);\n"), ("libraries/BOSL2/std.scad", "include <a.scad>\n"), ("libraries/BOSL2/a.scad", "a = 1;\n")]);
        let id = unit_id(Path::new("model.scad"));
        prepare(t.path(), Path::new("model.scad")).unwrap().unwrap();
        assert_eq!(read(t.path(), &format!("libraries/BOSL2/std.~{id}.scad")), format!("include <a.~{id}.scad>\n"));
    }
}
