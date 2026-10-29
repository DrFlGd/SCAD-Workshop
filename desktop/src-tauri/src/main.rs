// SCAD Workshop desktop app: the website's front end in a window, with native
// OpenSCAD and the library folder behind web/platform-desktop.js. Nearly every
// command goes through workshop_core::api::App::call (the same table
// `workshop-cli serve` uses for testing); this file adds what needs the window:
// renders with progress, save and pick dialogs, and the library:// protocol.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::ipc::{Channel, InvokeBody, Request, Response};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use workshop_core::api::{App, AppPaths, Reply};
use workshop_core::RenderRequest;

struct AppState {
    app: Arc<App>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// One of the app's commands (see workshop_core::api), answered as JSON.
#[tauri::command]
async fn api(cmd: String, args: Value, state: State<'_, AppState>) -> Result<Value, String> {
    match state.app.call(&cmd, args).await? {
        Reply::Json(v) => Ok(v),
        Reply::Bytes(b) => Ok(json!({ "bytes": b.len() })),
    }
}

/// One of the app's commands, answered as raw bytes (files).
#[tauri::command]
async fn api_bytes(cmd: String, args: Value, state: State<'_, AppState>) -> Result<Response, String> {
    match state.app.call(&cmd, args).await? {
        Reply::Bytes(b) => Ok(Response::new(b)),
        Reply::Json(v) => Ok(Response::new(serde_json::to_vec(&v).map_err(err)?)),
    }
}

/// Render with native OpenSCAD. Progress and log lines go to `on_event`; the result is the STL.
#[tauri::command]
async fn render(job: String, req: RenderRequest, on_event: Channel<Value>, state: State<'_, AppState>) -> Result<Response, Value> {
    let r = state.app.renderer().await.map_err(|e| json!({ "message": e, "cancelled": false, "logs": [] }))?;
    let ch = on_event.clone();
    let res = r
        .render(&job, &req, move |ev| {
            let _ = ch.send(serde_json::to_value(ev).unwrap_or(Value::Null));
        })
        .await;
    match res {
        Ok(out) => {
            let tail: Vec<&String> = out.logs.iter().rev().take(200).rev().collect();
            let _ = on_event.send(json!({ "type": "done", "ms": out.ms, "cached": out.cached, "format": out.format, "warning": out.warning, "logs": tail }));
            Ok(Response::new(out.stl))
        }
        Err(e) => Err(serde_json::to_value(&e).unwrap_or_else(|_| json!({ "message": e.message }))),
    }
}

#[tauri::command]
async fn render_cancel(job: String, state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.app.renderer().await.map(|r| r.cancel(&job)).unwrap_or(false))
}

/// Ask where to save, then write the bytes sent as the request body. The file
/// name comes in the `x-name` header (URI-encoded). Returns the saved path.
#[tauri::command]
async fn save_file(app: AppHandle, request: Request<'_>) -> Result<Option<String>, String> {
    let data = match request.body() {
        InvokeBody::Raw(bytes) => bytes.clone(),
        _ => return Err("No file data was sent.".into()),
    };
    let name = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .map(workshop_core::api::percent_decode)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "model.stl".into());
    let state = app.state::<AppState>();
    let config_path = state.app.paths.config_dir.join("config.json");
    let mut cfg = state.app.config();
    let mut dialog = app.dialog().file().set_file_name(&name).set_title("Save");
    if let Some(dir) = cfg.last_save_dir.clone().filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(ext) = name.rsplit_once('.').map(|(_, e)| e.to_string()) {
        dialog = dialog.add_filter(ext.to_uppercase(), &[ext.as_str()]);
    }
    let Some(picked) = dialog.blocking_save_file() else { return Ok(None) };
    let path = picked.into_path().map_err(err)?;
    std::fs::write(&path, data).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
    cfg.last_save_dir = path.parent().map(|p| p.to_path_buf());
    let _ = cfg.save(&config_path);
    Ok(Some(path.display().to_string()))
}

/// Ask where to save a file the app writes itself (a library backup). None if cancelled.
#[tauri::command]
async fn pick_save_path(app: AppHandle, title: Option<String>, name: Option<String>, extensions: Option<Vec<String>>) -> Result<Option<String>, String> {
    let state = app.state::<AppState>();
    let config_path = state.app.paths.config_dir.join("config.json");
    let mut cfg = state.app.config();
    let mut d = app.dialog().file().set_title(title.unwrap_or_else(|| "Save".into()));
    if let Some(n) = name.filter(|n| !n.is_empty()) {
        d = d.set_file_name(&n);
    }
    if let Some(dir) = cfg.last_save_dir.clone().filter(|d| d.is_dir()) {
        d = d.set_directory(dir);
    }
    if let Some(exts) = extensions.filter(|e| !e.is_empty()) {
        let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
        d = d.add_filter(exts.join(", ").to_uppercase(), &refs);
    }
    let Some(picked) = d.blocking_save_file() else { return Ok(None) };
    let path = picked.into_path().map_err(err)?;
    cfg.last_save_dir = path.parent().map(|p| p.to_path_buf());
    let _ = cfg.save(&config_path);
    Ok(Some(path.display().to_string()))
}

/// Pick a folder (opening a library, adding a project folder). None if cancelled.
/// (async: blocking dialogs must not run on the main thread)
#[tauri::command]
async fn pick_folder(app: AppHandle, title: Option<String>) -> Result<Option<String>, String> {
    let picked = app.dialog().file().set_title(title.unwrap_or_else(|| "Choose a folder".into())).blocking_pick_folder();
    match picked {
        Some(p) => Ok(Some(p.into_path().map_err(err)?.display().to_string())),
        None => Ok(None),
    }
}

/// Pick a file (a ZIP to add). None if cancelled.
#[tauri::command]
async fn pick_file(app: AppHandle, title: Option<String>, extensions: Option<Vec<String>>) -> Result<Option<String>, String> {
    let mut d = app.dialog().file().set_title(title.unwrap_or_else(|| "Choose a file".into()));
    if let Some(exts) = extensions.filter(|e| !e.is_empty()) {
        let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
        d = d.add_filter(exts.join(", ").to_uppercase(), &refs);
    }
    match d.blocking_pick_file() {
        Some(p) => Ok(Some(p.into_path().map_err(err)?.display().to_string())),
        None => Ok(None),
    }
}

#[tauri::command]
fn reveal(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().reveal_item_in_dir(PathBuf::from(path)).map_err(err)
}

/// Open a folder (the library, a project) in the file manager.
#[tauri::command]
fn open_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().open_path(path, None::<&str>).map_err(err)
}

/// Open a web link (a project's GitHub page, the releases page) in the default browser.
#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web links open in the browser.".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

/// The app's own addresses: its pages, the library's files, and the frames it makes (a README,
/// a PDF). Anything else a link or a PDF leads to opens in the browser instead of replacing the app.
fn in_app(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" | "library" | "asset" | "ipc" | "about" | "blob" | "data" => true,
        "http" | "https" => url.host_str().is_some_and(|h| h == "localhost" || h == "127.0.0.1" || h.ends_with(".localhost")),
        _ => false,
    }
}

/// A web or mail link that tried to open in the app: hand it to the system.
fn open_outside(app: &AppHandle, url: &tauri::Url) {
    if matches!(url.scheme(), "http" | "https" | "mailto") {
        let _ = app.opener().open_url(url.to_string(), None::<&str>);
    }
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "webp" => "image/webp",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "gif" => "image/gif",
        "stl" => "model/stl",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "txt" | "md" | "scad" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // library://localhost/<path> (http://library.localhost/<path> on Windows): files in the open library
        .register_asynchronous_uri_scheme_protocol("library", |ctx, request, responder| {
            let app = ctx.app_handle().state::<AppState>().app.clone();
            let path = request.uri().path().to_string();
            tauri::async_runtime::spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let body = app.library_file(&path);
                    (path, body)
                })
                .await;
                let response = match res {
                    Ok((path, Ok(body))) => tauri::http::Response::builder()
                        .status(200)
                        .header("Content-Type", mime(&path))
                        .header("Access-Control-Allow-Origin", "*")
                        .body(body),
                    Ok((_, Err(e))) => tauri::http::Response::builder().status(404).body(e.to_string().into_bytes()),
                    Err(e) => tauri::http::Response::builder().status(500).body(e.to_string().into_bytes()),
                };
                responder.respond(response.unwrap_or_else(|_| tauri::http::Response::new(Vec::new())));
            });
        })
        .setup(|app| {
            let resources = app.path().resource_dir()?;
            // development overrides: WORKSHOP_SITE, WORKSHOP_STARTER, WORKSHOP_ENGINE, WORKSHOP_LIBS
            let env = |k: &str, d: PathBuf| std::env::var_os(k).map(PathBuf::from).unwrap_or(d);
            let starter = env("WORKSHOP_STARTER", resources.join("starter"));
            let paths = AppPaths {
                config_dir: app.path().app_config_dir()?,
                data_dir: app.path().app_local_data_dir()?,
                site: env("WORKSHOP_SITE", resources.join("site")),
                starter: starter.is_dir().then_some(starter),
                engine: env("WORKSHOP_ENGINE", resources.join("engine")),
                libs: Some(env("WORKSHOP_LIBS", resources.join("libs"))).filter(|l| l.is_dir()),
                library_url: if cfg!(windows) { "http://library.localhost/".into() } else { "library://localhost/".into() },
            };
            let core = App::new(paths)?;
            app.manage(AppState { app: core });
            // the window (tauri.conf.json, "create": false), made here so that a link in a README or a
            // PDF can't take it over: the web opens in the browser, and no other windows open
            let cfg = app.config().app.windows.iter().find(|w| w.label == "main").cloned().ok_or("tauri.conf.json has no main window")?;
            let (to_browser, to_browser2) = (app.handle().clone(), app.handle().clone());
            tauri::WebviewWindowBuilder::from_config(app.handle(), &cfg)?
                .on_navigation(move |url| {
                    if in_app(url) {
                        return true;
                    }
                    open_outside(&to_browser, url);
                    false
                })
                .on_new_window(move |url, _features| {
                    if !in_app(&url) {
                        open_outside(&to_browser2, &url);
                    }
                    tauri::webview::NewWindowResponse::Deny
                })
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api, api_bytes, render, render_cancel, save_file, pick_save_path, pick_folder, pick_file, reveal, open_path, open_url])
        .run(tauri::generate_context!())
        .expect("error while running SCAD Workshop");
}
