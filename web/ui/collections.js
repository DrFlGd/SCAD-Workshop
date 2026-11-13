// Collections (milestone 5, 0.4.17): lists of parts and models kept for one use (an RC car, a rugged
// case), by id, so nothing is copied and one part can be in several. Each entry has a quantity, a
// status (to print / printed), notes and the format to use; a model keeps the settings it was added
// with. Stored in the library as collections/<id>/collection.json. A collection's page lists its
// entries with a printed count and exports them as one ZIP with a parts list and credits.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx } from "./context.js";
import { Icon } from "./icons.js";
import { api, readOnly } from "./library.js";
import { plural, FORMAT_LABEL } from "../lib/util.js";
import { makeZip } from "../zip.js";

/** Read the library's collections into the page's state. */
export async function refreshCollections() {
  try {
    const list = await api("collections_list");
    ui.set({ collections: list });
    return list;
  } catch {
    return [];
  }
}

export const collectionById = (id) => (ui.get().collections || []).find((c) => c.id === id) || null;

/** Store a collection and put it in the list straight away. */
async function putCollection(c) {
  const saved = await api("collection_put", { collection: c });
  const list = (ui.get().collections || []).filter((x) => x.id !== saved.id);
  ui.set({ collections: [...list, saved].sort((a, b) => a.name.localeCompare(b.name)) });
  return saved;
}

const newEntryId = () => Math.random().toString(36).slice(2, 10);

/** Which files a model entry means (Review 6, F-27): the fingerprint of the files the model used and its
 * project's version, as a made part's recipe keeps them. Nothing for a part or a component. */
function madeWith(ref) {
  const it = ctx.index?.get(ref);
  if (!it || it.kind !== "generator" || !it.filesId) return {};
  const version = (ctx.catalog?.sources || []).find((s) => s.id === it.sourceId)?.version || null;
  return { files_id: it.filesId, project_version: version };
}
/** An entry's model has other files since it was added ("Made from an earlier version"). */
const changedSince = (e, it) => !!(it && it.kind === "generator" && e.files_id && it.filesId && e.files_id !== it.filesId);

/** Copies printed (UI-40): `printed`, a count up to the quantity; an entry written before 0.4.20 has only
 * its status, "printed" meaning all of them. */
export const printedOf = (e) => {
  const q = Math.max(1, e.quantity || 1);
  return Math.min(q, Math.max(0, Number.isFinite(e.printed) ? e.printed : e.status === "printed" ? q : 0));
};
const withPrinted = (e, n) => {
  const q = Math.max(1, e.quantity || 1);
  const printed = Math.max(0, Math.min(q, n));
  return { printed, status: printed >= q ? "printed" : "to print" };
};
const sameSettings = (a, b) => (a.settings || null) === (b.settings || null) && JSON.stringify(a.values || {}) === JSON.stringify(b.values || {});

/**
 * Add entries ({ ref: an index id, name, settings?: a saved setting's id, values?: changed settings })
 * to a collection: `id` for one that exists, else a new one called `name`. The same thing with the
 * same settings is counted again rather than listed twice.
 */
export async function addToCollection(entries, { id = null, name = "" }) {
  const c = id ? structuredClone(collectionById(id)) : { name: name.trim(), summary: "", items: [] };
  if (!c) throw new Error("That collection isn't in the library any more.");
  for (const e of entries) {
    const same = c.items.find((i) => i.ref === e.ref && sameSettings(i, e));
    if (same) same.quantity = (same.quantity || 1) + 1;
    else c.items.push({ id: newEntryId(), ref: e.ref, name: e.name || "", settings: e.settings || null, values: e.values || null, quantity: 1, status: "to print", printed: 0, notes: "", format: null, ...madeWith(e.ref) });
  }
  return putCollection(c);
}

/** What an index item adds to a collection: parts and models as they are (a model at its defaults). */
export const entryOf = (it) => ({ ref: it.id, name: it.name });

const close = () => ui.set({ dialog: null });

/** Add to collection… (and New collection, with nothing to add): pick a collection or name a new one. */
export function CollectDialog({ spec }) {
  const list = useStore(ui, (s) => s.collections || []);
  const entries = spec.entries || [];
  const last = ui.get().lastCollection;
  const [pick, setPick] = useState(entries.length && list.some((c) => c.id === last) ? last : entries.length && list.length ? list[0].id : "");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const what = entries.length === 1 ? entries[0].name : plural(entries.length, "item");
  const ok = pick ? true : !!name.trim();
  const save = async (e) => {
    e.preventDefault();
    if (!ok || busy) return;
    setBusy(true);
    try {
      const c = await addToCollection(entries, pick ? { id: pick } : { name });
      ui.set({ dialog: null, lastCollection: c.id });
      ctx.toast(entries.length ? `Added ${what} to ${c.name}.` : `Made the collection ${c.name}.`, { label: "Show", run: () => { location.hash = `#/browse/coll/${c.id}`; } });
    } catch (err) {
      setBusy(false);
      ctx.toast(String(err.message || err));
    }
  };
  return html`<form class="dialog collect-dialog" onSubmit=${save} aria-label=${entries.length ? "Add to collection" : "New collection"}>
    <div class="dialog-head"><h2>${entries.length ? `Add ${what} to a collection` : "New collection"}</h2>
      <button type="button" class="ghost" aria-label="Close" onClick=${close}>${Icon.close(16)}</button></div>
    ${entries.length && list.length ? html`<div class="collect-list" role="radiogroup" aria-label="Collection">
      ${list.map((c) => html`<label class="check"><input type="radio" name="collection" value=${c.id} checked=${pick === c.id} onChange=${() => setPick(c.id)} data-collection=${c.id} />
        ${c.name} <span class="muted">${plural(c.items.length, "item")}</span></label>`)}
      <label class="check"><input type="radio" name="collection" value="" checked=${!pick} onChange=${() => setPick("")} id="collect-new" /> A new collection</label>
    </div>` : null}
    ${!pick ? html`<label class="field-block"><span>Name</span>
      <input id="collect-name" value=${name} onInput=${(e) => setName(e.target.value)} maxlength="80" placeholder="RC car, Workshop wall, Rugged case…" autofocus /></label>` : null}
    <p class="muted">A collection lists parts and models by name: nothing is copied, and one part can be in several. Each gets a quantity, a printed tick and notes${entries.some((e) => e.settings || e.values) ? "; a model keeps the settings it has now" : ""}.</p>
    <div class="dialog-actions"><button type="button" class="ghost" onClick=${close}>Cancel</button>
      <button type="submit" class="primary" id="collect-save" disabled=${!ok || busy}>${entries.length ? "Add" : "Make it"}</button></div>
  </form>`;
}

// ---------------------------------------------------------------- the collection's page

/** The formats an entry can be had in: a part's files, a model's render as STL or 3MF. */
function formatsOf(it) {
  if (!it) return [];
  if (it.kind === "part") return [...new Set(it.files.map((f) => f.format))];
  return ["stl", "3mf"];
}

/** The format an entry exports in: its own choice, else Settings › Downloads' rule. */
const formatFor = (e, it) => {
  const formats = formatsOf(it);
  return e.format && formats.includes(e.format) ? e.format : ctx.preferredFormatOf(formats);
};

const settingsText = (e) => (e.settings ? e.name.split(" · ").slice(1).join(" · ") || "saved settings" : e.values && Object.keys(e.values).length ? plural(Object.keys(e.values).length, "setting") + " changed" : "");

export function CollectionPage({ id }) {
  const c = useStore(ui, (s) => (s.collections || []).find((x) => x.id === id) || null);
  useStore(ui, (s) => s.catalogVersion);
  const [busy, setBusy] = useState(null); // { text, cancel }
  const [renaming, setRenaming] = useState(false);
  // entries added before 0.4.20 take the files their model has now, once (F-27)
  useEffect(() => {
    if (!c || readOnly() || !ctx.index) return;
    const fill = c.items.filter((e) => !e.files_id && madeWith(e.ref).files_id);
    if (fill.length) putCollection({ ...c, items: c.items.map((e) => (e.files_id ? e : { ...e, ...madeWith(e.ref) })) }).catch(() => {});
  }, [c?.id, c?.updated]);
  if (!c) return html`<div class="collection-page"><p class="empty">This collection isn't in the library.</p></div>`;
  const ro = readOnly();
  const total = c.items.reduce((n, e) => n + (e.quantity || 1), 0);
  const printed = c.items.reduce((n, e) => n + printedOf(e), 0);
  const changed = c.items.filter((e) => changedSince(e, ctx.index.get(e.ref))).length;
  const change = (eid, patch) => putCollection({ ...c, items: c.items.map((e) => (e.id === eid ? { ...e, ...patch } : e)) }).catch((err) => ctx.toast(String(err.message || err)));
  const remove = async (eid) => {
    const before = c;
    await putCollection({ ...c, items: c.items.filter((e) => e.id !== eid) });
    ctx.toast("Removed it from the collection.", { label: "Undo", run: () => putCollection(before) });
  };
  const removeCollection = async () => {
    try {
      const was = await api("collection_remove", { id: c.id });
      ui.set({ collections: (ui.get().collections || []).filter((x) => x.id !== c.id) });
      location.hash = "#/";
      ctx.toast(`Deleted the collection ${was.name}. Its parts and models are still in the library.`, { label: "Undo", run: () => putCollection(was) });
    } catch (err) { ctx.toast(String(err.message || err)); }
  };
  const run = async () => {
    const stop = { cancelled: false };
    setBusy({ text: "Starting…", cancel: () => { stop.cancelled = true; } });
    try {
      const r = await exportCollection(c, (text) => setBusy({ text, cancel: () => { stop.cancelled = true; } }), stop);
      ctx.toast(r.saved === null ? "Not saved." : `Exported ${plural(r.files, "file")} for ${plural(r.entries, "item")}${r.missing ? ` (${r.missing} no longer in the library)` : ""}${r.saved ? ` to ${r.saved}` : ""}.`
        + (changed && r.saved !== null ? ` ${changed === 1 ? "One model has" : `${changed} models have`} changed since ${changed === 1 ? "it was" : "they were"} added: made from the current version.` : ""));
    } catch (err) {
      ctx.toast(err.cancelled ? "Export cancelled." : `Couldn't export: ${err.message || err}`);
    } finally { setBusy(null); }
  };
  return html`<div class="collection-page" data-collection=${c.id}>
    <div class="browse-head">
      <div class="browse-title">
        ${renaming ? html`<form class="collection-rename" onSubmit=${(e) => { e.preventDefault(); const n = e.target.elements.name.value.trim(); if (n) putCollection({ ...c, name: n }); setRenaming(false); }}>
            <input name="name" id="collection-name" value=${c.name} maxlength="80" autofocus onKeyDown=${(e) => { if (e.key === "Escape") { e.preventDefault(); setRenaming(false); } }} />
            <button type="submit" class="primary">Rename</button></form>`
          : html`<h1>${c.name}</h1>`}
        <span class="browse-count">${plural(c.items.length, "item")}${total !== c.items.length ? ` · ${total} to make` : ""}</span>
      </div>
      <div class="source-actions">
        <button type="button" class="primary" id="collection-export" disabled=${!c.items.length || !!busy} onClick=${run}>${Icon.download(14)} Export as ZIP…</button>
        <details class="menu" id="collection-more"><summary class="ghost" aria-label="More" title="More">⋯</summary><div class="menu-panel" onClick=${(e) => {
          if (e.target.closest("button")) e.currentTarget.closest("details").open = false;
        }}>
          <button type="button" disabled=${ro} onClick=${() => setRenaming(true)} data-act="rename">Rename…</button>
          <hr />
          <button type="button" class="danger-text" disabled=${ro} onClick=${removeCollection} data-act="delete-collection">Delete collection</button>
        </div></details>
      </div>
    </div>
    <div class="collection-progress" aria-label="Printed">
      <div class="collection-bar"><span style=${`width:${total ? (printed / total) * 100 : 0}%`}></span></div>
      <span id="collection-printed">${printed} of ${total} printed</span>
      ${c.items.some((e) => (e.quantity || 1) > 1) ? html`<label class="tool-select collection-copies"><span>Copies in the ZIP</span>
        <select id="collection-copies" value=${c.copies || "each"} disabled=${ro} onChange=${(e) => putCollection({ ...c, copies: e.target.value })}>
          <option value="each">A file for each copy</option>
          <option value="count">One file, the count in its name</option>
        </select></label>` : null}
    </div>
    ${busy ? html`<p class="collection-busy" id="collection-busy">${busy.text} <button type="button" class="link-btn" onClick=${busy.cancel}>Cancel</button></p>` : null}
    ${!c.items.length ? html`<p class="empty">Nothing in it yet. Select parts or models and use Add to collection… in the inspector, or on a part's or model's page.</p>` : html`
    <table class="collection-table" id="collection-items">
      <thead><tr><th></th><th>Item</th><th>Format</th><th>Quantity</th><th>Printed</th><th>Notes</th><th></th></tr></thead>
      <tbody>${c.items.map((e) => {
        const it = ctx.index.get(e.ref);
        const formats = formatsOf(it);
        const href = it ? it.href : null;
        const extra = it && it.kind !== "part" ? settingsText(e) : "";
        const q = Math.max(1, e.quantity || 1), done = printedOf(e);
        const stale = changedSince(e, it);
        return html`<tr data-entry=${e.id} data-ref=${e.ref} class=${done >= q ? "printed" : ""}>
          <td class="collection-thumb">${it?.thumb ? html`<img src=${it.thumb} alt="" />` : html`<span class="thumb-none">${(e.name || "?").slice(0, 2)}</span>`}</td>
          <td><div class="collection-name">${href ? html`<a href=${href}>${it.name}</a>` : html`<span>${e.name}</span>`}</div>
            <div class="muted">${it ? [it.sub || it.project, extra].filter(Boolean).join(" · ") : "No longer in the library"}</div>
            ${stale ? html`<div class="collection-stale" data-stale>Made from an earlier version.
              <button type="button" class="link-btn" disabled=${ro} onClick=${() => change(e.id, madeWith(e.ref))} data-use-current>Use the current version</button></div>` : null}</td>
          <td>${formats.length > 1 ? html`<select class="format-pick" aria-label="Format" data-format disabled=${ro} value=${formatFor(e, it)} onChange=${(ev) => change(e.id, { format: ev.target.value })}>
              ${formats.map((f) => html`<option value=${f}>${FORMAT_LABEL[f] || f.toUpperCase()}</option>`)}</select>` : html`<span>${formats[0] ? FORMAT_LABEL[formats[0]] || formats[0].toUpperCase() : ""}</span>`}</td>
          <td><input type="number" class="collection-qty" min="1" max="999" step="1" aria-label="Quantity" data-qty value=${e.quantity || 1} disabled=${ro}
            onChange=${(ev) => { const quantity = Math.max(1, Math.min(999, Math.round(Number(ev.target.value) || 1))); change(e.id, { quantity, ...withPrinted({ ...e, quantity }, done) }); }} /></td>
          <td>${q === 1
            ? html`<label class="collection-done"><input type="checkbox" aria-label="Printed" data-printed checked=${done >= 1} disabled=${ro}
                onChange=${(ev) => change(e.id, withPrinted(e, ev.target.checked ? 1 : 0))} /></label>`
            : html`<span class="collection-count" data-printed-count>
                <button type="button" class="ghost" aria-label="One fewer printed" disabled=${ro || done <= 0} onClick=${() => change(e.id, withPrinted(e, done - 1))} data-printed-less>−</button>
                <span>${done >= q ? "✓ " : null}${done} of ${q}</span>
                <button type="button" class="ghost" aria-label="One more printed" disabled=${ro || done >= q} onClick=${() => change(e.id, withPrinted(e, done + 1))} data-printed-more>+</button>
              </span>`}</td>
          <td><input class="collection-notes" aria-label="Notes" data-notes value=${e.notes || ""} placeholder="PETG, 0.2 mm…" disabled=${ro}
            onChange=${(ev) => change(e.id, { notes: ev.target.value })} /></td>
          <td><button type="button" class="ghost" aria-label="Remove from the collection" title="Remove from the collection" data-remove disabled=${ro}
            onClick=${() => remove(e.id)}>${Icon.close(14)}</button></td>
        </tr>`;
      })}</tbody>
    </table>`}
  </div>`;
}

// ---------------------------------------------------------------- export

const fileSafe = (s) => s.replace(/[\\/:*?"<>|]+/g, "-").replace(/\s+/g, " ").trim().slice(0, 100) || "item";
const csvCell = (v) => (/[",\n]/.test(String(v)) ? `"${String(v).replace(/"/g, '""')}"` : String(v));

/**
 * The collection as one ZIP: each entry's file in its format (models made at their settings first),
 * a copy per quantity or one file with the count in its name, and parts.csv and README.txt with each
 * entry's license and where it's from. Resolves to { saved, files, entries, missing }.
 */
export async function exportCollection(c, progress, stop) {
  const files = [];
  const rows = [];
  const used = new Set();
  const unique = (name) => {
    let n = name, k = 2;
    while (used.has(n.toLowerCase())) n = name.replace(/(\.[^.]+)$/, ` (${k++})$1`);
    used.add(n.toLowerCase());
    return n;
  };
  let missing = 0;
  for (let k = 0; k < c.items.length; k++) {
    if (stop.cancelled) throw Object.assign(new Error("cancelled"), { cancelled: true });
    const e = c.items[k];
    const it = ctx.index.get(e.ref);
    if (!it) { missing++; rows.push({ e, it: null, names: [] }); continue; }
    const fmt = formatFor(e, it);
    let data;
    if (it.kind === "part") {
      progress(`Adding ${k + 1} of ${c.items.length}: ${it.name}`);
      const f = it.files.find((x) => x.format === fmt) || it.files[0];
      const r = await ctx.platform.fetch(f.url);
      if (!r.ok) throw new Error(`couldn't read ${f.path}`);
      data = new Uint8Array(await r.arrayBuffer());
    } else {
      progress(`Making ${k + 1} of ${c.items.length}: ${it.name}`);
      // (a 3MF is OpenSCAD's own where it can be: each object apart, colours kept, 0.4.19)
      const blob = await ctx.renderModel(it.key, { values: e.values, settings: e.settings, format: fmt === "3mf" ? "3mf" : null });
      data = new Uint8Array(await blob.arrayBuffer());
    }
    const q = Math.max(1, e.quantity || 1);
    const base = fileSafe((e.settings || (e.values && Object.keys(e.values).length)) && e.name ? e.name : it.name);
    const names = [];
    if (q === 1 || c.copies === "count") names.push(unique(`${base}${q > 1 ? ` x${q}` : ""}.${fmt}`));
    else for (let n = 1; n <= q; n++) names.push(unique(`${base} ${n}.${fmt}`));
    for (const name of names) files.push({ name, data });
    rows.push({ e, it, names, fmt });
  }
  progress("Writing the parts list…");
  const lic = (it) => (it?.license?.spdx && it.license.spdx !== "NOASSERTION" ? it.license.spdx : "Not stated");
  const csv = [["Item", "Quantity", "Printed", "Format", "Files", "Project", "Authors", "License", "Notes"].join(",")]
    .concat(rows.map(({ e, it, names, fmt }) => [e.name || it?.name || e.ref, e.quantity || 1, printedOf(e), fmt || "", names.join("; "),
      it?.project || "", (it?.authors || []).join("; "), it ? lic(it) : "", e.notes || ""].map(csvCell).join(","))).join("\r\n") + "\r\n";
  const readme = [`${c.name}`, "", c.summary || "", `Exported from SCAD Workshop on ${new Date().toISOString().slice(0, 10)}: ${plural(rows.length, "item")}, ${plural(files.length, "file")}.`,
    "parts.csv lists each item with its quantity, files, project and license.", "", "Credits and licenses (each item keeps its own):", "",
    ...rows.map(({ e, it }) => it ? `- ${it.name}${e.quantity > 1 ? ` (x${e.quantity})` : ""}: from ${it.project}${it.authors?.length ? ` by ${it.authors.join(", ")}` : ""}. License: ${lic(it)}.`
      : `- ${e.name || e.ref}: no longer in the library, not included.`), ""].filter((l, i) => i !== 2 || l).join("\n");
  const enc = new TextEncoder();
  files.push({ name: "parts.csv", data: enc.encode(csv) }, { name: "README.txt", data: enc.encode(readme) });
  progress("Saving the ZIP…");
  const saved = await ctx.deliver(makeZip(files), `${fileSafe(c.name)}.zip`);
  return { saved, files: files.length - 2, entries: rows.length - missing, missing };
}

const dataUrl = (blob) => new Promise((resolve, reject) => {
  const r = new FileReader();
  r.onload = () => resolve(r.result);
  r.onerror = () => reject(r.error);
  r.readAsDataURL(blob);
});
