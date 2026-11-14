//! A model's page data (its settings form, names, license and files) from a project's
//! description and OpenSCAD's settings export: what reading a project
//! ([`crate::project`]) writes for each model. Also the built-in categories and
//! the line-ending fix every .scad file gets before OpenSCAD reads it.

use crate::ingest;
use anyhow::Result;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const CATEGORY_LABELS: [(&str, &str); 5] =
    [("gridfinity", "Gridfinity"), ("carrying", "Boxes & Baskets"), ("labels", "Labels"), ("wall", "Wall Storage"), ("other", "Other")];

pub fn category_label(id: &str) -> String {
    CATEGORY_LABELS.iter().find(|(c, _)| *c == id).map(|(_, l)| l.to_string()).unwrap_or_else(|| id.to_string())
}

/// SCAD text with Windows line endings normalised: OpenSCAD's Customizer can't
/// read dropdown lists otherwise (the files on disk stay untouched).
pub fn replace_crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\r' && data.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

fn or_obj(v: &Value) -> Value {
    if v.is_null() { json!({}) } else { v.clone() }
}
fn or_arr(v: &Value) -> Value {
    if v.is_null() { json!([]) } else { v.clone() }
}

/// Everything a model page needs, from a family manifest, one of its models,
/// the model's files and OpenSCAD's settings export. Returns (catalog summary, model detail).
#[allow(clippy::too_many_arguments)]
pub fn assemble_model(
    fam: &Value,
    m: &Value,
    key: &str,
    entry: &str,
    files: &BTreeMap<String, String>,
    editor: &Value,
    raw: &Value,
    input_bytes: u64,
) -> Result<(Value, Value)> {
    let (mut params, mut groups) = ingest::convert_params(raw);
    // site-declared settings for values OpenSCAD's Customizer can't expose
    // (computed with an expression in the file); the page passes these with -D
    for extra in m["extra_params"].as_array().into_iter().flatten() {
        let mut e = Map::new();
        e.insert("type".into(), json!("number"));
        e.insert("widget".into(), json!("number"));
        e.insert("description".into(), Value::Null);
        for (k, v) in extra.as_object().into_iter().flatten() {
            e.insert(k.clone(), v.clone());
        }
        e.insert("define".into(), json!(true));
        let after = e.shift_remove("after");
        let at = after.as_ref().and_then(|a| params.iter().position(|p| &p["name"] == a));
        let group = e.get("group").and_then(Value::as_str).unwrap_or("Parameters").to_string();
        params.insert(at.map(|i| i + 1).unwrap_or(params.len()), Value::Object(e));
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    let (params, groups, tabs) = ingest::apply_form(&params, &groups, fam, m, editor);
    let category = fam["category"].as_str().unwrap_or("other");
    let mut summary = Map::new();
    summary.insert("key".into(), json!(key));
    summary.insert("family".into(), fam["id"].clone());
    summary.insert("family_name".into(), fam["name"].clone());
    summary.insert("id".into(), m["id"].clone());
    summary.insert("name".into(), m["name"].clone());
    summary.insert("category".into(), json!(category));
    summary.insert(
        "category_label".into(),
        json!(fam["category_label"].as_str().map(String::from).unwrap_or_else(|| category_label(category))),
    );
    summary.insert("summary".into(), json!(fam["summary"].as_str().unwrap_or("")));
    summary.insert("tags".into(), or_arr(&fam["tags"]));
    summary.insert("license".into(), or_obj(&fam["license"]));
    if m["browser"] == Value::Bool(false) {
        summary.insert("browser".into(), json!(false));
    }
    // when the pinned upstream version was made, or when supplied files were added
    let src = &fam["source"];
    if let Some(d) = src["commit_date"].as_str() {
        summary.insert("updated".into(), json!(d.chars().take(10).collect::<String>()));
        summary.insert("updated_from".into(), json!("upstream"));
    } else if let Some(d) = src["supplied_by"].as_str().and_then(find_date) {
        summary.insert("updated".into(), json!(d));
        summary.insert("updated_from".into(), json!("supplied"));
    }
    let visible = params.iter().filter(|p| !p.get("hidden").is_some_and(ingest::truthy)).count();
    summary.insert("settings".into(), json!(visible));
    summary.insert("terms".into(), json!(ingest::setting_terms(&params)));
    let mut detail = summary.clone();
    detail.insert("authors".into(), or_arr(&fam["authors"]));
    detail.insert("links".into(), or_obj(&fam["links"]));
    detail.insert("source".into(), fam["source"].clone());
    detail.insert("notes".into(), m["notes"].clone());
    detail.insert("part_parameter".into(), m["part_parameter"].clone());
    detail.insert(
        "description_html".into(),
        ingest::clean_html(editor["description-extra-html"].as_str()).map(Value::String).unwrap_or(Value::Null),
    );
    detail.insert("entry".into(), json!(entry));
    detail.insert("files".into(), serde_json::to_value(files)?);
    detail.insert("fixed".into(), or_obj(&m["fixed"]));
    detail.insert("groups".into(), json!(groups));
    detail.insert("tabs".into(), tabs);
    detail.insert("parameters".into(), Value::Array(params));
    detail.insert("presets".into(), or_arr(&m["presets"]));
    detail.insert("input_bytes".into(), json!(input_bytes));
    Ok((Value::Object(summary), Value::Object(detail)))
}

/// The first YYYY-MM-DD in a string.
pub fn find_date(s: &str) -> Option<String> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(9)).find_map(|i| {
        let w = &b[i..i + 10];
        let ok = w.iter().enumerate().all(|(j, c)| if j == 4 || j == 7 { *c == b'-' } else { c.is_ascii_digit() });
        ok.then(|| String::from_utf8_lossy(w).into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_line_endings() {
        assert_eq!(find_date("owner, 2026-10-03 ZIP").as_deref(), Some("2026-10-03"));
        assert_eq!(find_date("no date"), None);
        assert_eq!(replace_crlf(b"a\r\nb\rc\r\n"), b"a\nb\rc\n");
    }
}
