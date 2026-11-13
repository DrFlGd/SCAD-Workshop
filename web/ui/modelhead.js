// The head of a model's (or component's) page: its name, project, one-line summary and credit
// line, then the row with Code and README, About this model, and ⋯ (this item's own actions, then
// Batch generate and Help). app.js draws the rest of the page: the author's notes and warnings,
// the form and the 3D view. (Review 7, item 13: moved from app.js to Preact.)
import { html, useEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx } from "./context.js";
import { sideModelButtons } from "./sideview.js";
import { STATUS_TEXT } from "../lib/util.js";

const link = (href, text, out = true) => (out ? html`<a href=${href} target="_blank" rel="noopener">${text}</a>` : html`<a href=${href}>${text}</a>`);

/** The model's project (a link to it), and a component's module and which copy of its library (UI-25). */
function Project({ detail }) {
  const c = detail.component;
  const from = c ? c.library || detail.family_name : detail.family_name;
  const copy = c ? (c.provider === "project" ? "your copy" : "the app's copy") : "";
  return html`<p id="model-project" class="model-project">${detail.source_id
    ? html`<a href=${`#/browse/source/${encodeURIComponent(detail.source_id)}`} title="The project it's from">${from}</a>` : from || ""}${c
    ? html` · <code>${c.module}()</code> · ${copy}` : null}</p>`;
}

/** One credit line on every page: By … · Source · License · Docs. */
function Credit({ detail }) {
  const c = detail.component;
  const parts = [];
  if (detail.authors?.length) parts.push(["By ", ...detail.authors.flatMap((a, i) => [
    i ? (i === detail.authors.length - 1 ? " and " : ", ") : "", a.url ? link(a.url, a.name) : a.name])]);
  if (detail.source?.repository) parts.push([link(detail.source.repository, "Source")]);
  // always "License <name>" (or "not stated"), linked, and "Check" when it needs checking before sharing prints
  const lic = detail.license || {};
  const licId = lic.spdx && lic.spdx !== "NOASSERTION" ? lic.spdx : null;
  const licHref = c ? `#/licenses/components-${encodeURIComponent(c.library)}` : `#/licenses/${detail.family}`;
  parts.push([link(licHref, `License ${licId || "not stated"}`, false),
    ...(lic.public_use && lic.public_use !== "ok" ? [" · ", html`<a href=${licHref} class="credit-check" id="credit-check" title=${STATUS_TEXT[lic.public_use] || "Check the license"}>Check</a>`] : [])]);
  if (detail.links?.docs) parts.push([link(detail.links.docs, "Docs")]);
  if (detail.pinned_from) parts.push(["Pinned from ", link(`#/m/${detail.pinned_from}`, "its component", false)]);
  return html`<p id="model-credit" class="credit">${parts.flatMap((part, i) => [i ? " · " : "", ...part]).filter((x) => x !== "")}</p>`;
}

/** ⋯'s first group: this item's own actions (in the library: edit the form, notes, picture, content;
 * a component's code and pin). */
function OwnActions({ detail, act }) {
  const ro = !!ctx.catalog?.library?.read_only;
  const it = ctx.index?.get(`gen:${detail.key}`);
  const btn = (id, text, title, onClick, disabled = false) => html`<button type="button" id=${id} title=${title} disabled=${disabled} onClick=${onClick}>${text}</button>`;
  const own = [];
  if (it?.sourceId) {
    own.push(btn("edit-form", "Edit form…", "Rename settings, add help, choices and conditions, hide settings (saved with this item in the library)",
      () => ui.set({ dialog: { type: "form", key: detail.key } }), ro));
  }
  // your notes, a picture of its own and content you add (0.4.23, milestone 6)
  if (it?.sourceId && detail.kind !== "component" && !detail.pinned_from) {
    const level = { source: it.sourceId, level: "item", target: detail.id, name: detail.name };
    own.push(btn("model-notes-open", "Your notes", "Write your own notes on this model (kept in the library)", act.ownNotes),
      btn("model-picture", "Choose picture…", "Show another picture for it: one from its project, or your own",
        () => ui.set({ dialog: { type: "picture", ...level, current: detail.meta?.icon || null } }), ro),
      btn("model-content", "Add content…", "Add a file or a link to it: a build video, an assembly guide, photos", () => ui.set({ dialog: { type: "content", ...level } }), ro));
  }
  // Show in folder: the model's .scad file where it is on disk (0.4.23)
  if (detail.source_id && detail.entry) {
    own.push(btn("model-reveal", "Show in folder", "Show the model's .scad file in its folder", () => act.reveal({ source: detail.source_id, rel: detail.entry })));
  }
  if (detail.kind === "component") {
    own.push(btn("copy-code", "Copy code", "Copy the OpenSCAD code for these settings (includes the library, calls the module)", act.copyCode),
      btn("pin-component", "Pin as a model…", "Keep this component under Parametric Models, with these settings as its defaults", act.pin, ro));
  }
  return html`<div id="model-more-own" class="menu-group">${own}${own.length ? html`<hr />` : null}</div>`;
}

/**
 * The head. `summary`: the line under the name (the model's, or its notes' first sentence);
 * `hasNotes`: the author wrote notes (About this model opens them); `act`: { about, ownNotes,
 * reveal(args), copyCode, pin, batch } from app.js.
 */
export function ModelHead({ detail, summary, hasNotes, act }) {
  const help = useStore(ui, (s) => !!s.help);
  const side = useRef(null), more = useRef(null);
  // the side viewer's buttons (Code, README, content you added), drawn by sideview.js
  useEffect(() => {
    if (!side.current) return;
    side.current.replaceChildren();
    sideModelButtons(side.current, detail);
  }, [detail]);
  // ⋯ closes for another model, but stays open when this one's head is drawn again (a catalog reload)
  useEffect(() => { if (more.current) more.current.open = false; }, [detail.key]);
  // ⋯ closes once something in it is chosen
  const chosen = (e) => { if (e.target.closest("button") && more.current) more.current.open = false; };
  return html`
    <h1 id="model-title">${detail.name}</h1>
    <${Project} detail=${detail} />
    <p id="model-summary" class="muted">${summary}</p>
    <${Credit} detail=${detail} />
    <div class="model-extra">
      <span id="model-extra" class="model-extra-side" ref=${side}></span>
      <button type="button" id="model-about" class="ghost small" hidden=${!hasNotes} title="The author's notes on this model" onClick=${act.about}>About this model</button>
      <details class="menu" id="model-more" ref=${more}><summary class="ghost small" title="More" aria-label="More">⋯</summary>
        <div class="menu-panel" onClick=${chosen}>
          <${OwnActions} detail=${detail} act=${act} />
          <button type="button" id="batch-open" title="Make many variations at once: download them as a ZIP, or keep them in the Parts Library" onClick=${act.batch}>Batch generate…</button>
          <button type="button" id="help-toggle" aria-pressed=${String(help)} title="Show the explanation under each setting" onClick=${act.help}>Help under each setting</button>
        </div>
      </details>
    </div>`;
}
