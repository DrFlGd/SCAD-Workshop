// Batch generate (⋯ on a model's page): vary one or two settings, or tick saved settings; each
// combination becomes its own part, downloaded as one ZIP or saved to the Parts Library. app.js
// opens the dialog (#batch) and makes the parts (`make`). (Review 7, item 13: moved from app.js to Preact.)
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { humanize, same, plural, partName, SIZE_KEYS } from "../lib/util.js";
import { withChanges } from "../settings-codec.js";
import { axesOf } from "./modelform.js";

export const BATCH_MAX = 200;
// settings that tune the render or debug it, not what's made: not offered (Review 7, item 2)
const SKIP_GROUP = /^(debug|hidden|help)\b/i;
const SKIP_NAME = /^\$?(fa|fs|fn)$|^random_?seed$|^enable_?help$/i;

/**
 * What Batch generate can vary: { key, p, index, label, type }. A paired or vector setting (a bin's
 * Width: units and mm) offers each of its numbers on its own: key "width[0]", label "Width (units)".
 */
export function batchEntries(model) {
  const out = [];
  for (const p of model.parameters) {
    if (p.hidden || SKIP_GROUP.test(p.group || "") || SKIP_NAME.test(p.name)) continue;
    const label = p.label || humanize(p.name);
    if (Array.isArray(p.default)) {
      const axes = axesOf(p);
      p.default.forEach((v, i) => {
        if (typeof v === "number") out.push({ key: `${p.name}[${i}]`, p, index: i, label: `${label} (${axes[i] || i + 1})`, type: "number" });
      });
    } else if (["number", "string", "boolean"].includes(p.type)) out.push({ key: p.name, p, index: null, label, type: p.type });
  }
  return out;
}
const entryValue = (e, values) => (e.index == null ? values[e.p.name] : values[e.p.name]?.[e.index]);
/** Values with one entry set: a vector setting is copied before one of its numbers changes. */
function withEntry(values, e, v) {
  if (e.index == null) values[e.p.name] = v;
  else { values[e.p.name] = [...values[e.p.name]]; values[e.p.name][e.index] = v; }
  return values;
}

/** A row for a setting, with sensible starting values (a range's numbers are kept as typed). */
function startRow(e, values) {
  const v = entryValue(e, values);
  if (e.index == null && e.p.widget === "dropdown") return { key: e.key, kind: "choice", picked: [v] };
  if (e.type === "boolean") return { key: e.key, kind: "choice", picked: [false, true] };
  if (e.type === "number") {
    const step = e.p.step && e.p.step > 0 ? e.p.step : (Number.isInteger(v) ? Math.max(1, Math.round(Math.abs(v) / 5) || 1) : 1);
    return { key: e.key, kind: "range", from: String(v), to: String(v + step * 4), step: String(step) };
  }
  return { key: e.key, kind: "list", text: String(v) };
}

const num = (s) => (String(s).trim() === "" ? NaN : Number(s));
function rowValues(row, e) {
  if (!e) return [];
  if (row.kind === "choice") return row.picked;
  if (row.kind === "list") return row.text.split(",").map((s) => s.trim()).filter(Boolean)
    .map((s) => (e.type === "number" ? Number(s) : s)).filter((x) => e.type !== "number" || Number.isFinite(x));
  const out = [];
  const [from, to, step] = [num(row.from), num(row.to), num(row.step)];
  if (!(step > 0) || !Number.isFinite(from) || !Number.isFinite(to)) return out;
  for (let x = from, i = 0; (from <= to ? x <= to + 1e-9 : x >= to - 1e-9) && i <= BATCH_MAX; x = from <= to ? x + step : x - step, i++) {
    out.push(Math.round(x * 1e6) / 1e6);
  }
  return out;
}

/** A saved setting with code edits: Batch generate uses the form's code, so it isn't offered. */
const hasCode = (r) => !!r.edits && Object.keys(r.edits).length > 0;

/**
 * What Batch generate makes: { values, file, part } for each, with the whole form's values, a file
 * name for the ZIP, and a name for the Parts Library ("Basic Bin 2×1": the model, its sizes, and
 * whatever else varies).
 */
function batchItems({ model: m, values: formValues, entries, saved, mode, rows, picked }) {
  if (mode === "saved") {
    return saved.filter((r) => picked.has(r.id) && !hasCode(r)).map((r) => ({
      values: withChanges(m, r.values).values, file: `${m.family}-${m.id}-${r.name}`, part: `${m.name} ${r.name}` }));
  }
  if (!rows.length) return [];
  const byKey = new Map(entries.map((e) => [e.key, e]));
  let combos = [{}];
  for (const row of rows) {
    const vals = rowValues(row, byKey.get(row.key));
    combos = combos.flatMap((c) => vals.map((v) => ({ ...c, [row.key]: v })));
  }
  return combos.map((c) => {
    const values = structuredClone(formValues);
    for (const [k, v] of Object.entries(c)) withEntry(values, byKey.get(k), v);
    const other = Object.entries(c).filter(([k]) => !SIZE_KEYS.includes(byKey.get(k).p.name)).map(([k, v]) => {
      const e = byKey.get(k), p = e.p;
      if (typeof v === "boolean") return v ? e.label : `no ${e.label.toLowerCase()}`;
      if (e.index != null) return `${(p.label || humanize(p.name)).toLowerCase()} ${v} ${axesOf(p)[e.index] || ""}`.trim();
      return p.options?.find((o) => same(o.value, v))?.label || (p.type === "number" ? `${e.label.toLowerCase()} ${v}` : String(v));
    });
    const fileBit = (k, v) => { const e = byKey.get(k); return `${e.p.name}${e.index != null ? `-${axesOf(e.p)[e.index] || e.index}` : ""}-${v}`; };
    return { values, file: `${m.family}-${m.id}-` + Object.entries(c).map(([k, v]) => fileBit(k, v)).join("-"), part: [partName(m, values), ...other].join(" ") };
  });
}

/** One setting to vary: which, and its range, values or choices. */
function Row({ row, index, entries, count, set, remove }) {
  const entry = entries.find((x) => x.key === row.key);
  const p = entry.p;
  // the settings by their groups, as in the form
  const groups = new Map();
  for (const e of entries) {
    const g = e.p.group && e.p.group !== "Parameters" ? e.p.group : "";
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g).push(e);
  }
  const option = (e) => html`<option value=${e.key}>${e.label}</option>`;
  let editor;
  if (row.kind === "range") {
    const field = (k, text) => html`<label>${text}<input type="number" step="any" value=${row[k]} onInput=${(e) => set({ ...row, [k]: e.target.value })} /></label>`;
    editor = html`<div class="batch-range">${field("from", "From")}${field("to", "To")}${field("step", "Step")}</div>`;
  } else if (row.kind === "choice") {
    const opts = entry.index == null && p.widget === "dropdown" ? p.options : [{ value: false, label: "Off" }, { value: true, label: "On" }];
    editor = html`<div class="batch-choices">${opts.map((o) => html`<label><input type="checkbox" checked=${row.picked.some((x) => same(x, o.value))}
      onChange=${(e) => set({ ...row, picked: e.target.checked ? [...row.picked, o.value] : row.picked.filter((x) => !same(x, o.value)) })} />${o.label}</label>`)}</div>`;
  } else {
    editor = html`<label class="batch-list">Values, separated by commas<input type="text" value=${row.text} onInput=${(e) => set({ ...row, text: e.target.value })} /></label>`;
  }
  return html`<fieldset class="batch-row"><legend>${index ? "And vary" : "Vary"}</legend>
    <div class="batch-row-head">
      <select aria-label="Setting to vary" value=${row.key} onChange=${(e) => set(null, entries.find((x) => x.key === e.target.value))}>${[...groups].map(([g, list]) => (g
        ? html`<optgroup label=${g}>${list.map(option)}</optgroup>` : list.map(option)))}</select>
      ${count > 1 ? html`<button type="button" class="ghost" onClick=${remove}>Remove</button>` : null}
    </div>
    ${editor}</fieldset>`;
}

/**
 * The dialog's contents, kept while the same model is open. `values`: the form's; `saved`: the
 * model's saved settings; `opened`: counts the times it's opened; `keepable`: the parts can go to the Parts Library; `make(items, keep,
 * report)`: makes them, reporting (text, done 0..1), and returns { promise (the final text), cancel }.
 */
export function Batch({ model, values, saved, keepable, make, opened }) {
  const entries = batchEntries(model);
  const [rows, setRows] = useState(() => {
    // start from the most likely thing to vary: the first number of the form's first group (a bin's
    // Width in units), else the first number, else the first setting
    const first = entries[0]?.p.group;
    const guess = entries.find((e) => e.type === "number" && e.p.group === first) || entries.find((e) => e.type === "number") || entries[0];
    return guess ? [startRow(guess, values)] : [];
  });
  const [chosenMode, setMode] = useState("vary");
  const [picked, setPicked] = useState(() => new Set());
  const [progress, setProgress] = useState(null);
  const [running, setRunning] = useState(null);
  const box = useRef(null);
  // saved settings are another way to choose, when there are any
  const mode = saved.length ? chosenMode : "vary";
  const items = batchItems({ model, values, entries, saved, mode, rows, picked });
  const n = items.length;
  const ok = n > 0 && n <= BATCH_MAX;

  const stop = () => { running?.cancel(); };
  // opened again: what the last batch did isn't shown any more
  useEffect(() => { if (!running) setProgress(null); }, [opened]);
  // Escape closes the dialog: what's being made stops
  useEffect(() => {
    const dialog = box.current?.closest("dialog");
    dialog?.addEventListener("cancel", stop);
    return () => dialog?.removeEventListener("cancel", stop);
  }, [running]);

  const start = async (keep) => {
    if (!ok || running) return;
    const report = (text, done) => setProgress((p) => ({ text, done: done ?? p?.done ?? 0 }));
    setProgress({ text: "", done: 0 });
    const job = make(items, keep, report);
    setRunning(job);
    const text = await job.promise;
    setProgress((p) => ({ text, done: p?.done ?? 0 }));
    setRunning(null);
  };
  const setRow = (i) => (row, entry) => setRows(rows.map((r, j) => (j === i ? row || startRow(entry, values) : r)));
  const add = () => {
    const used = new Set(rows.map((r) => r.key));
    const next = entries.find((e) => !used.has(e.key));
    if (next) setRows([...rows, startRow(next, values)]);
  };
  const tick = (id, on) => { const s = new Set(picked); if (on) s.add(id); else s.delete(id); setPicked(s); };

  return html`<form method="dialog" id="batch-form" ref=${box}>
    <h2 id="batch-title">Batch generate</h2>
    <div class="batch-mode" id="batch-mode" role="radiogroup" aria-label="What to make" hidden=${!saved.length}>
      <label><input type="radio" name="batch-mode" value="vary" checked=${mode === "vary"} onChange=${() => setMode("vary")} /> Vary settings</label>
      <label><input type="radio" name="batch-mode" value="saved" checked=${mode === "saved"} onChange=${() => setMode("saved")} /> Your saved settings</label>
    </div>
    <div id="batch-vary" hidden=${mode !== "vary"}>
      <p class="muted">Every other setting stays as it is in the form. Each combination becomes its own part.</p>
      <div id="batch-rows">${rows.map((row, i) => html`<${Row} key=${i} row=${row} index=${i} entries=${entries} count=${rows.length}
        set=${setRow(i)} remove=${() => setRows(rows.filter((_, j) => j !== i))} />`)}</div>
      <button type="button" class="ghost" id="batch-add" hidden=${rows.length >= 2 || entries.length < 2} onClick=${add}>Vary a second setting</button>
    </div>
    <div id="batch-saved" class="batch-saved" hidden=${mode !== "saved"}>
      <p class="muted">Each one you tick becomes its own part, with its own settings.</p>
      ${saved.map((r) => html`<label class="batch-saved-row" title=${hasCode(r) ? "Saved with code edits: open it to make it" : null}>
        <input type="checkbox" value=${r.id} disabled=${hasCode(r)} checked=${picked.has(r.id) && !hasCode(r)} onChange=${(e) => tick(r.id, e.target.checked)} />${r.name}${hasCode(r)
          ? html`<small class="muted"> (code edited: open it to make it)</small>` : null}</label>`)}
    </div>
    <p id="batch-count" class="batch-count">${n > BATCH_MAX ? `${n} combinations: the limit is ${BATCH_MAX}. Narrow the ranges.`
      : n ? `${plural(n, "part")} will be made.` : mode === "saved" ? "Tick at least one." : "Pick at least one value."}</p>
    <div class="batch-progress" id="batch-progress" hidden=${!progress}>
      <div class="progress-bar"><span id="batch-bar" style=${{ width: `${(progress?.done || 0) * 100}%` }}></span></div>
      <p id="batch-progress-text">${progress?.text}</p>
    </div>
    <div class="batch-actions">
      <button type="button" class="ghost" id="batch-close" onClick=${(e) => { stop(); e.currentTarget.closest("dialog").close(); }}>Close</button>
      <button type="button" class="secondary" id="batch-keep" hidden=${!keepable} disabled=${!ok || !!running} onClick=${() => start(true)}>${ok ? `Save ${n} to the Parts Library` : "Save to the Parts Library"}</button>
      <button type="button" class="primary" id="batch-run" disabled=${!ok || !!running} onClick=${() => start(false)}>${ok ? `Make ${n} and download ZIP` : "Make and download ZIP"}</button>
    </div>
  </form>`;
}
