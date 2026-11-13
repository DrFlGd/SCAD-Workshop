//! The library folder: everything the user adds, makes or edits, as plain files
//! with relative paths, so it can be moved, copied, synced or kept in git, and
//! opened by any build of the app (docs/DESKTOP_PLAN.md, "Library folder").
//!
//! ```text
//! library/
//!   library.json                  format, id, name, favourites, library-level metadata, categories
//!   sources/<id>/source.json      where a project came from, what was detected, versions, updates
//!   sources/<id>/metadata.json    the user's edits (project, folder and item level)
//!   sources/<id>/files/<version>/ downloaded or extracted files, untouched
//!   sources/<id>/derived/<v>.json what ingest found (models, parts, problems); rebuildable
//!   sources/<id>/thumbs/          thumbnails; rebuildable
//!   sources/<id>/checks.json      each model rendered at its defaults: ok or OpenSCAD's message; rebuildable
//!   sources/<id>/workshop.json    the description the app uses (models, names, forms); rebuildable
//!   sources/<id>/description.json the app's own description of a project it knows from GitHub (F-1)
//!   local/<project>/              the user's own projects, edited in place
//!   collections/<id>/collection.json  a collection: items by id (not copies), quantities, status, notes
//!   icons/, thumbs/components/    category pictures; component thumbnails (rebuildable)
//!   trash/                        deleted projects
//!   recipes/<id>.json             saved settings
//! ```
//!
//! No database lives here: the app's search index and render cache are on the computer.

use crate::config::{read_json_object, write_json};
use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::path::{Component, Path, PathBuf};

/// The library format this build writes. A library with a higher number opens read-only.
/// 2 (0.4.22): every JSON file the app writes says its format (format.rs), and saved
/// settings and made parts' recipes have one shape. A format 1 library is upgraded on open.
pub const FORMAT: u64 = 2;

const SUBDIRS: [&str; 4] = ["sources", "local", "collections", "recipes"];

/// The README.txt that 0.4.22 and 0.4.23 wrote: replaced when found unchanged.
const README_0_4_22: &str = concat!(
    "SCAD Workshop library\n",
    "\n",
    "This folder is a library for the SCAD Workshop desktop app: projects you added,\n",
    "your own projects, saved settings and all your edits. Move it, copy it, sync it\n",
    "or keep it in git, then use \"Open library...\" in the app to use it anywhere.\n",
    "\n",
    "Library format 2 (SCAD Workshop 0.4.22 on). Each JSON file the app writes says\n",
    "its \"format\"; a file with a higher number than the app knows was made by a\n",
    "newer version, and the app reads it without writing over it. Library settings >\n",
    "Check library lists anything that isn't as the app keeps it. The full\n",
    "description: docs/LIBRARY_FORMAT.md in the app's source code.\n",
    "\n",
    "  library.json    name, favourites, categories and library-wide settings\n",
    "  sources/        one folder per project (below)\n",
    "  local/          your own projects: put .scad or model files in a folder here\n",
    "  recipes/        saved settings, with any code edits (a recipe, below)\n",
    "  collections/    lists of parts and models with quantities and how many are\n",
    "                  printed: collections/<id>/collection.json. Entries refer to\n",
    "                  items by id (part:<project>/<part>, gen:<project>/<model>), not\n",
    "                  copies; a model's entry keeps its settings and the fingerprint\n",
    "                  of the files it used, as a recipe does\n",
    "  icons/          pictures you chose for categories\n",
    "  thumbs/         thumbnails of components (rebuildable)\n",
    "  trash/          deleted projects, until you empty the trash\n",
    "  backups/        JSON files as they were before the app upgraded them to a\n",
    "                  newer format, as ZIPs (delete them when you like)\n",
    "\n",
    "Inside sources/<project>/:\n",
    "  source.json     where it came from and its versions; \"version\" is the one in\n",
    "                  use. origin.type says where from: github (checked for updates),\n",
    "                  zip, folder (copied in), linked (a folder elsewhere, below),\n",
    "                  local (local/<name>/), app (a starter project from an earlier\n",
    "                  app) or pinned (your pinned components).\n",
    "                  origin.site names the model's page on a site (Printables) and\n",
    "                  the PDF of that page among its files. \"detected\" is what was\n",
    "                  read from the files and that PDF; your edits win over it\n",
    "  metadata.json   your edits: names, categories, licenses, forms, parts made into\n",
    "                  one and files kept apart (\"apart\"), projects merged into it,\n",
    "                  and what its folder of examples should be\n",
    "  files/<version>/   the project's files exactly as they came, never changed. It\n",
    "                  can hold several versions: on the project's page, Versions >\n",
    "                  Remove versions not in use deletes the others\n",
    "  derived/        what the app found when it read them (rebuildable)\n",
    "  generated/      copies made so typed-in values become settings (rebuildable)\n",
    "  docs/           the README as HTML, PDFs (rebuildable)\n",
    "  thumbs/         pictures of its models and parts (rebuildable)\n",
    "  examples/<fingerprint>/   parts made from a folder of examples, kept by the\n",
    "                  files each example uses (rebuildable)\n",
    "  previews/       models the app made for its parts (rebuildable)\n",
    "  checks.json     whether each model renders at its defaults (rebuildable)\n",
    "  workshop.json   what the app read the project as: its models and their\n",
    "                  names and forms. It reports; it doesn't decide (rebuildable)\n",
    "  description.json   the app's description of a project it knows from GitHub (a\n",
    "                  starter project from an earlier app, Gridfinity Rebuilt...):\n",
    "                  names and forms for the files it lists, and the files it leaves\n",
    "                  out; any other file that draws something is a model too. Used\n",
    "                  when the project has no workshop.json of its own. Keep it\n",
    "\n",
    "Your own projects (local/<name>/) are read where they are: their sources/ folder\n",
    "has no files/. Linked projects (origin \"linked\") are read from their own folder\n",
    "outside the library: they don't move with it and aren't in its backups.\n",
    "sources/pinned/pins/ holds the components you pinned as models: keep it, it\n",
    "isn't rebuildable.\n",
    "\n",
    "local/Made parts/<model>/ holds the parts you made from models (Save as part,\n",
    "Make several), in a folder per model (\"<model> (<project>)\" when two models\n",
    "share a name), as .stl or .3mf (the format chosen beside Download). Beside each\n",
    "part, <name>.recipe.json says how it was made. Saved settings (recipes/) and\n",
    "made parts' recipes have one shape: the model, its project and the project's\n",
    "version, the fingerprint of the files the model used, the changed settings and\n",
    "code edits, the file type (\"output\": stl or 3mf) and when it was made.\n",
    "Make again and Open with these settings read it: keep it, it isn't rebuildable.\n",
    "\n",
    "Rebuildable folders can be deleted: the app makes them again.\n",
    "The app's render cache and search index are kept on each computer, not here.\n",
);

const README_0_4_23: &str = concat!(
    "SCAD Workshop library\n",
    "\n",
    "This folder is a library for the SCAD Workshop desktop app: projects you added,\n",
    "your own projects, saved settings and all your edits. Move it, copy it, sync it\n",
    "or keep it in git, then use \"Open library...\" in the app to use it anywhere.\n",
    "\n",
    "Library format 2 (SCAD Workshop 0.4.22 on). Each JSON file the app writes says\n",
    "its \"format\"; a file with a higher number than the app knows was made by a\n",
    "newer version, and the app reads it without writing over it. Library settings >\n",
    "Check library lists anything that isn't as the app keeps it. The full\n",
    "description: docs/LIBRARY_FORMAT.md in the app's source code.\n",
    "\n",
    "  library.json    name, favourites, categories and library-wide settings\n",
    "  sources/        one folder per project (below)\n",
    "  local/          your own projects: put .scad or model files in a folder here\n",
    "  recipes/        saved settings, with any code edits (a recipe, below)\n",
    "  collections/    lists of parts and models with quantities and how many are\n",
    "                  printed: collections/<id>/collection.json. Entries refer to\n",
    "                  items by id (part:<project>/<part>, gen:<project>/<model>), not\n",
    "                  copies; a model's entry keeps its settings and the fingerprint\n",
    "                  of the files it used, as a recipe does\n",
    "  icons/          pictures you chose for categories\n",
    "  thumbs/         thumbnails of components (rebuildable)\n",
    "  trash/          deleted projects, until you empty the trash\n",
    "  backups/        JSON files as they were before the app upgraded them to a\n",
    "                  newer format, as ZIPs (delete them when you like)\n",
    "\n",
    "Inside sources/<project>/:\n",
    "  source.json     where it came from and its versions; \"version\" is the one in\n",
    "                  use. origin.type says where from: github (checked for updates),\n",
    "                  zip, folder (copied in), linked (a folder elsewhere, below),\n",
    "                  local (local/<name>/), app (a starter project from an earlier\n",
    "                  app) or pinned (your pinned components).\n",
    "                  origin.site names the model's page on a site (Printables) and\n",
    "                  the PDF of that page among its files. \"detected\" is what was\n",
    "                  read from the files and that PDF; your edits win over it\n",
    "  metadata.json   your edits: names, categories, licenses, forms, your notes\n",
    "                  (\"my_notes\"), parts made into one and files kept apart\n",
    "                  (\"apart\"), projects merged into it, and what its folder of\n",
    "                  examples should be\n",
    "  files/<version>/   the project's files exactly as they came, never changed. It\n",
    "                  can hold several versions: on the project's page, Versions >\n",
    "                  Remove versions not in use deletes the others\n",
    "  derived/        what the app found when it read them (rebuildable)\n",
    "  generated/      copies made so typed-in values become settings (rebuildable)\n",
    "  docs/           the README as HTML, PDFs (rebuildable)\n",
    "  thumbs/         pictures of its models and parts (rebuildable)\n",
    "  examples/<fingerprint>/   parts made from a folder of examples, kept by the\n",
    "                  files each example uses (rebuildable)\n",
    "  previews/       models the app made for its parts (rebuildable)\n",
    "  checks.json     whether each model renders at its defaults (rebuildable)\n",
    "  workshop.json   what the app read the project as: its models and their\n",
    "                  names and forms. It reports; it doesn't decide (rebuildable)\n",
    "  description.json   the app's description of a project it knows from GitHub (a\n",
    "                  starter project from an earlier app, Gridfinity Rebuilt...):\n",
    "                  names and forms for the files it lists, and the files it leaves\n",
    "                  out; any other file that draws something is a model too. Used\n",
    "                  when the project has no workshop.json of its own. Keep it\n",
    "\n",
    "Your own projects (local/<name>/) are read where they are: their sources/ folder\n",
    "has no files/. Linked projects (origin \"linked\") are read from their own folder\n",
    "outside the library: they don't move with it and aren't in its backups.\n",
    "sources/pinned/pins/ holds the components you pinned as models: keep it, it\n",
    "isn't rebuildable.\n",
    "\n",
    "local/Made parts/<model>/ holds the parts you made from models (Save as part,\n",
    "Batch generate), in a folder per model (\"<model> (<project>)\" when two models\n",
    "share a name), as .stl or .3mf (the format chosen beside Download). Beside each\n",
    "part, <name>.recipe.json says how it was made. Saved settings (recipes/) and\n",
    "made parts' recipes have one shape: the model, its project and the project's\n",
    "version, the fingerprint of the files the model used, the changed settings and\n",
    "code edits, the file type (\"output\": stl or 3mf) and when it was made.\n",
    "Make again and Open with these settings read it: keep it, it isn't rebuildable.\n",
    "\n",
    "Rebuildable folders can be deleted: the app makes them again.\n",
    "The app's render cache and search index are kept on each computer, not here.\n",
);

pub const README: &str = concat!(
    "SCAD Workshop library\n",
    "\n",
    "This folder is a library for the SCAD Workshop desktop app: projects you added,\n",
    "your own projects, saved settings and all your edits. Move it, copy it, sync it\n",
    "or keep it in git, then use \"Open library...\" in the app to use it anywhere.\n",
    "\n",
    "Library format 2 (SCAD Workshop 0.4.22 on). Each JSON file the app writes says\n",
    "its \"format\"; a file with a higher number than the app knows was made by a\n",
    "newer version, and the app reads it without writing over it. Library settings >\n",
    "Check library lists anything that isn't as the app keeps it. The full\n",
    "description: docs/LIBRARY_FORMAT.md in the app's source code.\n",
    "\n",
    "  library.json    name, favourites, categories and library-wide settings\n",
    "  sources/        one folder per project (below)\n",
    "  local/          your own projects: put .scad or model files in a folder here\n",
    "  recipes/        saved settings, with any code edits (a recipe, below)\n",
    "  collections/    lists of parts and models with quantities and how many are\n",
    "                  printed: collections/<id>/collection.json. Entries refer to\n",
    "                  items by id (part:<project>/<part>, gen:<project>/<model>), not\n",
    "                  copies; a model's entry keeps its settings and the fingerprint\n",
    "                  of the files it used, as a recipe does\n",
    "  icons/          pictures you chose for categories\n",
    "  thumbs/         thumbnails of components (rebuildable)\n",
    "  trash/          deleted projects, until you empty the trash\n",
    "  backups/        JSON files as they were before the app upgraded them to a\n",
    "                  newer format, as ZIPs (delete them when you like)\n",
    "\n",
    "Inside sources/<project>/:\n",
    "  source.json     where it came from and its versions; \"version\" is the one in\n",
    "                  use. origin.type says where from: github (checked for updates),\n",
    "                  zip, folder (copied in), linked (a folder elsewhere, below),\n",
    "                  local (local/<name>/), app (a starter project from an earlier\n",
    "                  app) or pinned (your pinned components).\n",
    "                  origin.site names the model's page on a site (Printables) and\n",
    "                  the PDF of that page among its files. \"detected\" is what was\n",
    "                  read from the files and that PDF; your edits win over it\n",
    "  metadata.json   your edits: names, categories, licenses, forms, your notes\n",
    "                  (\"my_notes\"), parts made into one and files kept apart\n",
    "                  (\"apart\"), projects merged into it, and what its folder of\n",
    "                  examples should be. A picture you chose is its \"icon\":\n",
    "                  file:<path> (one of its files) or picture:<name> (below);\n",
    "                  content you added is listed under \"attachments\"\n",
    "  pictures/       pictures you added for it (keep)\n",
    "  attachments/    files you added to it: guides, videos, photos (keep)\n",
    "  files/<version>/   the project's files exactly as they came, never changed. It\n",
    "                  can hold several versions: on the project's page, Versions >\n",
    "                  Remove versions not in use deletes the others\n",
    "  derived/        what the app found when it read them (rebuildable)\n",
    "  generated/      copies made so typed-in values become settings (rebuildable)\n",
    "  docs/           the README as HTML, PDFs (rebuildable)\n",
    "  thumbs/         pictures of its models and parts (rebuildable)\n",
    "  examples/<fingerprint>/   parts made from a folder of examples, kept by the\n",
    "                  files each example uses (rebuildable)\n",
    "  previews/       models the app made for its parts (rebuildable)\n",
    "  checks.json     whether each model renders at its defaults (rebuildable)\n",
    "  workshop.json   what the app read the project as: its models and their\n",
    "                  names and forms. It reports; it doesn't decide (rebuildable)\n",
    "  description.json   the app's description of a project it knows from GitHub (a\n",
    "                  starter project from an earlier app, Gridfinity Rebuilt...):\n",
    "                  names and forms for the files it lists, and the files it leaves\n",
    "                  out; any other file that draws something is a model too. Used\n",
    "                  when the project has no workshop.json of its own. Keep it\n",
    "\n",
    "Your own projects (local/<name>/) are read where they are: their sources/ folder\n",
    "has no files/. Linked projects (origin \"linked\") are read from their own folder\n",
    "outside the library: they don't move with it and aren't in its backups.\n",
    "sources/pinned/pins/ holds the components you pinned as models: keep it, it\n",
    "isn't rebuildable.\n",
    "\n",
    "local/Made parts/<model>/ holds the parts you made from models (Save as part,\n",
    "Batch generate), in a folder per model (\"<model> (<project>)\" when two models\n",
    "share a name), as .stl or .3mf (the format chosen beside Download). Beside each\n",
    "part, <name>.recipe.json says how it was made. Saved settings (recipes/) and\n",
    "made parts' recipes have one shape: the model, its project and the project's\n",
    "version, the fingerprint of the files the model used, the changed settings and\n",
    "code edits, the file type (\"output\": stl or 3mf) and when it was made.\n",
    "Make again and Open with these settings read it: keep it, it isn't rebuildable.\n",
    "\n",
    "Rebuildable folders can be deleted: the app makes them again.\n",
    "The app's render cache and search index are kept on each computer, not here.\n",
);

#[derive(Clone, Debug)]
pub struct Library {
    root: PathBuf,
    /// Made by a newer app (format above [`FORMAT`]): nothing is written.
    read_only: Option<String>,
}

/// What opening a library changed.
#[derive(Debug, Default)]
pub struct Migration {
    /// Files brought up to this version's format on opening (backed up first).
    pub upgraded: Vec<String>,
}

pub fn valid_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 96 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.') || id.starts_with('.') {
        bail!("invalid id {id}");
    }
    Ok(())
}

/// A library-relative path ("sources/x/files/abc/a.scad") that stays inside the library.
pub fn rel_inside(rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel.trim_start_matches('/'));
    if rel.is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("invalid library path {rel}");
    }
    Ok(p.to_path_buf())
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    iso_from_unix(secs as i64)
}

/// "2026-10-05T09:48:26Z" from seconds since 1970.
pub fn iso_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil_from_days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn now() -> String {
    now_iso()
}

fn random_id(prefix: &str) -> String {
    use sha2::{Digest, Sha256};
    let seed = format!("{:?}{}{:?}", std::time::SystemTime::now(), std::process::id(), std::thread::current().id());
    format!("{prefix}{}", &hex::encode(Sha256::digest(seed.as_bytes()))[..16])
}

impl Library {
    /// Open a library folder, creating it if needed, and bring a format 1 library (0.4.21 and
    /// before) to format 2.
    pub fn open(root: impl Into<PathBuf>) -> Result<(Self, Migration)> {
        let root = root.into();
        std::fs::create_dir_all(&root).with_context(|| format!("couldn't create {}", root.display()))?;
        let meta_path = root.join("library.json");
        let mut migration = Migration::default();
        if meta_path.is_file() {
            let meta = read_json_object(&meta_path);
            let format = meta["format"].as_u64().unwrap_or(0);
            if format > FORMAT {
                let msg = format!(
                    "This library was made by a newer version of SCAD Workshop (library format {format}; this version reads up to {FORMAT}). \
                     It's open read-only: update the app to change it."
                );
                return Ok((Self { root, read_only: Some(msg) }, migration));
            }
        }
        let lib = Self { root, read_only: None };
        for d in SUBDIRS {
            let p = lib.root.join(d);
            std::fs::create_dir_all(&p).with_context(|| format!("couldn't create {}", p.display()))?;
        }
        let fresh = !meta_path.is_file();
        if fresh {
            let name = lib.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Library".into());
            write_json(&meta_path, &json!({ "format": FORMAT, "id": random_id("lib-"), "name": name, "created": now_iso(), "favourites": [] }))?;
        }
        let readme = lib.root.join("README.txt");
        let old_readme = std::fs::read_to_string(&readme).ok();
        // written by an older version and left as it was: replaced; one the user changed is left alone
        if old_readme.as_deref().is_none_or(|t| [README_0_4_22, README_0_4_23].contains(&t)) {
            let _ = std::fs::write(&readme, README);
        }
        if fresh || lib.meta()["format"].as_u64().unwrap_or(1) < FORMAT {
            migration.upgraded = lib.upgrade_files("format-1")?;
            let mut meta = lib.meta();
            meta["format"] = json!(FORMAT);
            write_json(&meta_path, &meta)?;
        }
        Ok((lib, migration))
    }

    /// Every JSON file the app writes, as the format table (format.rs) names them: the
    /// library's own, each project's, collections, saved settings, made parts' recipes
    /// (anywhere under local/) and trash entries.
    pub fn json_files(&self) -> Vec<PathBuf> {
        let mut out = vec![self.root.join("library.json")];
        let kids = |d: PathBuf| -> Vec<PathBuf> { std::fs::read_dir(d).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default() };
        for s in kids(self.sources_dir()) {
            out.extend(["source.json", "metadata.json", "checks.json"].map(|f| s.join(f)));
            out.extend(kids(s.join("pins")));
        }
        out.extend(kids(self.collections_dir()).into_iter().map(|c| c.join("collection.json")));
        out.extend(kids(self.recipes_dir()));
        out.extend(kids(self.trash_dir()).into_iter().map(|t| t.join("trash.json")));
        fn walk(d: &Path, depth: usize, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let p = e.path();
                let Ok(t) = e.file_type() else { continue };
                if t.is_dir() && depth < 12 {
                    walk(&p, depth + 1, out);
                } else if t.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".recipe.json")) {
                    out.push(p);
                }
            }
        }
        walk(&self.root.join("local"), 0, &mut out);
        out.retain(|p| p.is_file() && crate::format::kind_of(p).is_some());
        out
    }

    /// Bring every JSON file up to this version's shape (format.rs): each gets its format
    /// number and recipes take one shape. The files it changes are first copied, as they
    /// were, to `backups/<label>-<date>.zip`. Files a newer version wrote, and files that
    /// aren't valid JSON, are left alone (Check library lists them). Returns the changed
    /// files, relative to the library. Run on opening a format 1 library, and after Merge.
    pub fn upgrade_files(&self, label: &str) -> Result<Vec<String>> {
        self.writable()?;
        let mut changed: Vec<(PathBuf, Vec<u8>, Value)> = vec![];
        for p in self.json_files() {
            let Some(kind) = crate::format::kind_of(&p) else { continue };
            let Ok(bytes) = std::fs::read(&p) else { continue };
            let Ok(mut v) = serde_json::from_slice::<Value>(&bytes) else { continue };
            if kind != crate::format::Kind::Library && crate::format::upgrade(kind, &mut v, Some(&p)) {
                changed.push((p, bytes, v));
            }
        }
        let rel = |p: &Path| p.strip_prefix(&self.root).unwrap_or(p).to_string_lossy().replace('\\', "/");
        if !changed.is_empty() {
            let stamp: String = now().chars().filter(char::is_ascii_digit).take(14).collect();
            let dest = self.root.join("backups").join(format!("{label}-{stamp}.zip"));
            std::fs::create_dir_all(dest.parent().unwrap())?;
            let mut z = zip::ZipWriter::new(std::io::BufWriter::new(std::fs::File::create(&dest).with_context(|| format!("couldn't write {}", dest.display()))?));
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (p, bytes, _) in &changed {
                z.start_file(rel(p), opts)?;
                std::io::Write::write_all(&mut z, bytes)?;
            }
            z.finish()?;
            for (p, _, v) in &changed {
                write_json(p, v)?;
            }
        }
        Ok(changed.iter().map(|(p, _, _)| rel(p)).collect())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn read_only(&self) -> Option<&str> {
        self.read_only.as_deref()
    }

    pub fn writable(&self) -> Result<()> {
        match &self.read_only {
            Some(msg) => bail!("{msg}"),
            None => Ok(()),
        }
    }

    pub fn meta(&self) -> Value {
        read_json_object(&self.root.join("library.json"))
    }

    /// Merge `patch` into library.json (top-level keys).
    pub fn update_meta(&self, patch: Value) -> Result<Value> {
        self.writable()?;
        let mut meta = self.meta();
        for (k, v) in patch.as_object().into_iter().flatten() {
            if k == "format" || k == "id" {
                continue;
            }
            if v.is_null() {
                meta.as_object_mut().unwrap().shift_remove(k);
            } else {
                meta[k] = v.clone();
            }
        }
        write_json(&self.root.join("library.json"), &meta)?;
        Ok(meta)
    }

    pub fn info(&self) -> Value {
        let meta = self.meta();
        json!({
            "path": self.root.display().to_string(),
            "id": meta["id"], "name": meta["name"], "format": meta["format"],
            "read_only": self.read_only,
        })
    }

    // ------------------------------------------------------------ favourites

    pub fn favourites(&self) -> Value {
        match self.meta().get("favourites") {
            Some(v @ Value::Array(_)) => v.clone(),
            _ => json!([]),
        }
    }

    pub fn set_favourites(&self, favs: &Value) -> Result<()> {
        if !favs.is_array() {
            bail!("favourites must be a list");
        }
        if &self.favourites() == favs {
            return Ok(());
        }
        self.update_meta(json!({ "favourites": favs }))?;
        Ok(())
    }

    // ------------------------------------------------------------ recipes (saved settings)

    fn recipes_dir(&self) -> PathBuf {
        self.root.join("recipes")
    }

    /// Saved settings for one model, sorted by name.
    pub fn recipes(&self, model: &str) -> Result<Vec<Value>> {
        let dir = self.recipes_dir();
        let mut out: Vec<Value> = std::fs::read_dir(&dir)
            .with_context(|| format!("couldn't read {}", dir.display()))?
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| serde_json::from_slice::<Value>(&std::fs::read(e.path()).ok()?).ok())
            .map(|v| crate::format::recipe_view(v, false, None))
            .filter(|v| v["model"].as_str() == Some(model))
            .collect();
        out.sort_by_key(|v| v["name"].as_str().unwrap_or_default().to_lowercase());
        Ok(out)
    }

    /// Every saved setting (for "More from" lists and the inspector).
    pub fn all_recipes(&self) -> Vec<Value> {
        std::fs::read_dir(self.recipes_dir())
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                    .filter_map(|e| serde_json::from_slice::<Value>(&std::fs::read(e.path()).ok()?).ok())
                    .map(|v| crate::format::recipe_view(v, false, None))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn recipe(&self, id: &str) -> Result<Option<Value>> {
        valid_id(id)?;
        let p = self.recipes_dir().join(format!("{id}.json"));
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(crate::format::recipe_view(serde_json::from_slice(&std::fs::read(p)?)?, false, None)))
    }

    /// Store a complete record (must have "id" and "model").
    pub fn put_recipe(&self, rec: &Value) -> Result<()> {
        self.writable()?;
        let id = rec["id"].as_str().context("record has no id")?;
        valid_id(id)?;
        if rec["model"].as_str().is_none() {
            bail!("record has no model");
        }
        write_json(&self.recipes_dir().join(format!("{id}.json")), rec)
    }

    pub fn remove_recipe(&self, id: &str) -> Result<()> {
        self.writable()?;
        valid_id(id)?;
        let p = self.recipes_dir().join(format!("{id}.json"));
        if p.exists() {
            std::fs::remove_file(p)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ collections (milestone 5)

    fn collections_dir(&self) -> PathBuf {
        self.root.join("collections")
    }

    /// Every collection (collections/<id>/collection.json), by name.
    pub fn collections(&self) -> Vec<Value> {
        let mut out: Vec<Value> = std::fs::read_dir(self.collections_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| serde_json::from_slice::<Value>(&std::fs::read(e.path().join("collection.json")).ok()?).ok())
            .filter(|c| c["id"].is_string())
            .collect();
        out.sort_by_key(|c| c["name"].as_str().unwrap_or_default().to_lowercase());
        out
    }

    /// Store a collection; a new one (no id) gets one from its name. Returns it as stored.
    pub fn put_collection(&self, c: &Value) -> Result<Value> {
        self.writable()?;
        let mut c = c.as_object().cloned().context("a collection is an object")?;
        let name = c.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
        if name.is_empty() {
            bail!("A collection needs a name.");
        }
        let id = match c.get("id").and_then(Value::as_str) {
            Some(id) => id.to_string(),
            None => {
                let base = Some(slug(&name)).filter(|s| !s.is_empty()).unwrap_or_else(|| "collection".into());
                let mut id = base.clone();
                let mut n = 2;
                while self.collections_dir().join(&id).exists() {
                    id = format!("{base}-{n}");
                    n += 1;
                }
                c.insert("created".into(), json!(now()));
                id
            }
        };
        valid_id(&id)?;
        c.insert("id".into(), json!(id));
        c.insert("name".into(), json!(name));
        c.entry("items").or_insert(json!([]));
        c.insert("updated".into(), json!(now()));
        let c = Value::Object(c);
        write_json(&self.collections_dir().join(&id).join("collection.json"), &c)?;
        Ok(c)
    }

    /// Remove a collection (its items stay where they are). Returns what it was, for undo.
    pub fn remove_collection(&self, id: &str) -> Result<Value> {
        self.writable()?;
        valid_id(id)?;
        let dir = self.collections_dir().join(id);
        let was: Value = serde_json::from_slice(&std::fs::read(dir.join("collection.json")).with_context(|| format!("no collection {id}"))?)?;
        std::fs::remove_dir_all(&dir)?;
        Ok(was)
    }

    // ------------------------------------------------------------ sources

    pub fn sources_dir(&self) -> PathBuf {
        self.root.join("sources")
    }

    pub fn source_dir(&self, id: &str) -> Result<PathBuf> {
        valid_id(id)?;
        Ok(self.sources_dir().join(id))
    }

    /// Ids of every project in the library, sorted.
    pub fn source_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(self.sources_dir())
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().join("source.json").is_file())
                    .filter_map(|e| e.file_name().to_str().map(String::from))
                    .filter(|id| valid_id(id).is_ok())
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids
    }

    pub fn source(&self, id: &str) -> Result<Value> {
        let p = self.source_dir(id)?.join("source.json");
        let v: Value = serde_json::from_slice(&std::fs::read(&p).with_context(|| format!("no project {id}"))?)
            .with_context(|| format!("{} isn't valid JSON", p.display()))?;
        Ok(v)
    }

    pub fn save_source(&self, src: &Value) -> Result<()> {
        self.writable()?;
        let id = src["id"].as_str().context("project has no id")?;
        write_json(&self.source_dir(id)?.join("source.json"), src)
    }

    /// A new, unused project id based on `base` ("github-belfryscad-bosl2", "-2", ...).
    pub fn new_source_id(&self, base: &str) -> String {
        let base = slug(base);
        let base = if base.is_empty() { "project".to_string() } else { base };
        let mut id = base.clone();
        let mut n = 2;
        while self.sources_dir().join(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    /// Where a project's files are on disk for one version.
    pub fn version_dir(&self, src: &Value, version: &str) -> Result<PathBuf> {
        let id = src["id"].as_str().context("project has no id")?;
        match src["kind"].as_str() {
            Some("local") => Ok(self.root.join(rel_inside(src["origin"]["path"].as_str().context("local project has no path")?)?)),
            Some("linked") => Ok(PathBuf::from(src["origin"]["path"].as_str().context("linked project has no path")?)),
            Some("pinned") => Ok(self.source_dir(id)?.join("pins")),
            _ => {
                valid_id(version)?;
                Ok(self.source_dir(id)?.join("files").join(version))
            }
        }
    }

    /// The library-relative form of a path inside the library (None if outside).
    pub fn relative(&self, p: &Path) -> Option<String> {
        let rel = p.strip_prefix(&self.root).ok()?;
        Some(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
    }

    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        Ok(self.root.join(rel_inside(rel)?))
    }

    pub fn derived(&self, id: &str, version: &str) -> Option<Value> {
        valid_id(version).ok()?;
        let p = self.source_dir(id).ok()?.join("derived").join(format!("{version}.json"));
        serde_json::from_slice(&std::fs::read(p).ok()?).ok()
    }

    pub fn save_derived(&self, id: &str, version: &str, v: &Value) -> Result<()> {
        self.writable()?;
        valid_id(version)?;
        let mut bytes = serde_json::to_vec(v)?;
        bytes.push(b'\n');
        crate::config::write_atomic(&self.source_dir(id)?.join("derived").join(format!("{version}.json")), &bytes)
    }

    // ------------------------------------------------------------ metadata (the user's edits)

    /// A project's edits file: sources/<id>/metadata.json.
    pub fn metadata_path(&self, id: &str) -> Result<PathBuf> {
        Ok(self.source_dir(id)?.join("metadata.json"))
    }

    pub fn metadata(&self, id: &str) -> Value {
        self.metadata_path(id).map(|p| read_json_object(&p)).unwrap_or_else(|_| json!({}))
    }

    pub fn save_metadata(&self, id: &str, meta: &Value) -> Result<()> {
        self.writable()?;
        let p = self.metadata_path(id)?;
        let empty = meta.as_object().is_none_or(|o| o.values().all(|v| v.is_null() || v.as_object().is_some_and(Map::is_empty)));
        if empty {
            if p.exists() {
                std::fs::remove_file(&p)?;
            }
            return Ok(());
        }
        write_json(&p, meta)
    }

    // ------------------------------------------------------------ default checks

    /// What rendering each model at its defaults gave: sources/<id>/checks.json,
    /// `{ version, engine, models: { <model>: { ok, message, seconds, date } } }`. Rebuildable.
    pub fn checks(&self, id: &str) -> Value {
        self.source_dir(id).map(|d| read_json_object(&d.join("checks.json"))).unwrap_or_else(|_| json!({}))
    }

    /// Record one model's check, starting afresh when the project's version or the engine changed.
    pub fn put_check(&self, id: &str, version: &str, engine: &str, model: &str, result: Value) -> Result<Value> {
        self.writable()?;
        let mut c = self.checks(id);
        if c["version"] != version || c["engine"] != engine || !c["models"].is_object() {
            c = json!({ "version": version, "engine": engine, "models": {} });
        }
        c["models"][model] = result;
        write_json(&self.source_dir(id)?.join("checks.json"), &c)?;
        Ok(c)
    }

    // ------------------------------------------------------------ trash

    /// Deleted projects wait in trash/<time>-<id>/ (source/, local/ for your own
    /// project's folder, trash.json) until restored or the trash is emptied.
    pub fn trash_dir(&self) -> PathBuf {
        self.root.join("trash")
    }

    /// Move a project to the trash. Returns its trash entry.
    pub fn trash_source(&self, id: &str, name: &str) -> Result<Value> {
        self.writable()?;
        let src = self.source(id)?;
        let stamp: String = now().chars().filter(char::is_ascii_digit).take(14).collect();
        let entry = format!("{stamp}-{id}");
        let dest = self.trash_dir().join(&entry);
        std::fs::create_dir_all(&dest)?;
        let mut local = Value::Null;
        if src["kind"] == "local" {
            let rel = src["origin"]["path"].as_str().unwrap_or("");
            let from = self.root.join(rel_inside(rel)?);
            if from.is_dir() {
                std::fs::rename(&from, dest.join("local")).with_context(|| format!("couldn't move {} to the trash", from.display()))?;
                local = json!(rel);
            }
        }
        let dir = self.source_dir(id)?;
        std::fs::rename(&dir, dest.join("source")).with_context(|| format!("couldn't move {} to the trash", dir.display()))?;
        let info = json!({ "entry": entry, "kind": "project", "id": id, "name": name, "deleted": now(), "local": local, "source_kind": src["kind"] });
        write_json(&dest.join("trash.json"), &info)?;
        Ok(info)
    }

    /// What's in the trash, newest first, with sizes.
    pub fn trash_list(&self) -> Vec<Value> {
        let mut out: Vec<Value> = std::fs::read_dir(self.trash_dir())
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let mut info = read_json_object(&e.path().join("trash.json"));
                        info.get("id")?;
                        info["entry"] = json!(e.file_name().to_string_lossy());
                        info["bytes"] = json!(dir_bytes(&e.path()));
                        Some(info)
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.sort_by(|a, b| b["deleted"].as_str().cmp(&a["deleted"].as_str()));
        out
    }

    /// Put a deleted project back. Fails if a project with its id came back meanwhile.
    pub fn trash_restore(&self, entry: &str) -> Result<Value> {
        self.writable()?;
        valid_id(entry)?;
        let dir = self.trash_dir().join(entry);
        let info = read_json_object(&dir.join("trash.json"));
        let id = info["id"].as_str().context("not a trash entry")?.to_string();
        let target = self.source_dir(&id)?;
        if target.exists() {
            bail!("A project with the id {id} is in the library already; delete it first to bring this one back.");
        }
        if let Some(rel) = info["local"].as_str() {
            let to = self.root.join(rel_inside(rel)?);
            if to.exists() {
                bail!("{rel} exists already; move it away to bring this project back.");
            }
            std::fs::create_dir_all(to.parent().unwrap_or(&self.root))?;
            std::fs::rename(dir.join("local"), &to)?;
        }
        std::fs::create_dir_all(self.sources_dir())?;
        std::fs::rename(dir.join("source"), &target)?;
        std::fs::remove_dir_all(&dir)?;
        Ok(info)
    }

    /// Delete one trash entry for good, or all of them. Returns how many went.
    pub fn trash_empty(&self, entry: Option<&str>) -> Result<usize> {
        self.writable()?;
        let entries: Vec<String> = match entry {
            Some(e) => {
                valid_id(e)?;
                vec![e.to_string()]
            }
            None => std::fs::read_dir(self.trash_dir()).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default(),
        };
        let mut n = 0;
        for e in entries {
            let p = self.trash_dir().join(&e);
            if p.is_dir() {
                std::fs::remove_dir_all(&p).with_context(|| format!("couldn't delete {}", p.display()))?;
                n += 1;
            } else if p.is_file() {
                std::fs::remove_file(&p)?;
            }
        }
        Ok(n)
    }
}

/// Total size of the files under a folder.
fn dir_bytes(p: &Path) -> u64 {
    std::fs::read_dir(p)
        .map(|rd| {
            rd.flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => dir_bytes(&e.path()),
                    Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
                    Err(_) => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

/// "BelfrySCAD/BOSL2" -> "belfryscad-bosl2": lowercase letters, digits and single hyphens.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(60).collect::<String>().trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("workshop-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn dates() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(1_791_150_506), "2026-10-04T21:48:26Z");
        assert_eq!(iso_from_unix(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn collections_are_kept_by_id() {
        let dir = temp_dir("collections");
        let (lib, _) = Library::open(&dir).unwrap();
        assert!(lib.put_collection(&json!({ "name": "  " })).is_err());
        let a = lib.put_collection(&json!({ "name": "RC car", "items": [{ "ref": "part:x/wheel", "quantity": 4 }] })).unwrap();
        let b = lib.put_collection(&json!({ "name": "RC car!" })).unwrap();
        assert_eq!((a["id"].as_str(), b["id"].as_str()), (Some("rc-car"), Some("rc-car-2")));
        assert!(dir.join("collections/rc-car/collection.json").is_file());
        let mut a2 = a.clone();
        a2["name"] = json!("Buggy");
        lib.put_collection(&a2).unwrap();
        let names: Vec<String> = lib.collections().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, ["Buggy", "RC car!"]);
        assert_eq!(lib.collections()[0]["items"][0]["quantity"], json!(4));
        let was = lib.remove_collection("rc-car").unwrap();
        assert_eq!(was["name"], json!("Buggy"));
        assert_eq!(lib.collections().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("BelfrySCAD/BOSL2"), "belfryscad-bosl2");
        assert_eq!(slug("  My  Project!! "), "my-project");
    }

    #[test]
    fn readme_is_refreshed_unless_changed() {
        let dir = temp_dir("readme");
        Library::open(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("README.txt")).unwrap(), README);
        assert!(README.contains("\n  library.json") && README.contains("\n                  use."), "the README keeps its indentation (F-18)");
        // the ones 0.4.22 and 0.4.23 wrote are replaced
        for old in [README_0_4_22, README_0_4_23] {
            std::fs::write(dir.join("README.txt"), old).unwrap();
            Library::open(&dir).unwrap();
            assert_eq!(std::fs::read_to_string(dir.join("README.txt")).unwrap(), README);
        }
        assert!(README.contains("recipe.json"), "the README describes made parts' recipes (F-22)");
        assert!(README.contains("origin.site") && README.contains("\"detected\""), "the README says where a project's site details are (F-25)");
        assert!(README.contains("\"apart\"") && README.contains("by id"), "the README says what metadata.json holds and how collections refer to items (F-29)");
        assert!(README.contains("Library format 2") && README.contains("Check library"));
        // one the user wrote stays
        std::fs::write(dir.join("README.txt"), "my notes").unwrap();
        Library::open(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("README.txt")).unwrap(), "my notes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn newer_format_is_read_only() {
        let dir = temp_dir("newer");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("library.json"), r#"{"format": 99, "id": "x", "name": "Future"}"#).unwrap();
        let (lib, _) = Library::open(&dir).unwrap();
        assert!(lib.read_only().is_some());
        assert!(lib.put_recipe(&json!({"id": "a", "model": "m/x"})).is_err());
        assert!(lib.set_favourites(&json!(["a"])).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_1_library_is_upgraded() {
        let dir = temp_dir("format1");
        for d in ["recipes", "collections/c", "sources/zip-a", "local/Made parts/Bin"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let files = [
            ("library.json", r#"{"format": 1, "id": "lib-x", "name": "Old", "favourites": []}"#),
            ("recipes/a1.json", r#"{"id":"a1","model":"zip-a/box","name":"A","values":{"w":2},"version":"v1"}"#),
            ("collections/c/collection.json", r#"{"id":"c","name":"C","items":[{"ref":"gen:zip-a/box","format":"stl"}]}"#),
            ("sources/zip-a/source.json", r#"{"id":"zip-a","kind":"zip","version":"v1","versions":[{"id":"v1"}]}"#),
            ("local/Made parts/Bin/Bin 2.recipe.json", r#"{"model":"zip-a/box","values":{},"date":"2026-10-01T00:00:00Z","format":"3mf"}"#),
            ("local/Made parts/Bin/Bin 2.3mf", "x"),
        ];
        for (p, t) in files {
            std::fs::write(dir.join(p), t).unwrap();
        }
        let (lib, m) = Library::open(&dir).unwrap();
        let read = |p: &str| -> Value { serde_json::from_slice(&std::fs::read(dir.join(p)).unwrap()).unwrap() };
        assert_eq!(read("library.json")["format"], FORMAT);
        assert_eq!(m.upgraded.len(), 4, "{:?}", m.upgraded);
        let a1 = read("recipes/a1.json");
        assert_eq!((a1["project_version"].as_str(), a1["kind"].as_str(), a1["format"].as_u64(), a1.get("version")), (Some("v1"), Some("settings"), Some(1), None));
        let part = read("local/Made parts/Bin/Bin 2.recipe.json");
        assert_eq!((part["output"].as_str(), part["created"].as_str(), part["name"].as_str(), part["format"].as_u64()), (Some("3mf"), Some("2026-10-01T00:00:00Z"), Some("Bin 2"), Some(1)));
        // an entry's own download format stays as it was
        assert_eq!(read("collections/c/collection.json")["items"][0]["format"], "stl");
        assert_eq!(read("sources/zip-a/source.json")["format"], 1);
        // the files as they were, in a backup
        let backup = std::fs::read_dir(dir.join("backups")).unwrap().flatten().next().unwrap().path();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&backup).unwrap()).unwrap();
        let mut old = String::new();
        std::io::Read::read_to_string(&mut z.by_name("recipes/a1.json").unwrap(), &mut old).unwrap();
        assert!(old.contains(r#""version":"v1""#));
        // saved settings read in the one shape, and a second open changes nothing
        assert_eq!(lib.recipes("zip-a/box").unwrap()[0]["project_version"], "v1");
        let (_, m2) = Library::open(&dir).unwrap();
        assert!(m2.upgraded.is_empty());
        // a file a newer version wrote is read, never written over
        std::fs::write(dir.join("recipes/n.json"), r#"{"id":"n","model":"zip-a/box","format":5}"#).unwrap();
        assert!(lib.put_recipe(&json!({"id": "n", "model": "zip-a/box"})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trash_round_trip() {
        let dir = temp_dir("trash");
        let (lib, _) = Library::open(&dir).unwrap();
        // your own project: its local/ folder goes to the trash with it
        std::fs::create_dir_all(dir.join("local/bevel")).unwrap();
        std::fs::write(dir.join("local/bevel/gear.scad"), "cube(1);").unwrap();
        lib.save_source(&json!({ "id": "local-bevel", "kind": "local", "origin": { "path": "local/bevel" }, "version": "v" })).unwrap();
        let info = lib.trash_source("local-bevel", "Bevel").unwrap();
        assert!(!dir.join("local/bevel").exists() && lib.source("local-bevel").is_err());
        let list = lib.trash_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "Bevel");
        assert!(list[0]["bytes"].as_u64().unwrap() > 0);
        lib.trash_restore(info["entry"].as_str().unwrap()).unwrap();
        assert!(dir.join("local/bevel/gear.scad").is_file() && lib.source("local-bevel").is_ok());
        assert!(lib.trash_list().is_empty());
        // emptying
        lib.trash_source("local-bevel", "Bevel").unwrap();
        assert_eq!(lib.trash_empty(None).unwrap(), 1);
        assert!(lib.trash_list().is_empty() && !dir.join("local/bevel").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paths_stay_inside() {
        assert!(rel_inside("sources/a/files/v/x.scad").is_ok());
        assert!(rel_inside("../x").is_err());
        assert!(rel_inside("/etc/passwd").is_ok()); // leading slash is dropped: still relative
        assert!(rel_inside("a/../../b").is_err());
        assert!(valid_id("../x").is_err());
    }
}
