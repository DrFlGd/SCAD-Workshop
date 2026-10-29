//! Fusion 360 files (`.f3d`) read with MitCad's reader (`desktop/core/mitcad/`): their user
//! parameters become a settings form and their saved preview a thumbnail, without MitCad installed.
//! Generating from them is [`crate::mitcad`]'s job. Experimental (0.4.27; docs/DESKTOP_PLAN.md,
//! "Fusion 360 files through MitCad").

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::path::Path;

/// What the app needs of an `.f3d` file.
#[derive(Debug, Default)]
pub struct F3dInfo {
    /// The design's name (the file's, without a cache GUID).
    pub label: String,
    /// User parameters whose expression is a plain number, as OpenSCAD's parameter export lists
    /// settings (`name`, `initial`, `caption`, `group`): [`crate::ingest::convert_params`] reads them.
    /// Each also has `unit` (`mm`, `deg`, … or empty), which values go back with.
    pub parameters: Vec<Value>,
    /// User parameters that follow a formula (`ScrewDiameter - 0.15 mm`): `name`, `expression`, `comment`.
    pub formulas: Vec<Value>,
    /// The preview Fusion saved with the file (PNG), if any.
    pub preview_png: Option<Vec<u8>>,
    /// Timeline items, components and the writer (Fusion's build), for the log.
    pub timeline_items: usize,
    pub components: usize,
    pub writer: String,
}

/// A number written as its expression (`70 mm`, `3`, `-0.5 deg`): the number and its unit.
fn plain_number(expression: &str) -> Option<(f64, &str)> {
    let s = expression.trim();
    let end = s.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))).unwrap_or(s.len());
    let (num, unit) = s.split_at(end);
    let unit = unit.trim();
    if num.is_empty() || !unit.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    num.parse::<f64>().ok().map(|n| (n, unit))
}

/// MitCad's dump keeps values in centimetres and radians; the form shows the parameter's own unit.
pub fn from_internal(value: f64, unit: &str) -> f64 {
    match unit {
        "mm" => value * 10.0,
        "cm" => value,
        "m" => value / 100.0,
        "in" => value / 2.54,
        "ft" => value / 30.48,
        "deg" => value.to_degrees(),
        _ => value,
    }
}

/// The step a number setting moves by: its last decimal (1.5 -> 0.1), 1 for a whole number.
fn step_of(v: f64) -> f64 {
    if v.fract() == 0.0 {
        1.0
    } else {
        crate::ingest::decimal_step(v)
    }
}

/// Read a file's design: its user parameters and preview. Quick (the design streams alone,
/// not the bodies' history), so it runs at ingest beside OpenSCAD's settings export.
pub fn read(path: &Path) -> Result<F3dInfo> {
    let package = mitcad_f3d_design::package(path).map_err(|e| anyhow!("{e}"))?;
    // an .f3z package: its root design, else the first document
    let (label, doc) = package
        .root
        .as_ref()
        .and_then(|r| package.documents.iter().find(|(l, _)| l == r))
        .or_else(|| package.documents.first())
        .context("no design in the file")?;
    let design = mitcad_f3d_design::decode_streams(doc, label).map_err(|e| anyhow!("{e}"))?;
    let dump = design.dump.context("the file has no design segment")?;
    let mut info = F3dInfo { label: mitcad_f3d_design::short_name(path), writer: dump.generator.as_ref().and_then(|g| g.writer.clone()).unwrap_or_default(), ..Default::default() };
    info.timeline_items = dump.timeline_items().len();
    info.components = dump.components.as_ref().map_or(0, Vec::len);
    for p in dump.parameters.as_ref().and_then(|p| p.user.as_deref()).unwrap_or_default() {
        let (name, expression, unit) = (p.name.clone().unwrap_or_default(), p.expression.clone().unwrap_or_default(), p.unit.clone().unwrap_or_default());
        if name.is_empty() || unit == "Text" {
            continue; // a text parameter drives nothing printable
        }
        let comment = p.comment.as_deref().unwrap_or("").trim();
        match plain_number(&expression) {
            Some((written, written_unit)) => {
                // the number as typed when it's in the parameter's unit; else converted (and rounded:
                // centimetres to millimetres gives 0.4000000000000001)
                let v = if written_unit == unit { written } else { (from_internal(p.value.unwrap_or(0.0), &unit) * 1e9).round() / 1e9 };
                let v = crate::ingest::int_if_whole(&json!(v));
                let mut e = json!({ "name": name, "group": "Parameters", "initial": v, "unit": unit });
                if !comment.is_empty() {
                    e["caption"] = json!(comment);
                }
                e["step"] = json!(step_of(v.as_f64().unwrap_or(1.0)));
                info.parameters.push(e);
            }
            None => info.formulas.push(json!({ "name": name, "expression": expression, "comment": comment })),
        }
    }
    // the preview: the largest PNG Fusion saved (Previews/small.png, usually the only one)
    let mut best: Option<(usize, Vec<u8>)> = None;
    for e in doc.entries() {
        if matches!(e.kind, mitcad_f3d_file::container::EntryKind::Preview) && e.name.to_lowercase().ends_with(".png") {
            if let Ok(bytes) = doc.read(&e.name) {
                if best.as_ref().is_none_or(|(n, _)| bytes.len() > *n) {
                    best = Some((bytes.len(), bytes));
                }
            }
        }
    }
    info.preview_png = best.map(|(_, b)| b);
    Ok(info)
}

/// The settings as OpenSCAD's parameter export would list them, for [`crate::sitebuild::assemble_model`].
pub fn raw_params(info: &F3dInfo) -> Value {
    json!({ "parameters": info.parameters })
}

/// The units of a file's settings, by name: what values go back to MitCad with.
pub fn units(info: &F3dInfo) -> std::collections::BTreeMap<String, String> {
    info.parameters
        .iter()
        .filter_map(|p| Some((p["name"].as_str()?.to_string(), p["unit"].as_str().unwrap_or("").to_string())))
        .collect()
}

/// A value as MitCad's `set_parameter` takes it: an expression in the parameter's unit (`12.5 mm`).
pub fn expression(value: &Value, unit: &str) -> String {
    let n = match value {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.trim().to_string(),
        Value::Bool(b) => if *b { "1".into() } else { "0".into() },
        other => other.to_string(),
    };
    if unit.is_empty() { n } else { format!("{n} {unit}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_numbers_and_formulas() {
        assert_eq!(plain_number("70 mm"), Some((70.0, "mm")));
        assert_eq!(plain_number("3"), Some((3.0, "")));
        assert_eq!(plain_number(" -0.5 deg "), Some((-0.5, "deg")));
        assert_eq!(plain_number("ScrewDiameter - 0.15 mm"), None);
        assert_eq!(plain_number("2 * 3 mm"), None);
        assert_eq!(plain_number("mm"), None);
    }

    #[test]
    fn internal_units() {
        assert_eq!(from_internal(7.0, "mm"), 70.0);
        assert!((from_internal(std::f64::consts::FRAC_PI_2, "deg") - 90.0).abs() < 1e-9);
        assert_eq!(from_internal(2.54, "in"), 1.0);
        assert_eq!(from_internal(4.0, ""), 4.0);
    }

    #[test]
    fn expressions_for_mitcad() {
        assert_eq!(expression(&json!(12.5), "mm"), "12.5 mm");
        assert_eq!(expression(&json!(3), ""), "3");
        assert_eq!(expression(&json!("4"), "deg"), "4 deg");
    }

    /// A real file, when `WORKSHOP_F3D_SAMPLE` names one (the owner's Rugged Box: 24 user
    /// parameters, 76 timeline items, a preview). Skipped otherwise.
    #[test]
    fn reads_a_sample_file() {
        let Ok(p) = std::env::var("WORKSHOP_F3D_SAMPLE") else { return };
        let info = read(Path::new(&p)).unwrap();
        assert!(!info.parameters.is_empty(), "no settings read from {p}");
        assert!(info.timeline_items > 0);
        assert!(info.preview_png.as_ref().is_some_and(|b| b.starts_with(b"\x89PNG")));
        let tol = info.parameters.iter().find(|q| q["name"] == "HingeLatchHolesTolerance").expect("HingeLatchHolesTolerance");
        assert_eq!(tol["initial"], json!(0.4));
        assert_eq!(tol["step"], json!(0.1));
        let wall = info.parameters.iter().find(|q| q["name"] == "WallThickness").expect("WallThickness");
        assert_eq!(wall["initial"], json!(2.4));
        assert_eq!(wall["unit"], "mm");
        assert_eq!(wall["caption"], "Thickness of the walls");
        assert!(info.formulas.iter().any(|f| f["name"] == "ScrewThreadDiameter"));
    }
}
