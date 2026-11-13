//! Reference documents (Phase 4): what a project, an app library or a built-in
//! family offers to read beside its models, shown in the side viewer. Three kinds:
//! **HTML** (a Markdown README, converted when the project is read and kept in the
//! library), **PDF** (kept as it is: Printables downloads) and **text** (readme.txt).

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// Largest README converted or shown (bytes); NopSCADlib's is about 350 KB.
pub const MAX_DOC: u64 = 4 << 20;
/// PDFs offered per project.
const MAX_PDFS: usize = 6;

/// Where relative links and images in a document point. `None` keeps them relative
/// (to the project's folder, which the page sets as the frame's base); with `drop`,
/// relative ones that can't resolve are removed (an image keeps its alt text).
#[derive(Default, Clone, Copy)]
pub struct Rebase<'a> {
    pub img: Option<&'a str>,
    pub link: Option<&'a str>,
    pub drop: bool,
}

impl<'a> Rebase<'a> {
    /// GitHub at one commit: images from raw.githubusercontent.com, links to the file pages.
    pub fn github(raw: &'a str, blob: &'a str) -> Self {
        Rebase { img: Some(raw), link: Some(blob), drop: false }
    }
}

/// `https://github.com/<owner>/<repo>` (or `owner/repo`) and a commit → (raw base, blob base),
/// each ending in `/`, under `sub` (a folder in the repository, "" for the top).
pub fn github_bases(repo: &str, commit: &str, sub: &str) -> Option<(String, String)> {
    let r = repo.trim().trim_end_matches('/').trim_end_matches(".git");
    let r = r.strip_prefix("https://github.com/").or_else(|| r.strip_prefix("http://github.com/")).unwrap_or(r);
    let mut parts = r.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    if owner.is_empty() || name.is_empty() || r.contains(':') || commit.is_empty() {
        return None;
    }
    let sub = sub.trim_matches('/');
    let sub = if sub.is_empty() { String::new() } else { format!("{sub}/") };
    Some((
        format!("https://raw.githubusercontent.com/{owner}/{name}/{commit}/{sub}"),
        format!("https://github.com/{owner}/{name}/blob/{commit}/{sub}"),
    ))
}

/// The documents in a project folder: `[{ id, title, kind: html|pdf|text, src, from }]`
/// (`from`: "markdown" or "html" for an HTML document). The README first (Markdown, or
/// README.html), then PDFs at the top level or one folder down, then readme.txt.
pub fn find(root: &Path) -> Vec<Value> {
    let files = crate::scan::list_files(root);
    let mut readme: Option<Value> = None;
    let mut readme_html: Option<Value> = None;
    let mut text: Option<Value> = None;
    let mut pdfs: Vec<Value> = vec![];
    for f in &files {
        let depth = f.rel.matches('/').count();
        let lower = f.rel.to_lowercase();
        let name = lower.rsplit('/').next().unwrap_or(&lower).to_string();
        let original = f.rel.rsplit('/').next().unwrap_or(&f.rel).to_string();
        if depth == 0 && readme.is_none() && ["readme.md", "readme.markdown", "readme.mdown"].contains(&name.as_str()) {
            readme = Some(json!({ "id": "readme", "title": "README", "kind": "html", "src": f.rel, "from": "markdown" }));
        } else if depth == 0 && readme_html.is_none() && (name == "readme.html" || name == "readme.htm") {
            readme_html = Some(json!({ "id": "readme", "title": "README", "kind": "html", "src": f.rel, "from": "html" }));
        } else if depth == 0 && text.is_none() && (name == "readme.txt" || name == "readme") {
            text = Some(json!({ "id": "readme-txt", "title": original, "kind": "text", "src": f.rel }));
        } else if depth <= 1 && name.ends_with(".pdf") && f.bytes > 0 {
            let stem = original.trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
            pdfs.push(json!({ "id": format!("pdf-{}", slug(&f.rel)), "title": stem, "kind": "pdf", "src": f.rel, "bytes": f.bytes }));
        }
    }
    // shallower first, then by name
    pdfs.sort_by_key(|p| {
        let s = p["src"].as_str().unwrap_or("").to_lowercase();
        (s.matches('/').count(), s)
    });
    pdfs.truncate(MAX_PDFS);
    readme.or(readme_html).into_iter().chain(pdfs).chain(text).collect()
}

/// A Markdown README as HTML: CommonMark with GitHub's tables, strikethrough, task lists
/// and footnotes; headings get GitHub's anchors ("Table of Contents" → `table-of-contents`)
/// so a README's own contents links work. Cleaned and re-based (see [`clean_html`]).
pub fn markdown_html(md: &str, base: Rebase) -> String {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_GFM;
    let mut events: Vec<Event> = Parser::new_ext(md, opts).collect();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut i = 0;
    while i < events.len() {
        if let Event::Start(Tag::Heading { id: None, .. }) = &events[i] {
            let mut text = String::new();
            for e in &events[i + 1..] {
                match e {
                    Event::End(TagEnd::Heading(_)) => break,
                    Event::Text(t) | Event::Code(t) => text.push_str(t),
                    _ => {}
                }
            }
            let mut anchor = github_anchor(&text);
            let n = seen.entry(anchor.clone()).or_insert(0);
            if *n > 0 {
                anchor = format!("{anchor}-{n}");
            }
            *n += 1;
            if let Event::Start(Tag::Heading { id, .. }) = &mut events[i] {
                *id = Some(CowStr::from(anchor));
            }
        }
        i += 1;
    }
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, events.into_iter());
    clean_html(&out, base)
}

/// GitHub's heading anchor: lower case, punctuation dropped (except `-` and `_`), spaces to `-`.
pub fn github_anchor(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// HTML from a README, made safe to show and pointed at its files: elements that run or
/// load things (script, style, iframe, object, embed, form, meta, link, base) are removed
/// with their content, as are `on…` attributes, `srcset` and `javascript:` / `data:` links;
/// relative `src` and `href` are re-based (see [`Rebase`]). In-page `#anchor` links stay.
pub fn clean_html(html: &str, base: Rebase) -> String {
    static BLOCKS: OnceLock<Vec<Regex>> = OnceLock::new();
    static SINGLES: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    static COMMENT: OnceLock<Regex> = OnceLock::new();
    let mut s = re(&COMMENT, r"(?s)<!--.*?-->").replace_all(html, "").into_owned();
    // (one pattern per element: the regex crate has no back-references)
    let blocks = BLOCKS.get_or_init(|| {
        ["script", "style", "iframe", "object", "embed", "form", "template", "noscript", "textarea", "select", "svg", "math"]
            .iter()
            .map(|el| Regex::new(&format!(r"(?is)<{el}\b.*?</{el}\s*>")).expect("valid pattern"))
            .collect()
    });
    for r in blocks {
        s = r.replace_all(&s, "").into_owned();
    }
    s = re(&SINGLES, r"(?is)</?(script|style|iframe|object|embed|form|template|noscript|meta|link|base|frame|frameset|applet|textarea|select|svg|math)\b[^>]*>")
        .replace_all(&s, "")
        .into_owned();
    re(&TAG, r"(?s)<([a-zA-Z][a-zA-Z0-9-]*)(\s[^<>]*?)?(/?)>")
        .replace_all(&s, |c: &regex::Captures| {
            let name = &c[1];
            let attrs = c.get(2).map(|m| m.as_str()).unwrap_or("");
            let mut a = clean_attrs(name, attrs, base);
            // a long README's pictures load as they're scrolled to
            if name.eq_ignore_ascii_case("img") && !a.contains(" loading=") {
                a.push_str(" loading=\"lazy\"");
            }
            format!("<{name}{a}{}>", &c[3])
        })
        .into_owned()
}

fn clean_attrs(tag: &str, attrs: &str, base: Rebase) -> String {
    static ATTR: OnceLock<Regex> = OnceLock::new();
    let mut out = String::new();
    for c in re(&ATTR, r#"(?s)([^\s"'<>/=]+)(?:\s*=\s*("[^"]*"|'[^']*'|[^\s"'=<>`]+))?"#).captures_iter(attrs) {
        let name = c[1].to_ascii_lowercase();
        let raw = c.get(2).map(|m| m.as_str());
        let value = raw.map(|v| v.trim_matches(|q| q == '"' || q == '\'').to_string());
        if name.starts_with("on") || name == "srcset" || name == "style" || name == "formaction" {
            continue;
        }
        let value = match (name.as_str(), value) {
            ("src" | "href" | "poster" | "action" | "background", Some(v)) => match rebase(&name, tag, v.trim(), base) {
                Some(v) => Some(v),
                None => continue,
            },
            (_, v) => v,
        };
        match value {
            Some(v) => out.push_str(&format!(" {name}=\"{}\"", v.replace('"', "&quot;"))),
            None => out.push_str(&format!(" {name}")),
        }
    }
    out
}

/// A link or image address as the document will use it, or None to drop the attribute.
fn rebase(attr: &str, tag: &str, url: &str, base: Rebase) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("javascript:") || lower.starts_with("vbscript:") || lower.starts_with("data:") || lower.starts_with("file:") {
        return None;
    }
    if url.is_empty() || url.starts_with('#') {
        return Some(url.to_string());
    }
    let has_scheme = url.find(':').is_some_and(|p| url[..p].chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)) && p > 0);
    if has_scheme || url.starts_with("//") {
        return Some(url.to_string());
    }
    // relative: to the README's folder ("/x" is the repository's top, which is where the README is)
    let path = url.trim_start_matches("./").trim_start_matches('/');
    let image = attr == "src" || (tag.eq_ignore_ascii_case("img") && attr != "href");
    let to = if image { base.img } else { base.link };
    match to {
        Some(b) => Some(format!("{b}{path}")),
        None if base.drop => None,
        None => Some(path.to_string()),
    }
}

/// A document's HTML from its source file: Markdown converted, HTML cleaned.
pub fn html_from_file(path: &Path, from: &str, base: Rebase) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    if bytes.len() as u64 > MAX_DOC {
        anyhow::bail!("{} is too large to show ({} MB)", path.display(), bytes.len() >> 20);
    }
    let text = String::from_utf8_lossy(&bytes);
    Ok(if from == "html" { clean_html(&text, base) } else { markdown_html(&text, base) })
}

/// The documents of a folder made ready to keep: each HTML one converted into `out_dir`
/// as `<id>.html` (its `file` set to `<rel_prefix><id>.html`). Problems are returned, not fatal.
pub fn convert_all(root: &Path, docs: &mut [Value], out_dir: &Path, rel_prefix: &str, base: Rebase) -> Vec<String> {
    let mut problems = vec![];
    for d in docs.iter_mut().filter(|d| d["kind"] == "html") {
        let id = d["id"].as_str().unwrap_or("readme").to_string();
        let src = d["src"].as_str().unwrap_or("").to_string();
        let from = d["from"].as_str().unwrap_or("markdown").to_string();
        let res = crate::library::rel_inside(&src).and_then(|rel| html_from_file(&root.join(rel), &from, base))
            .and_then(|h| {
                std::fs::create_dir_all(out_dir)?;
                crate::config::write_atomic(&out_dir.join(format!("{id}.html")), h.as_bytes())?;
                Ok(())
            });
        match res {
            Ok(()) => {
                d["file"] = json!(format!("{rel_prefix}{id}.html"));
                // links and images relative to the project's folder (the page sets it as the base)
                d["relative"] = json!(base.img.is_none() && !base.drop);
            }
            Err(e) => problems.push(format!("{src}: {e:#}")),
        }
    }
    problems
}

/// An app library's documents (BOSL2, NopSCADlib, …): found in its folder, the HTML ones
/// converted with images and links to GitHub at the pinned commit. With `out`, the HTML is
/// written there as `<id>.html` (`file` names it); without, it's left to be made on request.
pub fn library_docs(info: &crate::components::LibInfo, root: &Path, out: Option<&Path>) -> Vec<Value> {
    let mut docs: Vec<Value> = find(root).into_iter().filter(|d| d["kind"] != "pdf").collect();
    if let Some(out) = out {
        let bases = library_bases(info);
        let base = bases.as_ref().map(|(r, b)| Rebase::github(r, b)).unwrap_or(Rebase { drop: true, ..Default::default() });
        for p in convert_all(root, &mut docs, out, "", base) {
            eprintln!("{}: {p}", info.name);
        }
        for d in docs.iter_mut() {
            d["relative"] = json!(false);
        }
    }
    docs
}

/// GitHub addresses for an app library's README (raw, blob).
pub fn library_bases(info: &crate::components::LibInfo) -> Option<(String, String)> {
    github_bases(info.repo.as_deref()?, info.commit.as_deref()?, "")
}

/// A built-in family's README for the website and the starter library: found in the
/// folder of its models (or a folder above, up to its vendored copy), converted with
/// images and links pointing to GitHub at the pinned commit, and written to
/// `<out>/data/docs/<family>.html` (or `.txt`). Returns the catalog's `docs` list.
pub fn family_docs(repo: &Path, fam: &Value, out: &Path) -> anyhow::Result<Vec<Value>> {
    let id = fam["id"].as_str().unwrap_or("");
    let vendor = fam["source"]["vendor_path"].as_str().map(|v| v.trim_matches('/').to_string());
    // the models' common folder
    let entries: Vec<&str> = fam["models"].as_array().into_iter().flatten().filter_map(|m| m["entrypoint"].as_str()).collect();
    let mut common: Option<Vec<&str>> = None;
    for e in &entries {
        let dirs: Vec<&str> = e.split('/').collect::<Vec<_>>().split_last().map(|(_, d)| d.to_vec()).unwrap_or_default();
        common = Some(match common {
            None => dirs,
            Some(c) => c.iter().zip(dirs.iter()).take_while(|(a, b)| a == b).map(|(a, _)| *a).collect(),
        });
    }
    let mut folder = common.map(|c| c.join("/")).or_else(|| vendor.clone()).unwrap_or_default();
    if let Some(v) = &vendor {
        if !folder.starts_with(v.as_str()) {
            folder = v.clone();
        }
    }
    loop {
        let found: Vec<Value> = find(&repo.join(&folder)).into_iter().filter(|d| d["kind"] != "pdf").collect();
        if let Some(d) = found.into_iter().next() {
            let src = repo.join(&folder).join(d["src"].as_str().unwrap_or(""));
            let dir = out.join("data/docs");
            std::fs::create_dir_all(&dir)?;
            if d["kind"] == "text" {
                std::fs::copy(&src, dir.join(format!("{id}.txt")))?;
                return Ok(vec![json!({ "id": "readme", "title": "README", "kind": "text", "url": format!("data/docs/{id}.txt") })]);
            }
            // where the folder is in the upstream repository (the vendored copy is its top)
            let sub = vendor.as_deref().and_then(|v| folder.strip_prefix(v)).unwrap_or("").trim_matches('/').to_string();
            let bases = match (fam["source"]["repository"].as_str(), fam["source"]["commit"].as_str()) {
                (Some(r), Some(c)) => github_bases(r, c, &sub),
                _ => None,
            };
            let base = match &bases {
                Some((raw, blob)) => Rebase::github(raw, blob),
                None => Rebase { drop: true, ..Default::default() },
            };
            let html = html_from_file(&src, d["from"].as_str().unwrap_or("markdown"), base)?;
            std::fs::write(dir.join(format!("{id}.html")), html)?;
            return Ok(vec![json!({ "id": "readme", "title": "README", "kind": "html", "url": format!("data/docs/{id}.html") })]);
        }
        // up one folder, not past the vendored copy
        if vendor.as_deref().is_none_or(|v| folder == v) || !folder.contains('/') {
            return Ok(vec![]);
        }
        folder = folder.rsplit_once('/').map(|(a, _)| a.to_string()).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_to_html() {
        let md = "# SCAD Workshop\n\n## Table of Contents\n\n- [Gears](#gears)\n\n## Gears\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n![pic](docs/pic.png)\n\n[code](gears.scad) [site](https://example.com)\n\n## Gears\n";
        let h = markdown_html(md, Rebase::default());
        assert!(h.contains(r#"<h2 id="table-of-contents">"#), "{h}");
        assert!(h.contains(r#"<h2 id="gears">"#) && h.contains(r#"<h2 id="gears-1">"#), "{h}");
        assert!(h.contains("<table>") && h.contains("<td>1</td>"), "{h}");
        assert!(h.contains(r#"src="docs/pic.png""#) && h.contains(r#"href="gears.scad""#) && h.contains(r##"href="#gears""##), "{h}");
        let (raw, blob) = github_bases("https://github.com/BelfrySCAD/BOSL2", "abc123", "").unwrap();
        let h = markdown_html(md, Rebase::github(&raw, &blob));
        assert!(h.contains(r#"src="https://raw.githubusercontent.com/BelfrySCAD/BOSL2/abc123/docs/pic.png""#), "{h}");
        assert!(h.contains(r#"href="https://github.com/BelfrySCAD/BOSL2/blob/abc123/gears.scad""#), "{h}");
        assert!(h.contains(r#"href="https://example.com""#) && h.contains(r##"href="#gears""##), "{h}");
        // no base and nothing to point to: images go, link text stays
        let h = markdown_html(md, Rebase { drop: true, ..Default::default() });
        assert!(!h.contains("docs/pic.png") && h.contains(r#"alt="pic""#) && h.contains(">code</a>"), "{h}");
    }

    #[test]
    fn raw_html_is_cleaned() {
        let md = "Hi <a href = \"#7_segments\">7</a> <a name=\"7_segments\"></a>\n\n<img src=\"libtest.png\" width=\"100%\" onerror=\"alert(1)\"/>\n\n<script>alert(1)</script>\n\n<iframe src=\"https://x\"></iframe><a href=\"javascript:alert(1)\">x</a><div style=\"position:fixed\" onclick='x()'>d</div>\n";
        let h = markdown_html(md, Rebase::default());
        assert!(h.contains(r##"href="#7_segments""##) && h.contains(r#"name="7_segments""#), "{h}");
        assert!(h.contains(r#"<img src="libtest.png" width="100%" loading="lazy"/>"#), "{h}");
        for bad in ["script", "alert", "iframe", "javascript", "onclick", "position:fixed", "onerror"] {
            assert!(!h.contains(bad), "{bad} in {h}");
        }
    }

    #[test]
    fn documents_in_a_folder() {
        let dir = std::env::temp_dir().join(format!("docs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("files/deep/er")).unwrap();
        std::fs::write(dir.join("README.md"), "# Hi\n").unwrap();
        std::fs::write(dir.join("readme.txt"), "plain").unwrap();
        std::fs::write(dir.join("Rugged Box.pdf"), "%PDF-1.4").unwrap();
        std::fs::write(dir.join("files/Assembly.pdf"), "%PDF-1.4").unwrap();
        std::fs::write(dir.join("files/deep/er/too-deep.pdf"), "%PDF-1.4").unwrap();
        let docs = find(&dir);
        let kinds: Vec<(&str, &str)> = docs.iter().map(|d| (d["kind"].as_str().unwrap(), d["title"].as_str().unwrap())).collect();
        assert_eq!(kinds, vec![("html", "README"), ("pdf", "Rugged Box"), ("pdf", "Assembly"), ("text", "readme.txt")]);
        let mut docs = docs;
        let probs = convert_all(&dir, &mut docs, &dir.join("out"), "docs/v1/", Rebase::default());
        assert!(probs.is_empty(), "{probs:?}");
        assert_eq!(docs[0]["file"], "docs/v1/readme.html");
        assert!(std::fs::read_to_string(dir.join("out/readme.html")).unwrap().contains(r#"<h1 id="hi">Hi</h1>"#));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
