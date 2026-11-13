// A model's saved settings: the bar above its form (Start from: Defaults, the author's presets and
// your saved settings; Save, Copy settings, and ⋯: rename, delete, export for OpenSCAD, import) and
// the Save settings dialog (#save-dialog). app.js keeps what they do (`act`). (Review 7, item 13:
// moved from app.js to Preact.)
import { html, useEffect, useRef } from "../lib/html.js";
import { plural } from "../lib/util.js";
import { openSide } from "./sideview.js";

const hasCode = (r) => !!r.edits && Object.keys(r.edits).length > 0;

/**
 * The bar. `active`: "default", "preset:<i>" or "saved:<id>" (what the picker shows); `note`:
 * { text, kind } under it; `edited`: how many of the model's files the Code tab has edited; `act`:
 * { choose(value), save, copy, rename, remove, exportAll, importFile }.
 */
export function SettingsBar({ presets, list, active, note, edited, act }) {
  const more = useRef(null);
  // ⋯ closes on a click anywhere else
  useEffect(() => {
    const close = (e) => { if (more.current && !more.current.contains(e.target)) more.current.open = false; };
    document.addEventListener("click", close);
    return () => document.removeEventListener("click", close);
  }, []);
  const run = (fn) => () => { if (more.current) more.current.open = false; fn(); };
  const known = active === "default" || (active?.startsWith("preset:") && presets[+active.slice(7)]) || list.some((r) => `saved:${r.id}` === active);
  const isSaved = !!active?.startsWith("saved:");
  return html`<div class="settings-bar">
    <div class="preset-bar">
      <label for="settings-pick" class="visually-hidden">Start from</label>
      <select id="settings-pick" aria-label="Start from defaults, a preset or your saved settings" value=${known ? active : "default"}
        onChange=${(e) => act.choose(e.target.value)}>
        <option value="default">Defaults</option>
        ${presets.length ? html`<optgroup label="Presets">${presets.map((ps, i) => html`<option value=${`preset:${i}`}>${ps.label}</option>`)}</optgroup>` : null}
        ${list.length ? html`<optgroup label="Saved">${list.map((r) => html`<option value=${`saved:${r.id}`}>${hasCode(r) ? `${r.name} · code edited` : r.name}</option>`)}</optgroup>` : null}
      </select>
      <button type="button" class="ghost" id="settings-save" title="Save these settings in the library" onClick=${act.save}>Save</button>
      <button type="button" class="ghost" id="settings-share" title="Copy these settings as an OpenSCAD Customizer set; paste them on a model page to use them"
        onClick=${act.copy}>Copy settings</button>
      <details class="menu" id="settings-more" ref=${more}>
        <summary class="ghost" aria-label="Saved settings: rename, delete, export, import" title="Saved settings: rename, delete, export for OpenSCAD, import">⋯</summary>
        <div class="menu-panel" role="menu">
          <button type="button" role="menuitem" id="settings-rename" disabled=${!isSaved} onClick=${run(act.rename)}>Rename…</button>
          <button type="button" role="menuitem" id="settings-delete" disabled=${!isSaved} onClick=${run(act.remove)}>Delete</button>
          <hr />
          <button type="button" role="menuitem" id="settings-export" title="All saved settings for this model, as an OpenSCAD Customizer file" onClick=${run(act.exportAll)}>Export for OpenSCAD</button>
          <button type="button" role="menuitem" id="settings-import-btn" onClick=${run(act.importFile)}>Import OpenSCAD file…</button>
        </div>
      </details>
    </div>
    <p id="settings-note" class=${`settings-note ${note?.kind || ""}`} role="status" hidden=${!note}>${note?.text}</p>
    <p id="code-note" class="settings-note code-note" hidden=${!edited}>${edited
      ? [`Code edited (${plural(edited, "file")}): settings left at their defaults take the code's values. `,
        html`<button type="button" class="link-btn" onClick=${() => openSide("code", { sideFile: null })}>Show the code</button>`] : null}</p>
  </div>`;
}

/**
 * The Save settings dialog's contents. `spec`: { mode ("save" or "rename"), name (to start from),
 * updating (the name of the saved settings Save updates, or null), note }; `save(asNew, name)`.
 */
export function SaveDialog({ spec, save }) {
  const name = useRef(null);
  useEffect(() => { name.current?.select(); }, []);
  const submit = (asNew) => {
    const v = name.current.value.trim();
    if (!v) { name.current.focus(); return; }
    save(asNew, v);
  };
  return html`<form method="dialog" id="save-form" onSubmit=${(e) => { e.preventDefault(); submit(!spec.updating); }}>
    <h2 id="save-title">${spec.mode === "rename" ? "Rename saved settings" : "Save settings"}</h2>
    <label class="batch-list" for="save-name">Name</label>
    <input id="save-name" type="text" maxlength="80" required autocomplete="off" ref=${name} value=${spec.name} />
    <p id="save-note" class="muted">${spec.note}</p>
    <div class="batch-actions">
      <button type="button" class="ghost" id="save-cancel" onClick=${(e) => e.currentTarget.closest("dialog").close()}>Cancel</button>
      <button type="button" class="ghost" id="save-alt" hidden=${!spec.updating} onClick=${() => submit(true)}>Save as new</button>
      <button type="submit" class="primary" id="save-primary">${spec.mode === "rename" ? "Rename" : spec.updating ? `Update “${spec.updating}”` : "Save"}</button>
    </div>
  </form>`;
}
