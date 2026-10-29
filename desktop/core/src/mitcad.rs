//! Generating from Fusion 360 files (`.f3d`) with MitCad's command-line tool, `mitcad-cli`.
//! Experimental (0.4.27; docs/DESKTOP_PLAN.md, "Fusion 360 files through MitCad").
//!
//! The user installs MitCad (https://github.com/mitcad/Mitcad/releases); the app finds `mitcad-cli`
//! where the installers put it, on `PATH`, or where Settings › Engine points. A file is imported once
//! per content hash into the render cache (`cache/mitcad/<sha>.mitcad`, its timeline replayed as
//! MitCad features), then each render runs a script that sets the parameters and exports a 3MF or
//! STL. Reading a file's settings without MitCad is [`crate::f3d`]'s job.

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

/// Find `mitcad-cli`: the configured path (a file, or a folder holding it) first, then `PATH`,
/// then where the installers put it. The error says where it looked.
pub fn locate(configured: Option<&Path>) -> Result<MitCad, String> {
    let found = match configured {
        Some(p) if p.is_file() => Some(p.to_path_buf()),
        Some(p) if p.is_dir() => [p.join(exe_name()), p.join("usr/bin").join(exe_name()), p.join("bin").join(exe_name())].into_iter().find(|f| f.is_file()),
        Some(p) => return Err(format!("MitCad isn't at {} (Settings › Engine).", p.display())),
        None => None,
    };
    let exe = found
        .or_else(on_path)
        .or_else(|| candidates().into_iter().find(|p| p.is_file()))
        .ok_or_else(|| format!("MitCad isn't installed, or its {} wasn't found. Install it from {RELEASES_URL}, or choose it in Settings › Engine.", exe_name()))?;
    let exe = std::path::absolute(&exe).unwrap_or(exe);
    let meta = std::fs::metadata(&exe).map_err(|e| format!("Couldn't read {}: {e}", exe.display()))?;
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    Ok(MitCad { exe, tag: format!("mitcad-{}-{modified}", meta.len()) })
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
                    if logs.len() > 400 {
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

/// Of an import report, how many timeline items were replayed as features: "12 of 14 features
/// parametric, 1 partial, 1 fixed bodies".
fn report_summary(report: &Value) -> Option<String> {
    let counts = report.get("counts").or_else(|| report.get("design").and_then(|d| d.get("counts")))?.as_object()?;
    let n = |k: &str| counts.get(k).and_then(Value::as_u64).unwrap_or(0);
    let total: u64 = counts.values().filter_map(Value::as_u64).sum();
    let mut parts = vec![format!("{} of {total} features parametric", n("parametric"))];
    if n("partial") > 0 {
        parts.push(format!("{} partial", n("partial")));
    }
    if n("fallback") > 0 {
        parts.push(format!("{} as fixed bodies", n("fallback")));
    }
    if n("skipped") > 0 {
        parts.push(format!("{} skipped", n("skipped")));
    }
    Some(parts.join(", "))
}

/// Import the file once (its timeline replayed), then set the parameters and export: the file and the log.
pub async fn generate<F>(job: &Job<'_>, on_event: &F, cancel: &Notify) -> Result<(Vec<u8>, Vec<String>), RenderFailure>
where
    F: Fn(RenderEvent) + Send + Sync,
{
    let mut logs: Vec<String> = vec![];
    let imports = job.cache_dir.join("mitcad");
    std::fs::create_dir_all(&imports).map_err(|e| RenderFailure::failed(format!("Couldn't create the MitCad cache folder: {e}"), vec![]))?;
    let imported = imports.join(format!("{}.mitcad", job.sha));
    let report = imports.join(format!("{}.report.json", job.sha));
    if !imported.is_file() {
        let _one_at_a_time = IMPORTING.lock().await;
        if !imported.is_file() {
            on_event(RenderEvent::Stage { stage: "Importing the Fusion file with MitCad (the first time only)…".into() });
            let tmp = imports.join(format!("{}.tmp-{}.mitcad", job.sha, std::process::id()));
            let mut cmd = tokio::process::Command::new(&job.tool.exe);
            cmd.arg("import-f3d").arg(job.file).arg("--save").arg(&tmp).arg("--report").arg(&report).arg("--json").current_dir(job.job_dir);
            let status = run_logged(cmd, &job.tool.exe, on_event, cancel, &mut logs).await?;
            if !status.success() || !tmp.is_file() {
                let _ = std::fs::remove_file(&tmp);
                let msg = first_error(&logs).unwrap_or_else(|| format!("MitCad couldn't import this file (code {}).", status.code().unwrap_or(-1)));
                return Err(RenderFailure::failed(msg, logs));
            }
            std::fs::rename(&tmp, &imported).map_err(|e| RenderFailure::failed(format!("Couldn't keep the imported design: {e}"), logs.clone()))?;
        }
    }
    if let Some(s) = std::fs::read(&report).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|r| report_summary(&r)) {
        let line = format!("MitCad import: {s}.");
        on_event(RenderEvent::Log { line: line.clone() });
        logs.push(line);
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
    cmd.arg("run").arg(&script_path).arg("--open").arg(&imported).arg("--json").current_dir(job.job_dir);
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
        let r = json!({ "counts": { "parametric": 12, "partial": 1, "fallback": 1 } });
        assert_eq!(report_summary(&r).unwrap(), "12 of 14 features parametric, 1 partial, 1 as fixed bodies");
        let r = json!({ "design": { "counts": { "parametric": 3 } } });
        assert_eq!(report_summary(&r).unwrap(), "3 of 3 features parametric");
        assert!(report_summary(&json!({})).is_none());
    }

    /// The whole path with the stand-in `tests/mitcad_stub.py` on a real file (`WORKSHOP_F3D_SAMPLE`):
    /// imported once, every setting sent as an expression in its unit, an STL back. Skipped otherwise.
    #[cfg(unix)]
    #[tokio::test]
    async fn generates_with_the_stand_in() {
        let Ok(sample) = std::env::var("WORKSHOP_F3D_SAMPLE") else { return };
        let stub = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/mitcad_stub.py");
        let tool = locate(Some(&stub)).unwrap();
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
        assert!(cache.join("mitcad/abc123.mitcad").is_file());
        assert!(logs.iter().any(|l| l.contains("70 of 76 features parametric")), "{logs:?}");
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.contains("\"import-f3d\""));
        assert!(calls.contains("\"WallThickness\": \"3 mm\"") && calls.contains("\"RibWidth\": \"5.5 mm\""), "{calls}");
        assert!(!calls.contains("NotASetting"));
        // the second render finds the import there
        let (stl2, _) = generate(&job, &|_| {}, &cancel).await.unwrap();
        assert_eq!(stl2.len(), stl.len());
        assert_eq!(std::fs::read_to_string(&log).unwrap().matches("import-f3d").count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_path_says_so() {
        let e = locate(Some(Path::new("/nowhere/mitcad-cli"))).unwrap_err();
        assert!(e.contains("/nowhere/mitcad-cli"), "{e}");
    }
}
