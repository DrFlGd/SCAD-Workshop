//! The app's commands, behind one entry point ([`App::call`]) so the Tauri app
//! and `workshop-cli serve` (a local stand-in used for testing the page) share
//! them exactly. Long work (adding, reading, updating projects) runs as jobs
//! the page polls.

use crate::catalog::{self, Catalog};
use crate::components::{self, ComponentLibrary};
use crate::config::{AppConfig, Prefs};
use crate::library::{self, Library};
use crate::meta;
use crate::project;
use crate::render::{default_concurrency, Renderer};
use crate::sources::{self, AddRequest};
use crate::{NativeEngine, SiteDir};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

/// Where the app keeps things on this computer, and what it ships with.
#[derive(Clone, Debug)]
pub struct AppPaths {
    /// config.json and prefs.json
    pub config_dir: PathBuf,
    /// render cache, unpacked engine
    pub data_dir: PathBuf,
    /// the app's own files: data/catalog.json (common files, engine) and fs/ (fonts)
    pub site: PathBuf,
    /// the starter library shipped with the app (sources/<id>/...)
    pub starter: Option<PathBuf>,
    /// bundled OpenSCAD (folder or executable)
    pub engine: PathBuf,
    /// OpenSCAD libraries shipped with the app (libs/: libraries.json, <Name>/, index/)
    pub libs: Option<PathBuf>,
    /// how the page reaches library files: "library://localhost/", "/library/"...
    pub library_url: String,
}

pub enum Reply {
    Json(Value),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, Default)]
struct Job {
    id: String,
    label: String,
    stage: String,
    done: bool,
    error: Option<String>,
    result: Value,
    started: String,
    /// Stop asked for (jobs that can stop check it: backups, restores)
    cancel: bool,
}

pub struct App {
    pub paths: AppPaths,
    site: SiteDir,
    common_files: Value,
    engine_version_site: String,
    engine: tokio::sync::OnceCell<Result<NativeEngine, String>>,
    renderer: tokio::sync::Mutex<Option<Arc<Renderer>>>,
    library: RwLock<Option<Library>>,
    catalog: Mutex<Option<Arc<Catalog>>>,
    /// Bumped by `invalidate`, so a catalog built from the library as it was before isn't kept.
    catalog_gen: std::sync::atomic::AtomicU64,
    jobs: Mutex<Vec<Job>>,
    job_seq: Mutex<u64>,
    /// one library change at a time (adding, reading, merging)
    busy: tokio::sync::Mutex<()>,
    /// one update check at a time (the daily one and a click can overlap)
    checking: tokio::sync::Mutex<()>,
    /// the bundled libraries and their components (read once)
    bundled: std::sync::OnceLock<Vec<ComponentLibrary>>,
    /// components of library projects read before 0.3 (no index in their derived data): (source, version) -> index
    project_indexes: Mutex<BTreeMap<(String, String), Value>>,
}

fn e2s(e: anyhow::Error) -> String {
    format!("{e:#}")
}

fn arg<'a>(args: &'a Value, k: &str) -> Result<&'a str> {
    args[k].as_str().filter(|s| !s.is_empty()).with_context(|| format!("missing {k}"))
}

/// Where a metadata edit goes in metadata.json, from { level, target }.
fn level_key(args: &Value) -> std::result::Result<(&'static str, Option<String>), String> {
    let target = || arg(args, "target").map(String::from).map_err(e2s);
    Ok(match args["level"].as_str().unwrap_or("item") {
        "project" => ("project", None),
        "folder" => ("folders", Some(target()?.trim_matches('/').to_string())),
        "part" => ("parts", Some(target()?)),
        _ => ("items", Some(target()?)),
    })
}

impl App {
    pub fn new(paths: AppPaths) -> Result<Arc<Self>> {
        let site = SiteDir::new(&paths.site)?;
        let cat = site.catalog()?;
        Ok(Arc::new(Self {
            common_files: cat["common_files"].clone(),
            engine_version_site: cat["engine"].as_str().unwrap_or("").to_string(),
            site,
            paths,
            engine: tokio::sync::OnceCell::new(),
            renderer: tokio::sync::Mutex::new(None),
            library: RwLock::new(None),
            catalog: Mutex::new(None),
            catalog_gen: std::sync::atomic::AtomicU64::new(0),
            jobs: Mutex::new(vec![]),
            job_seq: Mutex::new(0),
            busy: tokio::sync::Mutex::new(()),
            checking: tokio::sync::Mutex::new(()),
            bundled: std::sync::OnceLock::new(),
            project_indexes: Mutex::new(BTreeMap::new()),
        }))
    }

    fn config_path(&self) -> PathBuf {
        self.paths.config_dir.join("config.json")
    }
    pub fn config(&self) -> AppConfig {
        AppConfig::load(&self.config_path())
    }
    fn prefs(&self) -> Prefs {
        Prefs::new(self.paths.config_dir.join("prefs.json"))
    }

    pub async fn engine(&self) -> Result<NativeEngine, String> {
        self.engine
            .get_or_init(|| async {
                let scratch = self.paths.data_dir.join("engine");
                NativeEngine::prepare(&self.paths.engine, &scratch).await.map_err(e2s)
            })
            .await
            .clone()
    }

    pub async fn renderer(&self) -> Result<Arc<Renderer>, String> {
        let mut g = self.renderer.lock().await;
        if let Some(r) = g.as_ref() {
            return Ok(r.clone());
        }
        let engine = self.engine().await?;
        let jobs = self.config().concurrency.unwrap_or_else(default_concurrency);
        let r = Arc::new(Renderer::new(engine, self.site.clone(), self.paths.data_dir.join("cache"), jobs).map_err(e2s)?);
        *g = Some(r.clone());
        drop(g);
        if let Ok(c) = self.catalog() {
            r.add_blobs(catalog::blob_list(&c));
        }
        Ok(r)
    }

    /// The open library (opening the configured one, with the starter projects, on first use).
    pub fn library(&self) -> Result<Library, String> {
        if let Some(l) = self.library.read().unwrap().as_ref() {
            return Ok(l.clone());
        }
        let path = self.config().library_path();
        self.open_library(&path).map_err(e2s)
    }

    fn open_library(&self, path: &Path) -> Result<Library> {
        let (lib, _migration) = Library::open(path, &self.prefs())?;
        if let Some(starter) = &self.paths.starter {
            if let Err(e) = project::sync_starter(&lib, starter) {
                eprintln!("starter library: {e:#}");
            }
        }
        let _ = sources::sync_local(&lib);
        if let Err(e) = sources::normalise(&lib) {
            eprintln!("library: {e:#}");
        }
        let mut cfg = self.config();
        if cfg.library.as_deref() != Some(path) || cfg.recent_libraries.first().map(PathBuf::as_path) != Some(path) {
            cfg.set_library(path);
            cfg.save(&self.config_path())?;
        }
        *self.library.write().unwrap() = Some(lib.clone());
        self.invalidate();
        Ok(lib)
    }

    fn invalidate(&self) {
        self.catalog_gen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.catalog.lock().unwrap() = None;
    }

    /// The libraries shipped with the app.
    pub fn bundled(&self) -> &[ComponentLibrary] {
        self.bundled.get_or_init(|| self.paths.libs.as_deref().map(components::bundled).unwrap_or_default())
    }

    /// Start values for modules without examples (shipped with the libraries).
    fn curated(&self) -> Value {
        self.paths.libs.as_deref().map(components::curated).unwrap_or(json!({}))
    }

    /// Library projects (role "library") with their components.
    fn project_libraries(&self, lib: &Library) -> Vec<ComponentLibrary> {
        let mut out = vec![];
        for ls in project::library_sources(lib) {
            let Ok(src) = lib.source(&ls.source) else { continue };
            // (an index stored by an older version is made again, in memory, until the project is read again)
            let stored = lib.derived(&ls.source, &ls.version).map(|d| d["components"].clone());
            let index = match stored.filter(|c| c.is_object() && c["format"].as_u64() == Some(components::FORMAT)) {
                Some(i) => i,
                None => {
                    let key = (ls.source.clone(), ls.version.clone());
                    let cached = self.project_indexes.lock().unwrap().get(&key).cloned();
                    match cached {
                        Some(i) => i,
                        None => {
                            let info = project::project_lib_info(&src, &ls.version, &ls.dir.name);
                            let cur = self.curated();
                            let i = components::index_library(&info, &ls.dir.path, cur.get(&ls.dir.name), cur.get("$no_guess").and_then(|n| n.get(&ls.dir.name)))
                                .unwrap_or(Value::Null);
                            self.project_indexes.lock().unwrap().insert(key, i.clone());
                            i
                        }
                    }
                }
            };
            if !index.is_object() {
                continue;
            }
            out.push(ComponentLibrary {
                info: project::project_lib_info(&src, &ls.version, &ls.dir.name),
                provider: "project".into(),
                source_id: Some(ls.source.clone()),
                root: ls.dir.path.clone(),
                index,
            });
        }
        out
    }

    fn prefer_bundled(lib: &Library) -> Vec<String> {
        lib.meta()["prefer_bundled"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect()
    }

    /// What reading a project needs about libraries.
    fn ingest_libs(&self, lib: &Library) -> project::IngestLibs {
        project::IngestLibs { bundled: self.bundled().to_vec(), prefer_bundled: Self::prefer_bundled(lib), curated: self.curated() }
    }

    pub fn catalog(&self) -> Result<Arc<Catalog>, String> {
        if let Some(c) = self.catalog.lock().unwrap().as_ref() {
            return Ok(c.clone());
        }
        let gen = self.catalog_gen.load(std::sync::atomic::Ordering::SeqCst);
        let lib = self.library()?;
        let projects = self.project_libraries(&lib);
        let prefer = Self::prefer_bundled(&lib);
        let effective = components::effective(self.bundled(), &projects, &prefer);
        let libs = catalog::Libs { effective: &effective, bundled: self.bundled(), projects: &projects, prefer_bundled: &prefer };
        let c = Arc::new(catalog::build(&lib, &self.engine_version_site, &self.common_files, &self.paths.library_url, libs));
        if let Ok(g) = self.renderer.try_lock() {
            if let Some(r) = g.as_ref() {
                r.add_blobs(catalog::blob_list(&c));
            }
        }
        // (kept only if nothing changed the library while it was built: a pin made meanwhile, say)
        let mut kept = self.catalog.lock().unwrap();
        if self.catalog_gen.load(std::sync::atomic::Ordering::SeqCst) == gen {
            *kept = Some(c.clone());
        }
        Ok(c)
    }

    // ------------------------------------------------------------ jobs

    fn start_job(self: &Arc<Self>, label: &str) -> String {
        let mut seq = self.job_seq.lock().unwrap();
        *seq += 1;
        let id = format!("job{}", *seq);
        let mut jobs = self.jobs.lock().unwrap();
        // keep running jobs and the last few finished ones
        let finished = jobs.iter().filter(|j| j.done).count();
        let mut drop_n = finished.saturating_sub(30);
        jobs.retain(|j| {
            if j.done && drop_n > 0 {
                drop_n -= 1;
                false
            } else {
                true
            }
        });
        jobs.push(Job { id: id.clone(), label: label.into(), stage: "Starting…".into(), started: library::now(), ..Default::default() });
        id
    }

    fn job_stage(&self, id: &str, stage: impl Into<String>) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
            j.stage = stage.into();
        }
    }

    fn job_cancelled(&self, id: &str) -> bool {
        self.jobs.lock().unwrap().iter().any(|j| j.id == id && j.cancel)
    }

    fn job_done(&self, id: &str, r: Result<Value>) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
            j.done = true;
            match r {
                Ok(v) => j.result = v,
                Err(e) => j.error = Some(e2s(e)),
            }
        }
        self.invalidate();
    }

    /// Run long work in the background; the page polls `job`.
    fn spawn<F, Fut>(self: &Arc<Self>, label: &str, work: F) -> Value
    where
        F: FnOnce(Arc<Self>, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Value>> + Send + 'static,
    {
        let id = self.start_job(label);
        let app = self.clone();
        let jid = id.clone();
        tokio::spawn(async move {
            let _g = app.busy.lock().await;
            let r = work(app.clone(), jid.clone()).await;
            app.job_done(&jid, r);
        });
        json!({ "job": id })
    }

    fn jobs_json(&self) -> Value {
        Value::Array(
            self.jobs
                .lock()
                .unwrap()
                .iter()
                .map(|j| json!({ "id": j.id, "label": j.label, "stage": j.stage, "done": j.done, "error": j.error, "result": j.result, "started": j.started }))
                .collect(),
        )
    }

    // ------------------------------------------------------------ reading projects

    /// Read (ingest) one project at its current version, or at `version`.
    pub async fn read_project(self: &Arc<Self>, id: &str, version: Option<&str>, job: Option<&str>) -> Result<Value> {
        let lib = self.library().map_err(|e| anyhow!(e))?;
        let src = lib.source(id)?;
        if src["kind"] == "pinned" {
            self.invalidate(); // pins are read when the catalog is made
            return Ok(json!({ "source": id, "version": "pins" }));
        }
        let version = version.map(String::from).unwrap_or_else(|| src["version"].as_str().unwrap_or("").to_string());
        let renderer = self.renderer().await.map_err(|e| anyhow!(e))?;
        let common: BTreeMap<String, String> = serde_json::from_value(self.common_files.clone()).unwrap_or_default();
        let app = self.clone();
        let jid = job.map(String::from);
        let libs_ctx = self.ingest_libs(&lib);
        let derived = project::ingest(&lib, &renderer, &common, &libs_ctx, &src, &version, move |s| {
            if let Some(j) = &jid {
                app.job_stage(j, s);
            }
        })
        .await?;
        self.invalidate();
        Ok(json!({
            "source": id, "version": version,
            "models": derived["models"].as_array().map(|m| m.len()).unwrap_or(0),
            "parts": derived["parts"].as_array().map(|m| m.len()).unwrap_or(0),
            "problems": derived["problems"],
        }))
    }

    /// Check GitHub projects for new commits; download and read what changed.
    async fn check_updates(self: &Arc<Self>, only: Option<String>, job: &str) -> Result<Value> {
        let _one = self.checking.lock().await;
        let lib = self.library().map_err(|e| anyhow!(e))?;
        let token = self.config().github_token();
        let mut found = vec![];
        let ids: Vec<String> = lib.source_ids().into_iter().filter(|id| only.as_deref().is_none_or(|o| o == id)).collect();
        for id in ids {
            let src = lib.source(&id)?;
            // a starter project from GitHub: only whether there's something newer there (F-17)
            if src["kind"] == "bundled" && src["origin"]["commit"].is_string() {
                let name = src["detected"]["name"].as_str().unwrap_or(&id).to_string();
                self.job_stage(job, format!("Checking {name}…"));
                let (s2, t) = (src.clone(), token.clone());
                if let Ok(Ok(newer)) = tokio::task::spawn_blocking(move || sources::upstream_newer(&s2, t)).await {
                    let mut src = lib.source(&id)?;
                    let u = match newer {
                        Some(latest) => json!({ "state": "upstream", "latest": latest }),
                        None => json!({ "state": "current" }),
                    };
                    if src["update"]["state"] != u["state"] || src["update"]["latest"]["commit"] != u["latest"]["commit"] {
                        src["update"] = u;
                        src["update"]["checked"] = json!(library::now());
                        lib.save_source(&src)?;
                    }
                }
                continue;
            }
            if src["kind"] != "github" {
                continue;
            }
            let name = src["detected"]["name"].as_str().unwrap_or(&id).to_string();
            self.job_stage(job, format!("Checking {name}…"));
            let t = token.clone();
            let lib2 = lib.clone();
            let s2 = src.clone();
            let r = tokio::task::spawn_blocking(move || sources::fetch_update(&lib2, &s2, t)).await?;
            let now = library::now();
            // the project as it is now: an update accepted (or details edited) while GitHub was being
            // asked must not be undone by saving the copy read before
            let mut src = lib.source(&id)?;
            let r = match r {
                Ok(Some((version, _))) if src["version"] == version.as_str() => Ok(None),
                r => r,
            };
            // source.json is written only when what it says changes, so a library shared between
            // computers or kept in git doesn't change with every check (when it last checked is kept
            // by each computer)
            let old = src["update"].clone();
            match r {
                Err(e) => {
                    // a failed check (offline, GitHub's hourly limit) keeps a waiting or skipped update
                    let mut u = if old.is_object() { old.clone() } else { json!({}) };
                    if !matches!(u["state"].as_str(), Some("available" | "skipped")) {
                        u["state"] = json!("error");
                    }
                    u["error"] = json!(e2s(e));
                    if u["state"] != old["state"] || u["error"] != old["error"] {
                        u["error_date"] = json!(now);
                        src["update"] = u;
                        lib.save_source(&src)?;
                    }
                }
                Ok(None) => {
                    if old["state"] != "current" || old.get("error").is_some() {
                        src["update"] = json!({ "state": "current", "checked": now });
                        lib.save_source(&src)?;
                    }
                }
                Ok(Some((version, info))) => {
                    if old["skipped"] == version.as_str() || (old["state"] == "available" && old["latest"]["id"] == version.as_str()) {
                        // already skipped, or already waiting
                        if old.get("error").is_some() {
                            let mut u = old.clone();
                            if let Some(o) = u.as_object_mut() {
                                o.remove("error");
                                o.remove("error_date");
                            }
                            src["update"] = u;
                            lib.save_source(&src)?;
                        }
                        if old["state"] == "available" {
                            found.push(id.clone());
                        }
                        continue;
                    }
                    let mut versions = src["versions"].as_array().cloned().unwrap_or_default();
                    if !versions.iter().any(|v| v["id"] == version.as_str()) {
                        versions.push(json!({ "id": version, "commit": info["commit"], "date": info["date"], "message": info["message"], "added": now, "files": info["files"] }));
                    }
                    src["versions"] = Value::Array(versions);
                    lib.save_source(&src)?;
                    self.job_stage(job, format!("Reading the new version of {name}…"));
                    let summary = match self.read_project(&id, Some(&version), Some(job)).await {
                        Ok(_) => changes(&lib, &id, src["version"].as_str().unwrap_or(""), &version),
                        Err(e) => json!({ "error": e2s(e) }),
                    };
                    src = lib.source(&id)?;
                    if src["version"] == version.as_str() {
                        // accepted while this check was reading it
                        src["update"] = json!({ "state": "current", "checked": now });
                    } else {
                        src["update"] = json!({ "state": "available", "checked": now, "from": "github", "latest": info, "changes": summary });
                        found.push(id.clone());
                    }
                    lib.save_source(&src)?;
                }
            }
        }
        Ok(json!({ "updates": found }))
    }

    // ------------------------------------------------------------ the command table

    pub async fn call(self: &Arc<Self>, cmd: &str, args: Value) -> Result<Reply, String> {
        let j = |v: Value| Ok(Reply::Json(v));
        match cmd {
            "app_info" => {
                let engine = self.engine().await;
                let lib = self.library();
                let r = self.renderer().await.ok();
                j(json!({
                    "version": crate::VERSION,
                    "os": std::env::consts::OS,
                    "engine": engine.as_ref().ok().map(|e| e.version.clone()),
                    "engine_path": engine.as_ref().ok().map(|e| e.exe.display().to_string()),
                    "engine_error": engine.err(),
                    "workspace": lib.as_ref().ok().map(|l| l.root().display().to_string()),
                    "library": lib.as_ref().ok().map(Library::info),
                    "library_error": lib.err(),
                    "library_url": self.paths.library_url,
                    "recent_libraries": self.config().recent_libraries,
                    "github_token": self.config().github_token().is_some(),
                    "concurrency": r.as_ref().map(|r| r.concurrency()),
                    "cache_bytes": r.as_ref().map(|r| r.cache_size()),
                    "starter": self.paths.starter.as_ref().is_some_and(|s| s.join("starter.json").is_file()),
                }))
            }
            "read" => self.read_file(arg(&args, "path").map_err(e2s)?).map(Reply::Bytes).map_err(e2s),
            "to_3mf" => {
                // { data, name }: a render (STL, as a data URL) as a 3MF, for Download as 3MF (milestone 5)
                let data = decode_data_url(arg(&args, "data").map_err(e2s)?).map_err(e2s)?;
                let tris = crate::mesh::read("render.stl", &data).map_err(e2s)?;
                crate::mesh::to_3mf(&tris, args["name"].as_str().unwrap_or("Model")).map(Reply::Bytes).map_err(e2s)
            }
            "mesh_coloured" => {
                // { data, name }: a 3D file (a data URL) as STL with its colours, for the 3D view (0.4.19)
                let data = decode_data_url(arg(&args, "data").map_err(e2s)?).map_err(e2s)?;
                let m = crate::mesh::read_mesh(args["name"].as_str().unwrap_or("model.3mf"), &data).map_err(e2s)?;
                Ok(Reply::Bytes(crate::mesh::to_stl_coloured(&m.tris, m.colours.as_deref())))
            }
            "blob" => {
                let sha = arg(&args, "sha").map_err(e2s)?;
                let p = self.site.blob(sha).map_err(e2s)?;
                if p.is_file() {
                    return std::fs::read(p).map(Reply::Bytes).map_err(|e| e.to_string());
                }
                // an edited file (the Code tab), by its hash
                if let Some(p) = self.edit_blob(sha).filter(|p| p.is_file()) {
                    return std::fs::read(p).map(Reply::Bytes).map_err(|e| e.to_string());
                }
                let c = self.catalog()?;
                let p = c.blobs.get(sha).ok_or_else(|| format!("file {sha} isn't in the library"))?;
                let data = std::fs::read(p).map_err(|e| e.to_string())?;
                let scad = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("scad"));
                Ok(Reply::Bytes(if scad { crate::sitebuild::replace_crlf(&data) } else { data }))
            }
            "jobs" => j(self.jobs_json()),
            "job_cancel" => {
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                let mut jobs = self.jobs.lock().unwrap();
                let found = jobs.iter_mut().find(|j| j.id == id && !j.done).map(|j| j.cancel = true).is_some();
                j(json!(found))
            }
            "library_backup" => {
                // the whole library folder to one ZIP (path from a save dialog)
                let dest = PathBuf::from(arg(&args, "path").map_err(e2s)?);
                let leave_out_thumbs = args["leave_out_thumbs"].as_bool().unwrap_or(false);
                let lib = self.library()?;
                Ok(Reply::Json(self.spawn("Backing up the library", move |app, job| async move {
                    let root = lib.root().to_path_buf();
                    let linked = lib.source_ids().into_iter().filter_map(|id| lib.source(&id).ok()).filter(|s| s["kind"] == "linked").count();
                    let app2 = app.clone();
                    let mut r = tokio::task::spawn_blocking(move || {
                        crate::backup::backup(
                            &root,
                            &dest,
                            leave_out_thumbs,
                            |i, n, done, total| app2.job_stage(&job, format!("{i} of {n} files ({} of {} MB)", done >> 20, total >> 20)),
                            || app2.job_cancelled(&job),
                        )
                    })
                    .await??;
                    r["linked"] = json!(linked);
                    Ok(r)
                })))
            }
            "library_restore" => {
                // a backup unpacked into a new folder, then opened as the library
                let zip = PathBuf::from(arg(&args, "zip").map_err(e2s)?);
                let dest = PathBuf::from(arg(&args, "folder").map_err(e2s)?);
                Ok(Reply::Json(self.spawn("Restoring a library", move |app, job| async move {
                    let app2 = app.clone();
                    let dest = tokio::task::spawn_blocking(move || {
                        crate::backup::restore(&zip, &dest, |i, n| app2.job_stage(&job, format!("{i} of {n} files")), || app2.job_cancelled(&job))
                    })
                    .await??;
                    Ok(json!({ "path": dest.display().to_string() }))
                })))
            }
            "blob_put" => {
                // an edited file's text (the Code tab): kept in the app's data folder by its hash, so a
                // render can use it in place of the original (the original is never written to)
                let text = args["text"].as_str().ok_or("no text")?;
                let bytes = crate::sitebuild::replace_crlf(text.as_bytes());
                let sha = { use sha2::Digest; hex::encode(sha2::Sha256::digest(&bytes)) };
                let p = self.edit_blob(&sha).ok_or("bad hash")?;
                if !p.is_file() {
                    std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
                    crate::config::write_atomic(&p, &bytes).map_err(e2s)?;
                }
                if let Ok(r) = self.renderer().await {
                    r.add_blobs([(sha.clone(), p)]);
                }
                j(json!(sha))
            }
            "prefs_get" => {
                let mut p = self.prefs().get();
                if let Ok(lib) = self.library() {
                    p["gw-favs"] = lib.favourites();
                }
                j(p)
            }
            "prefs_set" => {
                let mut p = args["prefs"].clone();
                if let Some(favs) = p.as_object_mut().and_then(|o| o.shift_remove("gw-favs")) {
                    if let Ok(lib) = self.library() {
                        if lib.read_only().is_none() {
                            lib.set_favourites(&favs).map_err(e2s)?;
                        }
                    }
                }
                self.prefs().set(&p).map_err(e2s)?;
                j(Value::Null)
            }
            "settings_list" => j(json!(self.library()?.recipes(arg(&args, "model").map_err(e2s)?).map_err(e2s)?)),
            "settings_get" => j(json!(self.library()?.recipe(arg(&args, "id").map_err(e2s)?).map_err(e2s)?)),
            "settings_put" => {
                self.library()?.put_recipe(&args["record"]).map_err(e2s)?;
                j(Value::Null)
            }
            // collections (milestone 5): lists of parts and models, by id, with quantities and status
            "collections_list" => j(json!(self.library()?.collections())),
            "collection_put" => j(self.library()?.put_collection(&args["collection"]).map_err(e2s)?),
            "collection_remove" => j(self.library()?.remove_collection(arg(&args, "id").map_err(e2s)?).map_err(e2s)?),
            "settings_remove" => {
                self.library()?.remove_recipe(arg(&args, "id").map_err(e2s)?).map_err(e2s)?;
                j(Value::Null)
            }
            "library_open" => {
                let path = PathBuf::from(arg(&args, "path").map_err(e2s)?);
                let lib = self.open_library(&path).map_err(e2s)?;
                if let Ok(g) = self.renderer.try_lock() {
                    if let Some(r) = g.as_ref() {
                        r.clear_blobs();
                    }
                }
                let _ = self.catalog();
                j(lib.info())
            }
            "library_rename" => {
                let lib = self.library()?;
                lib.update_meta(json!({ "name": arg(&args, "name").map_err(e2s)? })).map_err(e2s)?;
                self.invalidate();
                j(lib.info())
            }
            "library_defaults" => {
                // library-level metadata (fills fields nothing else sets)
                let lib = self.library()?;
                let mut m = lib.meta()["metadata"].as_object().cloned().unwrap_or_default();
                let prev = meta::patch_level(&mut m, args["patch"].as_object().ok_or("missing patch")?);
                lib.update_meta(json!({ "metadata": if m.is_empty() { Value::Null } else { Value::Object(m) } })).map_err(e2s)?;
                self.invalidate();
                j(json!({ "previous": prev }))
            }
            "category_set" => {
                let lib = self.library()?;
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                let mut cats = lib.meta()["categories"].as_object().cloned().unwrap_or_default();
                let prev = cats.get(&id).cloned().unwrap_or(Value::Null);
                let mut c = cats.get(&id).and_then(Value::as_object).cloned().unwrap_or_default();
                // moved_to: remove the category, its items going to that one ("" brings it back)
                if args["moved_to"].as_str() == Some(id.as_str()) {
                    return Err("A category can't move to itself.".into());
                }
                for k in ["label", "icon", "moved_to"] {
                    match &args[k] {
                        Value::Null => {}
                        Value::String(s) if s.is_empty() => {
                            c.shift_remove(k);
                        }
                        v => {
                            c.insert(k.into(), v.clone());
                        }
                    }
                }
                if c.is_empty() {
                    cats.shift_remove(&id);
                } else {
                    cats.insert(id, Value::Object(c));
                }
                lib.update_meta(json!({ "categories": cats })).map_err(e2s)?;
                self.invalidate();
                j(json!({ "previous": prev }))
            }
            "library_prefer" => {
                // { name, bundled: bool }: use the bundled copy of a library instead of your own (or back);
                // projects that include it are read again
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let name = arg(&args, "name").map_err(e2s)?.to_string();
                let mut list = Self::prefer_bundled(&lib);
                list.retain(|n| !n.eq_ignore_ascii_case(&name));
                if args["bundled"].as_bool() == Some(true) {
                    list.push(name.clone());
                }
                lib.update_meta(json!({ "prefer_bundled": if list.is_empty() { Value::Null } else { json!(list) } })).map_err(e2s)?;
                self.invalidate();
                Ok(Reply::Json(self.spawn(&format!("Switching {name}"), move |app, job| async move {
                    let c = app.catalog().map_err(|e| anyhow!(e))?;
                    let ids: Vec<String> = c.catalog["attention"].as_array().into_iter().flatten()
                        .filter(|a| a["kind"] == "libraries").filter_map(|a| a["source"].as_str().map(String::from)).collect();
                    for id in &ids {
                        app.read_project(id, None, Some(&job)).await?;
                    }
                    Ok(json!({ "read": ids }))
                })))
            }
            "component_code" => {
                // { key, values } -> the OpenSCAD file a render of this component uses
                let c = self.catalog()?;
                let key = arg(&args, "key").map_err(e2s)?;
                let model = c.models.get(key).ok_or_else(|| format!("no model {key}"))?;
                let call: components::Call = serde_json::from_value(model["call"].clone()).map_err(|_| format!("{key} isn't a component"))?;
                let mut values: BTreeMap<String, Value> = serde_json::from_value(args["values"].clone()).unwrap_or_default();
                for (k, v) in model["fixed"].as_object().into_iter().flatten() {
                    values.insert(k.clone(), v.clone());
                }
                j(json!(components::call_source(&call, &values).map_err(e2s)?))
            }
            "pin_create" => {
                // { component, name, category?, summary?, defaults, hidden: [names], fixed: {name: value} } -> { key }
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let comp = arg(&args, "component").map_err(e2s)?.to_string();
                let c = self.catalog()?;
                let model = c.models.get(&comp).filter(|m| m["kind"] == "component").ok_or_else(|| format!("no component {comp}"))?;
                let name = args["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).unwrap_or_else(|| model["name"].as_str().unwrap_or("Pinned component")).to_string();
                let src = ensure_pinned(&lib).map_err(e2s)?;
                let dir = lib.source_dir(&src).map_err(e2s)?.join("pins");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let base = library::slug(&name);
                let base = if base.is_empty() { "pin".to_string() } else { base };
                let mut pid = base.clone();
                let mut n = 2;
                while dir.join(format!("{pid}.json")).exists() {
                    pid = format!("{base}-{n}");
                    n += 1;
                }
                let group = model["category"].as_str().unwrap_or("other");
                let pin = json!({
                    "id": pid, "name": name, "component": comp,
                    // the copy and version of the library it was pinned from (a change is marked)
                    "from": { "library": model["component"]["library"], "copy": model["component"]["copy"] },
                    "category": args["category"].as_str().filter(|c| !c.is_empty()).unwrap_or(components::model_category(group)),
                    "summary": args["summary"].as_str().map(String::from).unwrap_or_else(|| model["summary"].as_str().unwrap_or("").to_string()),
                    "defaults": args["defaults"].as_object().cloned().unwrap_or_default(),
                    "hidden": args["hidden"].as_array().cloned().unwrap_or_default(),
                    "fixed": args["fixed"].as_object().cloned().unwrap_or_default(),
                    "created": library::now(),
                });
                crate::config::write_json(&dir.join(format!("{pid}.json")), &pin).map_err(e2s)?;
                self.invalidate();
                j(json!({ "key": format!("{src}/{pid}"), "source": src, "id": pid }))
            }
            "pin_accept" => {
                // { key: "pinned/<id>" }: keep a pin with the copy of its library in use now
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let key = arg(&args, "key").map_err(e2s)?;
                let (src, pid) = key.split_once('/').ok_or("not a pin")?;
                library::valid_id(pid).map_err(e2s)?;
                let p = lib.source_dir(src).map_err(e2s)?.join("pins").join(format!("{pid}.json"));
                let mut pin: Value = serde_json::from_slice(&std::fs::read(&p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                let c = self.catalog()?;
                let comp = c.models.get(pin["component"].as_str().unwrap_or("")).ok_or("its component isn't in any library now")?;
                pin["from"] = json!({ "library": comp["component"]["library"], "copy": comp["component"]["copy"] });
                crate::config::write_json(&p, &pin).map_err(e2s)?;
                self.invalidate();
                j(Value::Null)
            }
            "github_token" => {
                let mut cfg = self.config();
                cfg.github_token = args["token"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from);
                cfg.save(&self.config_path()).map_err(e2s)?;
                j(json!(cfg.github_token.is_some()))
            }
            "source_add" => {
                let kind = arg(&args, "kind").map_err(e2s)?;
                let req = match kind {
                    "github" => AddRequest::GitHub { url: arg(&args, "url").map_err(e2s)?.to_string(), releases: args["follow"] == "releases" },
                    "zip" => AddRequest::Zip { path: PathBuf::from(arg(&args, "path").map_err(e2s)?) },
                    "folder" => AddRequest::Folder { path: PathBuf::from(arg(&args, "path").map_err(e2s)?), link: args["link"].as_bool() == Some(true) },
                    // files dropped on the window, copied in as one project (milestone 5)
                    "files" => AddRequest::Files {
                        paths: args["paths"].as_array().into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect(),
                        name: args["name"].as_str().map(String::from),
                    },
                    other => return Err(format!("unknown kind {other}")),
                };
                let role = if args["role"] == "library" { "library" } else { "project" }.to_string();
                let label = match &req {
                    AddRequest::GitHub { url, .. } => format!("Adding {}", url.trim_start_matches("https://").trim_start_matches("github.com/")),
                    AddRequest::Zip { path } | AddRequest::Folder { path, .. } => {
                        format!("Adding {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
                    }
                    AddRequest::Files { paths, .. } => match paths.as_slice() {
                        [one] => format!("Adding {}", one.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                        many => format!("Adding {} files", many.len()),
                    },
                };
                Ok(Reply::Json(self.spawn(&label, move |app, job| async move {
                    let lib = app.library().map_err(|e| anyhow!(e))?;
                    app.job_stage(&job, match &req { AddRequest::GitHub { .. } => "Downloading…", _ => "Copying files…" });
                    let token = app.config().github_token();
                    let lib2 = lib.clone();
                    let src = tokio::task::spawn_blocking(move || sources::add(&lib2, &req, &role, token)).await??;
                    let id = src["id"].as_str().unwrap_or("").to_string();
                    app.invalidate();
                    if let Some(version) = src["added_version"].as_str().map(String::from) {
                        // another download of a model site's page a project has: read as a new version
                        // of that project and offered as an update (Review 5, F-23)
                        let read = app.read_project(&id, Some(&version), Some(&job)).await?;
                        if src["added_older"] == true {
                            // dated before the version in use: only listed under Versions (Review 6, F-26)
                            return Ok(json!({ "source": id, "read": read, "duplicates": [], "version": version, "older": true }));
                        }
                        let mut s = lib.source(&id)?;
                        let summary = changes(&lib, &id, s["version"].as_str().unwrap_or(""), &version);
                        let date = s["versions"].as_array().into_iter().flatten().find(|v| v["id"] == version.as_str()).map(|v| v["date"].clone()).unwrap_or(Value::Null);
                        s["update"] = json!({ "state": "available", "checked": library::now(), "from": "download",
                            "latest": { "id": version, "date": date }, "changes": summary });
                        lib.save_source(&s)?;
                        app.invalidate();
                        return Ok(json!({ "source": id, "read": read, "duplicates": [], "version": version }));
                    }
                    let read = app.read_project(&id, None, Some(&job)).await?;
                    // its model files that are in another project already (added anyway, milestone 5)
                    let duplicates = app.duplicates(&id);
                    Ok(json!({ "source": id, "read": read, "duplicates": duplicates }))
                })))
            }
            "paths_info" => {
                // { paths }: what was dropped on the window: [{ path, name, dir, zip, there }] (milestone 5)
                let out: Vec<Value> = args["paths"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|p| {
                    let path = Path::new(p);
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string());
                    let zip = name.to_lowercase().ends_with(".zip");
                    json!({ "path": p, "name": name, "dir": path.is_dir(), "zip": zip && path.is_file(), "there": path.exists() })
                }).collect();
                j(json!(out))
            }
            "local_add" => {
                // { paths, folder? }: dropped files and folders copied into one of your own projects
                // (local/<folder>/), or a new one; the scan for changes that follows reads it
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let paths: Vec<PathBuf> = args["paths"].as_array().into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect();
                let folder = sources::add_to_local(&lib, &paths, args["folder"].as_str()).map_err(e2s)?;
                j(json!({ "folder": folder }))
            }
            "source_read" => {
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                Ok(Reply::Json(self.spawn(&format!("Reading {id}"), move |app, job| async move {
                    // its model page's PDF too, when it has one (Review 5, F-24)
                    let lib = app.library().map_err(|e| anyhow!(e))?;
                    let i2 = id.clone();
                    if matches!(tokio::task::spawn_blocking(move || sources::refresh_site_details(&lib, &i2)).await, Ok(Ok(true))) {
                        app.invalidate();
                    }
                    app.read_project(&id, None, Some(&job)).await
                })))
            }
            "source_rescan" => {
                // Scan for changes: the library's local/ folder and linked folders, and the stored copies of
                // added projects; read again where they changed
                Ok(Reply::Json(self.spawn("Looking for changed projects", move |app, job| async move {
                    let lib = app.library().map_err(|e| anyhow!(e))?;
                    let known_before = lib.source_ids();
                    let changed = sources::sync_local(&lib)?;
                    let mut unread: Vec<String> = lib.source_ids().into_iter().filter(|id| {
                        lib.source(id).ok().is_some_and(|s| s["kind"] != "bundled" && s["kind"] != "pinned" && lib.derived(id, s["version"].as_str().unwrap_or("")).is_none())
                    }).collect();
                    // and those whose libraries changed (the app's copy, or which copy is used), or read by an older app
                    if let Ok(c) = app.catalog() {
                        for a in c.catalog["attention"].as_array().into_iter().flatten().filter(|a| a["kind"] == "libraries" || a["kind"] == "reread") {
                            if let Some(id) = a["source"].as_str() {
                                unread.push(id.to_string());
                            }
                        }
                    }
                    // projects' stored copies changed since they were read (or read before format 4), and
                    // projects whose files are gone
                    let scan = project::scan_changes(&lib, &unread);
                    unread.extend(scan.read.iter().cloned());
                    let mut read = vec![];
                    for id in changed.iter().chain(unread.iter()) {
                        if read.contains(id) {
                            continue;
                        }
                        app.read_project(id, None, Some(&job)).await?;
                        read.push(id.clone());
                    }
                    let added: Vec<&String> = changed.iter().filter(|id| !known_before.contains(id)).collect();
                    Ok(json!({ "read": read, "added": added, "missing": scan.missing }))
                })))
            }
            "source_get" => {
                let id = arg(&args, "id").map_err(e2s)?;
                let lib = self.library()?;
                let src = lib.source(id).map_err(e2s)?;
                let c = self.catalog()?;
                let entry = c.catalog["sources"].as_array().into_iter().flatten().find(|s| s["id"] == id).cloned().unwrap_or(json!({}));
                let (readme, license_text, files) = match c.derived.get(id) {
                    Some((d, dir)) => (
                        d["readme"].as_str().and_then(|p| std::fs::read_to_string(dir.join(p)).ok()).map(|t| t.chars().take(100_000).collect::<String>()),
                        d["license_file"].as_str().and_then(|p| std::fs::read_to_string(dir.join(p)).ok()).map(|t| t.chars().take(20_000).collect::<String>()),
                        json!(crate::scan::list_files(dir).iter().take(2000).map(|f| json!([f.rel, f.bytes])).collect::<Vec<_>>()),
                    ),
                    None => (None, None, json!([])),
                };
                let derived = c.derived.get(id).map(|(d, _)| json!({ "stats": d["stats"], "problems": d["problems"], "made": d["made"], "engine": d["engine"], "editor_toml": d["editor_toml"] }));
                j(json!({
                    "source": src, "entry": entry, "metadata": lib.metadata(id), "readme": readme, "license_text": license_text,
                    "files": files, "derived": derived,
                    // the description on a model site's page (a Printables PDF's), as HTML
                    "site_description": src["detected"]["description"].as_str().map(|d| crate::docs::markdown_html(d, crate::docs::Rebase { img: None, link: None, drop: true })),
                    "folder": lib.version_dir(&src, src["version"].as_str().unwrap_or("")).ok().map(|p| p.display().to_string()),
                    "folder_rel": lib.version_dir(&src, src["version"].as_str().unwrap_or("")).ok().and_then(|p| folder_rel(&lib, id, &src, &p)),
                }))
            }
            // { source, rel?, examples? }: where a project's file is on disk, for Show in folder (0.4.23); the
            // project's folder when the file isn't there
            "reveal_path" => {
                let id = arg(&args, "source").map_err(e2s)?;
                let lib = self.library()?;
                let src = lib.source(id).map_err(e2s)?;
                let c = self.catalog()?;
                let root = match c.derived.get(id) {
                    // a part made from a folder of examples
                    _ if args["examples"] == true => lib.source_dir(id).map_err(e2s)?.join("examples"),
                    Some((_, dir)) => dir.clone(),
                    None => lib.version_dir(&src, src["version"].as_str().unwrap_or("")).map_err(e2s)?,
                };
                let file = args["rel"].as_str().filter(|r| !r.is_empty()).and_then(|r| library::rel_inside(r).ok()).map(|r| root.join(r));
                j(json!(file.filter(|f| f.exists()).unwrap_or(root).display().to_string()))
            }
            // { source }: pictures to choose from (milestone 6): those in its files, and those you added
            "pictures_list" => {
                let id = arg(&args, "source").map_err(e2s)?;
                let lib = self.library()?;
                let src = lib.source(id).map_err(e2s)?;
                let c = self.catalog()?;
                let dir = match c.derived.get(id) {
                    Some((_, dir)) => dir.clone(),
                    None => lib.version_dir(&src, src["version"].as_str().unwrap_or("")).map_err(e2s)?,
                };
                let mut v = crate::content::pictures(&lib, id, &dir);
                // each as { value (the icon), url }
                let files_rel = folder_rel(&lib, id, &src, &dir).unwrap_or_else(|| format!("sources/{id}/files/{}", src["version"].as_str().unwrap_or("")));
                let url = |rel: &str| format!("{}{}", self.paths.library_url, rel.split('/').map(catalog::urlencode).collect::<Vec<_>>().join("/"));
                v["files"] = json!(v["files"].as_array().into_iter().flatten().filter_map(Value::as_str)
                    .map(|p| json!({ "value": format!("file:{p}"), "path": p, "url": url(&format!("{files_rel}/{p}")) })).collect::<Vec<_>>());
                v["added"] = json!(v["added"].as_array().into_iter().flatten().filter_map(Value::as_str)
                    .map(|n| json!({ "value": format!("picture:{n}"), "path": n, "url": url(&format!("sources/{id}/pictures/{n}")) })).collect::<Vec<_>>());
                j(v)
            }
            // { source, path }: a picture you chose, copied into sources/<id>/pictures/ -> { icon }
            "picture_add" => {
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "source").map_err(e2s)?;
                lib.source(id).map_err(e2s)?;
                let from = PathBuf::from(arg(&args, "path").map_err(e2s)?);
                let ext = from.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if !crate::content::PICTURE_EXTS.contains(&ext.as_str()) {
                    return Err("Choose a PNG, JPEG, WebP or GIF picture.".into());
                }
                let name = crate::content::copy_in(&lib.source_dir(id).map_err(e2s)?.join("pictures"), &from).map_err(e2s)?;
                j(json!({ "icon": format!("picture:{name}") }))
            }
            // { source, level, target?, path | url, name? }: content you add (milestone 6), copied into
            // sources/<id>/attachments/ and listed under its level in metadata.json
            "attachment_add" => {
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "source").map_err(e2s)?;
                lib.source(id).map_err(e2s)?;
                let (section, key) = level_key(&args)?;
                let entry = match (args["path"].as_str().filter(|p| !p.is_empty()), args["url"].as_str().map(str::trim).filter(|u| !u.is_empty())) {
                    (Some(p), _) => {
                        let file = crate::content::copy_in(&lib.source_dir(id).map_err(e2s)?.join("attachments"), Path::new(p)).map_err(e2s)?;
                        let name = args["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(String::from).unwrap_or_else(|| file.rsplit_once('.').map_or(file.as_str(), |(s, _)| s).to_string());
                        json!({ "name": name, "file": file, "added": library::now() })
                    }
                    (None, Some(u)) if u.starts_with("https://") || u.starts_with("http://") => {
                        let name = args["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(u);
                        json!({ "name": name, "url": u, "added": library::now() })
                    }
                    (None, Some(_)) => return Err("A link starts with https:// or http://.".into()),
                    _ => return Err("Choose a file or give a link.".into()),
                };
                let mut m = lib.metadata(id);
                crate::content::list_mut(&mut m, section, key.as_deref()).push(entry.clone());
                lib.save_metadata(id, &m).map_err(e2s)?;
                self.invalidate();
                j(entry)
            }
            // { source, level, target?, index }: take one off the list; its file goes to backups/removed/
            "attachment_remove" => {
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "source").map_err(e2s)?;
                let (section, key) = level_key(&args)?;
                let i = args["index"].as_u64().ok_or("missing index")? as usize;
                let mut m = lib.metadata(id);
                let list = crate::content::list_mut(&mut m, section, key.as_deref());
                if i >= list.len() {
                    return Err("That content isn't there any more.".into());
                }
                let gone = list.remove(i);
                meta::prune(&mut m);
                lib.save_metadata(id, &m).map_err(e2s)?;
                if let Some(f) = gone["file"].as_str() {
                    crate::content::set_aside(&lib, id, f).map_err(e2s)?;
                }
                self.invalidate();
                j(gone)
            }
            // { data, name }: what a 3D file holds: objects and colours (0.4.19, naming a 3MF download)
            "mesh_info" => {
                let data = decode_data_url(arg(&args, "data").map_err(e2s)?).map_err(e2s)?;
                j(crate::mesh::summary(args["name"].as_str().unwrap_or("model.3mf"), &data).map_err(e2s)?)
            }
            // its part files that are in another project too (Review 5, UI-34: on its page, not only when it's added)
            "source_duplicates" => j(json!(self.duplicates(arg(&args, "id").map_err(e2s)?))),
            // a model's details, or null when it isn't in the library (an open tab whose project is being read
            // again): not an error, so a stale list doesn't log a failed request
            "model_get" => j(self.catalog()?.models.get(arg(&args, "key").map_err(e2s)?).cloned().unwrap_or(Value::Null)),
            "docs_list" => {
                // a project's ({ source }) or an app library's ({ library }) documents, for the side viewer
                let t = self.doc_target(&args).map_err(e2s)?;
                let list: Vec<Value> = t
                    .docs
                    .iter()
                    .map(|d| if d["attachment"] == true { d.clone() } else {
                        json!({ "id": d["id"], "title": d["title"], "kind": d["kind"], "src": d["src"], "bytes": d["bytes"], "relative": d["relative"].as_bool().unwrap_or(d["kind"] == "html") })
                    })
                    .collect();
                j(json!({ "docs": list, "folder": t.folder_rel, "path": t.src_root.display().to_string(), "github": t.github.as_ref().map(|g| &g.1) }))
            }
            "doc_get" => {
                // one document: { kind, html } / { kind, text }, or a PDF's bytes
                let t = self.doc_target(&args).map_err(e2s)?;
                let id = arg(&args, "doc").map_err(e2s)?;
                let d = t.docs.iter().find(|d| d["id"] == id).ok_or_else(|| format!("no document {id}"))?;
                // content you added (milestone 6) is under sources/<id>/attachments/, whatever the document's level
                let stored = if d["attachment"] == true { d["path"].as_str().map(PathBuf::from) } else { d["file"].as_str().and_then(|f| library::rel_inside(f).ok()).map(|f| t.file_root.join(f)) }
                    .filter(|p| p.is_file());
                let src = d["src"].as_str().and_then(|f| library::rel_inside(f).ok()).map(|f| t.src_root.join(f));
                let read_text = |p: &std::path::Path| -> Result<String, String> {
                    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
                    if b.len() as u64 > crate::docs::MAX_DOC {
                        return Err(format!("{} is too large to show here ({} MB).", p.display(), b.len() >> 20));
                    }
                    Ok(String::from_utf8_lossy(&b).into_owned())
                };
                match d["kind"].as_str().unwrap_or("") {
                    "html" => {
                        let html = match (stored, src) {
                            (Some(p), _) => read_text(&p)?,
                            (None, Some(p)) => {
                                // not converted yet (a project read before 0.4): make it now, and keep it when we can
                                let base = match &t.github {
                                    Some((raw, blob)) if !t.relative => crate::docs::Rebase::github(raw, blob),
                                    _ => crate::docs::Rebase::default(),
                                };
                                let h = crate::docs::html_from_file(&p, d["from"].as_str().unwrap_or("markdown"), base).map_err(e2s)?;
                                if let Some(dir) = &t.keep_dir {
                                    if std::fs::create_dir_all(dir).is_ok() {
                                        let _ = crate::config::write_atomic(&dir.join(format!("{id}.html")), h.as_bytes());
                                    }
                                }
                                h
                            }
                            _ => return Err(format!("document {id} has no file")),
                        };
                        j(json!({ "kind": "html", "html": html, "relative": d["relative"].as_bool().unwrap_or(t.relative) }))
                    }
                    "text" => {
                        let p = stored.or(src).ok_or_else(|| format!("document {id} has no file"))?;
                        j(json!({ "kind": "text", "text": read_text(&p)? }))
                    }
                    "pdf" => {
                        let p = if d["attachment"] == true { stored } else { src }.ok_or_else(|| format!("document {id} has no file"))?;
                        std::fs::read(&p).map(Reply::Bytes).map_err(|e| format!("{}: {e}", p.display()))
                    }
                    k => Err(format!("unknown kind of document: {k}")),
                }
            }
            "source_remove" => {
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "id").map_err(e2s)?;
                let src = lib.source(id).map_err(e2s)?;
                if src["kind"] == "bundled" {
                    let mut removed = lib.meta()["removed_starter"].as_array().cloned().unwrap_or_default();
                    if !removed.iter().any(|v| v == id) {
                        removed.push(json!(id));
                    }
                    lib.update_meta(json!({ "removed_starter": removed })).map_err(e2s)?;
                }
                // into the library's trash/ folder (with your own project's local/ folder), until the trash is emptied
                let name = self.catalog().ok().and_then(|c| catalog::sorted_sources(&c).get(id).and_then(|s| s["name"].as_str().map(String::from)))
                    .or_else(|| src["detected"]["name"].as_str().map(String::from))
                    .unwrap_or_else(|| id.to_string());
                let info = lib.trash_source(id, &name).map_err(e2s)?;
                self.invalidate();
                j(info)
            }
            "trash_list" => j(json!(self.library()?.trash_list())),
            "trash_restore" => {
                let lib = self.library()?;
                let info = lib.trash_restore(arg(&args, "entry").map_err(e2s)?).map_err(e2s)?;
                if info["source_kind"] == "bundled" {
                    let mut removed = lib.meta()["removed_starter"].as_array().cloned().unwrap_or_default();
                    removed.retain(|v| *v != info["id"]);
                    lib.update_meta(json!({ "removed_starter": if removed.is_empty() { Value::Null } else { json!(removed) } })).map_err(e2s)?;
                }
                self.invalidate();
                j(info)
            }
            "trash_empty" => {
                let lib = self.library()?;
                let n = lib.trash_empty(args["entry"].as_str()).map_err(e2s)?;
                j(json!({ "removed": n }))
            }
            "source_restore_starter" => {
                let lib = self.library()?;
                lib.update_meta(json!({ "removed_starter": Value::Null })).map_err(e2s)?;
                let r = match &self.paths.starter {
                    Some(s) => project::sync_starter(&lib, s).map_err(e2s)?,
                    None => (vec![], vec![]),
                };
                self.invalidate();
                j(json!({ "installed": r.0 }))
            }
            "starter_convert" => {
                // { ids?: [..] } -> job { converted: [{ id, name }], failed: [{ id, name, message }] }: starter
                // projects that are on GitHub become GitHub projects (F-1); one that doesn't read, or
                // loses a model, is put back as it was
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let only: Option<Vec<String>> = args["ids"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
                let ids: Vec<String> = lib.source_ids().into_iter().filter(|id| {
                    only.as_ref().is_none_or(|o| o.contains(id)) && sources::starter_on_github(id).is_some() && lib.source(id).is_ok_and(|s| s["kind"] == "bundled")
                }).collect();
                Ok(Reply::Json(self.spawn("Getting projects from GitHub", move |app, job| async move {
                    let lib = app.library().map_err(|e| anyhow!(e))?;
                    let (mut converted, mut failed) = (vec![], vec![]);
                    for id in ids {
                        let old = lib.source(&id)?;
                        let name = old["detected"]["name"].as_str().unwrap_or(&id).to_string();
                        app.job_stage(&job, format!("Downloading {name}…"));
                        let before: Vec<String> = lib.derived(&id, old["version"].as_str().unwrap_or(""))
                            .map(|d| d["models"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        let (lib2, id2, token) = (lib.clone(), id.clone(), app.config().github_token());
                        let r = match tokio::task::spawn_blocking(move || sources::convert_starter(&lib2, &id2, token)).await? {
                            Err(e) => Err(e),
                            Ok(_) => match app.read_project(&id, None, Some(&job)).await {
                                Err(e) => Err(e),
                                Ok(_) => {
                                    let now = lib.source(&id)?;
                                    let d = lib.derived(&id, now["version"].as_str().unwrap_or("")).unwrap_or_default();
                                    let have: Vec<String> = d["models"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(String::from)).collect();
                                    let lost: Vec<&String> = before.iter().filter(|m| !have.contains(m)).collect();
                                    // with what reading said about each, when it said something
                                    let why = |m: &str| d["problems"].as_array().into_iter().flatten()
                                        .find(|p| p["model"].as_str() == Some(&format!("{id}/{m}")))
                                        .and_then(|p| p["message"].as_str()).map(|t| format!("{m} ({t})")).unwrap_or_else(|| m.to_string());
                                    if lost.is_empty() { Ok(()) } else { Err(anyhow!("upstream's files don't give the models {}", lost.iter().map(|m| why(m)).collect::<Vec<_>>().join(", "))) }
                                }
                            },
                        };
                        match r {
                            Ok(()) => {
                                if let Some(c) = sources::starter_on_github(&id) {
                                    sources::rebase_recipe_edits(&lib, &id, &c.vendor_path)?;
                                    let _ = sources::drop_same_old_copy(&lib, &id, &c.vendor_path);
                                }
                                converted.push(json!({ "id": id, "name": name }));
                            }
                            Err(e) => {
                                if lib.source(&id).is_ok_and(|s| s["kind"] != "bundled") {
                                    sources::unconvert_starter(&lib, &old)?;
                                }
                                failed.push(json!({ "id": id, "name": name, "message": format!("{e:#}") }));
                            }
                        }
                        app.invalidate();
                    }
                    Ok(json!({ "converted": converted, "failed": failed }))
                })))
            }
            "source_role" => {
                let lib = self.library()?;
                let id = arg(&args, "id").map_err(e2s)?.to_string();
                let mut src = lib.source(&id).map_err(e2s)?;
                src["role"] = json!(if args["role"] == "library" { "library" } else { "project" });
                if let Some(n) = args["library_name"].as_str() {
                    src["library_name"] = json!(n);
                }
                lib.save_source(&src).map_err(e2s)?;
                Ok(Reply::Json(self.spawn(&format!("Reading {id}"), move |app, job| async move {
                    app.read_project(&id, None, Some(&job)).await
                })))
            }
            "updates_check" => {
                let only = args["id"].as_str().map(String::from);
                Ok(Reply::Json(self.spawn("Checking for updates", move |app, job| async move { app.check_updates(only, &job).await })))
            }
            "update_apply" => {
                let lib = self.library()?;
                let id = arg(&args, "id").map_err(e2s)?;
                let mut src = lib.source(id).map_err(e2s)?;
                let latest = src["update"]["latest"]["id"].as_str().ok_or("No update is waiting.")?.to_string();
                if lib.derived(id, &latest).is_none() {
                    return Err("The new version hasn't been read; check for updates again.".into());
                }
                let previous = src["version"].clone();
                src["version"] = json!(latest);
                src["version_date"] = src["update"]["latest"]["date"].clone();
                src["previous_version"] = previous;
                src["update"] = json!({ "state": "current", "checked": src["update"]["checked"], "applied": library::now() });
                lib.save_source(&src).map_err(e2s)?;
                self.invalidate();
                j(src)
            }
            "update_skip" => {
                let lib = self.library()?;
                let id = arg(&args, "id").map_err(e2s)?;
                let mut src = lib.source(id).map_err(e2s)?;
                let latest = src["update"]["latest"]["id"].clone();
                src["update"] = json!({ "state": "skipped", "skipped": latest, "checked": src["update"]["checked"] });
                lib.save_source(&src).map_err(e2s)?;
                self.invalidate();
                j(src)
            }
            "source_clean" => {
                let lib = self.library()?;
                let src = lib.source(arg(&args, "id").map_err(e2s)?).map_err(e2s)?;
                let removed = sources::clean_versions(&lib, &src).map_err(e2s)?;
                let mut src = src;
                if let Some(vs) = src["versions"].as_array_mut() {
                    vs.retain(|v| !v["id"].as_str().is_some_and(|i| removed.iter().any(|r| r == i)));
                }
                lib.save_source(&src).map_err(e2s)?;
                j(json!({ "removed": removed }))
            }
            "meta_set" => {
                // { source, level: project|folder|item|part, target, patch } -> { previous }
                let lib = self.library()?;
                let id = arg(&args, "source").map_err(e2s)?;
                lib.source(id).map_err(e2s)?;
                let patch = args["patch"].as_object().ok_or("missing patch")?.clone();
                let mut m = lib.metadata(id);
                let (section, key) = level_key(&args)?;
                let prev = meta::patch_level(meta::level_mut(&mut m, section, key.as_deref()), &patch);
                meta::prune(&mut m);
                lib.save_metadata(id, &m).map_err(e2s)?;
                self.invalidate();
                j(json!({ "previous": prev }))
            }
            "meta_set_many" => {
                // { edits: [{ source, level, target, patch }] } -> { previous: [same shape, for undo] }
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let edits = args["edits"].as_array().ok_or("missing edits")?;
                let mut by_source: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
                for e in edits {
                    by_source.entry(arg(e, "source").map_err(e2s)?.to_string()).or_default().push(e);
                }
                let mut previous = vec![];
                for (id, es) in by_source {
                    lib.source(&id).map_err(e2s)?;
                    let mut m = lib.metadata(&id);
                    for e in es {
                        let (section, key) = level_key(e)?;
                        let patch = e["patch"].as_object().ok_or("missing patch")?;
                        let prev = meta::patch_level(meta::level_mut(&mut m, section, key.as_deref()), patch);
                        previous.push(json!({ "source": id, "level": e["level"], "target": e["target"], "patch": prev }));
                    }
                    meta::prune(&mut m);
                    lib.save_metadata(&id, &m).map_err(e2s)?;
                }
                self.invalidate();
                j(json!({ "previous": previous }))
            }
            "meta_clear_items" => {
                // remove one field from every item/part of a project ("use the project's value everywhere")
                let lib = self.library()?;
                let id = arg(&args, "source").map_err(e2s)?;
                let field = arg(&args, "field").map_err(e2s)?.to_string();
                let mut m = lib.metadata(id);
                let mut previous = Map::new();
                for section in ["items", "parts", "folders"] {
                    if let Some(Value::Object(sec)) = m.get_mut(section) {
                        for (k, v) in sec.iter_mut() {
                            if let Some(old) = v.as_object_mut().and_then(|o| o.shift_remove(&field)) {
                                previous.insert(format!("{section}/{k}"), old);
                            }
                        }
                    }
                }
                meta::prune(&mut m);
                lib.save_metadata(id, &m).map_err(e2s)?;
                self.invalidate();
                j(json!({ "previous": previous, "cleared": previous.len() }))
            }
            "meta_restore" => {
                // undo of meta_clear_items: { source, field, previous: { "items/x": value } }
                let lib = self.library()?;
                let id = arg(&args, "source").map_err(e2s)?;
                let field = arg(&args, "field").map_err(e2s)?;
                let mut m = lib.metadata(id);
                for (k, v) in args["previous"].as_object().into_iter().flatten() {
                    if let Some((section, key)) = k.split_once('/') {
                        meta::level_mut(&mut m, section, Some(key)).insert(field.to_string(), v.clone());
                    }
                }
                lib.save_metadata(id, &m).map_err(e2s)?;
                self.invalidate();
                j(Value::Null)
            }
            "part_save" => {
                // { model, model_name, project, project_version, name, category?, values, edits?, data, replace?, folder? }:
                // a part made from a model, kept in the library (milestone 4): local/Made parts/<model>/<name>.stl (or .3mf)
                // and <name>.recipe.json beside it, saying how it was made so it can be made again
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let clean = |s: &str| -> String {
                    let t: String = s.chars().map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c }).collect();
                    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                    t.trim_matches('.').chars().take(100).collect()
                };
                let name = clean(arg(&args, "name").map_err(e2s)?);
                // made again (replace): into the folder it's in now, whatever the model is called today.
                // Otherwise the model's name, with its project's when another model in the library has
                // that name too ("Bevel gear (BOSL2)"), so two models' parts don't share a folder (F-19)
                let folder = match args["folder"].as_str() {
                    Some(f) => clean(f),
                    None => {
                        let model_name = args["model_name"].as_str().unwrap_or("Parts");
                        let key = args["model"].as_str().unwrap_or("");
                        let cat = self.catalog()?;
                        let all = || ["models", "components"].into_iter().flat_map(|k| cat.catalog[k].as_array().into_iter().flatten());
                        let shared = all().any(|m| m["key"].as_str() != Some(key) && m["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(model_name)));
                        let project = all().find(|m| m["key"].as_str() == Some(key)).and_then(|m| m["family_name"].as_str()).map(String::from);
                        match project.filter(|_| shared) {
                            Some(p) => clean(&format!("{model_name} ({p})")),
                            None => clean(model_name),
                        }
                    }
                };
                if name.is_empty() || folder.is_empty() {
                    return Err("Give the part a name.".into());
                }
                let dir = lib.root().join("local").join(project::MADE_PARTS).join(&folder);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                // the format chosen beside Download: OpenSCAD's 3MF keeps colours and objects (Review 6, F-28)
                let ext = if args["format"] == "3mf" { "3mf" } else { "stl" };
                let taken = |s: &str| ["stl", "3mf"].iter().any(|e| dir.join(format!("{s}.{e}")).exists());
                let mut stem = name.clone();
                if args["replace"] != true {
                    let mut n = 2;
                    while taken(&stem) {
                        stem = format!("{name} {n}");
                        n += 1;
                    }
                }
                let data = decode_data_url(arg(&args, "data").map_err(e2s)?).map_err(e2s)?;
                // made again: the recipe keeps when the part was first made
                let created = std::fs::read(dir.join(format!("{stem}.recipe.json"))).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                    .map(|r| crate::format::recipe_view(r, true, Some(&stem))["created"].clone()).filter(Value::is_string);
                crate::config::write_atomic(&dir.join(format!("{stem}.{ext}")), &data).map_err(e2s)?;
                // made again in the other format: the old file goes, so the part has one
                let _ = std::fs::remove_file(dir.join(format!("{stem}.{}", if ext == "3mf" { "stl" } else { "3mf" })));
                // the one recipe shape (format.rs): what saved settings have, plus the file it made
                let now = library::now();
                let recipe = json!({
                    "format": 1, "kind": "part", "name": stem, "made_by": format!("SCAD Workshop {}", env!("CARGO_PKG_VERSION")),
                    "model": args["model"], "model_name": args["model_name"], "project": args["project"], "project_version": args["project_version"],
                    "files_id": args["files_id"], "category": args["category"], "values": args["values"], "edits": args["edits"],
                    "engine": self.engine_version_site, "created": created.unwrap_or_else(|| json!(now)), "updated": now, "output": ext,
                });
                crate::config::write_json(&dir.join(format!("{stem}.recipe.json")), &recipe).map_err(e2s)?;
                j(json!({ "name": stem, "folder": folder, "file": format!("{stem}.{ext}") }))
            }
            "thumb_put" => {
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "source").map_err(e2s)?;
                let name = arg(&args, "name").map_err(e2s)?;
                if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
                    return Err("invalid thumbnail name".into());
                }
                let data = decode_data_url(arg(&args, "data").map_err(e2s)?).map_err(e2s)?;
                let p = lib.source_dir(id).map_err(e2s)?.join("thumbs").join(format!("{name}.webp"));
                crate::config::write_atomic(&p, &data).map_err(e2s)?;
                self.invalidate();
                j(Value::Null)
            }
            "workshop_save" => {
                // { source }: write the description the app uses for one of your own projects (local/ or
                // linked), with your edits, into its folder as workshop.json (F-5, "Save as project file")
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "source").map_err(e2s)?.to_string();
                let src = lib.source(&id).map_err(e2s)?;
                if src["kind"] != "local" && src["kind"] != "linked" {
                    return Err("Only your own projects (in the library's local/ folder, or linked) keep a project file in their folder.".into());
                }
                let c = self.catalog()?;
                let (derived, root) = c.derived.get(&id).cloned().ok_or("Read the project first.")?;
                let mut fam = derived["family"].clone();
                let entry = c.catalog["sources"].as_array().into_iter().flatten().find(|s| s["id"] == id.as_str()).cloned().unwrap_or_default();
                for k in ["name", "summary", "category", "tags", "license", "authors"] {
                    if let Some(v) = entry["meta"].get(k).filter(|v| !v.is_null()) {
                        fam[k] = v.clone();
                    }
                }
                let models: Vec<Value> = fam["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| {
                        let d = c.models.get(&format!("{id}/{}", m["id"].as_str().unwrap_or("")))?; // (deleted ones are left out)
                        let mut m = m.clone();
                        m["name"] = d["name"].clone();
                        let form = &d["form"];
                        for (k, v) in form.as_object().into_iter().flatten() {
                            match (k.as_str(), m.get_mut(k)) {
                                ("ui", Some(Value::Object(mine))) => {
                                    for (name, over) in v.as_object().into_iter().flatten() {
                                        let mut both = mine.get(name).and_then(Value::as_object).cloned().unwrap_or_default();
                                        both.extend(over.as_object().cloned().unwrap_or_default());
                                        mine.insert(name.clone(), Value::Object(both));
                                    }
                                }
                                ("ui" | "hidden" | "fixed" | "presets", _) => m[k] = v.clone(),
                                _ => {}
                            }
                        }
                        Some(m)
                    })
                    .collect();
                fam["models"] = json!(models);
                let mut out = project::describe(&fam, &id, src["version"].as_str().unwrap_or(""));
                out["made_by"] = json!(format!("Saved by SCAD Workshop {} on {}", env!("CARGO_PKG_VERSION"), library::now()));
                let path = root.join("workshop.json");
                crate::config::write_json(&path, &out).map_err(e2s)?;
                self.invalidate();
                j(json!({ "path": path.display().to_string(), "models": models.len() }))
            }
            "defaults_put" => {
                // { source, model, ok, message?, seconds? }: what rendering a model at its defaults gave
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "source").map_err(e2s)?;
                let model = arg(&args, "model").map_err(e2s)?;
                let src = lib.source(id).map_err(e2s)?;
                let version = src["version"].as_str().unwrap_or("");
                let ok = args["ok"].as_bool().unwrap_or(false);
                let message: String = args["message"].as_str().unwrap_or("").chars().take(600).collect();
                let result = json!({
                    "ok": ok, "message": if message.is_empty() { Value::Null } else { json!(message) },
                    "seconds": args["seconds"].as_f64().map(|s| (s * 10.0).round() / 10.0), "date": library::now(),
                });
                lib.put_check(id, version, &self.engine_version_site, model, result).map_err(e2s)?;
                self.invalidate();
                j(Value::Null)
            }
            "example_put" => {
                // { source, model, data }: an example made into a part (F-15), made at its defaults by the app:
                // sources/<id>/examples/<fingerprint>/<model>.stl (rebuildable), kept by the fingerprint of the
                // files the model uses (F-20); the model's earlier ones go
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "source").map_err(e2s)?;
                let model = arg(&args, "model").map_err(e2s)?;
                if !model.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
                    return Err("invalid model id".into());
                }
                let fp = self.catalog()?.models.values().find(|m| m["source_id"] == id && m["id"] == model).and_then(|m| m["files_id"].as_str()).map(String::from)
                    .ok_or("That example isn't in the library.")?;
                let data = decode_data_url(arg(&args, "data").map_err(e2s)?).map_err(e2s)?;
                let p = lib.resolve(&format!("sources/{id}/examples/{fp}/{model}.stl")).map_err(e2s)?;
                crate::config::write_atomic(&p, &data).map_err(e2s)?;
                for e in std::fs::read_dir(p.parent().and_then(Path::parent).ok_or("no folder")?).into_iter().flatten().flatten() {
                    if e.file_name() != fp.as_str() && e.path().is_dir() {
                        let _ = std::fs::remove_file(e.path().join(format!("{model}.stl")));
                        let _ = std::fs::remove_dir(e.path()); // (only when nothing else is left in it)
                    }
                }
                self.invalidate();
                j(Value::Null)
            }
            "parts_merge" => {
                // { source, ids: [keep, ...others] }: parts made one (near misses, milestone 5), kept in the
                // project's metadata.json as `merged: { keep: [others] }`; with `split: true`, made separate again;
                // with `apart: true`, the group isn't offered again
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "source").map_err(e2s)?.to_string();
                let ids: Vec<String> = args["ids"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
                let keep = ids.first().ok_or("Name the parts to make one.")?.clone();
                let mut meta = lib.metadata(&id);
                if args["apart"] == true {
                    // Keep apart (Review 5, UI-35): metadata.json's `apart: [[ids...]]`, not offered again
                    let mut group = ids.clone();
                    group.sort();
                    let mut apart = meta["apart"].as_array().cloned().unwrap_or_default();
                    if !apart.iter().any(|g| *g == json!(group)) {
                        apart.push(json!(group));
                    }
                    meta["apart"] = Value::Array(apart);
                    lib.save_metadata(&id, &meta).map_err(e2s)?;
                    self.invalidate();
                    return j(json!({ "apart": ids }));
                }
                if !meta["merged"].is_object() {
                    meta["merged"] = json!({});
                }
                if args["split"] == true {
                    meta["merged"].as_object_mut().unwrap().remove(&keep);
                } else {
                    let mut all: Vec<Value> = meta["merged"][&keep].as_array().cloned().unwrap_or_default();
                    all.extend(ids[1..].iter().map(|i| json!(i)));
                    meta["merged"][&keep] = json!(all);
                }
                if meta["merged"].as_object().is_some_and(|m| m.is_empty()) {
                    meta.as_object_mut().unwrap().remove("merged");
                }
                lib.save_metadata(&id, &meta).map_err(e2s)?;
                self.invalidate();
                j(json!({ "part": keep }))
            }
            "examples_set" => {
                // { source, as: "models" | "parts" | "leave" }: what a project's folder of examples is (F-15),
                // kept in its metadata.json so it holds across updates; the project is read again
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "source").map_err(e2s)?.to_string();
                let r#as = arg(&args, "as").map_err(e2s)?.to_string();
                if !["models", "parts", "leave"].contains(&r#as.as_str()) {
                    return Err(format!("unknown choice {as}", as = r#as));
                }
                let src = lib.source(&id).map_err(e2s)?;
                let folder = lib.derived(&id, src["version"].as_str().unwrap_or("")).map(|d| d["examples"]["folder"].clone()).filter(Value::is_string)
                    .ok_or("This project has no folder of examples.")?;
                let mut meta = lib.metadata(&id);
                meta["examples"] = json!({ "folder": folder, "as": r#as });
                lib.save_metadata(&id, &meta).map_err(e2s)?;
                self.invalidate();
                Ok(Reply::Json(self.spawn(&format!("Reading {id} again"), move |app, job| async move {
                    app.read_project(&id, None, Some(&job)).await
                })))
            }
            "defaults_clear" => {
                let lib = self.library()?;
                lib.clear_checks(arg(&args, "source").map_err(e2s)?).map_err(e2s)?;
                self.invalidate();
                j(Value::Null)
            }
            "icon_import" => {
                // { category, path }: an image of your own as a category's icon, copied into the library's icons/ folder
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let id = arg(&args, "category").map_err(e2s)?.to_string();
                let from = PathBuf::from(arg(&args, "path").map_err(e2s)?);
                let ext = from.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
                if !["png", "jpg", "jpeg", "webp", "svg"].contains(&ext.as_str()) {
                    return Err("Choose a PNG, JPEG, WebP or SVG image.".into());
                }
                let data = std::fs::read(&from).map_err(|e| format!("couldn't read {}: {e}", from.display()))?;
                if data.len() > 4 << 20 {
                    return Err("That image is over 4 MB; use a smaller one.".into());
                }
                let rel = format!("icons/category-{}.{ext}", library::slug(&id));
                crate::config::write_atomic(&lib.root().join(&rel), &data).map_err(e2s)?;
                let mut cats = lib.meta()["categories"].as_object().cloned().unwrap_or_default();
                let mut c = cats.get(&id).and_then(Value::as_object).cloned().unwrap_or_default();
                let prev = c.insert("icon".into(), json!(rel));
                cats.insert(id, Value::Object(c));
                lib.update_meta(json!({ "categories": cats })).map_err(e2s)?;
                self.invalidate();
                j(json!({ "icon": rel, "previous": prev }))
            }
            "component_thumb" => {
                // { key: "@lib/module", data: webp data URL }: a thumbnail made after the component was rendered
                let lib = self.library()?;
                lib.writable().map_err(e2s)?;
                let key = arg(&args, "key").map_err(e2s)?;
                let c = self.catalog()?;
                let m = c.models.get(key).filter(|m| m["kind"] == "component").ok_or_else(|| format!("no component {key}"))?;
                let module = m["id"].as_str().unwrap_or("");
                let libname = m["component"]["library"].as_str().unwrap_or("");
                let copy = m["component"]["copy"].as_str().unwrap_or("");
                let ok = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-@.".contains(&b));
                if !module.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || !ok(copy) {
                    return Err("invalid module name".into());
                }
                let data = decode_data_url(arg(&args, "data").map_err(e2s)?).map_err(e2s)?;
                // (made by this copy and version of the library: see catalog::component_thumb_dir)
                let rel = format!("thumbs/components/{}/{copy}/{module}.webp", library::slug(libname));
                crate::config::write_atomic(&lib.root().join(&rel), &data).map_err(e2s)?;
                self.invalidate();
                j(json!(rel))
            }
            "library_merge" => {
                let from = PathBuf::from(arg(&args, "path").map_err(e2s)?);
                Ok(Reply::Json(self.spawn("Merging a library", move |app, job| async move {
                    let lib = app.library().map_err(|e| anyhow!(e))?;
                    app.job_stage(&job, "Copying projects…");
                    let lib2 = lib.clone();
                    let prefs = app.prefs();
                    tokio::task::spawn_blocking(move || crate::merge::merge(&lib2, &from, &prefs)).await?
                })))
            }
            "library_check" | "library_repair" => {
                // Check library (0.4.22): the folder against docs/LIBRARY_FORMAT.md; Repair fixes what's safe
                let lib = self.library()?;
                let c = self.catalog()?;
                let mut known: std::collections::HashSet<String> =
                    c.models.keys().map(|k| if k.starts_with('@') { format!("comp:{k}") } else { format!("gen:{k}") }).collect();
                for (pid, pack) in &c.parts {
                    known.extend(pack["items"].as_array().into_iter().flatten().filter_map(|it| it["id"].as_str()).map(|it| format!("part:{pid}/{it}")));
                }
                // a project not read yet (your own are read just after the library opens): its items aren't known
                for id in lib.source_ids() {
                    if lib.source(&id).is_ok_and(|s| s["kind"] != "pinned" && lib.derived(&id, s["version"].as_str().unwrap_or("")).is_none()) {
                        known.insert(format!("proj:{id}"));
                    }
                }
                let known = Some(known);
                if cmd == "library_check" {
                    j(crate::check::check(&lib, &known))
                } else {
                    let out = crate::check::repair(&lib, &known).map_err(e2s)?;
                    self.invalidate();
                    j(out)
                }
            }
            "merge_resolve" => {
                let lib = self.library()?;
                crate::merge::resolve(&lib, &args["choices"]).map_err(e2s)?;
                self.invalidate();
                j(Value::Null)
            }
            "render_default" => {
                // render a model at its defaults (tests and the starter check)
                let key = arg(&args, "key").map_err(e2s)?;
                let c = self.catalog()?;
                let model = c.models.get(key).ok_or_else(|| format!("no model {key}"))?;
                let req = crate::render::default_request(model, &c.catalog["common_files"]).map_err(e2s)?;
                let r = self.renderer().await?;
                r.add_blobs(catalog::blob_list(&c));
                let out = r.render(&format!("default-{}", key.replace('/', "-")), &req, |_| {}).await.map_err(|e| e.message)?;
                j(json!({ "key": key, "triangles": crate::render::stl_triangles(&out.stl), "bytes": out.stl.len(), "ms": out.ms, "cached": out.cached }))
            }
            "cache_clear" => {
                let r = self.renderer().await?;
                let freed = tokio::task::spawn_blocking(move || r.clear_cache()).await.map_err(|e| e.to_string())?.map_err(e2s)?;
                j(json!(freed))
            }
            other => Err(format!("unknown command {other}")),
        }
    }

    /// Files the page reads: the catalog, model pages and parts packs (from the
    /// library, with edits applied), library files, and the app's own data.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let path = path.trim_start_matches("./").trim_start_matches('/');
        if path == "data/catalog.json" {
            let c = self.catalog().map_err(|e| anyhow!(e))?;
            return Ok(serde_json::to_vec(&c.catalog)?);
        }
        if let Some(rest) = path.strip_prefix("data/models/").and_then(|r| r.strip_suffix(".json")) {
            let key = rest.replacen("--", "/", 1);
            let c = self.catalog().map_err(|e| anyhow!(e))?;
            if let Some(d) = c.models.get(&key) {
                return Ok(serde_json::to_vec(d)?);
            }
            bail!("no model {key}");
        }
        if let Some(id) = path.strip_prefix("data/libraries/").and_then(|r| r.strip_suffix(".json")) {
            let c = self.catalog().map_err(|e| anyhow!(e))?;
            if let Some(p) = c.parts.get(id) {
                return Ok(serde_json::to_vec(p)?);
            }
            bail!("no parts pack {id}");
        }
        if let Some(rel) = path.strip_prefix("library/") {
            return self.library_file(rel).with_context(|| format!("couldn't read {}", percent_decode(rel)));
        }
        // a part's 3MF or OBJ as STL for the viewer, with a 3MF's colours (milestone 5)
        if let Some(rel) = path.strip_prefix("mesh/library/") {
            let name = percent_decode(rel);
            let data = self.library_file(rel).with_context(|| format!("couldn't read {name}"))?;
            let m = crate::mesh::read_mesh(&name, &data).with_context(|| format!("couldn't read the shapes in {name}"))?;
            return Ok(crate::mesh::to_stl_coloured(&m.tris, m.colours.as_deref()));
        }
        // the picture a slicer saved in a 3MF, for its thumbnail; nothing (not an error) when it has
        // none, so the page asks without the browser logging a failed request
        if let Some(rel) = path.strip_prefix("picture/library/") {
            let data = self.library_file(rel).with_context(|| format!("couldn't read {}", percent_decode(rel)))?;
            return Ok(crate::mesh::picture_3mf(&data).unwrap_or_default());
        }
        self.site.read(path)
    }

    /// A file in the open library, for the library:// protocol (thumbnails, part files).
    /// Where an edited file's text is kept (`blob_put`).
    fn edit_blob(&self, sha: &str) -> Option<PathBuf> {
        (sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit())).then(|| self.paths.data_dir.join("edits").join(sha))
    }

    /// Where a project's or an app library's documents are (see `docs_list`).
    fn doc_target(&self, args: &Value) -> Result<DocTarget> {
        if let Some(name) = args["library"].as_str() {
            let b = self.bundled().iter().find(|b| b.info.name.eq_ignore_ascii_case(name)).with_context(|| format!("no library {name}"))?;
            let docs_dir = b.root.parent().map(|p| p.join("docs").join(&b.info.name)).unwrap_or_default();
            let docs = std::fs::read(docs_dir.join("docs.json"))
                .ok()
                .and_then(|v| serde_json::from_slice::<Vec<Value>>(&v).ok())
                .unwrap_or_else(|| crate::docs::library_docs(&b.info, &b.root, None));
            return Ok(DocTarget {
                docs,
                file_root: docs_dir,
                src_root: b.root.clone(),
                folder_rel: None,
                github: crate::docs::library_bases(&b.info),
                relative: false,
                keep_dir: None,
            });
        }
        let id = arg(args, "source")?;
        let lib = self.library().map_err(|e| anyhow!(e))?;
        let src = lib.source(id)?;
        let version = src["version"].as_str().unwrap_or("").to_string();
        let c = self.catalog().map_err(|e| anyhow!(e))?;
        let (derived, dir) = match c.derived.get(id) {
            Some((d, dir)) => (Some(d.clone()), dir.clone()),
            None => (None, lib.version_dir(&src, &version)?),
        };
        let mut docs = derived.as_ref().and_then(|d| d["docs"].as_array().cloned()).unwrap_or_else(|| {
            // read before 0.4: find them now (the HTML is made when first opened)
            let mut d = crate::docs::find(&dir);
            for x in d.iter_mut().filter(|x| x["kind"] == "html") {
                x["file"] = json!(format!("docs/{version}/{}.html", x["id"].as_str().unwrap_or("readme")));
                x["relative"] = json!(true);
            }
            d
        });
        // a model site's page saved as a PDF (a Printables download) is named for what it is (UI-37)
        if let (Some(pdf), Some("printables")) = (src["origin"]["site"]["pdf"].as_str(), src["origin"]["site"]["name"].as_str()) {
            for d in docs.iter_mut().filter(|d| d["src"] == pdf) {
                d["title"] = json!("Printables page (PDF)");
            }
        }
        let github = (src["kind"] == "github")
            .then(|| {
                let o = &src["origin"];
                crate::docs::github_bases(&format!("{}/{}", o["owner"].as_str()?, o["repo"].as_str()?), &version, o["subdir"].as_str().unwrap_or(""))
            })
            .flatten();
        // content you added (milestone 6), each saying which level it's for
        docs.extend(crate::content::docs(&lib, id, &self.paths.library_url));
        let writable = lib.writable().is_ok();
        Ok(DocTarget {
            docs,
            file_root: lib.source_dir(id)?,
            src_root: dir.clone(),
            folder_rel: folder_rel(&lib, id, &src, &dir),
            github,
            relative: true,
            keep_dir: writable.then(|| lib.source_dir(id).map(|d| d.join("docs").join(&version)).ok()).flatten(),
        })
    }

    /// The model files of project `id` that another project has too (same size, then same SHA-256):
    /// [{ file, project }], for the message after adding it (milestone 5).
    pub fn duplicates(&self, id: &str) -> Vec<Value> {
        let (Ok(c), Ok(lib)) = (self.catalog(), self.library()) else { return vec![] };
        let path_of = |f: &Value| f["url"].as_str().and_then(|u| u.strip_prefix("library/")).and_then(|r| lib.resolve(&percent_decode(r)).ok());
        // (a file's path, the project it's in: its name and id)
        let mut by_size: HashMap<u64, Vec<(PathBuf, String, String)>> = HashMap::new();
        for (pid, pack) in c.parts.iter().filter(|(pid, _)| pid.as_str() != id) {
            let name = pack["name"].as_str().unwrap_or(pid).to_string();
            let source = pack["source_id"].as_str().unwrap_or(pid).to_string();
            for f in pack["items"].as_array().into_iter().flatten().flat_map(|it| it["files"].as_array().into_iter().flatten()) {
                if let Some(p) = path_of(f) {
                    if let Ok(m) = std::fs::metadata(&p) {
                        by_size.entry(m.len()).or_default().push((p, name.clone(), source.clone()));
                    }
                }
            }
        }
        let mut out = vec![];
        let mut hashes: HashMap<PathBuf, String> = HashMap::new();
        let mut sha = |p: &Path| hashes.entry(p.to_path_buf()).or_insert_with(|| sources::sha256_file(p).unwrap_or_default()).clone();
        for it in c.parts.get(id).into_iter().flat_map(|pack| pack["items"].as_array().into_iter().flatten()) {
            for f in it["files"].as_array().into_iter().flatten() {
                let Some(p) = path_of(f) else { continue };
                let Some(same) = std::fs::metadata(&p).ok().and_then(|m| by_size.get(&m.len())) else { continue };
                let mine = sha(&p);
                if let Some((_, project, source)) = same.iter().find(|(q, _, _)| !mine.is_empty() && sha(q) == mine) {
                    // `item`: the part here; `source`: the project it's also in (Review 5, UI-34)
                    out.push(json!({ "file": f["path"], "project": project, "source": source, "item": it["id"] }));
                }
            }
        }
        out
    }

    pub fn library_file(&self, rel: &str) -> Result<Vec<u8>> {
        let lib = self.library().map_err(|e| anyhow!(e))?;
        let rel = percent_decode(rel.trim_start_matches('/'));
        if let Some(rest) = rel.strip_prefix(LINKED) {
            // a linked project's own folder (outside the library)
            let (id, inner) = rest.trim_start_matches('/').split_once('/').context("no file")?;
            let src = lib.source(id)?;
            if src["kind"] != "linked" {
                bail!("{id} isn't a linked project");
            }
            let dir = lib.version_dir(&src, src["version"].as_str().unwrap_or(""))?;
            return Ok(std::fs::read(dir.join(library::rel_inside(inner)?))?);
        }
        Ok(std::fs::read(lib.resolve(&rel)?)?)
    }
}

/// Library paths under this name are a linked project's own folder: `~linked/<id>/<path>`.
const LINKED: &str = "~linked";

/// A project's folder as a library path (for its pictures and files in the side viewer): inside
/// the library, or a linked project's folder through `~linked/<id>`.
pub(crate) fn folder_rel(lib: &Library, id: &str, src: &Value, dir: &Path) -> Option<String> {
    lib.relative(dir).or_else(|| (src["kind"] == "linked").then(|| format!("{LINKED}/{id}")))
}

/// Where one project's (or app library's) documents live.
struct DocTarget {
    docs: Vec<Value>,
    /// what a document's `file` is relative to (the project's folder in sources/, or libs/docs/<Name>/)
    file_root: PathBuf,
    /// the project's (or library's) own files: a document's `src`
    src_root: PathBuf,
    /// that folder inside the library, for relative links and images (None: outside it, or an app library)
    folder_rel: Option<String>,
    /// GitHub (raw, blob) addresses at the version in use
    github: Option<(String, String)>,
    /// documents made on request keep links relative to the project's folder
    relative: bool,
    /// where HTML made on request is kept
    keep_dir: Option<PathBuf>,
}

/// The library's "Pinned components" project (made on first use; a trashed one is made again).
fn ensure_pinned(lib: &Library) -> Result<String> {
    let id = "pinned";
    if lib.source(id).is_ok() {
        return Ok(id.into());
    }
    lib.save_source(&json!({
        "id": id, "kind": "pinned", "role": "project", "added": library::now(), "version": "pins",
        "origin": { "type": "pinned" }, "detected": { "name": "Pinned components", "summary": "Components from libraries, pinned as models.", "category": "other" },
    }))?;
    Ok(id.into())
}

/// What changed between two read versions of a project: models added or removed,
/// and settings added, removed or with a new default.
pub fn changes(lib: &Library, id: &str, old: &str, new: &str) -> Value {
    let (Some(a), Some(b)) = (lib.derived(id, old), lib.derived(id, new)) else { return json!({}) };
    let by_id = |d: &Value| -> BTreeMap<String, Value> {
        d["models"].as_array().into_iter().flatten().filter_map(|m| Some((m["id"].as_str()?.to_string(), m.clone()))).collect()
    };
    let (ma, mb) = (by_id(&a), by_id(&b));
    let added: Vec<Value> = mb.iter().filter(|(k, _)| !ma.contains_key(*k)).map(|(_, m)| m["name"].clone()).collect();
    let removed: Vec<Value> = ma.iter().filter(|(k, _)| !mb.contains_key(*k)).map(|(_, m)| m["name"].clone()).collect();
    let mut settings = vec![];
    for (k, old_m) in &ma {
        let Some(new_m) = mb.get(k) else { continue };
        let params = |m: &Value| -> BTreeMap<String, Value> {
            m["parameters"].as_array().into_iter().flatten().filter_map(|p| Some((p["name"].as_str()?.to_string(), p.clone()))).collect()
        };
        let (pa, pb) = (params(old_m), params(new_m));
        let p_added: Vec<&String> = pb.keys().filter(|n| !pa.contains_key(*n)).collect();
        let p_removed: Vec<&String> = pa.keys().filter(|n| !pb.contains_key(*n)).collect();
        let defaults: Vec<Value> = pa
            .iter()
            .filter_map(|(n, p)| {
                let q = pb.get(n)?;
                (!crate::ingest::json_eq(&p["default"], &q["default"])).then(|| json!({ "name": n, "from": p["default"], "to": q["default"] }))
            })
            .collect();
        if !p_added.is_empty() || !p_removed.is_empty() || !defaults.is_empty() {
            settings.push(json!({ "model": old_m["name"], "added": p_added, "removed": p_removed, "defaults": defaults }));
        }
    }
    // parts too: a ZIP of print files changes only its parts (Review 6, F-26). Changed: their files'
    // names or sizes differ
    let parts = |d: &Value| -> BTreeMap<String, Value> {
        d["parts"].as_array().into_iter().flatten().filter_map(|p| Some((p["id"].as_str()?.to_string(), p.clone()))).collect()
    };
    let (pa, pb) = (parts(&a), parts(&b));
    let files = |p: &Value| -> Vec<(Value, Value)> { p["files"].as_array().into_iter().flatten().map(|f| (f["path"].clone(), f["bytes"].clone())).collect() };
    let parts_added: Vec<Value> = pb.iter().filter(|(k, _)| !pa.contains_key(*k)).map(|(_, p)| p["name"].clone()).collect();
    let parts_removed: Vec<Value> = pa.iter().filter(|(k, _)| !pb.contains_key(*k)).map(|(_, p)| p["name"].clone()).collect();
    let parts_changed: Vec<Value> = pa.iter().filter(|(k, p)| pb.get(*k).is_some_and(|q| files(p) != files(q))).map(|(_, p)| p["name"].clone()).collect();
    let problems_new = b["problems"].as_array().map(|p| p.len()).unwrap_or(0);
    json!({ "added": added, "removed": removed, "settings": settings, "problems": problems_new,
        "parts_added": parts_added, "parts_removed": parts_removed, "parts_changed": parts_changed })
}

fn decode_data_url(s: &str) -> Result<Vec<u8>> {
    let b64 = s.split_once(',').map(|(_, d)| d).unwrap_or(s);
    base64_decode(b64)
}

/// Standard base64 (no external crate).
pub fn base64_decode(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => bail!("invalid base64"),
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64() {
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert_eq!(decode_data_url("data:image/webp;base64,AAEC").unwrap(), vec![0, 1, 2]);
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("x%2"), "x%2");
    }
}
