//! Native renders: one OpenSCAD process per job, a limited number at once,
//! cancellable, with finished results cached on disk by a hash of everything
//! that affects the output.

use crate::{engine::NativeEngine, site::SiteDir};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, Notify, Semaphore};

/// What to render. `files` maps the model's virtual paths ("/vendor/x/y.scad",
/// "/libraries/BOSL/...", "/fonts/...") to content hashes in the site's `fs/`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RenderRequest {
    #[serde(default)]
    pub model: String,
    pub entry: String,
    pub files: BTreeMap<String, String>,
    pub values: BTreeMap<String, Value>,
    /// Settings passed with -D instead of the parameter file (values the file computes).
    #[serde(default)]
    pub defines: Vec<String>,
    /// A component (a library module): the entry file is written for this render,
    /// calling the module with `values` (see [`crate::components::call_source`]).
    #[serde(default)]
    pub call: Option<crate::components::Call>,
    /// "3mf": OpenSCAD's own 3MF, each top-level object apart (lazy union) and `color()` kept as
    /// materials (0.4.19); "svg" or "dxf": a 2D model's drawing (0.4.23); otherwise binary STL.
    #[serde(default)]
    pub format: Option<String>,
    /// "mitcad": a Fusion 360 file, generated with MitCad (experimental, 0.4.27); otherwise OpenSCAD.
    #[serde(default)]
    pub engine: Option<String>,
}

impl RenderRequest {
    /// A Fusion 360 file, generated with MitCad rather than OpenSCAD (an `.f3d` entry counts as asked).
    pub fn mitcad(&self) -> bool {
        self.engine.as_deref() == Some("mitcad") || self.entry.to_lowercase().ends_with(".f3d")
    }

    pub fn three_mf(&self) -> bool {
        self.format.as_deref() == Some("3mf")
    }

    /// The file type this render makes: "stl", "3mf", "svg" or "dxf".
    pub fn ext(&self) -> &str {
        match self.format.as_deref() {
            Some(f @ ("3mf" | "svg" | "dxf")) => f,
            _ => "stl",
        }
    }

    /// A drawing (a 2D model's SVG or DXF), not a solid.
    pub fn flat(&self) -> bool {
        matches!(self.ext(), "svg" | "dxf")
    }

    /// The same render as an SVG drawing: what a model that turns out to be 2D shows.
    fn as_svg(&self) -> Self {
        Self { format: Some("svg".into()), ..self.clone() }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RenderEvent {
    Stage { stage: String },
    Log { line: String },
}

#[derive(Debug)]
pub struct RenderOutput {
    /// The file: STL, or what [`RenderRequest::ext`] says (`format`).
    pub stl: Vec<u8>,
    /// Its type: "stl", "3mf", "svg" or "dxf" (a 2D model asked for as STL comes back as "svg").
    pub format: String,
    /// Total time for this render (preparing files + OpenSCAD), not counting queueing.
    pub ms: u64,
    /// Part of `ms` spent laying out the model's files (only on a model's first render).
    pub prepare_ms: u64,
    pub cached: bool,
    pub logs: Vec<String>,
    /// The model came out, but not whole: what to tell the user (MitCad's features that failed
    /// with these settings, 0.4.29). Kept with a cached render.
    pub warning: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RenderFailure {
    pub message: String,
    pub cancelled: bool,
    pub logs: Vec<String>,
}

impl std::fmt::Display for RenderFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for RenderFailure {}

impl RenderFailure {
    pub(crate) fn failed(message: impl Into<String>, logs: Vec<String>) -> Self {
        Self { message: message.into(), cancelled: false, logs }
    }
    pub(crate) fn cancelled(logs: Vec<String>) -> Self {
        Self { message: "Render cancelled.".into(), cancelled: true, logs }
    }
}

#[derive(Serialize, Deserialize)]
struct CacheMeta {
    ms: u64,
    engine: String,
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

const MAX_LOG_LINES: usize = 400;

pub struct Renderer {
    engine: NativeEngine,
    site: SiteDir,
    /// Content-addressed files that aren't in the site's fs/: the open library's
    /// project files, by SHA-256 (see [`Renderer::add_blobs`]).
    blobs: Arc<std::sync::RwLock<HashMap<String, PathBuf>>>,
    cache_dir: PathBuf,
    slots: Arc<Semaphore>,
    concurrency: usize,
    jobs: Mutex<HashMap<String, Arc<Notify>>>,
    /// Render cache size limit in bytes; oldest results are removed beyond it.
    pub cache_limit: u64,
    /// Reuse and store finished renders (off for benchmarks).
    pub use_cache: bool,
    /// Extra environment for OpenSCAD (Windows: a fontconfig setup with a persistent cache).
    env: Vec<(String, std::ffi::OsString)>,
    /// MitCad's command-line tool for Fusion 360 files, when found (experimental, 0.4.27).
    mitcad: std::sync::RwLock<Result<crate::mitcad::MitCad, String>>,
}

/// Default number of renders at once: all cores but one, at least one.
pub fn default_concurrency() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).saturating_sub(1).max(1)
}

fn rel_path(virtual_path: &str) -> Result<PathBuf> {
    let rel = Path::new(virtual_path.trim_start_matches('/'));
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("invalid model file path {virtual_path}");
    }
    Ok(rel.to_path_buf())
}

fn sha_hex(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u64).to_le_bytes());
        h.update(p);
    }
    hex::encode(h.finalize())
}

/// Number of triangles in a binary STL.
pub fn stl_triangles(stl: &[u8]) -> Option<u32> {
    (stl.len() >= 84).then(|| u32::from_le_bytes([stl[80], stl[81], stl[82], stl[83]]))
}

/// OpenSCAD parameter-set file: strings unquoted, everything else as JSON text
/// (mirrors web/render-worker.js).
fn parameter_set(values: &BTreeMap<String, Value>, skip: &[String]) -> String {
    let set: serde_json::Map<String, Value> = values
        .iter()
        .filter(|(k, _)| !skip.contains(k))
        .map(|(k, v)| {
            let s = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.clone(), Value::String(s))
        })
        .collect();
    serde_json::json!({ "fileFormatVersion": "1", "parameterSets": { "site": set } }).to_string()
}

fn stage_for(line: &str) -> Option<&'static str> {
    if line.starts_with("Parsing design") {
        Some("Parsing…")
    } else if line.starts_with("Compiling design") {
        Some("Compiling…")
    } else if line.starts_with("Rendering") || line.contains("Rendering Polygon Mesh") {
        Some("Rendering…")
    } else {
        None
    }
}

impl Renderer {
    pub fn new(engine: NativeEngine, site: SiteDir, cache_dir: PathBuf, concurrency: usize) -> Result<Self> {
        // OpenSCAD runs with the model's folder as working directory: paths must be absolute
        let cache_dir = std::path::absolute(&cache_dir).unwrap_or(cache_dir);
        let site = SiteDir::new(std::path::absolute(site.root()).unwrap_or_else(|_| site.root().to_path_buf()))?;
        for d in ["renders", "trees", "jobs"] {
            std::fs::create_dir_all(cache_dir.join(d))
                .with_context(|| format!("couldn't create {}", cache_dir.join(d).display()))?;
        }
        let concurrency = concurrency.max(1);
        let env = font_env(&engine, &cache_dir);
        Ok(Self {
            engine,
            site,
            blobs: Arc::default(),
            cache_dir,
            slots: Arc::new(Semaphore::new(concurrency)),
            concurrency,
            jobs: Mutex::new(HashMap::new()),
            cache_limit: 1 << 30,
            use_cache: true,
            env,
            mitcad: std::sync::RwLock::new(Err("MitCad hasn't been looked for.".into())),
        })
    }

    /// Where MitCad is (or why not), for Fusion 360 files; set from the app's configuration.
    pub fn set_mitcad(&self, found: Result<crate::mitcad::MitCad, String>) {
        *self.mitcad.write().unwrap() = found;
    }
    pub fn mitcad(&self) -> Result<crate::mitcad::MitCad, String> {
        self.mitcad.read().unwrap().clone()
    }

    pub fn engine(&self) -> &NativeEngine {
        &self.engine
    }
    pub fn site(&self) -> &SiteDir {
        &self.site
    }
    pub fn concurrency(&self) -> usize {
        self.concurrency
    }
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Make more files renderable: {sha256: path on disk} (a library's project files).
    pub fn add_blobs(&self, blobs: impl IntoIterator<Item = (String, PathBuf)>) {
        self.blobs.write().unwrap().extend(blobs);
    }

    /// Forget files added with [`Renderer::add_blobs`] (another library was opened).
    pub fn clear_blobs(&self) {
        self.blobs.write().unwrap().clear();
    }

    fn resolver(&self) -> BlobResolver {
        BlobResolver { site: self.site.clone(), extra: self.blobs.clone() }
    }

    /// OpenSCAD's Customizer description of a model (`--export-format=param`):
    /// its settings, groups, defaults and choices. Cached like renders.
    pub async fn export_params(&self, req: &RenderRequest) -> Result<Value, RenderFailure> {
        let key = format!("{}-params", self.cache_key(&RenderRequest { values: BTreeMap::new(), defines: vec![], format: None, ..req.clone() }));
        let cached = self.cache_dir.join("renders").join(format!("{key}.json"));
        if self.use_cache {
            if let Some(v) = std::fs::read(&cached).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
                return Ok(v);
            }
        }
        let _permit = self.slots.clone().acquire_owned().await.map_err(|_| RenderFailure::failed("The render queue was closed.", vec![]))?;
        let tree = {
            let files = req.files.clone();
            let blobs = self.resolver();
            let trees = self.cache_dir.join("trees");
            tokio::task::spawn_blocking(move || ensure_tree(&blobs, &trees, &files))
                .await
                .map_err(|e| RenderFailure::failed(format!("Couldn't prepare the model files: {e}"), vec![]))?
                .map_err(|e| RenderFailure::failed(format!("Couldn't prepare the model files: {e:#}"), vec![]))?
        };
        let entry = rel_path(&req.entry).map_err(|e| RenderFailure::failed(e.to_string(), vec![]))?;
        // (one folder per call: two reads of the same file at once mustn't share one, or the first to
        // finish removes the other's)
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let job_dir = self.cache_dir.join("jobs").join(format!("params-{}-{}-{n}", &key[..16], std::process::id()));
        let _cleanup = RemoveOnDrop(job_dir.clone());
        std::fs::create_dir_all(&job_dir).map_err(|e| RenderFailure::failed(format!("Couldn't create a job folder: {e}"), vec![]))?;
        let target = job_dir.join("params.json");
        let mut cmd = self.engine.command();
        cmd.arg(tree.join(&entry))
            .args(["--export-format=param", "-o"])
            .arg(&target)
            .current_dir(&tree)
            .env("OPENSCADPATH", tree.join("libraries"))
            .env("OPENSCAD_FONT_PATH", tree.join("fonts"))
            .envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_os_str())))
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let out = tokio::time::timeout(std::time::Duration::from_secs(600), cmd.output())
            .await
            .map_err(|_| RenderFailure::failed("Reading the settings took over 10 minutes.", vec![]))?
            .map_err(|e| RenderFailure::failed(format!("Couldn't start OpenSCAD ({}): {e}", self.engine.exe.display()), vec![]))?;
        let logs: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .chain(String::from_utf8_lossy(&out.stderr).lines())
            .map(String::from)
            .collect();
        let bytes = std::fs::read(&target).map_err(|_| {
            let first = logs.iter().find(|l| l.starts_with("ERROR:")).cloned();
            RenderFailure::failed(first.unwrap_or_else(|| "OpenSCAD couldn't read this file's settings.".into()), logs.clone())
        })?;
        let v: Value = serde_json::from_slice(&bytes)
            .map_err(|e| RenderFailure::failed(format!("OpenSCAD's settings export wasn't valid JSON: {e}"), logs.clone()))?;
        if self.use_cache {
            let _ = std::fs::write(&cached, &bytes);
        }
        Ok(v)
    }

    /// Everything that changes the output: engine version, entry, every input file, values, defines.
    pub fn cache_key(&self, req: &RenderRequest) -> String {
        let files = serde_json::to_vec(&req.files).unwrap_or_default();
        let values = serde_json::to_vec(&req.values).unwrap_or_default();
        let mut defines = req.defines.clone();
        defines.sort();
        let defines = defines.join("\n");
        let call = req.call.as_ref().map(|c| serde_json::to_vec(c).unwrap_or_default()).unwrap_or_default();
        // (a Fusion 360 file's render depends on MitCad's build, not OpenSCAD's)
        let engine = if req.mitcad() { self.mitcad().map(|m| m.tag).unwrap_or_else(|_| "mitcad".into()) } else { self.engine.version.clone() };
        let key = sha_hex(&[engine.as_bytes(), req.entry.as_bytes(), &files, &values, defines.as_bytes(), &call]);
        // (a 3MF or a drawing is cached beside the STL of the same render, by its own key)
        if req.ext() == "stl" { key } else { format!("{key}-{}", req.ext()) }
    }

    /// Ask a running or queued job to stop. Returns false if it isn't known (already finished).
    pub fn cancel(&self, job: &str) -> bool {
        match self.jobs.lock().unwrap().get(job) {
            Some(n) => {
                n.notify_one();
                true
            }
            None => false,
        }
    }

    /// Look up a finished render without starting one.
    pub fn cached(&self, req: &RenderRequest) -> Option<RenderOutput> {
        let key = self.cache_key(req);
        let stl_path = self.cache_dir.join("renders").join(format!("{key}.{}", req.ext()));
        let stl = std::fs::read(&stl_path).ok()?;
        if let Ok(f) = std::fs::File::options().append(true).open(&stl_path) {
            let _ = f.set_modified(SystemTime::now()); // keep recently used results when pruning
        }
        let meta: Option<CacheMeta> = std::fs::read(self.cache_dir.join("renders").join(format!("{key}.json")))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let (ms, warning) = meta.map(|m| (m.ms, m.warning)).unwrap_or((0, None));
        Some(RenderOutput { stl, format: req.ext().into(), ms, prepare_ms: 0, cached: true, logs: vec![], warning })
    }

    pub async fn render<F>(&self, job: &str, req: &RenderRequest, on_event: F) -> Result<RenderOutput, RenderFailure>
    where
        F: Fn(RenderEvent) + Send + Sync,
    {
        // a model that turned out to be 2D (below) is found as its drawing next time
        let may_be_flat = req.call.is_none() && !req.flat() && !req.mitcad();
        if self.use_cache {
            if let Some(hit) = self.cached(req).or_else(|| may_be_flat.then(|| self.cached(&req.as_svg())).flatten()) {
                return Ok(hit);
            }
        }
        let cancel = Arc::new(Notify::new());
        self.jobs.lock().unwrap().insert(job.to_string(), cancel.clone());
        let mut result = self.render_uncached(job, req, &on_event, &cancel).await;
        // a component that turned out to be a 2D shape: show it extruded (and a 3D one
        // the docs drew flat: without the extrusion)
        if let (Err(e), Some(call)) = (&result, &req.call) {
            let said = |t: &str| e.logs.iter().any(|l| l.contains(t)) || e.message.contains(t);
            let retry = if e.cancelled {
                None
            } else if call.extrude.is_none() && (said("not a 3D object") || said("is a 2D object")) {
                on_event(RenderEvent::Log { line: "This is a 2D shape: extruding it 1 mm to show it.".into() });
                Some(crate::components::Call { extrude: Some(crate::components::EXTRUDE.into()), ..call.clone() })
            } else if call.extrude.is_some() && said("object is empty") {
                Some(crate::components::Call { extrude: None, ..call.clone() })
            } else {
                None
            };
            if let Some(c) = retry {
                let mut again = req.clone();
                again.call = Some(c);
                result = self.render_uncached(job, &again, &on_event, &cancel).await;
            }
        }
        // a model that makes a 2D shape (for a laser cutter, a plotter, a sign): its drawing, as SVG (0.4.23)
        if let Err(e) = &result {
            let said = |t: &str| e.logs.iter().any(|l| l.contains(t)) || e.message.contains(t);
            if may_be_flat && !e.cancelled && (said("not a 3D object") || said("is a 2D object")) {
                on_event(RenderEvent::Log { line: "This model makes a 2D shape: drawing it as SVG.".into() });
                result = self.render_uncached(job, &req.as_svg(), &on_event, &cancel).await;
            }
        }
        self.jobs.lock().unwrap().remove(job);
        result
    }

    async fn render_uncached<F>(&self, job: &str, req: &RenderRequest, on_event: &F, cancel: &Notify) -> Result<RenderOutput, RenderFailure>
    where
        F: Fn(RenderEvent) + Send + Sync,
    {
        // wait for a free engine slot (batches queue here)
        let _permit = match self.slots.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                on_event(RenderEvent::Stage { stage: "Waiting for a free engine…".into() });
                tokio::select! {
                    p = self.slots.clone().acquire_owned() => p.map_err(|_| RenderFailure::failed("The render queue was closed.", vec![]))?,
                    _ = cancel.notified() => return Err(RenderFailure::cancelled(vec![])),
                }
            }
        };

        let started = Instant::now(); // render time, not counting the wait for a slot
        on_event(RenderEvent::Stage { stage: "Preparing model files…".into() });
        let tree = {
            let files = req.files.clone();
            let blobs = self.resolver();
            let trees = self.cache_dir.join("trees");
            tokio::task::spawn_blocking(move || ensure_tree(&blobs, &trees, &files))
                .await
                .map_err(|e| RenderFailure::failed(format!("Couldn't prepare the model files: {e}"), vec![]))?
                .map_err(|e| RenderFailure::failed(format!("Couldn't prepare the model files: {e:#}"), vec![]))?
        };
        let prepare_ms = started.elapsed().as_millis() as u64;
        let entry = rel_path(&req.entry).map_err(|e| RenderFailure::failed(e.to_string(), vec![]))?;

        let safe_job: String = job.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(40).collect();
        let job_dir = self.cache_dir.join("jobs").join(format!("{}-{}", safe_job, std::process::id()));
        let _cleanup = RemoveOnDrop(job_dir.clone());
        std::fs::create_dir_all(&job_dir).map_err(|e| RenderFailure::failed(format!("Couldn't create a job folder: {e}"), vec![]))?;
        if req.mitcad() {
            return self.render_mitcad(req, &tree, &entry, &job_dir, started, prepare_ms, on_event, cancel).await;
        }
        let params = job_dir.join("params.json");
        let out = job_dir.join(format!("out.{}", req.ext()));
        let mut cmd = self.engine.command();
        if let Some(call) = &req.call {
            // a component: this render's own file (includes the library, calls the module)
            let source = crate::components::call_source(call, &req.values).map_err(|e| RenderFailure::failed(format!("{e:#}"), vec![]))?;
            let file = job_dir.join("component.scad");
            std::fs::write(&file, source).map_err(|e| RenderFailure::failed(format!("Couldn't write the component file: {e}"), vec![]))?;
            cmd.arg(&file);
        } else {
            std::fs::write(&params, parameter_set(&req.values, &req.defines))
                .map_err(|e| RenderFailure::failed(format!("Couldn't write settings: {e}"), vec![]))?;
            cmd.arg(tree.join(&entry));
            for d in &req.defines {
                if let Some(v) = req.values.get(d) {
                    cmd.arg("-D").arg(format!("{d}={v}"));
                }
            }
            cmd.args(["-P", "site"]).arg("-p").arg(&params);
        }
        match req.ext() {
            "3mf" => cmd.args(["--enable=lazy-union", "--export-format=3mf"]),
            "svg" => cmd.arg("--export-format=svg"),
            "dxf" => cmd.arg("--export-format=dxf"),
            _ => cmd.arg("--export-format=binstl"),
        };
        cmd.arg("--backend=Manifold")
            .arg("-o")
            .arg(&out)
            .current_dir(&tree)
            .env("OPENSCADPATH", tree.join("libraries"))
            .env("OPENSCAD_FONT_PATH", tree.join("fonts"))
            .envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_os_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        on_event(RenderEvent::Stage { stage: "Starting OpenSCAD…".into() });
        let mut child = cmd
            .spawn()
            .map_err(|e| RenderFailure::failed(format!("Couldn't start OpenSCAD ({}): {e}", self.engine.exe.display()), vec![]))?;
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
                       child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)]
            .into_iter()
            .flatten()
        {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        let mut logs: Vec<String> = vec![];
        let push = |line: String, logs: &mut Vec<String>| {
            if let Some(s) = stage_for(&line) {
                on_event(RenderEvent::Stage { stage: s.into() });
            }
            on_event(RenderEvent::Log { line: line.clone() });
            logs.push(line);
            if logs.len() > MAX_LOG_LINES {
                logs.remove(0);
            }
        };
        let mut status = None;
        let mut streams_open = true;
        while status.is_none() || streams_open {
            tokio::select! {
                line = rx.recv(), if streams_open => match line {
                    Some(l) => push(l, &mut logs),
                    None => streams_open = false,
                },
                s = child.wait(), if status.is_none() => {
                    status = Some(s.map_err(|e| RenderFailure::failed(format!("OpenSCAD failed: {e}"), vec![]))?);
                }
                _ = cancel.notified() => {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    return Err(RenderFailure::cancelled(logs));
                }
            }
        }
        let status = status.unwrap();
        let ms = started.elapsed().as_millis() as u64;
        let first_error = logs.iter().find(|l| l.starts_with("ERROR:")).cloned();
        let stl = std::fs::read(&out).unwrap_or_default();
        // (an STL's header alone is 84 bytes; a drawing has no header to speak of)
        let empty = if req.flat() { stl.is_empty() } else { stl.len() <= 84 };
        if !status.success() || first_error.is_some() || empty {
            let msg = first_error.unwrap_or_else(|| match status.code() {
                Some(0) | None if empty => "OpenSCAD produced no geometry. These settings may not be valid for this model.".into(),
                Some(c) => format!("OpenSCAD stopped with code {c}."),
                None => "OpenSCAD stopped unexpectedly.".into(),
            });
            return Err(RenderFailure::failed(msg, logs));
        }

        if !self.use_cache {
            return Ok(RenderOutput { stl, format: req.ext().into(), ms, prepare_ms, cached: false, logs, warning: None });
        }
        let key = self.cache_key(req);
        let renders = self.cache_dir.join("renders");
        let tmp = renders.join(format!("{key}.tmp-{safe_job}"));
        if std::fs::write(&tmp, &stl).is_ok() && std::fs::rename(&tmp, renders.join(format!("{key}.{}", req.ext()))).is_ok() {
            let meta = CacheMeta { ms, engine: self.engine.version.clone(), model: req.model.clone(), warning: None };
            let _ = std::fs::write(renders.join(format!("{key}.json")), serde_json::to_vec(&meta).unwrap_or_default());
            let _ = self.prune();
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
        Ok(RenderOutput { stl, format: req.ext().into(), ms, prepare_ms, cached: false, logs, warning: None })
    }

    /// A Fusion 360 file through MitCad (experimental, 0.4.27): 3MF when asked for, else STL; cached as
    /// OpenSCAD's renders are.
    #[allow(clippy::too_many_arguments)]
    async fn render_mitcad<F>(&self, req: &RenderRequest, tree: &Path, entry: &Path, job_dir: &Path, started: Instant, prepare_ms: u64, on_event: &F, cancel: &Notify) -> Result<RenderOutput, RenderFailure>
    where
        F: Fn(RenderEvent) + Send + Sync,
    {
        let tool = self.mitcad().map_err(|e| RenderFailure::failed(e, vec![]))?;
        let sha = req.files.get(&req.entry).or_else(|| req.files.get(&format!("/{}", req.entry.trim_start_matches('/')))).cloned()
            .ok_or_else(|| RenderFailure::failed("The Fusion file isn't among the model's files.", vec![]))?;
        let ext = if req.ext() == "3mf" { "3mf" } else { "stl" };
        let file = tree.join(entry);
        let job = crate::mitcad::Job { tool: &tool, cache_dir: &self.cache_dir, job_dir, file: &file, sha: &sha, values: &req.values, ext };
        let (stl, logs, warning) = crate::mitcad::generate(&job, on_event, cancel).await?;
        let ms = started.elapsed().as_millis() as u64;
        if self.use_cache {
            let key = self.cache_key(req);
            let renders = self.cache_dir.join("renders");
            let tmp = renders.join(format!("{key}.tmp-mitcad-{}", std::process::id()));
            if std::fs::write(&tmp, &stl).is_ok() && std::fs::rename(&tmp, renders.join(format!("{key}.{ext}"))).is_ok() {
                let meta = CacheMeta { ms, engine: tool.tag.clone(), model: req.model.clone(), warning: warning.clone() };
                let _ = std::fs::write(renders.join(format!("{key}.json")), serde_json::to_vec(&meta).unwrap_or_default());
                let _ = self.prune();
            } else {
                let _ = std::fs::remove_file(&tmp);
            }
        }
        Ok(RenderOutput { stl, format: ext.into(), ms, prepare_ms, cached: false, logs, warning })
    }

    /// Total size of cached renders in bytes.
    pub fn cache_size(&self) -> u64 {
        std::fs::read_dir(self.cache_dir.join("renders"))
            .map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
            .unwrap_or(0)
    }

    /// Remove the oldest cached renders until the cache is below 80% of its limit.
    pub fn prune(&self) -> Result<()> {
        let dir = self.cache_dir.join("renders");
        let mut items: Vec<(SystemTime, u64, PathBuf)> = std::fs::read_dir(&dir)?
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "stl" || x == "3mf" || x == "svg" || x == "dxf"))
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                Some((m.modified().ok()?, m.len(), e.path()))
            })
            .collect();
        let mut total: u64 = items.iter().map(|i| i.1).sum();
        if total <= self.cache_limit {
            return Ok(());
        }
        items.sort();
        for (_, len, path) in items {
            if total <= self.cache_limit / 10 * 8 {
                break;
            }
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(path.with_extension("json"));
            total = total.saturating_sub(len);
        }
        Ok(())
    }

    /// Delete cached renders and prepared model folders. Returns bytes freed.
    pub fn clear_cache(&self) -> Result<u64> {
        let before = self.cache_size();
        for d in ["renders", "trees"] {
            let p = self.cache_dir.join(d);
            if p.exists() {
                std::fs::remove_dir_all(&p)?;
            }
            std::fs::create_dir_all(&p)?;
        }
        Ok(before)
    }
}

/// The Windows OpenSCAD snapshot ships no fontconfig setup, so only the fonts we
/// pass in are found and nothing is cached. Give it one: Windows' fonts, its own
/// bundled fonts, and a cache folder in the workspace (scanned once, then reused).
fn font_env(engine: &NativeEngine, cache_dir: &Path) -> Vec<(String, std::ffi::OsString)> {
    if !cfg!(windows) {
        return vec![]; // Linux: the system's fontconfig setup and cache
    }
    let fc = cache_dir.join("fontconfig");
    if std::fs::create_dir_all(&fc).is_err() {
        return vec![];
    }
    let esc = |p: &Path| p.display().to_string().replace('&', "&amp;").replace('<', "&lt;");
    let bundled = engine.exe.parent().map(|d| d.join("fonts"));
    let conf = format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"fonts.dtd\">\n<fontconfig>\n  <dir>WINDOWSFONTDIR</dir>\n{}  <cachedir>{}</cachedir>\n</fontconfig>\n",
        bundled.filter(|d| d.is_dir()).map(|d| format!("  <dir>{}</dir>\n", esc(&d))).unwrap_or_default(),
        esc(&fc.join("cache")),
    );
    let file = fc.join("fonts.conf");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(conf.as_str()) && std::fs::write(&file, &conf).is_err() {
        return vec![];
    }
    vec![("FONTCONFIG_FILE".into(), file.into_os_string())]
}

struct RemoveOnDrop(PathBuf);
impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Finds content-addressed files: the site's fs/ first, then the library's files.
#[derive(Clone)]
struct BlobResolver {
    site: SiteDir,
    extra: Arc<std::sync::RwLock<HashMap<String, PathBuf>>>,
}

impl BlobResolver {
    fn blob(&self, sha: &str) -> Result<PathBuf> {
        let p = self.site.blob(sha)?;
        if p.is_file() {
            return Ok(p);
        }
        match self.extra.read().unwrap().get(sha) {
            Some(p) => Ok(p.clone()),
            None => bail!("file {} isn't in the app or the library", &sha[..12.min(sha.len())]),
        }
    }
}

/// Lay a model's files out as a real folder tree (once per distinct file set) so
/// native OpenSCAD can follow its includes. Returns the tree's root.
fn ensure_tree(site: &BlobResolver, trees: &Path, files: &BTreeMap<String, String>) -> Result<PathBuf> {
    let listing: String = files.iter().map(|(p, s)| format!("{p}\0{s}\n")).collect();
    let key = &sha_hex(&[listing.as_bytes()])[..24];
    let root = trees.join(key);
    if root.join(".complete").is_file() {
        return Ok(root);
    }
    let tmp = trees.join(format!("{key}.tmp-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
    let _ = std::fs::remove_dir_all(&tmp);
    for (vpath, sha) in files {
        let dst = tmp.join(rel_path(vpath)?);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let src = site.blob(sha)?;
        if vpath.ends_with(".scad") {
            // Windows line endings stop OpenSCAD's Customizer reading dropdowns (library files arrive as they are)
            let data = std::fs::read(&src).with_context(|| format!("reading {vpath}"))?;
            let fixed = if data.contains(&b'\r') { crate::sitebuild::replace_crlf(&data) } else { data };
            std::fs::write(&dst, fixed).with_context(|| format!("copying {vpath}"))?;
        } else {
            std::fs::copy(&src, &dst).with_context(|| format!("copying {vpath}"))?;
        }
    }
    std::fs::write(tmp.join(".complete"), b"")?;
    match std::fs::rename(&tmp, &root) {
        Ok(()) => Ok(root),
        Err(_) if root.join(".complete").is_file() => {
            // another render prepared the same tree at the same time
            let _ = std::fs::remove_dir_all(&tmp);
            Ok(root)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&root);
            std::fs::rename(&tmp, &root).with_context(|| format!("preparing model folder: {e}"))?;
            Ok(root)
        }
    }
}

/// Default settings for a built model (parameters' defaults plus its fixed values),
/// with the names that go through -D. Used by the CLI and tests.
pub fn default_request(model: &Value, common_files: &Value) -> Result<RenderRequest> {
    let mut files: BTreeMap<String, String> = serde_json::from_value(common_files.clone()).unwrap_or_default();
    let model_files: BTreeMap<String, String> =
        serde_json::from_value(model["files"].clone()).context("model has no files")?;
    files.extend(model_files);
    let mut values = BTreeMap::new();
    let mut defines = vec![];
    for p in model["parameters"].as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or_default().to_string();
        values.insert(name.clone(), p["default"].clone());
        if p["define"].as_bool() == Some(true) {
            defines.push(name);
        }
    }
    if let Some(fixed) = model["fixed"].as_object() {
        for (k, v) in fixed {
            values.insert(k.clone(), v.clone());
        }
    }
    Ok(RenderRequest {
        model: model["key"].as_str().unwrap_or_default().to_string(),
        entry: model["entry"].as_str().context("model has no entry")?.to_string(),
        files,
        values,
        defines,
        call: serde_json::from_value(model["call"].clone()).ok(),
        format: None,
        engine: model["engine"].as_str().map(String::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_set_matches_the_browser_worker() {
        let mut v = BTreeMap::new();
        v.insert("text".to_string(), json_str("M3"));
        v.insert("size".to_string(), serde_json::json!([1, 2.5]));
        v.insert("on".to_string(), serde_json::json!(true));
        v.insert("computed".to_string(), serde_json::json!(4));
        let s: Value = serde_json::from_str(&parameter_set(&v, &["computed".to_string()])).unwrap();
        let set = &s["parameterSets"]["site"];
        assert_eq!(set["text"], "M3");
        assert_eq!(set["size"], "[1,2.5]");
        assert_eq!(set["on"], "true");
        assert!(set.get("computed").is_none());
    }

    fn json_str(s: &str) -> Value {
        Value::String(s.to_string())
    }

    #[test]
    fn model_paths_stay_inside_the_tree() {
        assert!(rel_path("/vendor/x/y.scad").is_ok());
        assert!(rel_path("/../etc/passwd").is_err());
        assert!(rel_path("/a/../../b").is_err());
        assert!(rel_path("").is_err());
    }

    #[test]
    fn stages_from_openscad_output() {
        assert_eq!(stage_for("Parsing design (AST generation)..."), Some("Parsing…"));
        assert_eq!(stage_for("Rendering Polygon Mesh using Manifold..."), Some("Rendering…"));
        assert_eq!(stage_for("ECHO: 1"), None);
    }

    #[test]
    fn triangle_count() {
        let mut stl = vec![0u8; 84];
        stl[80..84].copy_from_slice(&12u32.to_le_bytes());
        assert_eq!(stl_triangles(&stl), Some(12));
        assert_eq!(stl_triangles(&[0u8; 10]), None);
    }
}
