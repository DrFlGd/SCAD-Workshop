// A parts pack's page (#/parts/<pack>/<part>): the rail beside the 3D view, with the pack's
// title and credit, its parts by category (with a search), and the part shown: where it came
// from, its downloads and its ⋯ menu. app.js shows the part's preview in the shared 3D view.
// (Review 7, item 13: moved from app.js to Preact.)
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { ui } from "./state.js";
import { ctx } from "./context.js";
import { madeFrom } from "./library.js";
import { SideButtons } from "./sideview.js";
import { pickFormat, rememberFormat } from "./settings.js";
import { bytes, FORMAT_LABEL } from "../lib/util.js";

const label = (f) => FORMAT_LABEL[f.format] || f.format;
const fileName = (f) => f.path.split("/").pop();
const readOnly = () => !!ctx.catalog?.library?.read_only;
const fail = (e) => ctx.toast(String(e.message || e));

/** The pack's credit line, like a model's: By … · Source · License. */
function Credit({ lib }) {
  const bits = [];
  if (lib.authors?.length) bits.push(`By ${lib.authors.map((a) => a.name).join(", ")}`);
  if (lib.source_url) bits.push(html`<a href=${lib.source_url} target="_blank" rel="noopener">Source</a>`);
  const spdx = lib.license?.spdx;
  // parts you've made from models: each is under the license of the project it was made from (UI-28)
  if (lib.id === "local-made-parts") bits.push("Each part keeps the license of the project it was made from");
  else bits.push(html`<a href=${`#/licenses/${lib.id}`}>${spdx && spdx !== "NOASSERTION" ? `License ${spdx}` : "License details"}</a>`);
  return html`<p id="lib-credit" class="credit">${bits.flatMap((x, i) => (i ? [" · ", x] : [x]))}</p>`;
}

/** The parts by category, by name with numbers as numbers (Brick 1x2 before 1x17), as the grid sorts (UI-29). */
function PartList({ lib, current, q }) {
  const nav = useRef(null);
  useEffect(() => {
    const el = nav.current, cur = el?.querySelector("a[aria-current]");
    if (!cur || el.scrollHeight <= el.clientHeight + 1) return;
    const top = cur.offsetTop - el.offsetTop;
    if (top < el.scrollTop || top > el.scrollTop + el.clientHeight - 80) el.scrollTop = top - el.clientHeight / 3;
  }, [current]);
  const groups = [...new Set(lib.items.map((i) => i.category))].map((c) => ({
    c,
    items: lib.items.filter((i) => i.category === c).sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }))
      .map((i) => ({ i, search: [i.name, c, ...(i.tags || [])].join(" ").toLowerCase() })),
  }));
  return html`<nav id="part-list" class="part-list" aria-label="Parts" ref=${nav}>${groups.map(({ c, items }) => {
    const shown = items.filter((x) => !q || x.search.includes(q));
    return html`<div class="part-group" data-cat=${c} hidden=${!shown.length}><h2>${c}</h2><ul>${items.map(({ i, search }) => html`<li hidden=${!!q && !search.includes(q)}>
      <a href=${`#/parts/${lib.id}/${i.id}`} data-id=${i.id} data-search=${search} aria-current=${i.id === current ? "true" : undefined}>
        <span>${i.name}</span><span class="formats">${[...new Set(i.files.map(label))].join(" ")}</span></a></li>`)}</ul></div>`;
  })}</nav>`;
}

/** Where the part came from: the model it was made from, the example it is, or its parametric model. */
function Origin({ lib, item }) {
  const made = madeFrom(item.made_from);
  if (made) {
    const link = `${made.href}?part=${lib.id}/${item.id}`;
    return html`<div class="made-from" id="made-from">
      <p class="help">${made.available ? made.text : `${made.text} That model isn't in the library now.`}</p>
      ${made.outdated ? html`<p class="help made-outdated">${made.outdated}</p>` : null}
      ${made.available ? html`<p class="made-actions"><a class="button secondary" id="made-open" href=${link}>Open with these settings</a>
        ${!readOnly() ? html`<a class="button secondary" id="made-again" href=${`${link}&again`} title="Make it with the same settings and replace its files, keeping the name">Make again</a>` : null}</p>` : null}
    </div>`;
  }
  if (item.example) {
    // one of a folder of examples made into parts: its own model prints this one size; the main file's model,
    // when it has settings, prints any (UI-29)
    const main = item.main_model && ctx.catalog?.models?.find((m) => m.key === item.main_model);
    return html`<p class="help"><a href=${`#/m/${item.generator}`} id="example-open">Open the example as a model</a>${main
      ? html`, or <a href=${`#/m/${item.main_model}`} id="example-main">open ${main.name}</a> to make it at other sizes.` : "."}</p>`;
  }
  if (item.generator) return html`<p class="help"><a href=${`#/m/${item.generator}`}>Open the parametric model</a> to print it at any size.</p>`;
  return null;
}

/** A part made from several files ("Make one part", milestone 5) can be split again. */
function Merged({ lib, item }) {
  if (!item.merged?.length || readOnly()) return null;
  const split = async () => {
    try {
      await ctx.platform.api("parts_merge", { source: lib.source_id || lib.id, ids: [item.id], split: true });
      await ctx.reloadCatalog();
      ctx.toast("Split into separate parts.");
    } catch (e) { fail(e); }
  };
  return html`<p class="help">Made one part from ${item.merged.length + 1} files. <button type="button" class="link-btn" id="part-split" onClick=${split}>Split into separate parts</button></p>`;
}

/** Download: one button with the format beside it, or a button per file. */
function Downloads({ lib, item }) {
  const key = `part:${lib.id}/${item.id}`;
  const formats = item.files.map((f) => f.format);
  const [fmt, setFmt] = useState(() => pickFormat(key, formats));
  useEffect(() => setFmt(pickFormat(key, formats)), [key]);
  if (formats.length > 1 && new Set(formats).size === formats.length && !item.files.some((f) => f.label)) {
    // one Download with the format beside it (milestone 5): the last one chosen for this part, else Settings' rule
    const f = item.files.find((x) => x.format === fmt) || item.files[0];
    const pick = (e) => { rememberFormat(key, e.target.value); setFmt(e.target.value); };
    return html`<div id="part-downloads" class="downloads"><span class="download-pick">
      <a class="button primary" id="part-download" href=${f.url} download=${fileName(f)} title=${fileName(f)}>Download (${bytes(f.bytes)})</a>
      <select class="format-pick" id="part-format" aria-label="Format" value=${f.format} onChange=${pick}>${item.files.map((x) => html`<option value=${x.format}>${label(x)}</option>`)}</select>
    </span></div>`;
  }
  if (item.files.length === 1 && !item.files[0].label) {
    // one file: the same Download, with its format named beside it (Review 6, UI-41)
    const f = item.files[0];
    return html`<div id="part-downloads" class="downloads"><span class="download-pick">
      <a class="button primary" id="part-download" href=${f.url} download=${fileName(f)} title=${fileName(f)}>Download (${bytes(f.bytes)})</a>
      <span class="format-label" id="part-format-label">${FORMAT_LABEL[f.format] || f.format.toUpperCase()}</span>
    </span></div>`;
  }
  return html`<div id="part-downloads" class=${`downloads${item.files.length > 4 ? " many" : ""}`}>${item.files.map((f, i) => html`<a
    class=${`button ${i === 0 && !["step", "shapr"].includes(f.format) ? "primary" : "secondary"}`} href=${f.url} download=${fileName(f)} title=${fileName(f)}>${f.label ? f.label + ": " : ""}${label(f)} (${bytes(f.bytes)})</a>`)}</div>`;
}

/** Add to collection…, and ⋯: Show in folder, Choose picture…, Add content… (0.4.23, milestone 6). */
function PartActions({ lib, item, reveal }) {
  const more = useRef(null);
  const ro = readOnly();
  const level = { source: lib.source_id || lib.id, level: "part", target: item.id, name: item.name };
  const run = (fn) => () => { if (more.current) more.current.open = false; fn(); };
  return html`<div class="part-row">
    <button type="button" id="part-collect" class="ghost" hidden=${ro} title="Add this part to one of your collections"
      onClick=${() => ui.set({ dialog: { type: "collect", entries: [{ ref: `part:${lib.id}/${item.id}`, name: item.name }] } })}>Add to collection…</button>
    <details class="menu" id="part-more" ref=${more}><summary class="ghost" aria-label="More" title="More">⋯</summary><div class="menu-panel">
      <button type="button" id="part-reveal" hidden=${!item.files.length} title="Show the part's file in its folder"
        onClick=${run(() => reveal({ source: level.source, rel: item.files[0].path, examples: !!item.example }))}>Show in folder</button>
      <button type="button" id="part-picture" disabled=${ro} title="Show another picture for it: one from its project, or your own"
        onClick=${run(() => ui.set({ dialog: { type: "picture", ...level, current: item.meta?.icon || null } }))}>Choose picture…</button>
      <button type="button" id="part-content" disabled=${ro} title="Add a file or a link to it: a build video, an assembly guide, photos"
        onClick=${run(() => ui.set({ dialog: { type: "content", ...level } }))}>Add content…</button></div></details>
  </div>`;
}

/**
 * The rail. `lib`: the pack (null: not found); `item`: the part shown; `partRef`: where the pack's
 * documents come from (the side viewer's buttons); `reveal(args)`: show a file in its folder.
 */
export function PartsRail({ lib, item, partRef, reveal }) {
  const [text, setText] = useState("");
  const q = text.trim().toLowerCase();
  if (!lib) {
    return html`<div class="rail-head"><h1 id="lib-title">Parts not found</h1><p id="lib-summary" class="muted">This collection doesn't exist. Go back to the catalog to pick one.</p></div>`;
  }
  return html`
    <div class="rail-head">
      <h1 id="lib-title">${lib.name}</h1>
      <p id="lib-summary" class="muted">${lib.summary || ""}</p>
      <${Credit} lib=${lib} />
      <div id="lib-extra" class="lib-extra">${partRef ? html`<${SideButtons} refObj=${partRef} />` : null}</div>
    </div>
    <div class="rail-tools">
      <label class="search small">
        <span class="visually-hidden">Find a part</span>
        <input id="part-search" type="search" placeholder="Find a part" autocomplete="off" value=${text} onInput=${(e) => setText(e.target.value)} />
      </label>
    </div>
    <${PartList} lib=${lib} current=${item?.id} q=${q} />
    ${item ? html`<div class="actionbar">
      <div id="part-status" class="part-status">
        <p class="part-name">${item.name}</p>
        ${item.description && item.description.trim() !== (lib.summary || "").trim() ? html`<p class="help">${item.description}</p>` : null}
        <${Origin} lib=${lib} item=${item} />
        <${Merged} lib=${lib} item=${item} />
      </div>
      <${Downloads} lib=${lib} item=${item} />
      <${PartActions} lib=${lib} item=${item} reveal=${reveal} />
    </div>` : null}`;
}
