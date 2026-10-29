//! Generating from Fusion 360 files (`.f3d`) with MitCad's command-line tool, `mitcad-cli`.
//! Experimental (0.4.27; docs/DESKTOP_PLAN.md, "Fusion 360 files through MitCad").
//!
//! MitCad is downloaded by the app (Settings › Engine › Download MitCad: the release `PIN.json`
//! names, the one our copy of its reader comes from; 0.4.28) or installed by the user; the app finds
//! `mitcad-cli` where Settings › Engine points, in its own download, on `PATH`, or where the
//! installers put it. A file is imported once into the render cache (`cache/mitcad/`, its timeline
//! replayed as MitCad features) from the design as *our* reader reads it (`import-f3d --dump`, so
//! the reader's patches reach MitCad without changing MitCad), then each render runs a script that
//! sets the parameters and exports a 3MF or STL. Reading a file's settings without MitCad is
//! [`crate::f3d`]'s job.

use crate::render::{RenderEvent, RenderFailure};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::sync::Notify;

/// Where `mitcad-cli` is, and a tag for the cache (the file's size and time: MitCad has no
/// `--version` and changes weekly).
#[derive(Clone, Debug)]
pub struct MitCad {
    pub exe: PathBuf,
    pub tag: String,
}

pub const RELEASES_URL: &str = "https://github.com/mitcad/Mitcad/releases";

fn exe_name() -> &'static str {
    if cfg!(windows) { "mitcad-cli.exe" } else { "mitcad-cli" }
}

/// Where the installers put it: Windows' per-user and all-users program folders; Linux' usual bins
/// and an extracted AppImage (`Mitcad.AppImage --appimage-extract` gives `squashfs-root/usr/bin/`).
fn candidates() -> Vec<PathBuf> {
    let mut v = vec![];
    let name = exe_name();
    if cfg!(windows) {
        for var in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                let base = if var == "LOCALAPPDATA" { base.join("Programs") } else { base };
                for dir in ["Mitcad", "MitCad"] {
                    v.push(base.join(dir).join(name));
                    v.push(base.join(dir).join("bin").join(name));
                }
            }
        }
    } else {
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            v.push(home.join(".local/bin").join(name));
            v.push(home.join("Applications/squashfs-root/usr/bin").join(name));
            v.push(home.join("squashfs-root/usr/bin").join(name));
        }
        for dir in ["/usr/local/bin", "/usr/bin", "/opt/mitcad/usr/bin", "/opt/mitcad/bin", "/opt/Mitcad/usr/bin"] {
            v.push(Path::new(dir).join(name));
        }
    }
    v
}

/// On `PATH`.
fn on_path() -> Option<PathBuf> {
    let name = exe_name();
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(name)).find(|f| f.is_file()))
}

/// The MitCad release our copy of its reader comes from (`desktop/core/mitcad/PIN.json`), which
/// Download MitCad installs.
pub struct Release {
    pub tag: String,
    pub version: String,
    pub page: String,
    /// For this system: the download, its size and SHA-256, and where `mitcad-cli` is once unpacked.
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub cli: String,
}

const PIN: &str = include_str!("../mitcad/PIN.json");

/// What `PIN.json` says about the reader: its commit (for the import cache's name).
pub fn reader_commit() -> String {
    serde_json::from_str::<Value>(PIN).ok().and_then(|p| p["commit"].as_str().map(String::from)).unwrap_or_default()
}

/// The pinned release, for this system (`None` where there's no download for it, as on macOS).
pub fn pinned_release() -> Option<Release> {
    let pin: Value = serde_json::from_str(PIN).ok()?;
    let r = &pin["release"];
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("linux", "x86_64") => "linux-x64",
        _ => return None,
    };
    let a = &r["assets"][platform];
    Some(Release {
        tag: r["tag"].as_str()?.into(),
        version: r["version"].as_str()?.into(),
        page: r["page"].as_str().unwrap_or(RELEASES_URL).into(),
        url: a["url"].as_str()?.into(),
        size: a["size"].as_u64()?,
        sha256: a["sha256"].as_str()?.into(),
        cli: a["cli"].as_str()?.into(),
    })
}

/// The downloaded copy of the pinned release under `root` (the app's `mitcad/` folder), if there.
pub fn downloaded(root: &Path) -> Option<PathBuf> {
    let r = pinned_release()?;
    Some(root.join(&r.version).join(&r.cli)).filter(|p| p.is_file())
}

/// Find `mitcad-cli`: the configured path (a file, or a folder holding it) first, then the app's
/// own download under `root` (the version our reader matches), then `PATH`, then where the
/// installers put it. The error says where it looked.
pub fn locate(configured: Option<&Path>, root: Option<&Path>) -> Result<MitCad, String> {
    let found = match configured {
        Some(p) if p.is_file() => Some(p.to_path_buf()),
        Some(p) if p.is_dir() => [p.join(exe_name()), p.join("usr/bin").join(exe_name()), p.join("bin").join(exe_name())].into_iter().find(|f| f.is_file()),
        Some(p) => return Err(format!("MitCad isn't at {} (Settings › Engine).", p.display())),
        None => None,
    };
    let exe = found
        .or_else(|| root.and_then(downloaded))
        .or_else(on_path)
        .or_else(|| candidates().into_iter().find(|p| p.is_file()))
        .ok_or_else(|| "MitCad isn't installed. Download it in Settings › Engine, or install it yourself and choose it there.".to_string())?;
    let exe = std::path::absolute(&exe).unwrap_or(exe);
    let meta = std::fs::metadata(&exe).map_err(|e| format!("Couldn't read {}: {e}", exe.display()))?;
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    Ok(MitCad { exe, tag: format!("mitcad-{}-{modified}", meta.len()) })
}

/// Download the pinned release into `root/<version>/` and unpack it (the Windows ZIP; the Linux
/// AppImage with `--appimage-extract`): where `mitcad-cli` is. Checks the size and SHA-256 PIN.json
/// records. `progress` gets what it's doing ("40 of 229 MB", "Unpacking"); `cancelled()` stops it.
/// Blocking.
pub fn install(root: &Path, progress: impl Fn(String), cancelled: impl Fn() -> bool) -> anyhow::Result<PathBuf> {
    use anyhow::{bail, Context};
    use sha2::Digest;
    use std::io::{Read, Write};
    let r = pinned_release().context("There's no MitCad download for this system: install it from MitCad's releases page and choose it here.")?;
    if let Some(p) = downloaded(root) {
        return Ok(p);
    }
    std::fs::create_dir_all(root).with_context(|| format!("couldn't create {}", root.display()))?;
    let part = root.join(format!(".download-{}-{}", r.version, std::process::id()));
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(120))
        .try_proxy_from_env(true)
        .user_agent(&format!("SCAD-Workshop/{}", crate::VERSION))
        .build();
    // `WORKSHOP_MITCAD_DOWNLOAD_URL` stands in for the release's address (tests serve it locally)
    let url = std::env::var("WORKSHOP_MITCAD_DOWNLOAD_URL").unwrap_or_else(|_| r.url.clone());
    let resp = agent.get(&url).call().map_err(|e| anyhow::anyhow!("Couldn't download MitCad {}: {e}", r.version))?;
    let mut reader = resp.into_reader().take(r.size + 1);
    let mut f = std::fs::File::create(&part).with_context(|| format!("couldn't create {}", part.display()))?;
    let (mut hash, mut done, mut buf) = (sha2::Sha256::new(), 0u64, vec![0u8; 1 << 20]);
    let mut shown = 0u64;
    let fail = |e: anyhow::Error| {
        let _ = std::fs::remove_file(&part);
        e
    };
    loop {
        if cancelled() {
            drop(f);
            return Err(fail(anyhow::anyhow!("Stopped.")));
        }
        let n = reader.read(&mut buf).map_err(|e| fail(anyhow::anyhow!("The download of MitCad broke off: {e}")))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| fail(e.into()))?;
        hash.update(&buf[..n]);
        done += n as u64;
        if done - shown >= 4 << 20 {
            progress(format!("{} of {} MB", done >> 20, r.size >> 20));
            shown = done;
        }
    }
    drop(f);
    if done != r.size || hex::encode(hash.finalize()) != r.sha256 {
        return Err(fail(anyhow::anyhow!("The MitCad download didn't match the release ({done} of {} bytes, or a different file). Try again.", r.size)));
    }
    progress("Unpacking".into());
    let unpack = root.join(format!(".unpack-{}-{}", r.version, std::process::id()));
    let _ = std::fs::remove_dir_all(&unpack);
    std::fs::create_dir_all(&unpack)?;
    let unpacked = if url.to_lowercase().ends_with(".zip") {
        unzip(&part, &unpack, &cancelled)?
    } else {
        // an AppImage unpacks itself, without FUSE
        let image = unpack.join("Mitcad.AppImage");
        std::fs::rename(&part, &image)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o755))?;
        }
        let out = std::process::Command::new(&image)
            .arg("--appimage-extract")
            .current_dir(&unpack)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .context("couldn't run the MitCad AppImage to unpack it")?;
        let _ = std::fs::remove_file(&image);
        if !out.status.success() {
            let _ = std::fs::remove_dir_all(&unpack);
            bail!("Unpacking MitCad failed: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        true
    };
    let _ = std::fs::remove_file(&part);
    if !unpacked {
        let _ = std::fs::remove_dir_all(&unpack);
        bail!("Stopped.");
    }
    if !unpack.join(&r.cli).is_file() {
        let _ = std::fs::remove_dir_all(&unpack);
        bail!("The MitCad download has no {} where expected ({}).", exe_name(), r.cli);
    }
    let target = root.join(&r.version);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&unpack, &target).with_context(|| format!("couldn't move MitCad into {}", target.display()))?;
    // earlier versions the app downloaded: no longer used
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name != r.version && e.path().is_dir() && name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    Ok(target.join(&r.cli))
}

/// Unpack a ZIP into `dest` (entries with unsafe paths are left out): false when stopped.
fn unzip(zip_path: &Path, dest: &Path, cancelled: &impl Fn() -> bool) -> anyhow::Result<bool> {
    use anyhow::Context;
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).context("the MitCad download isn't a ZIP")?;
    for i in 0..zip.len() {
        if cancelled() {
            return Ok(false);
        }
        let mut e = zip.by_index(i)?;
        let Some(rel) = e.enclosed_name() else { continue };
        let out = dest.join(rel);
        if e.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut w = std::fs::File::create(&out).with_context(|| format!("couldn't create {}", out.display()))?;
        std::io::copy(&mut e, &mut w)?;
    }
    Ok(true)
}

/// What a render needs from the renderer: where things are and what to make.
pub struct Job<'a> {
    pub tool: &'a MitCad,
    /// The render cache folder (imports go in `mitcad/` under it).
    pub cache_dir: &'a Path,
    /// This render's own folder (the script and the output).
    pub job_dir: &'a Path,
    /// The `.f3d` file as laid out for this render, and its content hash.
    pub file: &'a Path,
    pub sha: &'a str,
    /// The settings' values, by parameter name.
    pub values: &'a BTreeMap<String, Value>,
    /// "3mf" or "stl".
    pub ext: &'a str,
}

/// Imports at once would race on the same file; one at a time, and a second waits for the first.
static IMPORTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Run a `mitcad-cli` command, its output lines going to `on_event` and `logs`; stopped on cancel.
async fn run_logged<F>(mut cmd: tokio::process::Command, exe: &Path, on_event: &F, cancel: &Notify, logs: &mut Vec<String>) -> Result<std::process::ExitStatus, RenderFailure>
where
    F: Fn(RenderEvent) + Send + Sync,
{
    crate::engine::no_window(&mut cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| RenderFailure::failed(format!("Couldn't start MitCad ({}): {e}", exe.display()), logs.clone()))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
                   child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)]
        .into_iter()
        .flatten()
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stream).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let mut status = None;
    let mut open = true;
    while status.is_none() || open {
        tokio::select! {
            line = rx.recv(), if open => match line {
                Some(l) => {
                    on_event(RenderEvent::Log { line: l.clone() });
                    logs.push(l);
                    if logs.len() > 1000 {
                        logs.remove(0);
                    }
                }
                None => open = false,
            },
            s = child.wait(), if status.is_none() => {
                status = Some(s.map_err(|e| RenderFailure::failed(format!("MitCad failed: {e}"), logs.clone()))?);
            }
            _ = cancel.notified() => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(RenderFailure::cancelled(logs.clone()));
            }
        }
    }
    Ok(status.unwrap())
}

/// The first line that reads as an error, for the message shown.
fn first_error(logs: &[String]) -> Option<String> {
    logs.iter().find(|l| l.starts_with("error") || l.starts_with("mitcad-cli:") || l.contains("Error:")).cloned()
}

/// Of an import report (`import-f3d --report`), how the timeline was replayed: "69 of 71 items
/// parametric", and each item MitCad couldn't rebuild (kept as the file's own bodies or left out),
/// with MitCad's reason. Items with no geometry of their own (groups, occurrences) aren't counted.
fn report_summary(report: &Value) -> Vec<String> {
    let design = report["designs"].get(0).unwrap_or(report);
    let Some(items) = design["items"].as_array() else { return vec![] };
    let no_geometry = |i: &Value| i["outcome"] == "skipped" && i["note"].as_str().is_some_and(|n| n.starts_with("no geometry"));
    let counted: Vec<&Value> = items.iter().filter(|i| !no_geometry(i)).collect();
    let parametric = counted.iter().filter(|i| i["outcome"] == "parametric").count();
    let mut out = vec![format!("MitCad import: {parametric} of {} timeline items parametric.", counted.len())];
    if let Some(p) = design["parameters"].as_object() {
        let n = |k: &str| p.get(k).and_then(Value::as_u64).unwrap_or(0);
        let literal = p.get("literal").and_then(Value::as_array).map_or(0, Vec::len);
        out.push(format!("MitCad import: {} parameters{}.", n("imported"), if literal > 0 { format!(", {literal} of them fixed numbers (their formulas weren't read)") } else { String::new() }));
    }
    for i in counted.iter().filter(|i| i["outcome"] != "parametric") {
        let what = match i["outcome"].as_str() {
            Some("fallback") => "kept as the file's shape (settings no longer change that body from here on)",
            Some("partial") => "partly rebuilt",
            Some("skipped") => "left out",
            _ => "not rebuilt",
        };
        out.push(format!(
            "MitCad import: {} ({}{}) {what}: {}.",
            i["name"].as_str().unwrap_or("?"),
            i["type"].as_str().unwrap_or("item"),
            i["component"].as_str().map(|c| format!(" in {c}")).unwrap_or_default(),
            i["note"].as_str().unwrap_or("no reason given"),
        ));
    }
    out
}

/// Import the file into `imported`: from the design as our reader reads it when it can
/// (`--dump`), else as MitCad reads it. The log says which.
async fn import<F>(job: &Job<'_>, imported: &Path, report: &Path, on_event: &F, cancel: &Notify, logs: &mut Vec<String>) -> Result<(), RenderFailure>
where
    F: Fn(RenderEvent) + Send + Sync,
{
    let dir = imported.parent().unwrap_or(job.cache_dir);
    let tmp = dir.join(format!("{}.tmp-{}.mitcad", job.sha, std::process::id()));
    let dump = dir.join(format!("{}.tmp-{}.design.json", job.sha, std::process::id()));
    let note = |logs: &mut Vec<String>, line: String| {
        on_event(RenderEvent::Log { line: line.clone() });
        logs.push(line);
    };
    // our reading of the design (its patches: older files' user parameters, 0.4.28)
    let file = job.file.to_path_buf();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(8);
    let ours = tokio::task::spawn_blocking(move || crate::f3d::design_dump(&file, threads)).await;
    let ours = match ours {
        Ok(Ok(text)) => match std::fs::write(&dump, text) {
            Ok(()) => true,
            Err(e) => {
                note(logs, format!("Couldn't write the design for MitCad ({e}); MitCad reads the file itself."));
                false
            }
        },
        Ok(Err(e)) => {
            note(logs, format!("The app's reader couldn't read the design ({e:#}); MitCad reads the file itself."));
            false
        }
        Err(e) => {
            note(logs, format!("The app's reader stopped ({e}); MitCad reads the file itself."));
            false
        }
    };
    let mut tries: Vec<bool> = if ours { vec![true, false] } else { vec![false] };
    let mut last = None;
    while let Some(with_dump) = tries.first().copied() {
        tries.remove(0);
        let _ = std::fs::remove_file(&tmp);
        let mut cmd = tokio::process::Command::new(&job.tool.exe);
        cmd.arg("import-f3d").arg(job.file);
        if with_dump {
            note(logs, "MitCad builds the design as the app's reader read it (import-f3d --dump).".into());
            cmd.arg("--dump").arg(&dump);
        }
        cmd.arg("--save").arg(&tmp).arg("--report").arg(report).current_dir(job.job_dir);
        let status = run_logged(cmd, &job.tool.exe, on_event, cancel, logs).await;
        let status = match status {
            Ok(s) => s,
            Err(e) => {
                let _ = std::fs::remove_file(&dump);
                return Err(e);
            }
        };
        if status.success() && tmp.is_file() {
            let _ = std::fs::remove_file(&dump);
            return std::fs::rename(&tmp, imported).map_err(|e| RenderFailure::failed(format!("Couldn't keep the imported design: {e}"), logs.clone()));
        }
        let msg = first_error(logs).unwrap_or_else(|| format!("MitCad couldn't import this file (code {}).", status.code().unwrap_or(-1)));
        if with_dump && !tries.is_empty() {
            note(logs, format!("MitCad couldn't build the app's reading of the design ({msg}); trying its own reading."));
        }
        last = Some(msg);
    }
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&dump);
    Err(RenderFailure::failed(last.unwrap_or_else(|| "MitCad couldn't import this file.".into()), logs.clone()))
}

/// Import the file once (its timeline replayed), then set the parameters and export: the file and the log.
pub async fn generate<F>(job: &Job<'_>, on_event: &F, cancel: &Notify) -> Result<(Vec<u8>, Vec<String>), RenderFailure>
where
    F: Fn(RenderEvent) + Send + Sync,
{
    let mut logs: Vec<String> = vec![];
    let imports = job.cache_dir.join("mitcad");
    std::fs::create_dir_all(&imports).map_err(|e| RenderFailure::failed(format!("Couldn't create the MitCad cache folder: {e}"), vec![]))?;
    // one import per file, reader and MitCad: either changing imports again
    let key = {
        use sha2::Digest;
        let h = sha2::Sha256::digest(format!("{}\0{}", reader_commit(), job.tool.tag).as_bytes());
        hex::encode(&h[..6])
    };
    let imported = imports.join(format!("{}-{key}.mitcad", job.sha));
    let report = imports.join(format!("{}-{key}.report.json", job.sha));
    if !imported.is_file() {
        let _one_at_a_time = IMPORTING.lock().await;
        if !imported.is_file() {
            on_event(RenderEvent::Stage { stage: "Importing the Fusion file with MitCad (the first time only)…".into() });
            import(job, &imported, &report, on_event, cancel, &mut logs).await?;
        }
    }
    if let Some(r) = std::fs::read(&report).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        for line in report_summary(&r) {
            on_event(RenderEvent::Log { line: line.clone() });
            logs.push(line);
        }
    }
    // the settings' units, from the file itself (what the form shows, MitCad gets as `12.5 mm`)
    let units = crate::f3d::read(job.file).map(|i| crate::f3d::units(&i)).map_err(|e| RenderFailure::failed(format!("Couldn't read the file's settings: {e:#}"), logs.clone()))?;
    let mut script = vec![];
    for (name, unit) in &units {
        if let Some(v) = job.values.get(name) {
            script.push(json!({ "cmd": "set_parameter", "name": name, "expression": crate::f3d::expression(v, unit) }));
        }
    }
    let out = job.job_dir.join(format!("out.{}", job.ext));
    script.push(json!({ "cmd": "recompute" }));
    script.push(json!({ "cmd": "export", "path": out }));
    let script_path = job.job_dir.join("script.json");
    std::fs::write(&script_path, serde_json::to_vec_pretty(&script).unwrap_or_default())
        .map_err(|e| RenderFailure::failed(format!("Couldn't write the MitCad script: {e}"), logs.clone()))?;
    on_event(RenderEvent::Stage { stage: "Rebuilding with MitCad…".into() });
    let mut cmd = tokio::process::Command::new(&job.tool.exe);
    cmd.arg("run").arg(&script_path).arg("--open").arg(&imported).current_dir(job.job_dir);
    let status = run_logged(cmd, &job.tool.exe, on_event, cancel, &mut logs).await?;
    let bytes = std::fs::read(&out).unwrap_or_default();
    let empty = if job.ext == "stl" { bytes.len() <= 84 } else { bytes.is_empty() };
    if !status.success() || empty {
        let msg = first_error(&logs).unwrap_or_else(|| match status.code() {
            Some(0) | None if empty => "MitCad produced no geometry. These settings may not be valid for this model.".into(),
            Some(c) => format!("MitCad stopped with code {c}."),
            None => "MitCad stopped unexpectedly.".into(),
        });
        return Err(RenderFailure::failed(msg, logs));
    }
    Ok((bytes, logs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarises_an_import_report() {
        let r = json!({ "file": "box.f3d", "designs": [{
            "parameters": { "imported": 216, "literal": [] },
            "items": [
                { "name": "Item0", "type": "Group", "outcome": "skipped", "note": "no geometry: a timeline group" },
                { "name": "Extrude1", "type": "ExtrudeFeature", "outcome": "parametric", "component": "Box" },
                { "name": "Fillet2", "type": "FilletFeature", "outcome": "fallback", "note": "its result is not in the file's history", "component": "Box" },
            ] }] });
        let lines = report_summary(&r);
        assert_eq!(lines[0], "MitCad import: 1 of 2 timeline items parametric.");
        assert_eq!(lines[1], "MitCad import: 216 parameters.");
        assert!(lines[2].starts_with("MitCad import: Fillet2 (FilletFeature in Box) kept as the file's shape"), "{lines:?}");
        assert!(lines[2].ends_with(": its result is not in the file's history."));
        let r = json!({ "designs": [{ "parameters": { "imported": 192, "literal": ["d6", "d7"] }, "items": [] }] });
        assert_eq!(report_summary(&r)[1], "MitCad import: 192 parameters, 2 of them fixed numbers (their formulas weren't read).");
        assert!(report_summary(&json!({})).is_empty());
    }

    #[test]
    fn the_pinned_release_names_a_download() {
        assert!(!reader_commit().is_empty());
        if let Some(r) = pinned_release() {
            assert!(r.url.starts_with("https://github.com/mitcad/Mitcad/releases/download/"), "{}", r.url);
            assert_eq!(r.sha256.len(), 64);
            assert!(r.size > 1 << 20);
            assert!(r.cli.ends_with(exe_name()), "{}", r.cli);
            assert!(r.tag.ends_with(&r.version));
        }
    }

    /// The whole path with the stand-in `tests/mitcad_stub.py` on a real file (`WORKSHOP_F3D_SAMPLE`):
    /// imported once, every setting sent as an expression in its unit, an STL back. Skipped otherwise.
    #[cfg(unix)]
    #[tokio::test]
    async fn generates_with_the_stand_in() {
        let Ok(sample) = std::env::var("WORKSHOP_F3D_SAMPLE") else { return };
        let stub = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/mitcad_stub.py");
        let tool = locate(Some(&stub), None).unwrap();
        let dir = std::env::temp_dir().join(format!("workshop-mitcad-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (cache, job_dir) = (dir.join("cache"), dir.join("job"));
        std::fs::create_dir_all(&job_dir).unwrap();
        let log = dir.join("calls.log");
        // SAFETY: the test's own process; set before the stub runs
        unsafe { std::env::set_var("MITCAD_STUB_LOG", &log) };
        let mut values = BTreeMap::new();
        values.insert("WallThickness".to_string(), json!(3));
        values.insert("RibWidth".to_string(), json!(5.5));
        values.insert("NotASetting".to_string(), json!(1));
        let job = Job { tool: &tool, cache_dir: &cache, job_dir: &job_dir, file: Path::new(&sample), sha: "abc123", values: &values, ext: "stl" };
        let cancel = Notify::new();
        let events = std::sync::Mutex::new(vec![]);
        let (stl, logs) = generate(&job, &|ev| events.lock().unwrap().push(ev), &cancel).await.unwrap();
        assert_eq!(crate::render::stl_triangles(&stl), Some(2), "one triangle per setting the stand-in was given");
        let imports: Vec<String> = std::fs::read_dir(cache.join("mitcad")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert!(imports.iter().any(|n| n.starts_with("abc123-") && n.ends_with(".mitcad")), "{imports:?}");
        assert!(!imports.iter().any(|n| n.contains(".tmp-")), "{imports:?}");
        assert!(logs.iter().any(|l| l == "MitCad import: 69 of 71 timeline items parametric."), "{logs:?}");
        assert!(logs.iter().any(|l| l.starts_with("MitCad import: Fillet2 (FilletFeature in Box) kept")), "{logs:?}");
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.contains("\"import-f3d\""));
        // MitCad got the app's reading of the design, with the user parameters
        assert!(calls.contains("\"--dump\"") && calls.contains("DUMP user parameters: 24"), "{calls}");
        assert!(calls.contains("\"WallThickness\": \"3 mm\"") && calls.contains("\"RibWidth\": \"5.5 mm\""), "{calls}");
        assert!(!calls.contains("NotASetting"));
        // the second render finds the import there
        let (stl2, _) = generate(&job, &|_| {}, &cancel).await.unwrap();
        assert_eq!(stl2.len(), stl.len());
        assert_eq!(std::fs::read_to_string(&log).unwrap().matches("import-f3d").count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Real MitCad (`WORKSHOP_MITCAD_CLI`, the pinned release) on a real file (`WORKSHOP_F3D_SAMPLE`,
    /// the owner's Rugged Box): the user parameters reach MitCad through the app's reading of the
    /// design, and a wall thickness changes the model. Skipped otherwise; takes about half a minute.
    #[tokio::test]
    async fn generates_with_mitcad() {
        let (Ok(sample), Ok(cli)) = (std::env::var("WORKSHOP_F3D_SAMPLE"), std::env::var("WORKSHOP_MITCAD_CLI")) else { return };
        let tool = locate(Some(Path::new(&cli)), None).unwrap();
        let dir = std::env::temp_dir().join(format!("workshop-mitcad-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = dir.join("cache");
        let cancel = Notify::new();
        let mut sizes = vec![];
        for (i, wall) in [2.4, 4.0].into_iter().enumerate() {
            let job_dir = dir.join(format!("job{i}"));
            std::fs::create_dir_all(&job_dir).unwrap();
            let mut values = BTreeMap::new();
            values.insert("WallThickness".to_string(), json!(wall));
            let job = Job { tool: &tool, cache_dir: &cache, job_dir: &job_dir, file: Path::new(&sample), sha: "rugged", values: &values, ext: "stl" };
            let (stl, logs) = generate(&job, &|_| {}, &cancel).await.unwrap();
            if i == 0 {
                assert!(logs.iter().any(|l| l.contains("--dump")), "{logs:?}");
                assert!(logs.iter().any(|l| l == "MitCad import: 216 parameters."), "{logs:?}");
            }
            sizes.push(crate::render::stl_triangles(&stl).unwrap());
        }
        // the lid's walls follow the setting (the box bottom is kept as the file's: MitCad can't rebuild its Fillet2)
        assert_ne!(sizes[0], sizes[1], "the same model for both wall thicknesses");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Download MitCad (`WORKSHOP_MITCAD_INSTALL_TEST` names a folder; about 230 MB, or from
    /// `WORKSHOP_MITCAD_DOWNLOAD_URL`). Skipped otherwise.
    #[test]
    fn downloads_mitcad() {
        let Ok(root) = std::env::var("WORKSHOP_MITCAD_INSTALL_TEST") else { return };
        let root = PathBuf::from(root);
        let stages = std::sync::Mutex::new(0);
        let cli = install(&root, |_| *stages.lock().unwrap() += 1, || false).unwrap();
        assert!(cli.is_file());
        assert!(*stages.lock().unwrap() > 10);
        assert_eq!(downloaded(&root).as_deref(), Some(cli.as_path()));
        assert_eq!(locate(None, Some(&root)).unwrap().exe, std::path::absolute(&cli).unwrap());
        // a second time finds it there
        assert_eq!(install(&root, |_| panic!("downloaded again"), || false).unwrap(), cli);
    }

    #[test]
    fn unzips_a_release_zip() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("workshop-mitcad-unzip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("m.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.add_directory("mitcad-9.9.9-windows-x64/bin/", opts).unwrap();
        w.start_file("mitcad-9.9.9-windows-x64/bin/mitcad-cli.exe", opts).unwrap();
        w.write_all(b"MZ").unwrap();
        w.start_file("../outside.txt", opts).unwrap();
        w.write_all(b"no").unwrap();
        w.finish().unwrap();
        let dest = dir.join("out");
        assert!(unzip(&zip_path, &dest, &|| false).unwrap());
        assert_eq!(std::fs::read(dest.join("mitcad-9.9.9-windows-x64/bin/mitcad-cli.exe")).unwrap(), b"MZ");
        assert!(!dir.join("outside.txt").exists());
        assert!(!unzip(&zip_path, &dir.join("stopped"), &|| true).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_path_says_so() {
        let e = locate(Some(Path::new("/nowhere/mitcad-cli")), None).unwrap_err();
        assert!(e.contains("/nowhere/mitcad-cli"), "{e}");
    }
}
