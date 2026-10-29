//! Command-line access to the desktop app's render path (same code, no window).
//!
//!   workshop-cli version --engine <dir|exe>
//!   workshop-cli render  --site _site --engine <dir|exe> --model family/model --out part.stl [--set name=<json>]...
//!   workshop-cli bench   --site _site --engine <dir|exe> [--out bench-native.json] [--jobs N] [--timeout S] [keys...]
//!   workshop-cli site-prepare --repo . --out _site [--desktop] [--public] [--hide-blocked]
//!   workshop-cli site-finish  --repo . --out _site --params raw.json --engine-version V [--native-engine <dir|exe>]
//!   workshop-cli bundle  --site <built site> --out <starter dir>
//!   workshop-cli libs-index --libs <dir>        (components of the bundled libraries, written to <dir>/index/)
//!   workshop-cli components --libs <dir> [--library NAME] [--json]   (what the libraries' modules become)
//!   workshop-cli components-check --libs <dir> --app-site <dir> --engine <dir|exe> [--library NAME] [--jobs N] [--out f.json] [modules...]
//!       (renders each component with its start values: the first doc example that is a plain call)
//!   workshop-cli bench   --app-site <dir> --starter <dir> --engine <dir|exe> [--out f.json]   (renders from a library)
//!   workshop-cli library-summary --library <dir> --app-site <dir> [--starter <dir>]
//!   workshop-cli library-call --library <dir> --app-site <dir> --engine <dir|exe> <command> '<json args>'   (waits for jobs)
//!   workshop-cli serve   --ui <dir> --app-site <dir> --engine <dir|exe> --home <dir> [--starter <dir>] [--port 8790]
//!
//! `bench` renders every model in the site (including desktop-only ones) with
//! default settings and a fresh cache, like `tools/engine/cli.mjs bench` does for
//! the WebAssembly engine, and exits non-zero if any fails.
//!
//! `site-prepare` and `site-finish` are the catalog steps of tools/build_site.py:
//! the same project reading (files, settings, metadata) as the app's library ingest.
//! `bundle` packages a built site as the starter library the app ships with.
//! `serve` runs the app's commands behind a local web server, so the page can be
//! tested in an ordinary browser (with a small stand-in for Tauri's `invoke`).

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use workshop_core::render::{default_request, stl_triangles};
use workshop_core::{NativeEngine, Renderer, SiteDir};

struct Args {
    rest: Vec<String>,
}

impl Args {
    fn flag(&mut self, name: &str) -> Option<String> {
        let i = self.rest.iter().position(|a| a == name)?;
        if i + 1 >= self.rest.len() {
            return None;
        }
        let v = self.rest.remove(i + 1);
        self.rest.remove(i);
        Some(v)
    }
    fn flags(&mut self, name: &str) -> Vec<String> {
        let mut v = vec![];
        while let Some(x) = self.flag(name) {
            v.push(x);
        }
        v
    }
    fn need(&mut self, name: &str) -> Result<String> {
        self.flag(name).with_context(|| format!("missing {name}"))
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e:#}");
        std::process::exit(2);
    }
}

async fn run() -> Result<()> {
    let mut all: Vec<String> = std::env::args().skip(1).collect();
    if all.is_empty() {
        bail!("usage: workshop-cli version|render|bench ...");
    }
    let cmd = all.remove(0);
    let mut a = Args { rest: all };
    match cmd.as_str() {
        "--version" | "-V" => {
            println!("workshop-cli {}", workshop_core::VERSION);
            return Ok(());
        }
        "site-prepare" => return site_prepare(a),
        "site-finish" => return site_finish(a).await,
        "bundle" => {
            let site = PathBuf::from(a.need("--site")?);
            let out = PathBuf::from(a.need("--out")?);
            let ids = workshop_core::project::bundle_site(&site, &out)?;
            eprintln!("starter library: {} projects in {}", ids.len(), out.display());
            return Ok(());
        }
        "libs-index" => {
            let libs = PathBuf::from(a.need("--libs")?);
            for (name, n) in workshop_core::components::write_bundled_indexes(&libs)? {
                eprintln!("{name}: {n} components");
            }
            return Ok(());
        }
        "components" => return components_list(a),
        "components-check" => return components_check(a).await,
        "library-summary" => return library_summary(a).await,
        "library-call" => return library_call(a).await,
        "serve" => return serve(a).await,
        "bench" if a.rest.iter().any(|x| x == "--starter") => return bench_library(a).await,
        _ => {}
    }
    let engine = NativeEngine::locate(&PathBuf::from(a.need("--engine")?)).await?;
    match cmd.as_str() {
        "version" => {
            println!("{} ({})", engine.version, engine.exe.display());
            Ok(())
        }
        "render" => {
            let site = SiteDir::new(a.need("--site")?)?;
            let key = a.need("--model")?;
            let out = PathBuf::from(a.need("--out")?);
            let cache = a.flag("--cache").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("workshop-cli-cache"));
            let sets = a.flags("--set");
            let model = site.model(&key)?;
            let mut req = default_request(&model, &site.catalog()?["common_files"])?;
            for s in sets {
                let (k, v) = s.split_once('=').context("--set needs name=value")?;
                let v: Value = serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.to_string()));
                req.values.insert(k.to_string(), v);
            }
            let r = Renderer::new(engine, site, cache, 1)?;
            let res = r
                .render("cli", &req, |ev| {
                    if let workshop_core::RenderEvent::Log { line } = ev {
                        eprintln!("{line}");
                    }
                })
                .await
                .map_err(|e| anyhow::anyhow!("{}", e.message))?;
            std::fs::write(&out, &res.stl)?;
            println!("{} triangles, {} ms{}", stl_triangles(&res.stl).unwrap_or(0), res.ms, if res.cached { " (cached)" } else { "" });
            Ok(())
        }
        "bench" => bench(engine, a).await,
        other => bail!("unknown command {other}"),
    }
}

async fn bench(engine: NativeEngine, mut a: Args) -> Result<()> {
    let site = SiteDir::new(a.need("--site")?)?;
    let out = a.flag("--out");
    let jobs: usize = a.flag("--jobs").map(|j| j.parse()).transpose()?.unwrap_or(1);
    let timeout = Duration::from_secs(a.flag("--timeout").map(|t| t.parse()).transpose()?.unwrap_or(900));
    // pass 1 is a first render (model files laid out fresh); pass 2 repeats it with the
    // files already prepared, like any later render in the app (the result cache is off)
    let passes: usize = a.flag("--passes").map(|t| t.parse()).transpose()?.unwrap_or(2).max(1);
    let cache = std::env::temp_dir().join(format!("workshop-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let catalog = site.catalog()?;
    let mut keys: Vec<String> = a.rest.clone();
    if keys.is_empty() {
        keys = catalog["models"].as_array().into_iter().flatten().filter_map(|m| m["key"].as_str().map(String::from)).collect();
    }
    let version = engine.version.clone();
    let mut r = Renderer::new(engine, site.clone(), cache.clone(), jobs)?;
    r.use_cache = false;
    let renderer = Arc::new(r);
    let common = catalog["common_files"].clone();

    let mut results: Vec<Value> = vec![];
    for pass in 0..passes {
        let mut handles = vec![];
        for key in &keys {
            let key = key.clone();
            let renderer = renderer.clone();
            let model = site.model(&key)?;
            let req = default_request(&model, &common)?;
            let desktop_only = model["browser"].as_bool() == Some(false);
            handles.push(tokio::spawn(async move {
                let t0 = Instant::now();
                let job = format!("bench-{pass}-{}", key.replace('/', "-"));
                let r = tokio::time::timeout(timeout, renderer.render(&job, &req, |_| {})).await;
                let secs = |ms: f64| (ms / 10.0).round() / 100.0;
                let (status, stl, logs, error, seconds, prepare) = match r {
                    Ok(Ok(o)) => ("pass", o.stl, o.logs, None, secs(o.ms as f64), secs(o.prepare_ms as f64)),
                    Ok(Err(e)) => ("fail", vec![], e.logs, Some(e.message), secs(t0.elapsed().as_millis() as f64), 0.0),
                    Err(_) => {
                        renderer.cancel(&job);
                        ("fail", vec![], vec![], Some(format!("timed out after {} s", timeout.as_secs())), timeout.as_secs_f64(), 0.0)
                    }
                };
                let warnings = logs.iter().filter(|l| l.starts_with("WARNING:")).count();
                let res = json!({
                    "id": key, "key": key, "status": status, "seconds": seconds, "prepare_seconds": prepare,
                    "bytes": stl.len(), "triangles": stl_triangles(&stl).unwrap_or(0), "warnings": warnings,
                    "desktop_only": desktop_only,
                    "error": error, "log_tail": if status == "pass" { Value::Null } else { json!(logs.iter().rev().take(15).rev().collect::<Vec<_>>()) },
                });
                eprintln!(
                    "{:<5} pass {} {:<44} {:>7}s (files {:>5}s)  {} tris{}",
                    status,
                    pass + 1,
                    key,
                    seconds,
                    prepare,
                    res["triangles"],
                    if desktop_only { "  (desktop only)" } else { "" }
                );
                res
            }));
        }
        let mut this_pass = vec![];
        for h in handles {
            this_pass.push(h.await?);
        }
        if pass == 0 {
            results = this_pass;
        } else {
            for (res, again) in results.iter_mut().zip(this_pass) {
                res["repeat_seconds"] = again["seconds"].clone();
                if again["status"] != "pass" {
                    res["status"] = again["status"].clone();
                    res["error"] = again["error"].clone();
                }
            }
        }
    }
    let failed = results.iter().filter(|r| r["status"] != "pass").count();
    let total = |k: &str| results.iter().filter_map(|r| r[k].as_f64()).sum::<f64>();
    let report = json!({
        "engine": version, "native": true, "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "jobs": jobs, "passes": passes,
        "total_seconds": (total("seconds") * 100.0).round() / 100.0,
        "total_repeat_seconds": (total("repeat_seconds") * 100.0).round() / 100.0,
        "results": results,
    });
    if let Some(out) = out {
        std::fs::write(&out, serde_json::to_string_pretty(&report)? + "\n")?;
    }
    let _ = std::fs::remove_dir_all(&cache);
    eprintln!(
        "{} of {} passed; total {} s first render, {} s repeated",
        results.len() - failed,
        results.len(),
        report["total_seconds"],
        report["total_repeat_seconds"]
    );
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// The components of the bundled libraries, as a list (or JSON model pages with --json).
fn components_list(mut a: Args) -> Result<()> {
    use workshop_core::components as wc;
    let libs = PathBuf::from(a.need("--libs")?);
    let only = a.flag("--library");
    let as_json = a.rest.iter().any(|x| x == "--json");
    let mut pages = vec![];
    for lib in wc::bundled(&libs) {
        if only.as_deref().is_some_and(|o| !o.eq_ignore_ascii_case(&lib.info.name)) {
            continue;
        }
        let comps = wc::components_of(&lib);
        let mut groups: std::collections::BTreeMap<String, usize> = Default::default();
        for c in &comps {
            *groups.entry(c.group.clone()).or_default() += 1;
            let m = wc::component_model(&lib, c);
            if as_json {
                pages.push(m);
            } else {
                let ex = c.examples.iter().position(|e| !e.calls.is_empty());
                println!("{:<14} {:<34} {:<12} {:>2} args  example:{:<6} {}", lib.info.name, c.module, c.group, c.args.len(),
                    ex.map(|i| if i == 0 { "first".to_string() } else { format!("#{}", i + 1) }).unwrap_or_else(|| "-".into()), c.synopsis.chars().take(60).collect::<String>());
            }
        }
        eprintln!("{}: {} components {:?}; problems: {}", lib.info.name, comps.len(), groups, lib.index["problems"]);
    }
    if as_json {
        println!("{}", serde_json::to_string_pretty(&pages)?);
    }
    Ok(())
}

/// Render every component (of one library, or all) with its start values.
async fn components_check(mut a: Args) -> Result<()> {
    use workshop_core::components as wc;
    let libs = PathBuf::from(a.need("--libs")?);
    let site = SiteDir::new(a.need("--app-site")?)?;
    let engine = NativeEngine::locate(&PathBuf::from(a.need("--engine")?)).await?;
    let only = a.flag("--library");
    let out = a.flag("--out");
    let jobs: usize = a.flag("--jobs").map(|j| j.parse()).transpose()?.unwrap_or_else(workshop_core::render::default_concurrency);
    let timeout = Duration::from_secs(a.flag("--timeout").map(|t| t.parse()).transpose()?.unwrap_or(300));
    let pick: Vec<String> = a.rest.iter().filter(|x| !x.starts_with("--")).cloned().collect();
    let cache = std::env::temp_dir().join(format!("workshop-components-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let common = site.catalog()?["common_files"].clone();
    let mut r = Renderer::new(engine, site, cache.clone(), jobs)?;
    r.use_cache = false;
    let renderer = Arc::new(r);
    // one render per engine slot at a time, so the time limit counts rendering, not waiting
    let slots = Arc::new(tokio::sync::Semaphore::new(jobs));
    let mut handles = vec![];
    for lib in wc::bundled(&libs) {
        if only.as_deref().is_some_and(|o| !o.eq_ignore_ascii_case(&lib.info.name)) {
            continue;
        }
        renderer.add_blobs(lib.index["files"].as_object().into_iter().flatten().filter_map(|(rel, sha)| Some((sha.as_str()?.to_string(), lib.root.join(rel)))));
        for c in wc::components_of(&lib) {
            if !pick.is_empty() && !pick.contains(&c.module) {
                continue;
            }
            let model = wc::component_model(&lib, &c);
            let req = default_request(&model, &common)?;
            let needs = model["needs"].as_array().map(|n| !n.is_empty()).unwrap_or(false);
            let start = match c.examples.iter().position(|e| !e.calls.is_empty()) {
                Some(i) if c.examples[i].title == "Suggested start" => "suggested",
                Some(i) if c.examples[i].title == "Guessed start values" => "guessed",
                Some(0) => "first example",
                Some(_) => "later example",
                None if needs => "needs input",
                None => "no example",
            };
            if start == "needs input" {
                eprintln!("skip  {:<46} needs input: {}", model["key"].as_str().unwrap_or(""), model["needs"]);
                handles.push(tokio::spawn(async move { json!({ "key": model["key"], "status": "skipped", "start": start }) }));
                continue;
            }
            let key = model["key"].as_str().unwrap_or("").to_string();
            let renderer = renderer.clone();
            let slots = slots.clone();
            handles.push(tokio::spawn(async move {
                let _slot = slots.acquire_owned().await;
                let job = format!("check-{}", key.replace(['/', '@'], "-"));
                let t0 = Instant::now();
                let r = tokio::time::timeout(timeout, renderer.render(&job, &req, |_| {})).await;
                let (status, tris, error) = match r {
                    Ok(Ok(o)) => ("pass", stl_triangles(&o.stl).unwrap_or(0), None),
                    Ok(Err(e)) => ("fail", 0, Some(e.message)),
                    Err(_) => {
                        renderer.cancel(&job);
                        ("fail", 0, Some(format!("timed out after {} s", timeout.as_secs())))
                    }
                };
                let secs = (t0.elapsed().as_millis() as f64 / 10.0).round() / 100.0;
                eprintln!("{status:<5} {key:<46} {start:<14} {secs:>6}s {tris:>8} tris {}", error.clone().unwrap_or_default());
                json!({ "key": key, "status": status, "start": start, "seconds": secs, "triangles": tris, "error": error })
            }));
        }
    }
    let mut results = vec![];
    for h in handles {
        results.push(h.await?);
    }
    let count = |start: &str, status: &str| results.iter().filter(|r| r["start"] == start && r["status"] == status).count();
    let mut summary = serde_json::Map::new();
    for start in ["first example", "later example", "suggested", "guessed", "no example"] {
        summary.insert(start.into(), json!({ "pass": count(start, "pass"), "fail": count(start, "fail") }));
    }
    summary.insert("needs input".into(), json!(count("needs input", "skipped")));
    eprintln!("{}", serde_json::to_string(&summary)?);
    if let Some(out) = out {
        std::fs::write(&out, serde_json::to_string_pretty(&json!({ "summary": summary, "results": results }))? + "\n")?;
    }
    let _ = std::fs::remove_dir_all(&cache);
    // the plan's check: every component whose first doc example is a plain call renders it
    if count("first example", "fail") > 0 && a.rest.iter().all(|x| x != "--no-fail") {
        std::process::exit(1);
    }
    Ok(())
}

/// Files and model stubs for every catalog model; writes <out>/.build/plan.json.
fn site_prepare(mut a: Args) -> Result<()> {
    let repo = PathBuf::from(a.need("--repo")?);
    let out = PathBuf::from(a.need("--out")?);
    let opts = workshop_core::sitebuild::PrepareOptions {
        public: a.rest.iter().any(|x| x == "--public"),
        hide_blocked: a.rest.iter().any(|x| x == "--hide-blocked"),
        desktop: a.rest.iter().any(|x| x == "--desktop"),
    };
    let plan = workshop_core::sitebuild::prepare(&repo, &out, &opts)?;
    std::fs::create_dir_all(out.join(".build"))?;
    std::fs::write(out.join(".build/plan.json"), serde_json::to_vec_pretty(&plan)?)?;
    let browser: Vec<&str> = plan.models.iter().filter(|m| m.browser).map(|m| m.key.as_str()).collect();
    println!("{}", browser.join("\n"));
    eprintln!("{} models ({} for the browser engine), {} problems", plan.models.len(), browser.len(), plan.problems.len());
    Ok(())
}

/// Model JSON and the catalog from the settings exports (desktop-only models read natively).
async fn site_finish(mut a: Args) -> Result<()> {
    let repo = PathBuf::from(a.need("--repo")?);
    let out = PathBuf::from(a.need("--out")?);
    let version = a.need("--engine-version")?;
    let plan: workshop_core::sitebuild::Plan = serde_json::from_slice(&std::fs::read(out.join(".build/plan.json"))?)?;
    let mut raw: serde_json::Map<String, Value> = match a.flag("--params") {
        Some(p) => serde_json::from_slice(&std::fs::read(&p).with_context(|| format!("couldn't read {p}"))?)?,
        None => Default::default(),
    };
    let native: Vec<_> = plan.models.iter().filter(|m| !m.browser).collect();
    if !native.is_empty() {
        let exe = a.flag("--native-engine").context("desktop-only models need --native-engine")?;
        let engine = NativeEngine::locate(&PathBuf::from(exe)).await?;
        let cache = out.join(".build/cache");
        let mut r = Renderer::new(engine, SiteDir::new(&out)?, cache, 2)?;
        r.use_cache = false;
        for m in native {
            let mut files = plan.common_files.clone();
            files.extend(m.files.clone());
            let req = workshop_core::RenderRequest { model: m.key.clone(), entry: m.entry.clone(), files, values: Default::default(), defines: vec![], call: None, format: None, engine: None };
            let v = match r.export_params(&req).await {
                Ok(v) => v,
                Err(e) => json!({ "error": format!("{}\n{}", e.message, e.logs.join("\n")) }),
            };
            raw.insert(m.key.clone(), v);
        }
    }
    let problems = workshop_core::sitebuild::finish(&repo, &out, &plan, &raw, &version)?;
    for p in &problems {
        eprintln!("PROBLEM: {}", p.replace('\n', " | "));
    }
    if !problems.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn app_for(home: &std::path::Path, app_site: PathBuf, starter: Option<PathBuf>, engine: PathBuf, library_url: &str) -> Result<Arc<workshop_core::api::App>> {
    // the bundled libraries: $WORKSHOP_LIBS, or libs/ next to the app's site folder (build/desktop/libs)
    let libs = std::env::var_os("WORKSHOP_LIBS")
        .map(PathBuf::from)
        .or_else(|| app_site.parent().map(|p| p.join("libs")))
        .filter(|l| l.join("libraries.json").is_file());
    workshop_core::api::App::new(workshop_core::api::AppPaths {
        libs,
        config_dir: home.join("config"),
        data_dir: home.join("data"),
        site: app_site,
        starter,
        engine,
        library_url: library_url.into(),
    })
}

/// Projects, models and resolved metadata of a library, as JSON (CI compares
/// this between Linux and Windows to check the library is portable).
async fn library_summary(mut a: Args) -> Result<()> {
    let library = PathBuf::from(a.need("--library")?);
    let app_site = PathBuf::from(a.need("--app-site")?);
    let starter = a.flag("--starter").map(PathBuf::from);
    let home = std::env::temp_dir().join(format!("workshop-summary-{}", std::process::id()));
    let app = app_for(&home, app_site, starter, PathBuf::from("."), "library:")?;
    app.call("library_open", json!({ "path": library.display().to_string() })).await.map_err(|e| anyhow::anyhow!(e))?;
    let c = app.catalog().map_err(|e| anyhow::anyhow!(e))?;
    let mut models: Vec<Value> = c.catalog["models"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| json!({ "key": m["key"], "name": m["name"], "license": m["license"]["spdx"], "category": m["category"], "tags": m["tags"], "settings": m["settings"] }))
        .collect();
    models.sort_by(|x, y| x["key"].as_str().cmp(&y["key"].as_str()));
    let sources: Vec<Value> = c.catalog["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| json!({ "id": s["id"], "kind": s["kind"], "version": s["version"], "name": s["name"], "models": s["models"], "parts": s["parts"], "license": s["meta"]["license"]["spdx"], "icon": s["icon"].as_str().map(|_| true) }))
        .collect();
    let parts: usize = c.parts.values().map(|p| p["items"].as_array().map(|i| i.len()).unwrap_or(0)).sum();
    println!("{}", serde_json::to_string_pretty(&json!({ "sources": sources, "models": models, "parts": parts, "blobs": c.blobs.len() }))?);
    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}

/// Render every model in a starter library, the way the app does (from library files).
async fn bench_library(mut a: Args) -> Result<()> {
    let app_site = PathBuf::from(a.need("--app-site")?);
    let starter = PathBuf::from(a.need("--starter")?);
    let engine = PathBuf::from(a.need("--engine")?);
    let out = a.flag("--out");
    let home = std::env::temp_dir().join(format!("workshop-bench-lib-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let app = app_for(&home, app_site, Some(starter), engine, "library:")?;
    app.call("library_open", json!({ "path": home.join("library").display().to_string() })).await.map_err(|e| anyhow::anyhow!(e))?;
    let c = app.catalog().map_err(|e| anyhow::anyhow!(e))?;
    let r = app.renderer().await.map_err(|e| anyhow::anyhow!(e))?;
    let mut keys: Vec<String> = a.rest.iter().filter(|k| k.contains('/')).cloned().collect();
    if keys.is_empty() {
        // the library's models (components have their own check: components-check)
        keys = c.catalog["models"].as_array().into_iter().flatten().filter_map(|m| m["key"].as_str().map(String::from)).collect();
    }
    keys.sort();
    let mut results = vec![];
    let mut failed = 0;
    for key in &keys {
        let model = &c.models[key];
        let req = default_request(model, &c.catalog["common_files"])?;
        let t0 = Instant::now();
        let res = r.render(&format!("bench-{}", key.replace('/', "-")), &req, |_| {}).await;
        let (status, tris, err) = match res {
            Ok(o) => ("pass", stl_triangles(&o.stl).unwrap_or(0), None),
            Err(e) => {
                failed += 1;
                ("fail", 0, Some(e.message))
            }
        };
        let secs = (t0.elapsed().as_millis() as f64 / 10.0).round() / 100.0;
        eprintln!("{status:<5} {key:<44} {secs:>7}s  {tris} tris");
        results.push(json!({ "key": key, "status": status, "seconds": secs, "triangles": tris, "error": err }));
    }
    let report = json!({ "library": true, "os": std::env::consts::OS, "models": keys.len(), "failed": failed, "results": results });
    if let Some(out) = out {
        std::fs::write(&out, serde_json::to_string_pretty(&report)? + "\n")?;
    }
    eprintln!("{} of {} rendered from the starter library", keys.len() - failed, keys.len());
    let _ = std::fs::remove_dir_all(&home);
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

// ------------------------------------------------------------------ local stand-in for the app

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "wasm" => "application/wasm",
        "ttf" => "font/ttf",
        "stl" => "model/stl",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        "txt" | "md" | "scad" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

struct Http {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_request(stream: &mut std::net::TcpStream) -> Result<Http> {
    use std::io::Read;
    let mut buf = vec![];
    let mut chunk = [0u8; 65536];
    let head_end = loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            bail!("connection closed");
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 1 << 20 {
            bail!("headers too long");
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))).collect();
    let len: usize = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(Http { method, path, headers, body })
}

fn respond(stream: &mut std::net::TcpStream, code: u16, ctype: &str, extra: &[(&str, String)], body: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut head = format!(
        "HTTP/1.1 {code} {}\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\naccess-control-allow-origin: *\r\naccess-control-expose-headers: x-events, x-failed\r\ncache-control: no-store\r\nconnection: close\r\n",
        if code < 400 { "OK" } else { "Error" },
        body.len()
    );
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    Ok(())
}

/// The app's commands over HTTP: POST /invoke/<command> (as tests/tauri_shim.js
/// sends them), GET /library/<path> for library files, everything else from --ui.
async fn serve(mut a: Args) -> Result<()> {
    let ui = PathBuf::from(a.need("--ui")?);
    let app_site = PathBuf::from(a.need("--app-site")?);
    let engine = PathBuf::from(a.need("--engine")?);
    let home = PathBuf::from(a.need("--home")?);
    let starter = a.flag("--starter").map(PathBuf::from);
    let port: u16 = a.flag("--port").map(|p| p.parse()).transpose()?.unwrap_or(8790);
    if let Some(lib) = a.flag("--library") {
        let mut cfg = workshop_core::config::AppConfig::load(&home.join("config/config.json"));
        cfg.set_library(std::path::Path::new(&lib));
        cfg.save(&home.join("config/config.json"))?;
    }
    let app = app_for(&home, app_site, starter, engine, "/library/")?;
    let picks: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("serving on http://127.0.0.1:{port}/ (home {})", home.display());
    let rt = tokio::runtime::Handle::current();
    let accept = tokio::task::spawn_blocking(move || -> Result<()> {
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else { continue };
        let app = app.clone();
        let ui = ui.clone();
        let picks = picks.clone();
        let home = home.clone();
        let rt = rt.clone();
        std::thread::spawn(move || {
            let Ok(req) = read_request(&mut stream) else { return };
            let path = req.path.split('?').next().unwrap_or("/").to_string();
            let r: Result<()> = rt.block_on(async {
                if let Some(cmd) = path.strip_prefix("/invoke/") {
                    let args: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                    match cmd {
                        "api" | "api_bytes" => {
                            let res = app.call(args["cmd"].as_str().unwrap_or(""), args["args"].clone()).await;
                            match res {
                                Ok(workshop_core::api::Reply::Json(v)) => respond(&mut stream, 200, "application/json", &[], &serde_json::to_vec(&v)?),
                                Ok(workshop_core::api::Reply::Bytes(b)) => respond(&mut stream, 200, "application/octet-stream", &[], &b),
                                Err(e) => {
                                    eprintln!("{} {}: {e}", args["cmd"].as_str().unwrap_or(""), args["args"]);
                                    respond(&mut stream, 400, "application/json", &[], &serde_json::to_vec(&json!(e))?)
                                }
                            }
                        }
                        "render" => {
                            let rq: workshop_core::RenderRequest = serde_json::from_value(args["req"].clone())?;
                            let job = args["job"].as_str().unwrap_or("job").to_string();
                            let r = match app.renderer().await {
                                Ok(r) => r,
                                Err(e) => return respond(&mut stream, 500, "application/json", &[], &serde_json::to_vec(&json!({ "message": e, "cancelled": false, "logs": [] }))?),
                            };
                            let events: Arc<std::sync::Mutex<Vec<Value>>> = Arc::default();
                            let ev2 = events.clone();
                            let res = r.render(&job, &rq, move |ev| ev2.lock().unwrap().push(serde_json::to_value(ev).unwrap_or(Value::Null))).await;
                            match res {
                                Ok(out) => {
                                    let mut evs = events.lock().unwrap().clone();
                                    evs.push(json!({ "type": "done", "ms": out.ms, "cached": out.cached, "format": out.format, "warning": out.warning, "logs": out.logs.iter().rev().take(50).rev().collect::<Vec<_>>() }));
                                    respond(&mut stream, 200, "application/octet-stream", &[("x-events", serde_json::to_string(&evs)?)], &out.stl)
                                }
                                // a cancelled render isn't a server error (the page cancels when you move on)
                                Err(e) if e.cancelled => respond(&mut stream, 200, "application/json", &[("x-failed", "1".into())], &serde_json::to_vec(&e)?),
                                Err(e) => {
                                    eprintln!("render {job} ({}): {}", rq.model, e.message);
                                    respond(&mut stream, 500, "application/json", &[], &serde_json::to_vec(&e)?)
                                }
                            }
                        }
                        "render_cancel" => {
                            if let Ok(r) = app.renderer().await {
                                r.cancel(args["job"].as_str().unwrap_or(""));
                            }
                            respond(&mut stream, 200, "application/json", &[], b"true")
                        }
                        "save_file" => {
                            let name = req.headers.iter().find(|(k, _)| k == "x-name").map(|(_, v)| workshop_core::api::percent_decode(v)).unwrap_or_else(|| "model.stl".into());
                            let dir = home.join("saved-files");
                            std::fs::create_dir_all(&dir)?;
                            let p = dir.join(name.replace(['/', '\\'], "_"));
                            std::fs::write(&p, &req.body)?;
                            respond(&mut stream, 200, "application/json", &[], &serde_json::to_vec(&json!(p.display().to_string()))?)
                        }
                        "pick_folder" | "pick_file" | "pick_save_path" => {
                            let next = { let mut p = picks.lock().unwrap(); if p.is_empty() { None } else { Some(p.remove(0)) } };
                            respond(&mut stream, 200, "application/json", &[], &serde_json::to_vec(&json!(next))?)
                        }
                        "reveal" | "open_path" => respond(&mut stream, 200, "application/json", &[], b"null"),
                        other => respond(&mut stream, 404, "application/json", &[], &serde_json::to_vec(&json!(format!("unknown command {other}")))?),
                    }
                } else if path == "/test/pick" {
                    let v: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                    if let Some(p) = v["path"].as_str() {
                        picks.lock().unwrap().push(p.to_string());
                    }
                    respond(&mut stream, 200, "application/json", &[], b"true")
                } else if let Some(rel) = path.strip_prefix("/library/") {
                    match app.library_file(rel) {
                        Ok(b) => respond(&mut stream, 200, content_type(rel), &[], &b),
                        Err(e) => respond(&mut stream, 404, "text/plain", &[], e.to_string().as_bytes()),
                    }
                } else if req.method == "GET" {
                    let rel = if path == "/" { "index.html".to_string() } else { workshop_core::api::percent_decode(path.trim_start_matches('/')) };
                    let p = workshop_core::library::rel_inside(&rel).map(|r| ui.join(r));
                    match p.ok().and_then(|p| std::fs::read(p).ok()) {
                        Some(b) => respond(&mut stream, 200, content_type(&rel), &[], &b),
                        None => respond(&mut stream, 404, "text/plain", &[], b"not found"),
                    }
                } else {
                    respond(&mut stream, 405, "text/plain", &[], b"")
                }
            });
            if let Err(e) = r {
                eprintln!("{path}: {e:#}");
            }
        });
    }
    Ok(())
    });
    accept.await?
}

/// Run one app command against a library (waiting for the job it starts), print the result.
async fn library_call(mut a: Args) -> Result<()> {
    let library = PathBuf::from(a.need("--library")?);
    let app_site = PathBuf::from(a.need("--app-site")?);
    let engine = PathBuf::from(a.flag("--engine").unwrap_or_else(|| ".".into()));
    let starter = a.flag("--starter").map(PathBuf::from);
    let home = a.flag("--home").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join(format!("workshop-call-{}", std::process::id())));
    let cmd = a.rest.first().cloned().context("which command?")?;
    let args: Value = a.rest.get(1).map(|j| serde_json::from_str(j)).transpose()?.unwrap_or(json!({}));
    let app = app_for(&home, app_site, starter, engine, "library:")?;
    app.call("library_open", json!({ "path": library.display().to_string() })).await.map_err(|e| anyhow::anyhow!(e))?;
    let r = match app.call(&cmd, args).await.map_err(|e| anyhow::anyhow!(e))? {
        workshop_core::api::Reply::Json(v) => v,
        workshop_core::api::Reply::Bytes(b) => json!({ "bytes": b.len() }),
    };
    let r = if let Some(job) = r["job"].as_str() {
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let jobs = match app.call("jobs", json!({})).await.map_err(|e| anyhow::anyhow!(e))? {
                workshop_core::api::Reply::Json(v) => v,
                _ => json!([]),
            };
            let j = jobs.as_array().into_iter().flatten().find(|j| j["id"] == job).cloned().unwrap_or(json!({}));
            if j["done"] == true {
                if let Some(e) = j["error"].as_str() {
                    bail!("{e}");
                }
                break j["result"].clone();
            }
        }
    } else {
        r
    };
    println!("{}", serde_json::to_string_pretty(&r)?);
    Ok(())
}
