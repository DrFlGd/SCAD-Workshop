// The head of a model's (or component's) page: its name, project, one-line summary and credit
// line, then the row with Code and README, About this model, and ⋯ (this item's own actions, then
// Batch generate and Help); and under it (ModelNotes) the author's notes, your own notes and the
// warnings above the form. app.js draws the form and the 3D view. (Review 7, item 13: moved from
// app.js to Preact.)
import { html, useEffect, useLayoutEffect, useRef, useState } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui } from "./state.js";
import { ctx } from "./context.js";
import { sideModelButtons } from "./sideview.js";
import { STATUS_TEXT } from "../lib/util.js";
import { when } from "./library.js";

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
    own.push(btn("model-notes-open", "Your notes", "Write your own notes on this model (kept in the library)", () => notesHandle.own?.()),
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
 * `hasNotes`: the author wrote notes (About this model opens them); `act`: { reveal(args),
 * copyCode, pin, batch, help } from app.js.
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
      <button type="button" id="model-about" class="ghost small" hidden=${!hasNotes} title="The author's notes on this model" onClick=${() => notesHandle.about?.()}>About this model</button>
      <details class="menu" id="model-more" ref=${more}><summary class="ghost small" title="More" aria-label="More">⋯</summary>
        <div class="menu-panel" onClick=${chosen}>
          <${OwnActions} detail=${detail} act=${act} />
          <button type="button" id="batch-open" title="Make many variations at once: download them as a ZIP, or keep them in the Parts Library" onClick=${act.batch}>Batch generate…</button>
          <button type="button" id="help-toggle" aria-pressed=${String(help)} title="Show the explanation under each setting" onClick=${act.help}>Help under each setting</button>
        </div>
      </details>
    </div>`;
}

// ---------------------------------------------------------------- notes and warnings
// About this model and ⋯ › Your notes open the notes ModelNotes draws
const notesHandle = {};

/** Nodes made elsewhere (the author's sanitised HTML, whose display conditions app.js applies), in a box. */
function Nodes({ nodes, ...attrs }) {
  const box = useRef(null);
  useLayoutEffect(() => { box.current?.replaceChildren(...nodes); }, [nodes]);
  return html`<div ref=${box} ...${attrs}></div>`;
}

/** Every notice that applies, stacked (Review 3, UI-26): red for broken or failed, the note style for heads-ups. */
function notices(detail, act) {
  const out = [];
  // flagged as broken in the library
  const broken = detail.meta?.broken;
  if (broken) out.push(html`<div class="alert-broken" role="note">Flagged as broken${broken.date ? ` on ${when(broken.date)}` : ""}${broken.note ? `: ${broken.note}` : "."}${" "}<button
    type="button" class="link-btn" onClick=${act.unflag}>Remove the flag</button></div>`);
  // rendered at its defaults when the project was added, and failed
  if (detail.check?.ok === false) out.push(html`<div class="alert-broken" role="note" id="default-check">Didn't render at its default settings when it was checked${detail.check.message
    ? `: ${detail.check.message.replace(/[^.!?]$/, "$&.")}` : "."} Changing the settings may help; Check defaults in the project's ⋯ menu tries again.</div>`);
  // a pinned component whose library is another copy or version now (F-9)
  if (detail.pin_changed) out.push(html`<div class="alert-note" role="note" id="pin-changed">${detail.pin_changed.message}${" "}<button type="button" class="link-btn"
    onClick=${act.keepPin}>Keep it with this copy</button> · <a href="#/library-settings/libraries">Choose which copy is used (Library settings)</a></div>`);
  // defaults the form's own limits don't allow (D-3)
  if (detail.limits?.length) out.push(html`<div class="alert-note" role="note" id="limits-note">Its defaults are outside the form's limits: ${detail.limits.join("; ")}. The form keeps them as they are until you change them.</div>`);
  return out;
}

/**
 * Under the head: the author's notes (About this model opens them; closed, they're hidden again),
 * your own notes (0.4.23: shown open when there are some, else from ⋯ › Your notes; saved when the
 * box loses focus), and the warnings above the form, the author's own among them. `notes` and
 * `alerts`: the author's sanitised nodes; `own`: { can, readOnly, save(text) }; `act`: { unflag, keepPin }.
 */
export function ModelNotes({ detail, notes, alerts, own, act }) {
  const saved = detail.meta?.my_notes || "";
  const [about, setAbout] = useState(false);
  const [ownOpen, setOwnOpen] = useState(!!saved);
  const notesBox = useRef(null), ownText = useRef(null), asked = useRef(null);
  notesHandle.about = () => { asked.current = "about"; setAbout(true); };
  notesHandle.own = () => { asked.current = "own"; setOwnOpen(true); };
  useEffect(() => {
    if (asked.current === "about") notesBox.current?.scrollIntoView({ block: "nearest" });
    if (asked.current === "own") ownText.current?.focus();
    asked.current = null;
  });
  // the saved text, unless it's being typed in
  useLayoutEffect(() => { const t = ownText.current; if (t && document.activeElement !== t) t.value = saved; }, [saved]);
  const warn = notices(detail, act);
  return html`
    <details id="model-notes" class="model-notes" hidden=${!about} open=${about} ref=${notesBox} onToggle=${(e) => { if (!e.target.open) setAbout(false); }}>
      <summary>About this model</summary><${Nodes} id="model-notes-body" nodes=${notes} /></details>
    <details id="model-own-notes" class="model-notes own-notes" hidden=${!own.can || (!saved && !ownOpen)} open=${ownOpen}
      onToggle=${(e) => setOwnOpen(e.target.open)}><summary>Your notes</summary>
      <div><textarea id="model-own-notes-text" rows="5" aria-label="Your notes" ref=${ownText} disabled=${own.readOnly} onChange=${(e) => own.save(e.target.value)}></textarea>
      <p class="help" id="model-own-notes-state">Kept with the model in the library, and found by search. Saved when you click away.</p></div></details>
    <div id="model-alerts" class="model-alerts" aria-live="polite" hidden=${!warn.length && !alerts.length}>${warn}<${Nodes} class="contents" nodes=${alerts} /></div>`;
}
