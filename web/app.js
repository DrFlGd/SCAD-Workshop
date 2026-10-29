// SCAD Workshop front end (static): catalog, settings forms with
// upstream editor metadata, in-browser OpenSCAD renders, 3D preview, parts.
import { Viewer } from "./viewer.js";
import { createPlatform } from "./platform.js";
import { makeZip } from "./zip.js";
import { changedValues, withChanges, encodeShare, decodeShare, toOpenSCAD, fromOpenSCAD } from "./settings-codec.js";
import { editsKey } from "./lib/edits.js";
import { $, el, humanize, fmt, same, bytes, plural, safeHTML, compileCondition, STATUS_TEXT, spdx, FORMAT_LABEL } from "./lib/util.js";
import { LocalIndex } from "./ui/index-local.js";
import { mountShell } from "./ui/shell.js";
import { sideModelButtons, openSide, SideButtons } from "./ui/sideview.js";
import { setContext } from "./ui/context.js";
import { html, render } from "./lib/html.js";
import { LibrarySettingsPage } from "./ui/libsettings.js";
import { unflagItems } from "./ui/actions.js";
import { saveComponentThumb, when, sourceById, rescanLocal, madeFrom, runJob } from "./ui/library.js";
import { ui, recordRecent, setTabs, layoutFor, isDark, setTheme, THEMES, setSkin, SKINS } from "./ui/state.js";
import { scopeFromPath } from "./ui/context.js";
import { scopeInfo } from "./ui/sidebar.js";
import { refreshCollections } from "./ui/collections.js";

// page errors, for the desktop UI test (tests/desktop_ui.py)
// (not the browser's harmless "ResizeObserver loop completed with undelivered notifications")
window.addEventListener("error", (e) => { if (!/ResizeObserver loop/.test(String(e.message))) (window.__errors ||= []).push(String(e.message)); });
window.addEventListener("unhandledrejection", (e) => { (window.__errors ||= []).push(String(e.reason?.message || e.reason)); });

const platform = await createPlatform();
const store = platform.store;

const getJSON = async (path) => {
  const r = await platform.fetch(path);
  if (!r.ok) throw Object.assign(new Error(`Couldn't load ${path} (${r.status})`), { status: r.status });
  return r.json();
};

const FILAMENTS = [
  ["Tool yellow", "#f2b705"], ["Signal blue", "#2f6fd0"], ["Galaxy grey", "#6b7680"],
  ["Orange", "#ee6a1f"], ["Green", "#3c9a5f"], ["White", "#f4f4f2"], ["Black", "#26292c"],
];
const GRID_CATEGORIES = new Set(["gridfinity"]);

const state = {
  catalog: null, engine: null, viewer: null, viewerOwner: null,
  model: null, values: {}, rendered: null, lastResult: null, job: null, conditions: [],
  libraries: [], libDetail: {}, openLib: null, downloadUrl: null,
};

// ---------------------------------------------------------------- helpers
const savedColor = store.prefs.get("gw-filament") || FILAMENTS[0][1];

// ---------------------------------------------------------------- routing
function route() {
  const h = location.hash.replace(/^#\/?/, "");
  const [path, query = ""] = h.split("?");
  const params = new URLSearchParams(query);
  // model keys: "<project>/<model>", or "@<library>/<module>" for a component
  const m = path.match(/^m\/(@?[a-z0-9-]+)\/([A-Za-z0-9_-]+)\/?$/);
  const p = path.match(/^parts\/([a-z0-9-]+)(?:\/([a-z0-9-]+))?\/?$/);
  if (m) openModel(`${m[1]}/${m[2]}`, params.get("s"), params.get("saved"), params.get("part") ? { part: params.get("part"), again: params.has("again") } : null);
  else if (p) openLibrary(p[1], p[2]);
  else if (path === "about" || path.startsWith("licenses")) showLicenses(path.split("/")[1]);
  else if (path === "settings") showSettings();
  else if (path === "library-settings" || path.startsWith("library-settings/")) showLibrarySettings(path.split("/")[1]);
  else if (path === "search") showBrowse("search", params.get("q") || "");
  else if (path.startsWith("browse")) showBrowse(scopeFromPath(path), params.get("q") || "");
  else showBrowse("home", "");
}
window.addEventListener("hashchange", route);

function showView(name) {
  for (const v of ["browse", "model", "library", "about"]) $(`#view-${v}`).hidden = v !== name;
  ui.set({ view: name, navOpen: false });
  const stage = $(".stage");
  if (name === "model" && stage.parentElement.id !== "view-model") $("#view-model").append(stage);
  if (name === "library" && stage.parentElement.id !== "view-library") $("#view-library").append(stage);
  if ((name === "model" || name === "library") && !state.viewer) {
    state.viewer = new Viewer($("#viewer"), { coverFrom: ".side-panel" });
    // a solid shown, or nothing: a 2D model's drawing goes (0.4.23)
    for (const m of ["clear", "load"]) {
      const f = state.viewer[m].bind(state.viewer);
      state.viewer[m] = (...a) => { showDrawing(null); return f(...a); };
    }
    window.addEventListener("gw-side-over", () => requestAnimationFrame(() => state.viewer.resize()));
    state.viewer.setColor(savedColor);
    state.viewer.setTheme(isDark());
    window.addEventListener("gw-theme", () => state.viewer.setTheme(isDark()));
  }
  state.viewer?.resize();
  if (name !== "model") cancelJob();
}

function showBrowse(scope, q) {
  if (!state.catalog) return;
  showView("browse");
  const st = ui.get();
  const place = st.scope !== scope;
  ui.set({ scope, q, layout: layoutFor(scope), lastBrowse: location.hash || "#/", ...(place ? { selection: [], anchor: null, filters: {}, openGroup: null, sourceTab: "items" } : {}) });
  const label = scope === "home" ? "" : q ? `“${q}”` : scopeInfo(scope).label;
  document.title = label ? `${label} | SCAD Workshop` : "SCAD Workshop";
  if (place) document.querySelector(".browse-scroll")?.scrollTo(0, 0);
}

// ---------------------------------------------------------------- model page
// Open generators are tabs; each keeps its settings and last result while you work in another.
const tabStates = new Map();
const details = new Map();

function addTab(key) {
  const tabs = ui.get().tabs.includes(key) ? ui.get().tabs : [...ui.get().tabs, key];
  setTabs(tabs, key);
}

function stashTab() {
  if (!state.model) return;
  const st = $("#status");
  tabStates.set(state.model.key, {
    model: state.model, values: state.values, rendered: state.rendered, renderedEdits: state.renderedEdits, lastResult: state.lastResult,
    lastResultValues: state.lastResultValues, savedActive: saved.active, status: [st.className.replace("status", "").trim(), st.textContent],
  });
}

// ---------------------------------------------------------------- code edits
// The side viewer's Code tab edits an open model's files: { "/path.scad": text } per model,
// rendered in place of the originals (never written over them) and kept with saved settings
// (`edits: { path: { text, base } }`, base = the original's hash). docs/DESKTOP_PLAN.md, "Editing the code".
const codeEdits = new Map();
const editsOf = (key) => codeEdits.get(key) || {};
const hasEdits = (key) => Object.keys(editsOf(key)).length > 0;
const editTexts = (recEdits) => Object.fromEntries(Object.entries(recEdits || {}).map(([p, e]) => [p, typeof e === "string" ? e : e.text]));

function setEdits(key, edits) {
  if (edits && Object.keys(edits).length) codeEdits.set(key, edits);
  else codeEdits.delete(key);
  ui.set((s) => ({ codeRev: (s.codeRev || 0) + 1 }));
  if (state.model?.key === key) { updateStatusForEdits(); renderSettingsPick(); renderCodeNote(); }
}

/** Above the form while the code is edited: where a setting's default now comes from. */
function renderCodeNote() {
  const n = $("#code-note");
  if (!n || !state.model) return;
  const files = Object.keys(editsOf(state.model.key)).length;
  n.hidden = !files;
  if (files) n.replaceChildren(`Code edited (${plural(files, "file")}): settings left at their defaults take the code's values. `,
    el("button", { type: "button", class: "link-btn", text: "Show the code", onclick: () => openSide("code", { sideFile: null }) }));
}

/** One file's edit (null: back to the original). */
function setCodeEdit(key, path, text) {
  const e = { ...editsOf(key) };
  if (text == null) delete e[path];
  else e[path] = text;
  setEdits(key, e);
}

/** "none", "saved" (the saved settings on screen have these edits) or "unsaved". */
function codeStatus(key) {
  if (!hasEdits(key)) return "none";
  const rec = state.model?.key === key ? activeSaved() : null;
  return rec?.edits && editsKey(editTexts(rec.edits)) === editsKey(editsOf(key)) ? "saved" : "unsaved";
}

/** The model as the engines get it: its edited files go in place of the originals. */
const forRender = (model) => (hasEdits(model.key) ? { ...model, edits: editsOf(model.key) } : model);
/** The values the engines get: while the code is edited, only those changed from the defaults, so a
 *  default changed in the code applies unless the form changes it. */
const renderValues = (model, values) => (hasEdits(model.key) ? changedValues(model, values) : values);

function closeModelTab(key) {
  const tabs = ui.get().tabs;
  const i = tabs.indexOf(key);
  const rest = tabs.filter((k) => k !== key);
  tabStates.delete(key);
  codeEdits.delete(key);
  const showing = ui.get().view === "model" && state.model?.key === key;
  if (state.model?.key === key) { cancelJob(); state.model = null; }
  setTabs(rest, showing ? null : ui.get().activeTab);
  if (showing) location.hash = rest.length ? `#/m/${rest[Math.min(i, rest.length - 1)]}` : ui.get().lastBrowse || "#/";
}

function openTabs(keys) {
  setTabs([...new Set([...ui.get().tabs, ...keys])]);
  if (keys.length) location.hash = `#/m/${keys[0]}`;
}

/** One of a folder of examples made into parts (F-15): not listed as a model, but its page opens. */
const isExampleModel = (key) => (state.catalog?.examples_todo || []).some((e) => e.key === key)
  || state.libraries.some((l) => l.items.some((i) => i.example && i.generator === key));

async function loadDetail(key) {
  // a model that's no longer in the library (an open tab, a list made before a project was read again):
  // say so without asking for it
  if (platform.kind === "desktop" && !key.startsWith("@") && state.index && !state.index.get(`gen:${key}`) && !isExampleModel(key)) {
    throw Object.assign(new Error(`${key} isn't in the library`), { status: 404 });
  }
  if (!details.has(key)) {
    // (on the desktop, asked for in a way that answers "not there" without a failed request)
    const d = platform.kind === "desktop" ? await platform.api("model_get", { key }) : await getJSON(`data/models/${key.replace("/", "--")}.json`);
    if (!d) throw Object.assign(new Error(`${key} isn't in the library`), { status: 404 });
    applyProfile(d);
    details.set(key, d);
  }
  return details.get(key);
}

/** For quick look: a model's built JSON and its default settings (profile applied). */
async function loadModel(key) {
  const detail = await loadDetail(key);
  return { detail, values: Object.fromEntries(detail.parameters.map((p) => [p.name, structuredClone(p.default)])) };
}

function renderModelHeader(detail) {
  document.title = `${detail.name}, ${detail.family_name} | SCAD Workshop`;
  // the model's own name, as on its card and tab, with its project (or library) on the line under it
  $("#model-title").textContent = detail.name;
  const c = detail.component;
  const from = c ? c.library || detail.family_name : detail.family_name;
  // a component's module and which copy of its library, beside the library's name (UI-25)
  const copy = c ? (c.provider === "project" ? "your copy" : "the app's copy") : "";
  $("#model-project").replaceChildren(detail.source_id
    ? el("a", { href: `#/browse/source/${encodeURIComponent(detail.source_id)}`, text: from, title: "The project it's from" })
    : from || "", ...(c ? [" · ", el("code", { text: `${c.module}()` }), ` · ${copy}`] : []),
    // a Fusion 360 file, generated with MitCad (experimental, 0.4.27)
    ...(detail.engine === "mitcad" ? [" · ", el("span", { id: "model-engine", title: "A Fusion 360 file (.f3d). Its settings are the file's user parameters; Generate rebuilds it with MitCad, which you install separately." }, "Fusion 360 file, made with MitCad (experimental)")] : []));
  $("#model-summary").textContent = detail.summary;
  // one credit line on every page: By … · Source · License · Docs
  const link = (href, text, out = true) => el("a", { href, text, ...(out ? { target: "_blank", rel: "noopener" } : {}) });
  const credit = [];
  if (detail.authors?.length) credit.push(["By ", ...detail.authors.flatMap((a, i) => [
    i ? (i === detail.authors.length - 1 ? " and " : ", ") : "", a.url ? link(a.url, a.name) : a.name])]);
  if (detail.source?.repository) credit.push([link(detail.source.repository, "Source")]);
  // always "License <name>" (or "not stated"), linked, and "Check" when it needs checking before sharing prints
  const lic = detail.license || {};
  const licId = lic.spdx && lic.spdx !== "NOASSERTION" ? lic.spdx : null;
  const licHref = c ? `#/licenses/components-${encodeURIComponent(c.library)}` : `#/licenses/${detail.family}`;
  credit.push([link(licHref, `License ${licId || "not stated"}`, false),
    ...(lic.public_use && lic.public_use !== "ok" ? [" · ", el("a", { href: licHref, class: "credit-check", id: "credit-check", text: "Check", title: STATUS_TEXT[lic.public_use] || "Check the license" })] : [])]);
  if (detail.links?.docs) credit.push([link(detail.links.docs, "Docs")]);
  if (detail.pinned_from) credit.push(["Pinned from ", link(`#/m/${detail.pinned_from}`, "its component", false)]);
  $("#model-credit").replaceChildren(...credit.flatMap((part, i) => [i ? " · " : "", ...part]).filter((x) => x !== ""));
  renderModelExtra(detail);
  renderOwnNotes(detail);
  // author notes fold away; their conditional warnings stay visible above the form
  const notesNodes = detail.description_html ? safeHTML(detail.description_html) : [];
  const holder = document.createElement("div");
  holder.append(...notesNodes);
  const alerts = [...holder.querySelectorAll("[class^=alert]")];
  alerts.forEach((a) => a.remove());
  // every notice that applies, stacked (Review 3, UI-26): red for broken or failed, the note style for heads-ups
  const notices = [];
  // flagged as broken in the library (desktop)
  const broken = detail.meta?.broken;
  if (broken) notices.push(el("div", { class: "alert-broken", role: "note" }, `Flagged as broken${broken.date ? ` on ${when(broken.date)}` : ""}${broken.note ? `: ${broken.note}` : "."} `,
    el("button", { type: "button", class: "link-btn", text: "Remove the flag", onclick: async () => {
      const it = state.index.get(`gen:${detail.key}`);
      if (it) await unflagItems([it]);
    } })));
  // rendered at its defaults when the project was added (desktop), and failed
  if (detail.check?.ok === false) notices.push(el("div", { class: "alert-broken", role: "note", id: "default-check" },
    `Didn't render at its default settings when it was checked${detail.check.message ? `: ${detail.check.message.replace(/[^.!?]$/, "$&.")}` : "."} Changing the settings may help; Check defaults in the project's ⋯ menu tries again.`));
  // a pinned component whose library is another copy or version now (F-9)
  if (detail.pin_changed) notices.push(el("div", { class: "alert-note", role: "note", id: "pin-changed" }, `${detail.pin_changed.message} `,
    el("button", { type: "button", class: "link-btn", text: "Keep it with this copy", onclick: async () => {
      try { await platform.api("pin_accept", { key: detail.key }); await reloadCatalog(); refreshModel(detail.key); } catch (e) { toast(String(e.message || e)); }
    } }), " · ", el("a", { href: "#/library-settings/libraries", text: "Choose which copy is used (Library settings)" })));
  // defaults the form's own limits don't allow (D-3)
  if (detail.limits?.length) notices.push(el("div", { class: "alert-note", role: "note", id: "limits-note" },
    `Its defaults are outside the form's limits: ${detail.limits.join("; ")}. The form keeps them as they are until you change them.`));
  alerts.unshift(...notices);
  $("#model-alerts").replaceChildren(...alerts);
  const hasNotes = !!holder.textContent.trim();
  // every page has a one-line summary under its name: a component (or a model without one) takes its notes' first sentence
  if (!detail.summary && hasNotes) {
    const text = holder.textContent.replace(/\s+/g, " ").trim();
    const first = (text.match(/^.{20,}?[.!?](?=\s|$)/) || [text])[0];
    $("#model-summary").textContent = first.length > 180 ? `${first.slice(0, 177).trimEnd()}…` : first;
  }
  $("#model-notes-body").replaceChildren(...holder.childNodes);
  // the author's notes open from About this model, beside Code and README (0.4.26)
  $("#model-notes").hidden = true;
  $("#model-notes").open = false;
  $("#model-about").hidden = !hasNotes;
}

/** The row above the form: the side viewer (Code, README), then ⋯ with this item's own actions (in the
 * app: edit the form; a component's code and pin) above Batch generate and Help. */
function renderModelExtra(detail) {
  const box = $("#model-extra");
  if (!box) return;
  box.replaceChildren();
  sideModelButtons(box, detail);
  const own = $("#model-more-own");
  own.replaceChildren();
  // ⋯ closes for another model, but stays open when this one's header is drawn again (a catalog reload)
  const more = $("#model-more");
  if (more.dataset.key !== detail.key) more.open = false;
  more.dataset.key = detail.key;
  if (platform.kind !== "desktop") return;
  const ro = !!state.catalog?.library?.read_only;
  const btn = (id, text, title, onclick, disabled = false) => el("button", { type: "button", id, text, title, onclick, disabled });
  const it = state.index?.get(`gen:${detail.key}`);
  if (it && it.sourceId) {
    own.append(btn("edit-form", "Edit form…", "Rename settings, add help, choices and conditions, hide settings (saved with this item in the library)", () =>
      ui.set({ dialog: { type: "form", key: detail.key } }), ro));
  }
  // your notes, a picture of its own and content you add (0.4.23, milestone 6)
  if (it && it.sourceId && detail.kind !== "component" && !detail.pinned_from) {
    const level = { source: it.sourceId, level: "item", target: detail.id, name: detail.name };
    own.append(btn("model-notes-open", "Your notes", "Write your own notes on this model (kept in the library)", () => {
      const box = $("#model-own-notes");
      box.hidden = false;
      box.open = true;
      $("#model-own-notes-text").focus();
    }), btn("model-picture", "Choose picture…", "Show another picture for it: one from its project, or your own", () =>
      ui.set({ dialog: { type: "picture", ...level, current: detail.meta?.icon || null } }), ro),
    btn("model-content", "Add content…", "Add a file or a link to it: a build video, an assembly guide, photos", () =>
      ui.set({ dialog: { type: "content", ...level } }), ro));
  }
  // Show in folder: the model's .scad file where it is on disk (0.4.23)
  if (detail.source_id && detail.entry) {
    own.append(btn("model-reveal", "Show in folder", "Show the model's .scad file in its folder", () => revealFile({ source: detail.source_id, rel: detail.entry })));
  }
  if (detail.kind === "component") {
    own.append(btn("copy-code", "Copy code", "Copy the OpenSCAD code for these settings (includes the library, calls the module)", copyComponentCode),
      btn("pin-component", "Pin as a model…", "Keep this component under Parametric Models, with these settings as its defaults", () =>
        ui.set({ dialog: { type: "pin", key: detail.key, values: structuredClone(state.values) } }), ro));
  }
  if (own.childNodes.length) own.append(el("hr"));
}
// your own notes on a model (0.4.23): in its metadata (my_notes), saved when the box loses focus; shown open
// when there are some, else from ⋯ › Your notes
function renderOwnNotes(detail) {
  const box = $("#model-own-notes"), text = $("#model-own-notes-text");
  const it = state.index?.get(`gen:${detail.key}`);
  const can = platform.kind === "desktop" && !!it?.sourceId && detail.kind !== "component" && !detail.pinned_from;
  const saved = detail.meta?.my_notes || "";
  if (box.dataset.key !== detail.key) {
    box.dataset.key = detail.key;
    box.open = !!saved;
  }
  box.hidden = !can || (!saved && !box.open);
  if (document.activeElement !== text) text.value = saved;
  text.disabled = !!state.catalog?.library?.read_only;
  text.onchange = async () => {
    const v = text.value;
    try {
      await platform.api("meta_set", { source: it.sourceId, level: "item", target: detail.id, patch: { my_notes: v.trim() ? v : null } });
      await reloadCatalog();
      toast(v.trim() ? "Notes saved." : "Notes removed.");
    } catch (e) { toast(String(e.message || e)); }
  };
}
$("#model-own-notes").addEventListener("toggle", (e) => { if (!e.target.open && !$("#model-own-notes-text").value.trim()) e.target.hidden = true; });

// ⋯ closes once something in it is chosen; About this model opens the author's notes above the form
$("#model-more").addEventListener("click", (e) => { if (e.target.closest(".menu-panel button")) $("#model-more").open = false; });
$("#model-about").addEventListener("click", () => {
  const notes = $("#model-notes");
  notes.hidden = false;
  notes.open = true;
  notes.scrollIntoView({ block: "nearest" });
});
$("#model-notes").addEventListener("toggle", (e) => { if (!e.target.open) e.target.hidden = true; });

// a project's file (or the project's folder when the file isn't there) in the system's file manager
function revealFile(args) {
  platform.api("reveal_path", args).then((p) => platform.reveal(p)).catch((e) => toast(String(e.message || e)));
}

async function copyComponentCode() {
  try {
    const code = await platform.api("component_code", { key: state.model.key, values: state.values });
    await navigator.clipboard.writeText(code).catch(() => { throw new Error("The clipboard isn't available."); });
    toast("OpenSCAD code copied. It needs the library in OpenSCAD's library folder.");
  } catch (e) {
    toast(`Couldn't copy the code: ${e.message || e}`);
  }
}

/** Open a model again from the library (its form changed). */
function refreshModel(key) {
  tabStates.delete(key);
  details.delete(key);
  if (state.model?.key === key && ui.get().view === "model") {
    cancelJob();
    state.model = null;
    openModel(key);
  }
}

/** The settings on the open model's page (the form editor's "Add as a preset"). */
const currentValues = (key) => (state.model?.key === key ? structuredClone(state.values) : null);

/** A component that needs values before it can be made: the settings still empty. */
const missingValues = (model, values) => (model.needs || []).filter((n) => values[n] == null || values[n] === "");

async function openModel(key, shareCode = null, savedId = null, made = null) {
  if (!state.catalog) return;
  showView("model");
  // a made part's "Open with these settings" or "Make again": its recipe, applied once the model is open
  const recipe = made && madeRecipe(made, key);
  if (shareCode || savedId || made) history.replaceState(null, "", `#/m/${key}`); // the link has done its job; edits shouldn't look shared
  if (state.model?.key === key) {
    addTab(key);
    if (recipe) {
      state.viewerOwner = "model";
      $("#stage-empty").hidden = true;
      applyRecipe(recipe);
    } else if (shareCode) {
      const shared = await readShare(shareCode);
      if (shared && !shared.error) { setAllValues(shared.values); generate(); }
      if (shared) noteShared(shared);
    } else if (savedId) {
      await loadSaved();
      chooseSettings(`saved:${savedId}`);
    } else if (state.viewerOwner !== "model") {
      state.viewerOwner = "model";
      $("#stage-empty").hidden = true;
      if (state.lastResult) showResult(state.lastResult, state.lastResultValues, true);
      else { state.viewer.clear(); $("#dims").hidden = true; }
    }
    return;
  }
  let detail;
  try {
    detail = await loadDetail(key);
  } catch (e) {
    showView("model");
    setStatus("error", e.status === 404 ? (platform.kind === "browser" && state.desktopOnly?.has(key)
      ? "This model is too complex for the browser engine. It works in the desktop app."
      : "This model doesn't exist. Pick one from the library.") : e.message);
    return;
  }
  if (detail.browser === false && platform.kind === "browser") {
    setStatus("error", `${detail.family_name} ${detail.name} is too complex for the browser engine. It works in the desktop app.`);
    return;
  }
  if (state.model) { cancelJob(); stashTab(); }
  addTab(key);
  recordRecent(`${key.startsWith("@") ? "comp" : "gen"}:${key}`); // components' keys start with @
  state.viewerOwner = "model";
  $("#stage-empty").hidden = true;
  $("#log").hidden = true;
  $("#param-search").value = "";
  renderModelHeader(detail);
  state.usesColour = (await checkUsesColour(detail)) ? key : null;
  const kept = !shareCode && tabStates.get(key);
  if (kept) {
    // back to an open tab: its settings and last result, no new render
    Object.assign(state, { model: kept.model, values: kept.values, rendered: kept.rendered, renderedEdits: kept.renderedEdits, lastResult: kept.lastResult, lastResultValues: kept.lastResultValues });
    saved.active = kept.savedActive;
    buildForm();
    await loadSaved();
    if (savedId) chooseSettings(`saved:${savedId}`);
    else if (recipe) applyRecipe(recipe);
    else if (kept.lastResult) {
      await showResult(kept.lastResult, kept.lastResultValues, true, kept.renderedEdits);
      setStatus(kept.status[0], kept.status[1]);
      updateStatusForEdits();
    } else generate();
    document.body.dataset.model = key;
    return;
  }
  state.model = detail;
  state.rendered = null;
  state.renderedEdits = "";
  state.lastResult = null;
  if (shareCode) setEdits(key, null); // a shared link opens the original code
  state.viewer.clear();
  $("#dims").hidden = true;
  setDownload(null);
  state.values = Object.fromEntries(detail.parameters.map((p) => [p.name, structuredClone(p.default)]));
  saved.active = null;
  const shared = shareCode ? await readShare(shareCode) : null;
  if (shared && !shared.error) state.values = shared.values;
  buildForm();
  if (shared) noteShared(shared);
  if (savedId) { await loadSaved(); chooseSettings(`saved:${savedId}`); }
  else if (recipe) { loadSaved(); applyRecipe(recipe); }
  else {
    loadSaved();
    const missing = missingValues(detail, state.values);
    if (missing.length) setStatus("stale", `Set ${missing.join(", ")} to make this component, then Generate.`);
    else generate(); // show the default part straight away
  }
  document.body.dataset.model = key; // lets tests know which model's render is on screen
}

// ---------------------------------------------------------------- form
function buildForm() {
  const form = $("#params");
  form.replaceChildren();
  const { parameters, groups, tabs = {}, presets = [] } = state.model;
  form.append(settingsBar(presets));
  const names = parameters.map((p) => p.name).filter((n) => /^[A-Za-z_$][\w$]*$/.test(n));
  state.conditions = [];
  for (const p of parameters) if (p.show_if) state.conditions.push({ test: compileCondition(p.show_if, names), name: p.name });
  for (const node of document.querySelectorAll("#model-alerts [data-display-condition], #model-notes-body [data-display-condition]")) {
    state.conditions.push({ test: compileCondition(node.dataset.displayCondition, names), node });
  }
  groups.forEach((g, gi) => {
    const tab = tabs[g] || {};
    const fields = parameters.filter((p) => p.group === g && !p.hidden && p.name !== tab.control);
    const control = tab.control && parameters.find((p) => p.name === tab.control);
    const body = el("div", { class: "group-body" },
      tab.description_html ? el("div", { class: "group-note" }, safeHTML(tab.description_html)) : null,
      fields.map(buildField));
    const title = el("span", { class: "group-title", text: g === "Parameters" ? "Settings" : g });
    const summary = el("summary", {}, title);
    if (control) {
      const box = el("input", { type: "checkbox", class: "group-switch", "aria-label": `Use ${g.toLowerCase()}`,
        onclick: (e) => e.stopPropagation(),
        onchange: (e) => { setValue(control, e.target.checked); det.open = e.target.checked; } });
      box.checked = !!state.values[control.name];
      summary.prepend(box);
    }
    if (tab.help_link) summary.append(el("a", { class: "help-link", href: tab.help_link, target: "_blank", rel: "noopener", text: "?", title: "Documentation", "aria-label": `Documentation for ${g}`, onclick: (e) => e.stopPropagation() }));
    summary.append(el("span", { class: "changed-count" }));
    const open = control ? !!state.values[control.name] : tab.collapsed ? false : gi < 2 || groups.length <= 3;
    const det = el("details", { class: "group", "data-group": g, open }, summary, body);
    if (!fields.length && !control) return;
    form.append(det);
  });
  refreshChanged();
  applyConditions();
}

/** A setting's step, or "any" when its own default isn't on it (reported when the project is
 * read), so the form never calls a default invalid. */
function fitStep(p) {
  if (typeof p.default !== "number" || !(p.step > 0)) return p.step;
  const n = (p.default - (p.min ?? 0)) / p.step;
  return Math.abs(n - Math.round(n)) < 1e-6 ? p.step : "any";
}

function buildField(p) {
  const id = `p-${p.name}`;
  // (components read before 0.4.6 have their code names as labels)
  const label = p.label && p.label !== p.name ? p.label : humanize(p.name);
  // a default outside its own range widens it (reported when the project is read)
  const lo = typeof p.default === "number" && p.min != null ? Math.min(p.min, p.default) : p.min;
  const hi = typeof p.default === "number" && p.max != null ? Math.max(p.max, p.default) : p.max;
  const helpNodes = [];
  if (p.description_html) helpNodes.push(el("div", { class: "help", id: `${id}-help` }, safeHTML(p.description_html)));
  else if (p.description) helpNodes.push(el("p", { class: "help", id: `${id}-help`, text: p.description }));
  const reset = el("button", { type: "button", class: "reset", text: "↺", hidden: true, title: `Reset to ${JSON.stringify(p.default)}`,
    "aria-label": `Reset ${label}`, onclick: () => { setValue(p, structuredClone(p.default)); syncField(p); } });
  const helpLink = p.help_link ? el("a", { class: "help-link", href: p.help_link, target: "_blank", rel: "noopener", text: "?",
    title: "Documentation", "aria-label": `Documentation for ${label}` }) : null;
  const tip = p.description || (p.description_html ? helpNodes[0].textContent : null);
  const head = (labelNode) => {
    // short labels; full explanation and the name in the code on hover (or with Help on)
    const code = label !== p.name ? `In the code: ${p.name}` : null;
    if (tip || code) labelNode.title = [tip, code].filter(Boolean).join("\n\n");
    const prof = p.profiled ? el("a", { class: "profile-tag", href: "#/settings", text: "profile",
      title: "Starts from your printer profile (Settings)" }) : null;
    return el("div", { class: "field-head" }, labelNode, prof, helpLink, reset);
  };
  const wrap = el("div", { class: "field", "data-name": p.name, "data-search": `${p.name} ${label} ${p.description || ""}`.toLowerCase() });
  const describedby = helpNodes.length ? `${id}-help` : null;

  if (p.widget === "checkbox") {
    wrap.classList.add("check");
    wrap.append(head(el("label", { for: id, text: label })),
      el("input", { type: "checkbox", id, "aria-describedby": describedby, onchange: (e) => setValue(p, e.target.checked) }), ...helpNodes);
  } else if (p.widget === "dropdown") {
    const sel = el("select", { id, "aria-describedby": describedby,
      onchange: (e) => setValue(p, p.options[e.target.selectedIndex].value) },
      p.options.map((o) => el("option", { text: o.label })));
    // choices longer than half the column get the whole row under the label
    if (p.options.some((o) => String(o.label).length > 24)) wrap.classList.add("wide");
    wrap.append(head(el("label", { for: id, text: label })), sel, ...helpNodes);
  } else if (p.widget === "slider" && !Array.isArray(p.default)) {
    const step = p.step != null ? fitStep(p) : (Number.isInteger(p.default) && Number.isInteger(p.min ?? 0) && Number.isInteger(p.max ?? 0) ? 1 : "any");
    const range = el("input", { type: "range", min: lo, max: hi, step: step === "any" ? (hi - lo) / 100 : step,
      "aria-hidden": "true", tabindex: "-1", oninput: (e) => { setValue(p, +e.target.value); syncField(p, "range"); } });
    const num = el("input", { type: "number", id, min: lo, max: hi, step, "aria-describedby": describedby,
      oninput: (e) => { if (e.target.value !== "" && e.target.checkValidity()) { setValue(p, +e.target.value); syncField(p, "number"); } } });
    wrap.append(head(el("label", { for: id, text: label })), el("div", { class: "slider" }, range, num), ...helpNodes);
  } else if (Array.isArray(p.default)) {
    const axes = p.axes || (p.default.length <= 3 ? ["X", "Y", "Z"] : p.default.map((_, i) => `${i + 1}`));
    const editable = p.default.every((v) => typeof v !== "object");
    const inputs = p.default.map((v, i) => {
      const t = typeof v === "boolean" ? "checkbox" : typeof v === "number" ? "number" : "text";
      const ax = axes[i] || `${i + 1}`;
      return el("label", { "data-axis": ax, style: `--ax:${ax.length}` }, ax, el("input", { type: t, "data-i": i, "aria-label": `${label} ${axes[i] || i + 1}`, step: "any", min: p.min, max: p.max, disabled: !editable,
        oninput: (e) => {
          const next = structuredClone(state.values[p.name]);
          next[i] = t === "checkbox" ? e.target.checked : t === "number" ? (e.target.value === "" ? next[i] : +e.target.value) : e.target.value;
          setValue(p, next);
        } }));
    });
    wrap.append(head(el("span", { class: "label", id, text: label })),
      el("div", { class: p.default.length >= 3 && axes.some((a) => a.length > 2) ? "vector named" : "vector", role: "group", "aria-labelledby": id, style: `--n:${Math.min(p.default.length, 4)}` }, inputs), ...helpNodes);
  } else {
    // optional settings (components) can be left empty: the module's own default applies
    const t = p.type === "number" ? "number" : "text";
    const expr = p.type === "expression";
    wrap.append(head(el("label", { for: id, text: label })),
      el("input", { type: t, id, step: p.step != null ? fitStep(p) : "any", min: lo, max: hi, maxlength: t === "text" ? (expr ? 2000 : 200) : null, "aria-describedby": describedby,
        placeholder: p.placeholder || (p.optional ? "(default)" : null), class: expr ? "expr" : null, spellcheck: expr ? "false" : null,
        oninput: (e) => {
          const v = e.target.value;
          if (t === "number") {
            if (v === "" && p.optional) setValue(p, null);
            else if (v !== "" && e.target.checkValidity()) setValue(p, +v);
          } else setValue(p, v === "" && p.optional ? null : v);
        } }), ...helpNodes);
    if (expr) wrap.classList.add("field-expr");
  }
  if ((state.model.needs || []).includes(p.name)) wrap.classList.add("needed");
  if (p.presets) {
    const sel = el("select", { class: "presets", "aria-label": `${p.presets.label} for ${label}`,
      onchange: (e) => {
        const pick = p.presets.values[e.target.selectedIndex - 1];
        if (pick) { setValue(p, structuredClone(pick.value)); syncField(p); }
      } },
      el("option", { text: `${p.presets.label}…` }), p.presets.values.map((v) => el("option", { text: v.label })));
    wrap.insertBefore(sel, wrap.querySelector(".help") || null);
  }
  queueMicrotask(() => syncField(p));
  return wrap;
}

function syncField(p, skip) {
  const wrap = $(`.field[data-name="${CSS.escape(p.name)}"]`);
  if (!wrap) return;
  const v = state.values[p.name];
  if (p.widget === "checkbox") $("input", wrap).checked = v;
  else if (p.widget === "dropdown") $("select", wrap).selectedIndex = Math.max(0, p.options.findIndex((o) => same(o.value, v)));
  else if (p.widget === "slider" && !Array.isArray(p.default)) {
    if (skip !== "range") $('input[type="range"]', wrap).value = v;
    if (skip !== "number") $('input[type="number"]', wrap).value = v;
  } else if (Array.isArray(p.default)) {
    wrap.querySelectorAll("input[data-i]").forEach((inp) => {
      const x = v[+inp.dataset.i];
      if (inp.type === "checkbox") inp.checked = x; else if (document.activeElement !== inp) inp.value = typeof x === "object" ? JSON.stringify(x) : x;
    });
  } else {
    const inp = $("input", wrap);
    if (document.activeElement !== inp) inp.value = v ?? "";
  }
  if (p.presets) {
    const i = p.presets.values.findIndex((x) => same(x.value, v));
    $(".presets", wrap).selectedIndex = i + 1;
  }
  refreshChanged();
}

function setValue(p, v) {
  state.values[p.name] = v;
  refreshChanged();
  applyConditions();
  updateStatusForEdits();
}

function applyConditions() {
  for (const c of state.conditions) {
    const show = c.test(state.values);
    if (c.node) c.node.hidden = !show, c.node.style.display = show ? "" : "none";
    else $(`.field[data-name="${CSS.escape(c.name)}"]`)?.classList.toggle("cond-hidden", !show);
  }
  // a section whose settings are all switched off by conditions disappears too
  document.querySelectorAll("#params .group").forEach((g) => {
    const fields = g.querySelectorAll(".field");
    g.classList.toggle("cond-hidden", !g.querySelector(".group-switch") && fields.length > 0 &&
      [...fields].every((f) => f.classList.contains("cond-hidden")));
  });
}

function refreshChanged() {
  if (!state.model) return;
  for (const p of state.model.parameters) {
    const wrap = $(`.field[data-name="${CSS.escape(p.name)}"]`);
    if (!wrap) continue;
    const changed = !same(state.values[p.name], p.default);
    wrap.classList.toggle("changed", changed);
    $(".reset", wrap).hidden = !changed;
  }
  document.querySelectorAll(".group").forEach((g) => {
    const n = g.querySelectorAll(".field.changed").length;
    $(".changed-count", g).textContent = n ? `${n} changed` : "";
  });
}

const helpBtn = $("#help-toggle");
const setHelp = (on) => {
  document.body.classList.toggle("show-help", on);
  helpBtn.setAttribute("aria-pressed", String(on));
  store.prefs.set("gw-help", on);
};
helpBtn.addEventListener("click", () => setHelp(!document.body.classList.contains("show-help")));
setHelp(!!store.prefs.get("gw-help", false));

/** Replace every setting (missing names fall back to defaults) and update the form. */
function setAllValues(values) {
  for (const p of state.model.parameters) {
    state.values[p.name] = structuredClone(p.name in values ? values[p.name] : p.default);
    syncField(p);
  }
  document.querySelectorAll(".group-switch").forEach((b) => {
    const g = b.closest(".group"); const tab = state.model.tabs?.[g.dataset.group];
    if (tab?.control) { b.checked = !!state.values[tab.control]; g.open = b.checked; }
  });
  applyConditions();
  updateStatusForEdits();
}

$("#param-search").addEventListener("input", (e) => {
  const q = e.target.value.trim().toLowerCase();
  document.querySelectorAll(".group").forEach((g) => {
    let any = false;
    g.querySelectorAll(".field").forEach((f) => {
      const hit = !q || f.dataset.search.includes(q);
      f.classList.toggle("search-hidden", !hit);
      any ||= hit && !f.classList.contains("cond-hidden");
    });
    g.hidden = q ? !any : false;
    if (q && any) g.open = true;
  });
});

// ---------------------------------------------------------------- saved settings and share links
// saved.active: "default" | "preset:<i>" | "saved:<id>" (what the picker shows)
const saved = { list: [], active: null };

function settingsBar(presets) {
  const pick = el("select", { id: "settings-pick", "aria-label": "Start from defaults, a preset or your saved settings",
    onchange: (e) => chooseSettings(e.target.value) });
  const more = el("details", { class: "menu", id: "settings-more" },
    el("summary", { class: "ghost", "aria-label": "Saved settings: rename, delete, export, import", title: "Saved settings: rename, delete, export for OpenSCAD, import", text: "⋯" }),
    el("div", { class: "menu-panel", role: "menu" },
      el("button", { type: "button", role: "menuitem", id: "settings-rename", text: "Rename…", onclick: () => { closeMenu(); openSaveDialog("rename"); } }),
      el("button", { type: "button", role: "menuitem", id: "settings-delete", text: "Delete", onclick: () => { closeMenu(); deleteSaved(); } }),
      // (a Fusion 360 file's settings aren't OpenSCAD's; 0.4.27)
      ...(state.model?.engine === "mitcad" ? [] : [el("hr"),
      el("button", { type: "button", role: "menuitem", id: "settings-export", text: "Export for OpenSCAD", title: "All saved settings for this model, as an OpenSCAD Customizer file", onclick: () => { closeMenu(); exportSettings(); } }),
      el("button", { type: "button", role: "menuitem", id: "settings-import-btn", text: "Import OpenSCAD file…", onclick: () => { closeMenu(); $("#settings-import").click(); } })])));
  const bar = el("div", { class: "preset-bar" },
    el("label", { for: "settings-pick", class: "visually-hidden", text: "Start from" }), pick,
    el("button", { type: "button", class: "ghost", id: "settings-save", text: "Save",
      title: platform.kind === "desktop" ? "Save these settings in the library" : "Save these settings in this browser", onclick: () => openSaveDialog("save") }),
    // the app's own address opens nowhere else, so the app copies the settings themselves (UI-22)
    platform.kind === "desktop"
      ? el("button", { type: "button", class: "ghost", id: "settings-share", text: "Copy settings", title: "Copy these settings as an OpenSCAD Customizer set; paste them on a model page to use them", onclick: copySettings })
      : el("button", { type: "button", class: "ghost", id: "settings-share", text: "Share", title: "Copy a link that opens this model with these settings", onclick: shareSettings }),
    more);
  const note = el("p", { id: "settings-note", class: "settings-note", role: "status", hidden: true });
  const codeNote = el("p", { id: "code-note", class: "settings-note code-note", hidden: true });
  saved.presets = presets;
  queueMicrotask(() => { renderSettingsPick(); renderCodeNote(); });
  return el("div", { class: "settings-bar" }, bar, note, codeNote);
}

const closeMenu = () => { const m = $("#settings-more"); if (m) m.open = false; };
document.addEventListener("click", (e) => { if (!e.target.closest?.("#settings-more")) closeMenu(); });

function renderSettingsPick() {
  const pick = $("#settings-pick");
  if (!pick || !state.model) return;
  const presets = state.model.presets || [];
  pick.replaceChildren(
    el("option", { value: "default", text: "Defaults" }),
    presets.length ? el("optgroup", { label: "Presets" }, presets.map((ps, i) => el("option", { value: `preset:${i}`, text: ps.label }))) : null,
    saved.list.length ? el("optgroup", { label: "Saved" }, saved.list.map((r) => el("option", { value: `saved:${r.id}`,
      text: r.edits && Object.keys(r.edits).length ? `${r.name} · code edited` : r.name }))) : null);
  pick.value = saved.active || "default";
  if (pick.selectedIndex < 0) pick.value = "default";
  const isSaved = (saved.active || "").startsWith("saved:");
  $("#settings-rename").disabled = !isSaved;
  $("#settings-delete").disabled = !isSaved;
  $("#settings-export").disabled = false;
}

async function loadSaved() {
  const key = state.model.key;
  try { saved.list = await store.settings.list(key); } catch { saved.list = []; }
  if (state.model?.key === key) renderSettingsPick();
}

const activeSaved = () => saved.list.find((r) => `saved:${r.id}` === saved.active) || null;

let noteTimer = null;
function note(text, kind = "") {
  const n = $("#settings-note");
  if (!n) return;
  n.textContent = text;
  n.className = `settings-note ${kind}`;
  n.hidden = false;
  clearTimeout(noteTimer);
  noteTimer = setTimeout(() => { n.hidden = true; }, 9000);
}

const skippedText = (n) => (n ? ` ${plural(n, "setting")} didn't match this version of the model and stayed at the default.` : "");

function chooseSettings(value) {
  const key = state.model.key;
  // Defaults and saved settings bring their own code (saved without edits: the original), presets only values
  const replacesCode = value === "default" || value.startsWith("saved:");
  if (replacesCode && codeStatus(key) === "unsaved" && !confirm("Go back to the original code? Your edits to the code aren't saved.")) {
    renderSettingsPick();
    return;
  }
  if (value === "default") {
    setAllValues({});
    setEdits(key, null);
  } else if (value.startsWith("preset:")) {
    const ps = state.model.presets[+value.slice(7)];
    if (!ps) return;
    setAllValues({ ...state.values, ...ps.values }); // author presets set only what they're about (e.g. size)
  } else if (value.startsWith("saved:")) {
    const rec = saved.list.find((r) => `saved:${r.id}` === value);
    if (!rec) return;
    const { values, skipped } = withChanges(state.model, rec.values);
    setAllValues(values);
    setEdits(key, editTexts(rec.edits));
    // the project's file changed since (an update): the edited copy is still what's used
    const moved = Object.entries(rec.edits || {}).filter(([p, e]) => e?.base && ![state.model.file_bases?.[p], state.model.files?.[p]].includes(e.base)).map(([p]) => p.split("/").pop());
    if (moved.length) note(`${moved.join(", ")} changed in the project since “${rec.name}” was saved; your edited copy is used.`, "warn");
    else if (skipped.length) note(skippedText(skipped.length).trim(), "warn");
  }
  saved.active = value;
  renderSettingsPick();
  generate();
}

function openSaveDialog(mode) {
  if (!state.model) return;
  const dlg = $("#save-dialog");
  const cur = activeSaved();
  const changes = Object.keys(changedValues(state.model, state.values)).length;
  dlg.dataset.mode = mode;
  $("#save-title").textContent = mode === "rename" ? "Rename saved settings" : "Save settings";
  const name = $("#save-name");
  name.value = mode === "rename" ? cur?.name || "" : cur ? cur.name : suggestName();
  const updating = mode === "save" && !!cur;
  dlg.dataset.updating = updating ? "1" : "";
  $("#save-alt").hidden = !updating; // "Save as new"
  $("#save-primary").textContent = mode === "rename" ? "Rename" : updating ? `Update “${cur.name}”` : "Save";
  store.settings.persistent().then((keep) => {
    const files = Object.keys(editsOf(state.model.key)).length;
    $("#save-note").textContent = mode === "rename" ? "" :
      (files && !changes ? `Keeps your edits to ${plural(files, "file")} of code (the original stays as it is)`
        : `Keeps the ${plural(changes, "setting")} you changed from the defaults` + (files ? ` and your edits to ${plural(files, "file")} of code (the original stays as it is)` : "")) +
      (platform.kind === "browser" ? (keep ? ", in this browser." : ". This browser isn't keeping site data (private window?), so they'll be gone when you close the tab.") : ".");
  });
  dlg.showModal();
  name.select();
}

function suggestName() {
  const c = changedValues(state.model, state.values);
  const byName = new Map(state.model.parameters.map((p) => [p.name, p]));
  const bits = Object.entries(c).slice(0, 3).map(([k, v]) => `${byName.get(k)?.label || humanize(k)} ${Array.isArray(v) ? v.join("×") : v}`);
  return bits.join(", ").slice(0, 60) || "My settings";
}

function uniqueName(name, exceptId = null) {
  const taken = new Set(saved.list.filter((r) => r.id !== exceptId).map((r) => r.name.toLowerCase()));
  if (!taken.has(name.toLowerCase())) return name;
  for (let i = 2; ; i++) if (!taken.has(`${name} (${i})`.toLowerCase())) return `${name} (${i})`;
}

/**
 * The open model's code edits as kept with saved settings and made parts' recipes: each with the hash
 * of the project's file it started from (and of the app's copy, when the app made one: a file whose
 * settings were lifted from its calls). null when there are none.
 */
function editsRecord(model) {
  const ed = Object.entries(editsOf(model.key)).map(([p, text]) => {
    const own = model.file_bases?.[p], used = model.files?.[p] || null;
    return [p, { text, base: own || used, ...(own && own !== used ? { copy: used } : {}) }];
  });
  return ed.length ? Object.fromEntries(ed) : null;
}

async function saveSettings(asNew) {
  const dlg = $("#save-dialog");
  const mode = dlg.dataset.mode;
  const name = $("#save-name").value.trim();
  if (!name) { $("#save-name").focus(); return; }
  const cur = activeSaved();
  const key = state.model.key;
  try {
    let rec;
    const edits = editsRecord(state.model);
    // what they were made with, as a made part's recipe records it (one recipe shape, 0.4.22)
    const m = state.model;
    const made = { project_version: sourceById(m.source_id)?.version || null, files_id: m.files_id || null, model_name: m.name, project: m.source_id || null };
    if (mode === "rename" && cur) rec = await store.settings.save({ id: cur.id, name: uniqueName(name, cur.id) });
    else if (!asNew && cur) rec = await store.settings.save({ id: cur.id, name: uniqueName(name, cur.id), values: changedValues(state.model, state.values), edits, ...made });
    else rec = await store.settings.save({ model: key, name: uniqueName(name), values: changedValues(state.model, state.values), edits, ...made });
    dlg.close();
    saved.active = `saved:${rec.id}`;
    await loadSaved();
    ui.set((s) => ({ codeRev: (s.codeRev || 0) + 1 })); // the Code tab says "saved"
    note(mode === "rename" ? `Renamed to “${rec.name}”.` : `Saved “${rec.name}”${edits ? " with your code" : ""}.`, "ok");
  } catch (e) {
    note(`Couldn't save: ${e.message}`, "warn");
  }
}

async function deleteSaved() {
  const cur = activeSaved();
  if (!cur || !confirm(`Delete saved settings “${cur.name}”? The form keeps its current values.`)) return;
  await store.settings.remove(cur.id);
  saved.active = null;
  await loadSaved();
  note(`Deleted “${cur.name}”.`);
}

async function shareSettings() {
  if (!state.model) return;
  const changes = changedValues(state.model, state.values);
  const n = Object.keys(changes).length;
  const base = `${location.origin}${location.pathname}#/m/${state.model.key}`;
  const url = n ? `${base}?s=${await encodeShare(changes)}` : base;
  let copied = false;
  try { await navigator.clipboard.writeText(url); copied = true; } catch { /* not allowed here */ }
  if (copied) note(n ? `Link copied. It opens this model with your ${plural(n, "changed setting")}.` : "Link copied. You haven't changed any settings, so it opens the defaults.", "ok");
  else prompt("Copy this link:", url);
  document.body.dataset.shareLink = url; // for tests
}

/** The settings as an OpenSCAD Customizer file's text, which OpenSCAD reads and a paste here takes back. */
async function copySettings() {
  if (!state.model) return;
  const n = Object.keys(changedValues(state.model, state.values)).length;
  const name = activeSaved()?.name || state.model.name || state.model.key;
  const text = toOpenSCAD(state.model, [{ name, values: state.values }]);
  let copied = false;
  try { await navigator.clipboard.writeText(text); copied = true; } catch { /* not allowed here */ }
  document.body.dataset.copiedSettings = text; // for tests
  if (copied) note(n ? `Copied ${plural(n, "changed setting")} as an OpenSCAD Customizer set. Paste on this model's page to use them, or into a .json file for OpenSCAD.`
    : "Copied the defaults as an OpenSCAD Customizer set. Paste on this model's page to use them, or into a .json file for OpenSCAD.", "ok");
  else note("Couldn't copy to the clipboard here. Use ⋯ › Export for OpenSCAD instead.", "warn");
}

// a Customizer set pasted on a model page (not into a box) fills in the form
document.addEventListener("paste", (e) => {
  if (ui.get().view !== "model" || !state.model || e.target.closest?.("input, textarea, select, [contenteditable], .cm-editor")) return;
  const text = e.clipboardData?.getData("text/plain") || "";
  if (!/"parameterSets"/.test(text)) return;
  let sets;
  try { sets = fromOpenSCAD(state.model, text); } catch (err) { note(err.message, "warn"); return; }
  if (!sets.length) return;
  e.preventDefault();
  const s = sets[0];
  setAllValues(withChanges(state.model, s.changes).values);
  saved.active = null;
  renderSettingsPick();
  generate();
  const n = Object.keys(s.changes).length;
  note(`Pasted “${s.name}”: ${plural(n, "setting")} changed from the defaults.${s.skipped ? ` ${plural(s.skipped, "value")} didn't match this model and ${s.skipped === 1 ? "was" : "were"} skipped.` : ""}`, s.skipped ? "warn" : "ok");
});

async function readShare(code) {
  try {
    const changes = await decodeShare(code);
    return withChanges(state.model || { parameters: [] }, changes);
  } catch {
    return { error: true };
  }
}

function noteShared(shared) {
  if (shared.error) { note("That share link is damaged or incomplete, so the defaults are shown.", "warn"); return; }
  note(`Opened a shared link: ${plural(shared.applied, "setting")} changed from the defaults.${skippedText(shared.skipped.length)}`, shared.skipped.length ? "warn" : "ok");
}

function exportSettings() {
  const sets = saved.list.map((r) => ({ name: r.name, values: withChanges(state.model, r.values).values }));
  const cur = changedValues(state.model, state.values);
  if (!sets.some((s) => same(changedValues(state.model, s.values), cur))) sets.unshift({ name: "Current settings", values: state.values });
  const text = toOpenSCAD(state.model, sets);
  const fname = state.model.entry.split("/").pop().replace(/\.scad$/i, ".json");
  deliver(new Blob([text], { type: "application/json" }), fname);
  note(`Exported ${plural(sets.length, "set")} as ${fname}. Put it next to the .scad file and OpenSCAD's Customizer lists them.`, "ok");
}

async function importSettings(file) {
  if (!file || !state.model) return;
  let sets;
  try { sets = fromOpenSCAD(state.model, await file.text()); } catch (e) { note(e.message, "warn"); return; }
  if (!sets.length) { note("That file has no parameter sets.", "warn"); return; }
  let first = null, skipped = 0;
  for (const s of sets) {
    const rec = await store.settings.save({ model: state.model.key, name: uniqueName(s.name), values: s.changes });
    saved.list.push(rec); // so the next name is unique too
    first ||= rec;
    skipped += s.skipped;
  }
  await loadSaved();
  chooseSettings(`saved:${first.id}`);
  note(`Imported ${plural(sets.length, "set")} and opened “${first.name}”.${skipped ? ` ${plural(skipped, "value")} didn't match this model and ${skipped === 1 ? "was" : "were"} skipped.` : ""}`, skipped ? "warn" : "ok");
}

$("#save-form").addEventListener("submit", (e) => { e.preventDefault(); saveSettings(!$("#save-dialog").dataset.updating); });
$("#save-alt").addEventListener("click", () => saveSettings(true));
$("#save-cancel").addEventListener("click", () => $("#save-dialog").close());
$("#settings-import").addEventListener("change", (e) => { importSettings(e.target.files[0]); e.target.value = ""; });

// ---------------------------------------------------------------- rendering
function setStatus(kind, text) {
  const s = $("#status");
  s.className = `status ${kind || ""}`;
  s.textContent = text;
}

function updateStatusForEdits() {
  if (state.job || !state.model) return;
  const a = $("#download");
  const valuesSame = state.rendered && same(JSON.parse(state.rendered), state.values);
  const codeSame = (state.renderedEdits || "") === editsKey(editsOf(state.model.key));
  if (valuesSame && codeSame) {
    setStatus("ok", `Preview matches your settings.${state.formatNote ? ` ${state.formatNote}` : ""}`);
    if (!a.classList.contains("is-disabled")) a.textContent = downloadText(state.downloadFormat);
  } else if (state.rendered) {
    setStatus("stale", valuesSame ? "Code changed. Generate to update the preview and download." : "Settings changed. Generate to update the preview and download.");
    if (!a.classList.contains("is-disabled")) a.textContent = $("#download-format").hidden ? "Download last render" : "Last render";
  }
}

/** Settings that are sizes (grid units, width, depth...), for naming what a model makes. */
const SIZE_KEYS = ["gridx", "gridy", "gridz", "Width", "Depth", "Height", "Width_Units", "Length_Units",
  "Board_Width", "Board_Height", "shelf_width", "shelf_depth", "GridSize", "plate_size"];

/** The sizes among a model's settings, for naming what it makes. */
function sizesOf(values) {
  const dims = [];
  for (const k of SIZE_KEYS) {
    if (k in values) [].concat(values[k]).forEach((x) => { if (typeof x === "number") dims.push(String(x)); });
  }
  return dims;
}

function friendlyName(model, values) {
  let base = model.kind === "component" ? `${model.component?.library || "component"}-${model.id}` : `${model.family}-${model.id}`;
  const dims = sizesOf(values);
  const part = model.part_parameter;
  if (part && ["string", "number"].includes(typeof values[part])) base += `-${values[part]}`;
  return (base + (dims.length ? "-" + dims.slice(0, 4).join("x") : "")).replace(/[^A-Za-z0-9._-]+/g, "_").slice(0, 120) + ".stl";
}

function setDownload(blob, name) {
  state.lastStl = blob ? { blob, name: name || "model.stl" } : null;
  return applyDownloadFormat();
}

// ---------------------------------------------------------------- download formats (milestone 5)
// A model's render downloads as STL or 3MF (the core writes the 3MF from the render); a part with
// several formats has one Download button and a format beside it. The choice is kept per model or
// part, and Settings › Downloads gives the default.
const PREFERRED = [["stl", "STL where there is one"], ["3mf", "3MF where there is one, otherwise STL"], ["step", "STEP where there is one, otherwise STL"]];
const preferredFormat = () => store.prefs.get("gw-format-pref") || "stl";
/** The format to offer first among `formats` (["stl", "3mf", ...]): the one last chosen for `key`, else Settings' rule. */
function pickFormat(key, formats) {
  const last = (store.prefs.get("gw-formats", {}) || {})[key];
  if (last && formats.includes(last)) return last;
  const pref = preferredFormat();
  return formats.includes(pref) ? pref : formats.includes("stl") ? "stl" : formats[0];
}
function rememberFormat(key, fmt) {
  store.prefs.set("gw-formats", { ...(store.prefs.get("gw-formats", {}) || {}), [key]: fmt });
}

let formatJob = 0;
async function applyDownloadFormat() {
  const a = $("#download"), sel = $("#download-format");
  const desktop = platform.kind === "desktop";
  sel.hidden = !desktop;
  // a 2D model downloads its drawing as SVG or DXF (0.4.23)
  const formats = isFlat() ? ["svg", "dxf"] : ["stl", "3mf"];
  if ([...sel.options].map((o) => o.value).join() !== formats.join()) sel.replaceChildren(...formats.map((f) => el("option", { value: f, text: FORMAT_LABEL[f] })));
  const fmt = desktop && state.model ? pickFormat(state.model.key, formats) : formats[0];
  sel.value = fmt;
  state.downloadFormat = fmt;
  const job = ++formatJob;
  const last = state.lastStl;
  let blob = last?.blob || null, name = last?.name || "model.stl";
  // OpenSCAD's own 3MF, made at the same settings: each object apart and its colours kept (0.4.19). Made
  // for a 3MF download, and after the preview of a model that calls color(), to show its colours (UI-39)
  const wantOwn = !!blob && !isFlat() && (fmt === "3mf" || (state.usesColour && state.usesColour === state.model?.key));
  let own = null, info = null;
  if (blob && fmt === "dxf") {
    // the same drawing as DXF, made by OpenSCAD at the same settings
    try {
      a.classList.add("is-disabled");
      a.textContent = "Making the DXF…";
      const m = state.model;
      blob = (await state.engine.render({ ...forRender(m), format: "dxf", browser: false }, renderValues(m, state.lastResultValues)).promise).blob;
    } catch (e) {
      toast(`Couldn't make the DXF: ${e?.message || e}`);
      blob = null;
    }
  }
  name = name.replace(/\.(stl|svg)$/i, `.${isFlat() ? fmt : "stl"}`);
  if (blob && fmt === "3mf") {
    try {
      // (the WebAssembly engine, or the code edited since: the app writes one from the render)
      a.classList.add("is-disabled");
      a.textContent = "Making the 3MF…";
      const r = await colour3mf().catch(() => null);
      if (job !== formatJob) return;
      own = r?.blob || null;
      info = r?.info || null;
      blob = own || new Blob([await platform.apiBytes("to_3mf", { data: await dataUrl(blob), name: state.model?.name || "Model" })], { type: "model/3mf" });
      name = name.replace(/\.stl$/i, ".3mf");
    } catch (e) {
      toast(`Couldn't make the 3MF: ${e?.message || e}`);
    }
  }
  if (job !== formatJob) return;
  if (state.downloadUrl) URL.revokeObjectURL(state.downloadUrl);
  state.downloadUrl = blob ? URL.createObjectURL(blob) : null;
  a.href = state.downloadUrl || "#";
  a.download = name;
  a.classList.toggle("is-disabled", !blob);
  if (blob) a.removeAttribute("aria-disabled"); else a.setAttribute("aria-disabled", "true");
  a.textContent = downloadText(fmt);
  if (fmt === "3mf" || !wantOwn) {
    await showColours(own, info, fmt);
    if (job !== formatJob) return;
    updateStatusForEdits();
    return;
  }
  // STL chosen: its download is ready now; the 3MF is made behind it, to show and name the colours
  updateStatusForEdits();
  const r = await colour3mf().catch(() => null);
  if (job !== formatJob) return;
  await showColours(r?.blob || null, r?.info || null, fmt);
  if (job === formatJob) updateStatusForEdits();
}
/** OpenSCAD's own 3MF of the last render (desktop), or null when it can't be made the same way. */
async function native3mf() {
  const m = state.model;
  if (!m || platform.kind !== "desktop" || !state.engine?.info?.engine || !state.lastResultValues) return null;
  const sent = forRender(m);
  if (editsKey(sent.edits) !== (state.renderedEdits || "")) return null;
  return (await state.engine.render({ ...sent, format: "3mf", browser: false }, renderValues(m, state.lastResultValues)).promise).blob;
}
/** OpenSCAD's 3MF of the last render and what's in it ({ blob, info: { objects, colours, triangles } }), made
 * once for each render and shared by the 3D view, the 3MF download and Save as part (UI-39, F-28). */
function colour3mf() {
  const k = JSON.stringify([state.model?.key, state.lastResultValues, state.renderedEdits || ""]);
  if (state.own3mf?.k !== k) {
    const promise = native3mf().then(async (blob) => blob
      ? { blob, info: await platform.api("mesh_info", { data: await dataUrl(blob), name: "model.3mf" }).catch(() => null) } : null);
    promise.catch(() => { if (state.own3mf?.k === k) state.own3mf = null; });
    state.own3mf = { k, promise };
  }
  return state.own3mf.promise;
}

/** Whether a model calls color() in its own file (comments aside): its preview then shows its colours (UI-39). */
async function checkUsesColour(detail) {
  const sha = detail?.files?.[detail.entry];
  if (platform.kind !== "desktop" || !sha || detail.kind === "component") return false;
  try {
    const src = (await platform.modelFile(sha)).replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");
    return /\bcolou?r\s*\(/.test(src);
  } catch { return false; }
}

/** What OpenSCAD's 3MF holds, said in the status line ("It has 3 parts in 2 colours: choose 3MF to keep them",
 * or "The 3MF has…" with 3MF chosen), and its colours in the 3D view (UI-39). */
async function showColours(own, info, fmt) {
  const parts = info ? [info.objects > 1 ? `${info.objects} parts` : "", info.colours > 1 ? `${info.colours} colours` : ""].filter(Boolean) : [];
  state.formatNote = !parts.length ? "" : fmt === "3mf" ? `The 3MF has ${parts.join(" in ")}.` : `It has ${parts.join(" in ")}: choose 3MF to keep them.`;
  const coloured = !!(own && info?.colours > 1);
  if (coloured === !!state.viewColoured) return;
  state.viewColoured = coloured;
  try {
    const bytes = coloured ? await platform.apiBytes("mesh_coloured", { data: await dataUrl(own), name: "model.3mf" }) : null;
    const url = URL.createObjectURL(coloured ? new Blob([bytes], { type: "model/stl" }) : state.lastResult.blob);
    fileColoursAvailable(coloured);
    await state.viewer.load(url);
    URL.revokeObjectURL(url);
  } catch { /* the plain render stays in view */ }
}

/** Show the file's colours (a 3MF's, a coloured model's): on unless unticked, one setting for parts and models,
 * kept on this computer. The box shows only when the view has colours; the filament swatches hide while they're shown. */
const fileColoursOn = () => store.prefs.get("gw-file-colours") !== false;
function fileColoursAvailable(on) {
  const hud = $("#file-colours-hud"), box = $("#file-colours");
  hud.hidden = !on;
  box.checked = fileColoursOn();
  state.viewer.setFileColours(on && fileColoursOn());
  $("#filament").hidden = on && fileColoursOn();
}
$("#file-colours").addEventListener("change", (e) => {
  store.prefs.set("gw-file-colours", e.target.checked);
  fileColoursAvailable(true);
});

/** "Download", with the format named only when there's no picker beside it to name it (UI-33). */
function downloadText(fmt = "stl") {
  return $("#download-format").hidden ? `Download ${FORMAT_LABEL[fmt] || fmt.toUpperCase()}` : "Download";
}
$("#download-format").addEventListener("change", (e) => {
  if (state.model) rememberFormat(state.model.key, e.target.value);
  applyDownloadFormat();
});

// ---------------------------------------------------------------- save as part (milestone 4)
// What's made can be kept in the Parts Library (local/Made parts/<model>/<name>.stl) with a recipe
// beside it, so its page can open the model with the same settings or make it again.
// docs/DESKTOP_PLAN.md, "Make parts from a generator design".

/** The preview shows the settings and code as they are now (nothing being made). */
const previewCurrent = () => !state.job && !!state.rendered && same(JSON.parse(state.rendered), state.values)
  && (state.renderedEdits || "") === editsKey(editsOf(state.model.key));

/** "Basic Bin 3×2": the model's name and the part and sizes it's set to. */
function partName(model, values) {
  const part = model.part_parameter && ["string", "number"].includes(typeof values[model.part_parameter]) ? ` ${values[model.part_parameter]}` : "";
  const dims = sizesOf(values).slice(0, 3);
  return `${model.name}${part}${dims.length ? ` ${dims.join("×")}` : ""}`;
}

$("#save-part").addEventListener("click", () => {
  const m = state.model;
  if (!m) return;
  ui.set({ dialog: { type: "save-part", name: partName(m, state.values), category: m.kind === "component" ? "" : m.category || "",
    modelName: m.name, edits: hasEdits(m.key), current: previewCurrent() } });
});

/** Save the open model's part (Save as part dialog): made first when the preview isn't up to date. */
function savePart(opts) {
  if (!state.model) return;
  if (previewCurrent()) { writePart(opts); return; }
  state.afterRender = { key: state.model.key, values: structuredClone(state.values), run: () => writePart(opts) };
  generate();
}

const dataUrl = (blob) => new Promise((resolve, reject) => {
  const r = new FileReader();
  r.onload = () => resolve(r.result);
  r.onerror = () => reject(r.error);
  r.readAsDataURL(blob);
});

/** Write one made part and its recipe to the library (local/Made parts/<model>/). Resolves to { name, folder }. */
async function savePartFile(m, blob, values, { name, category = null, replace = false, folder = null, format = "stl" }) {
  return platform.api("part_save", {
    model: m.key, model_name: m.name, project: m.source_id || null, project_version: sourceById(m.source_id)?.version || null,
    files_id: m.files_id || null, name, category, values: changedValues(m, values), edits: editsRecord(m), data: await dataUrl(blob), replace, folder, format });
}

/** Whether made parts are kept as OpenSCAD's own 3MF (3MF chosen beside Download, the app's OpenSCAD there; F-28). */
function partsAs3mf() {
  return state.downloadFormat === "3mf" && platform.kind === "desktop" && !!state.engine?.info?.engine;
}

/** After saving: read Made parts again and find a saved part's page (`#/parts/<pack>/<part>`). */
async function madePartHref({ name, folder, file }) {
  await rescanLocal(true);
  const rel = `${folder}/${file || `${name}.stl`}`;
  const lib = state.libraries.find((l) => l.items.some((i) => i.made_from && i.files[0]?.path === rel));
  const it = lib?.items.find((i) => i.made_from && i.files[0]?.path === rel);
  return it ? `#/parts/${lib.id}/${it.id}` : null;
}

/** Write the last render and its recipe to the library, read Made parts again, and say where it went. */
async function writePart(opts) {
  const m = state.model;
  let blob = state.lastResult?.blob;
  if (!m || !blob) return;
  if (isFlat()) { setStatus("error", "A 2D model's drawing can't be kept in the Parts Library yet: download it as SVG or DXF."); return; }
  try {
    setStatus("busy", "Saving the part…");
    // in the format chosen beside Download, or the one it was made in (Make again); a 3MF from OpenSCAD
    // itself, so its colours and objects are kept (Review 6, F-28)
    let format = "stl";
    if ((opts.format || (partsAs3mf() ? "3mf" : "stl")) === "3mf") {
      const own = (await colour3mf().catch(() => null))?.blob;
      if (own) { blob = own; format = "3mf"; }
    }
    const r = await savePartFile(m, blob, state.lastResultValues, { ...opts, format });
    const href = await madePartHref(r);
    if (state.model === m) updateStatusForEdits();
    toast(opts.replace ? `Made ${r.name} again.` : `Saved ${r.name} to the Parts Library.`,
      href ? { label: "Show", run: () => { location.hash = href; } } : null);
  } catch (e) {
    setStatus("error", `Couldn't save the part: ${e.message || e}`);
  }
}

/** A model made at some settings (a collection's export, 0.4.17): `values` (changes from its defaults) or a
 * saved setting's id, whose code edits come too. Resolves to the STL blob. */
async function renderModel(key, { values = null, settings = null, format = null } = {}) {
  const detail = await loadDetail(key);
  const rec = settings ? await platform.api("settings_get", { id: settings }).catch(() => null) : null;
  const edits = rec?.edits && Object.keys(rec.edits).length ? editTexts(rec.edits) : null;
  const full = withChanges(detail, values || rec?.values || {}).values;
  const sent = { ...detail, ...(edits ? { edits } : {}) };
  // a 3MF (0.4.19): OpenSCAD's own where the app's OpenSCAD is there (objects apart, colours kept), else
  // written by the app from the STL
  if (format === "3mf" && state.engine?.info?.engine) {
    const own = await state.engine.render({ ...sent, format: "3mf", browser: false }, edits ? changedValues(detail, full) : full).promise.catch(() => null);
    if (own) return own.blob;
  }
  const job = state.engine.render(sent, edits ? changedValues(detail, full) : full);
  const blob = (await job.promise).blob;
  return format === "3mf" ? new Blob([await platform.apiBytes("to_3mf", { data: await dataUrl(blob), name: detail.name || "Model" })], { type: "model/3mf" }) : blob;
}

$("#model-collect").addEventListener("click", () => {
  const m = state.model;
  if (!m) return;
  const values = changedValues(m, state.values);
  const rec = activeSaved();
  const settings = rec && JSON.stringify(changedValues(m, withChanges(m, rec.values || {}).values)) === JSON.stringify(values) ? rec.id : null;
  const named = rec && settings ? `${m.name} · ${rec.name}` : Object.keys(values).length ? partName(m, state.values) : m.name;
  ui.set({ dialog: { type: "collect", entries: [{ ref: `${m.kind === "component" ? "comp" : "gen"}:${m.key}`, name: named, settings, values: Object.keys(values).length ? values : null }] } });
});
$("#part-collect").addEventListener("click", () => {
  if (state.partEntry) ui.set({ dialog: { type: "collect", entries: [state.partEntry] } });
});

/** A made part's recipe, from its parts link (`#/m/<model>?part=<pack>/<part>`), when it was made from this model. */
function madeRecipe({ part, again }, key) {
  const [libId, pid] = part.split("/");
  const it = state.libraries.find((l) => l.id === libId)?.items.find((i) => i.id === pid);
  if (!it?.made_from || it.made_from.model !== key) return null;
  const rel = it.files[0]?.path || "";
  const at = rel.lastIndexOf("/");
  return { ...it.made_from, again, stem: rel.slice(at + 1).replace(/\.(stl|3mf)$/i, ""), folder: at > 0 ? rel.slice(0, at) : null };
}

/** Set the form to a recipe's settings and code, and make it (Make again: then replace the part's files). */
function applyRecipe(r) {
  const key = state.model.key;
  const { values, skipped } = withChanges(state.model, r.values || {});
  setAllValues(values);
  setEdits(key, r.edits ? editTexts(r.edits) : null);
  saved.active = null;
  renderSettingsPick();
  if (skipped.length) note(skippedText(skipped.length).trim(), "warn");
  state.afterRender = r.again
    ? { key, values: structuredClone(state.values), run: () => writePart({ name: r.stem, category: r.category, replace: true, folder: r.folder, format: r.output || "stl" }) }
    : null;
  generate();
}

function generate() {
  if (!state.model || !state.engine) return;
  const missing = missingValues(state.model, state.values);
  if (missing.length) { state.afterRender = null; setStatus("error", `Set ${missing.join(", ")} first: this component needs ${missing.length === 1 ? "it" : "them"}.`); return; }
  cancelJob();
  const values = structuredClone(state.values);
  const model = state.model;
  const sent = forRender(model); // with the Code tab's edits
  const ek = editsKey(sent.edits);
  const started = Date.now();
  $("#generate").disabled = true;
  $("#log").hidden = true;
  let stage = "Starting…";
  const job = state.engine.render(sent, renderValues(model, values), (ev) => { if (ev.type === "stage") stage = ev.stage; });
  state.job = job;
  const tick = () => {
    if (state.job !== job) return;
    const secs = Math.round((Date.now() - started) / 1000);
    setStatus("busy", `${stage} ${secs}s`);
    if (secs >= 1 || !state.rendered) showOverlay(`${stage}${secs > 15 ? " Detailed parts can take a minute or more on slower devices." : ""}`);
    job.timer = setTimeout(tick, 500);
  };
  tick();
  job.promise.then(async (result) => {
    if (state.job !== job) return;
    finishJob();
    await showResult(result, values, false, ek);
  }, (err) => {
    if (state.job !== job) return;
    finishJob();
    if (err.cancelled) { setStatus("stale", "Render cancelled."); return; }
    if (state.afterRender) { state.afterRender = null; toast("The part wasn't saved: it couldn't be made."); }
    setStatus("error", err.message);
    offerBrokenFlag(model, err.message);
    if (err.logs?.length) { $("#log-text").textContent = err.logs.join("\n"); $("#log").hidden = false; }
    // at its defaults: the project's own presets are tried instead (0.4.24)
    // (once per model, so choosing Defaults afterwards shows why)
    if (!Object.keys(changedValues(model, values)).length && !ek && model.presets?.length && !presetsTried.has(model.key)) {
      presetsTried.add(model.key);
      presetInstead(model, values, err.message);
    }
  });
}

const presetsTried = new Set();
/** A model whose defaults don't render: its first three presets (a project's parameter sets, a component's
 * examples) are tried in turn, and the first that renders is chosen, with a note. Nothing else changes. */
async function presetInstead(model, values, message) {
  for (const [i, ps] of model.presets.slice(0, 3).entries()) {
    const v = { ...values, ...ps.values };
    if (same(v, values)) continue;
    if (state.model !== model || state.job) return;
    setStatus("busy", `Its defaults don't render; trying the preset “${ps.label}”…`);
    try {
      await state.engine.render(forRender(model), renderValues(model, v)).promise;
    } catch (e) {
      if (e.cancelled) return;
      continue;
    }
    if (state.model !== model || state.job) return;
    chooseSettings(`preset:${i}`);
    note(`Its defaults don't render, so its preset “${ps.label}” is shown. Choose Defaults to see why.`, "warn");
    return;
  }
  if (state.model === model && !state.job) setStatus("error", message);
}

/** After a failed render (desktop): "Flag as broken", with the error as the note. */
function offerBrokenFlag(model, message) {
  if (platform.kind !== "desktop" || state.catalog?.library?.read_only) return;
  const it = state.index?.get(`gen:${model.key}`);
  if (!it || it.broken) return;
  $("#status").append(" ", el("button", { type: "button", class: "link-btn", id: "flag-broken", text: "Flag as broken",
    onclick: () => ui.set({ dialog: { type: "flag", items: [it.id], note: String(message || "").slice(0, 300) } }) }));
}

function finishJob() {
  if (state.job) clearTimeout(state.job.timer);
  state.job = null;
  showOverlay(null);
  $("#generate").disabled = false;
}

function cancelJob() {
  if (!state.job) return;
  const j = state.job;
  finishJob();
  j.cancel();
}

async function showResult(result, values, restoring = false, ek = state.renderedEdits || "") {
  document.body.dataset.engine = result.engine || ""; // which engine made it (desktop on Windows races two)
  try {
    state.viewColoured = false; // (the render itself: a 3MF's colours are shown again once it's made)
    fileColoursAvailable(false);
    let dims;
    if (result.format === "svg") {
      state.viewer.clear();
      dims = await showDrawing(result.blob);
    } else {
      const url = URL.createObjectURL(result.blob);
      dims = await state.viewer.load(url);
      URL.revokeObjectURL(url);
    }
    state.rendered = JSON.stringify(values);
    state.renderedEdits = ek; // the code edits it was made with
    state.lastResult = result;
    state.lastResultValues = values;
    // (a 2D model's drawing isn't a part: no file in the Parts Library takes it yet)
    $("#save-part").disabled = isFlat();
    $("#save-part").title = isFlat() ? "A 2D drawing can't be kept in the Parts Library yet: download it as SVG or DXF"
      : "Keep what you made in the Parts Library, with how it was made";
    showDims(dims, result.blob.size);
    setDownload(result.blob, friendlyName(state.model, values));
    if (!restoring) {
      setStatus("ok", result.cached ? "Preview matches your settings." : `Preview matches your settings. Made in ${fmt(result.ms / 1000)} s on this device.`);
      if (state.model?.kind === "component") saveComponentThumb(state.model.key, result.blob); // the first render becomes its thumbnail
      // Save as part or Make again waiting for this render (and not one with other settings since)
      const after = state.afterRender;
      if (after && after.key === state.model?.key) {
        state.afterRender = null;
        if (same(values, after.values) && ek === editsKey(editsOf(after.key))) after.run();
      }
    }
    if (!same(values, state.values) || ek !== editsKey(editsOf(state.model?.key))) updateStatusForEdits();
  } catch (e) {
    setStatus("error", e.message);
  }
}

/** A 2D model's drawing (0.4.23): its SVG in place of the 3D view, or none. Resolves to its size in mm. */
async function showDrawing(blob) {
  const img = $("#stage-drawing");
  if (img.dataset.url) URL.revokeObjectURL(img.dataset.url);
  delete img.dataset.url;
  img.hidden = !blob;
  $(".stage").classList.toggle("flat", !!blob);
  if (!blob) { img.removeAttribute("src"); return null; }
  img.dataset.url = URL.createObjectURL(blob);
  img.src = img.dataset.url;
  // its size from the outlines' points (OpenSCAD's own width and height are rounded out to whole mm)
  const svg = new DOMParser().parseFromString(await blob.text(), "image/svg+xml");
  const xs = [], ys = [];
  for (const p of svg.querySelectorAll("path")) {
    for (const [, x, y] of (p.getAttribute("d") || "").matchAll(/(-?[\d.]+(?:e-?\d+)?)[ ,](-?[\d.]+(?:e-?\d+)?)/gi)) { xs.push(+x); ys.push(+y); }
  }
  const span = (v) => (v.length ? Math.max(...v) - Math.min(...v) : 0);
  return { flat: true, x: span(xs), y: span(ys) };
}

/** The open model's last render is a drawing (a 2D model). */
const isFlat = () => state.lastResult?.format === "svg";

function showOverlay(text) {
  $("#stage-overlay").hidden = !text;
  if (text) $("#overlay-text").textContent = text;
}

function showDims(d, size) {
  const box = $("#dims");
  if (d.flat) {
    box.replaceChildren(el("span", { class: "mm", text: `${fmt(d.x)} × ${fmt(d.y)} mm` }),
      el("span", { class: "meta", text: `2D drawing${size ? `, ${bytes(size)}` : ""}` }));
    box.hidden = false;
    return;
  }
  const grid = state.viewerOwner === "model" && GRID_CATEGORIES.has(state.model?.category);
  const units = (v) => fmt(Math.round(v / 42 * 2) / 2);
  const gridText = `About ${units(d.x)} × ${units(d.y)} grid units` + (d.z >= 7 ? `, ${fmt(d.z / 7)} height units` : "");
  box.replaceChildren(...[
    el("span", { class: "mm", text: `${fmt(d.x)} × ${fmt(d.y)} × ${fmt(d.z)} mm` }),
    grid && el("span", { class: "units", text: gridText }),
    el("span", { class: "meta", text: `${d.triangles.toLocaleString()} triangles${size ? `, ${bytes(size)}` : ""}` }),
  ].filter(Boolean));
  box.hidden = false;
}

$("#generate").addEventListener("click", generate);
$("#params").addEventListener("submit", (e) => { e.preventDefault(); generate(); });
$("#params").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && e.target.matches("input:not([type=checkbox])")) { e.preventDefault(); generate(); }
});
$("#cancel").addEventListener("click", () => { cancelJob(); setStatus("stale", "Render cancelled."); });

// ---------------------------------------------------------------- batch
const BATCH_MAX = 200;
// what to make: combinations of settings (rows), or saved settings (Batch generate, milestone 4)
const batch = { rows: [], running: null, mode: "vary", picked: new Set() };
const batchable = () => state.model.parameters.filter((p) => !p.hidden && !Array.isArray(p.default) &&
  ["number", "string", "boolean"].includes(p.type));

function batchRow(p) {
  // sensible starting values for the chosen setting
  const v = state.values[p.name];
  if (p.widget === "dropdown") return { name: p.name, kind: "choice", picked: [v] };
  if (p.type === "boolean") return { name: p.name, kind: "choice", picked: [false, true] };
  if (p.type === "number") {
    const step = p.step && p.step > 0 ? p.step : (Number.isInteger(v) ? Math.max(1, Math.round(Math.abs(v) / 5) || 1) : 1);
    return { name: p.name, kind: "range", from: v, to: v + step * 4, step };
  }
  return { name: p.name, kind: "list", text: String(v) };
}

function rowValues(row) {
  const p = state.model.parameters.find((x) => x.name === row.name);
  if (row.kind === "choice") return row.picked;
  if (row.kind === "list") return row.text.split(",").map((s) => s.trim()).filter(Boolean)
    .map((s) => (p.type === "number" ? Number(s) : s)).filter((x) => p.type !== "number" || Number.isFinite(x));
  const out = [];
  const { from, to, step } = row;
  if (!(step > 0) || !Number.isFinite(from) || !Number.isFinite(to)) return out;
  for (let x = from, i = 0; (from <= to ? x <= to + 1e-9 : x >= to - 1e-9) && i <= BATCH_MAX; x = from <= to ? x + step : x - step, i++) {
    out.push(Math.round(x * 1e6) / 1e6);
  }
  return out;
}

function batchCombos() {
  let combos = [{}];
  for (const row of batch.rows) {
    const vals = rowValues(row);
    combos = combos.flatMap((c) => vals.map((v) => ({ ...c, [row.name]: v })));
  }
  return combos;
}

/**
 * What Batch generate makes: { values, file, part } for each, with the whole form's values, a file
 * name for the ZIP, and a name for the Parts Library ("Basic Bin 2×1": the model, its sizes, and
 * whatever else varies).
 */
function batchItems() {
  const m = state.model;
  if (batch.mode === "saved") {
    return saved.list.filter((r) => batch.picked.has(r.id) && !hasCode(r)).map((r) => ({
      values: withChanges(m, r.values).values, file: `${m.family}-${m.id}-${r.name}`, part: `${m.name} ${r.name}` }));
  }
  if (!batch.rows.length) return [];
  return batchCombos().map((c) => {
    const values = { ...structuredClone(state.values), ...c };
    const other = Object.entries(c).filter(([k]) => !SIZE_KEYS.includes(k)).map(([k, v]) => {
      const p = m.parameters.find((x) => x.name === k);
      if (typeof v === "boolean") return v ? (p?.label || humanize(k)) : `no ${(p?.label || humanize(k)).toLowerCase()}`;
      return p?.options?.find((o) => same(o.value, v))?.label || String(v);
    });
    return { values, file: `${m.family}-${m.id}-` + Object.entries(c).map(([k, v]) => `${k}-${v}`).join("-"), part: [partName(m, values), ...other].join(" ") };
  });
}

/** A saved setting with code edits: Batch generate uses the form's code, so it isn't offered. */
const hasCode = (r) => !!r.edits && Object.keys(r.edits).length > 0;

function renderBatchSaved() {
  const box = $("#batch-saved");
  box.replaceChildren(
    el("p", { class: "muted", text: "Each one you tick becomes its own part, with its own settings." }),
    ...saved.list.map((r) => {
      const cb = el("input", { type: "checkbox", value: r.id, disabled: hasCode(r), onchange: (e) => {
        if (e.target.checked) batch.picked.add(r.id); else batch.picked.delete(r.id);
        updateBatchCount();
      } });
      cb.checked = batch.picked.has(r.id) && !hasCode(r);
      return el("label", { class: "batch-saved-row", title: hasCode(r) ? "Saved with code edits: open it to make it" : null }, cb, r.name,
        hasCode(r) ? el("small", { class: "muted", text: " (code edited: open it to make it)" }) : null);
    }));
}

function setBatchMode(mode) {
  batch.mode = mode;
  document.querySelectorAll('#batch-mode input').forEach((i) => { i.checked = i.value === mode; });
  $("#batch-vary").hidden = mode !== "vary";
  $("#batch-saved").hidden = mode !== "saved";
  updateBatchCount();
}
document.querySelectorAll('#batch-mode input').forEach((i) => i.addEventListener("change", () => setBatchMode(i.value)));

function renderBatchRows() {
  const box = $("#batch-rows");
  const params = batchable();
  box.replaceChildren(...batch.rows.map((row, ri) => {
    const p = params.find((x) => x.name === row.name);
    const label = (x) => x.label || humanize(x.name);
    const pick = el("select", { "aria-label": "Setting to vary", onchange: (e) => {
      batch.rows[ri] = batchRow(params[e.target.selectedIndex]); renderBatchRows();
    } }, params.map((x) => el("option", { text: `${label(x)}${x.group !== "Parameters" ? ` (${x.group})` : ""}` })));
    pick.selectedIndex = params.indexOf(p);
    let editor;
    if (row.kind === "range") {
      const num = (k, text) => el("label", {}, text, el("input", { type: "number", step: "any", value: row[k],
        oninput: (e) => { row[k] = e.target.value === "" ? NaN : +e.target.value; updateBatchCount(); } }));
      editor = el("div", { class: "batch-range" }, num("from", "From"), num("to", "To"), num("step", "Step"));
    } else if (row.kind === "choice") {
      const opts = p.widget === "dropdown" ? p.options : [{ value: false, label: "Off" }, { value: true, label: "On" }];
      editor = el("div", { class: "batch-choices" }, opts.map((o) => {
        const cb = el("input", { type: "checkbox", onchange: (e) => {
          row.picked = e.target.checked ? [...row.picked, o.value] : row.picked.filter((x) => !same(x, o.value));
          updateBatchCount();
        } });
        cb.checked = row.picked.some((x) => same(x, o.value));
        return el("label", {}, cb, o.label);
      }));
    } else {
      editor = el("label", { class: "batch-list" }, "Values, separated by commas",
        el("input", { type: "text", value: row.text, oninput: (e) => { row.text = e.target.value; updateBatchCount(); } }));
    }
    const remove = batch.rows.length > 1 ? el("button", { type: "button", class: "ghost", text: "Remove",
      onclick: () => { batch.rows.splice(ri, 1); renderBatchRows(); } }) : null;
    return el("fieldset", { class: "batch-row" }, el("legend", { text: ri ? "And vary" : "Vary" }), el("div", { class: "batch-row-head" }, pick, remove), editor);
  }));
  $("#batch-add").hidden = batch.rows.length >= 2 || params.length < 2;
  updateBatchCount();
}

function updateBatchCount() {
  const n = batchItems().length;
  const run = $("#batch-run"), keep = $("#batch-keep");
  $("#batch-count").textContent = n > BATCH_MAX ? `${n} combinations: the limit is ${BATCH_MAX}. Narrow the ranges.`
    : n ? `${plural(n, "part")} will be made.` : batch.mode === "saved" ? "Tick at least one." : "Pick at least one value.";
  run.disabled = keep.disabled = !n || n > BATCH_MAX || !!batch.running;
  run.textContent = n && n <= BATCH_MAX ? `Make ${n} and download ZIP` : "Make and download ZIP";
  keep.textContent = n && n <= BATCH_MAX ? `Save ${n} to the Parts Library` : "Save to the Parts Library";
}

$("#batch-open").addEventListener("click", () => {
  if (!state.model) return;
  const params = batchable();
  if (!batch.rows.length || batch.model !== state.model.key) {
    batch.model = state.model.key;
    // start from the most likely thing to vary: a length/size number, else the first number
    const guess = params.find((p) => p.type === "number" && /len|length|size|width|height|grid/i.test(p.name)) ||
      params.find((p) => p.type === "number") || params[0];
    batch.rows = guess ? [batchRow(guess)] : [];
    batch.picked = new Set();
    batch.mode = "vary";
  }
  $("#batch-progress").hidden = true;
  // saved settings as another way to choose (when there are any); the Parts Library as another way out (the app)
  $("#batch-mode").hidden = !saved.list.length;
  if (!saved.list.length) batch.mode = "vary";
  $("#batch-keep").hidden = platform.kind !== "desktop" || !!state.catalog.library?.read_only || isFlat();
  renderBatchRows();
  renderBatchSaved();
  setBatchMode(batch.mode);
  $("#batch").showModal();
});
$("#batch-add").addEventListener("click", () => {
  const used = new Set(batch.rows.map((r) => r.name));
  const next = batchable().find((p) => !used.has(p.name));
  if (next) { batch.rows.push(batchRow(next)); renderBatchRows(); }
});
$("#batch-close").addEventListener("click", () => {
  if (batch.running) { batch.running.cancel(); batch.running = null; }
  $("#batch").close();
});
$("#batch").addEventListener("cancel", () => { if (batch.running) { batch.running.cancel(); batch.running = null; } });

$("#batch-run").addEventListener("click", () => runBatch(false));
$("#batch-keep").addEventListener("click", () => runBatch(true));

/** Make every part Batch generate lists, then download them as one ZIP or (keep) save each to the Parts Library. */
async function runBatch(keep) {
  const model = state.model;
  const combos = batchItems();
  if (!combos.length || combos.length > BATCH_MAX) return;
  cancelJob();
  const files = [];
  const names = new Set();
  const bar = $("#batch-bar"), text = $("#batch-progress-text");
  $("#batch-progress").hidden = false;
  batch.running = { cancelled: false, job: null, cancel() { this.cancelled = true; this.job?.cancel(); } };
  const run = batch.running;
  updateBatchCount();
  const started = Date.now();
  run.jobs = new Set();
  run.cancel = function () { this.cancelled = true; for (const j of this.jobs) j.cancel(); };
  const width = Math.max(1, Math.min(state.engine.concurrency || 1, combos.length));
  const results = new Array(combos.length);
  // 3MF chosen beside Download: each made as OpenSCAD's own 3MF, colours and objects kept (F-28)
  // a 2D model: its drawings, in the format chosen beside Download (0.4.23)
  const flat = isFlat() ? state.downloadFormat : null;
  if (flat && keep) { text.textContent = "A 2D model's drawings can't be kept in the Parts Library yet: download them as a ZIP."; batch.running = null; return; }
  const as3mf = !flat && partsAs3mf();
  const ext = flat || (as3mf ? "3mf" : "stl");
  let next = 0, done = 0;
  const progress = () => {
    bar.style.width = `${(done / combos.length) * 100}%`;
    text.textContent = width > 1 ? `Making ${combos.length} parts, ${width} at a time: ${done} done`
      : `Making ${Math.min(done + 1, combos.length)} of ${combos.length}: ${combos[Math.min(done, combos.length - 1)].part}`;
  };
  const worker = async () => {
    while (next < combos.length) {
      if (run.cancelled) throw Object.assign(new Error("Batch cancelled."), { cancelled: true });
      const i = next++;
      const { values } = combos[i];
      const job = state.engine.render(as3mf || flat ? { ...forRender(model), format: ext, browser: false } : forRender(model), renderValues(model, values));
      run.jobs.add(job);
      try { results[i] = { values, result: await job.promise }; } finally { run.jobs.delete(job); }
      done++;
      progress();
    }
  };
  // the preview shows the last one: made as an STL too when the parts are 3MFs
  const previewOf = async (r) => as3mf || flat === "dxf" ? { values: r.values, result: await state.engine.render(forRender(model), renderValues(model, r.values)).promise } : r;
  try {
    progress();
    await Promise.all(Array.from({ length: width }, worker));
    if (keep) {
      // each to local/Made parts/<model>/ with its recipe, named from what varies
      let first = null;
      for (let i = 0; i < combos.length; i++) {
        if (run.cancelled) throw Object.assign(new Error("Batch cancelled."), { cancelled: true });
        text.textContent = `Saving ${i + 1} of ${combos.length} to the Parts Library…`;
        const r = await savePartFile(model, results[i].result.blob, results[i].values, { name: combos[i].part, category: model.kind === "component" ? null : model.category || null, format: ext });
        first ||= r;
      }
      const href = await madePartHref(first);
      text.textContent = `Done: ${plural(combos.length, "part")} saved to the Parts Library under Made parts › ${first.folder}, in ${Math.round((Date.now() - started) / 1000)} s.`;
      toast(`Saved ${plural(combos.length, "part")} to the Parts Library.`, href ? { label: "Show", run: () => { $("#batch").close(); location.hash = href; } } : null);
      const last = await previewOf(results[combos.length - 1]);
      state.viewerOwner = "model";
      await showResult(last.result, last.values, true, editsKey(editsOf(model.key)));
      bar.style.width = "100%";
      setStatus("ok", `${plural(combos.length, "part")} saved to the Parts Library. The preview shows the last one.`);
      return;
    }
    for (let i = 0; i < combos.length; i++) {
      let name = combos[i].file;
      name = name.replace(/[^A-Za-z0-9._-]+/g, "_").slice(0, 140);
      while (names.has(name)) name += "_";
      names.add(name);
      files.push({ name: `${name}.${ext}`, data: new Uint8Array(await results[i].result.blob.arrayBuffer()) });
    }
    const last = await previewOf(results[combos.length - 1]);
    state.viewerOwner = "model";
    await showResult(last.result, last.values, true, editsKey(editsOf(model.key)));
    bar.style.width = "100%";
    const zip = makeZip(files);
    const took = Math.round((Date.now() - started) / 1000);
    const saved = await deliver(zip, `${model.family}-${model.id}-batch-${files.length}.zip`);
    text.textContent = `Done: ${files.length} files (${bytes(zip.size)}) in ${took} s. ` +
      (saved === null ? "Not saved." : saved ? `Saved to ${saved}.` : "Your download has started.");
    setStatus("ok", `Batch of ${files.length} ${saved === null ? "made" : saved ? "saved" : "downloaded"}. The preview shows the last one.`);
  } catch (e) {
    text.textContent = e.cancelled ? `Cancelled. ${keep ? "Parts already saved stay in the Parts Library." : "Nothing was downloaded."}` : `Stopped: ${e.message}`;
  } finally {
    batch.running = null;
    updateBatchCount();
  }
}

// ---------------------------------------------------------------- part libraries
async function openLibrary(libId, itemId) {
  showView("library");
  let lib = state.libDetail[libId];
  if (!lib) {
    try {
      lib = state.libDetail[libId] = await getJSON(`data/libraries/${libId}.json`);
    } catch {
      $("#lib-title").textContent = "Parts not found";
      $("#lib-summary").textContent = "This collection doesn't exist. Go back to the catalog to pick one.";
      return;
    }
  }
  if (state.openLib !== libId) {
    state.openLib = libId;
    $("#lib-title").textContent = lib.name;
    $("#lib-summary").textContent = lib.summary || "";
    const credit = $("#lib-credit");
    credit.replaceChildren();
    // the same credit line as a model's: By … · Source · License
    const parts = [];
    if (lib.authors?.length) parts.push(`By ${lib.authors.map((a) => a.name).join(", ")}`);
    if (lib.source_url) parts.push(el("a", { href: lib.source_url, target: "_blank", rel: "noopener", text: "Source" }));
    const spdx = lib.license?.spdx;
    // parts you've made from models: each is under the license of the project it was made from (UI-28)
    if (libId === "local-made-parts") parts.push("Each part keeps the license of the project it was made from");
    else parts.push(el("a", { href: `#/licenses/${lib.id}`, text: spdx && spdx !== "NOASSERTION" ? `License ${spdx}` : "License details" }));
    credit.append(...parts.flatMap((x, i) => (i ? [" · ", x] : [x])));
    // the pack's project documents (its README), as on model pages, and in the side viewer's tabs (UI-25)
    const sourceId = lib.source_id || state.libraries.find((l) => l.id === libId)?.source_id || null;
    const partRef = sourceId ? { source: sourceId } : null;
    ui.set({ partRef });
    render(partRef ? html`<${SideButtons} refObj=${partRef} />` : null, $("#lib-extra"));
    $("#part-search").value = "";
    buildPartList(lib);
  }
  const item = lib.items.find((i) => i.id === itemId) || lib.items.find((i) => i.preview_url) || lib.items[0];
  if (!itemId && item) history.replaceState(null, "", `#/parts/${libId}/${item.id}`);
  showPart(lib, item);
}

function buildPartList(lib) {
  const nav = $("#part-list");
  nav.replaceChildren();
  for (const c of [...new Set(lib.items.map((i) => i.category))]) {
    nav.append(el("div", { class: "part-group", "data-cat": c },
      el("h2", { text: c }),
      // by name, numbers as numbers (Brick 1x2 before 1x17), as the grid sorts (UI-29)
      el("ul", {}, lib.items.filter((i) => i.category === c).sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true })).map((i) => el("li", {},
        el("a", { href: `#/parts/${lib.id}/${i.id}`, "data-id": i.id, "data-search": [i.name, c, ...(i.tags || [])].join(" ").toLowerCase() },
          el("span", { text: i.name }),
          el("span", { class: "formats", text: [...new Set(i.files.map((f) => FORMAT_LABEL[f.format] || f.format))].join(" ") })))))));
  }
}

$("#part-search").addEventListener("input", (e) => {
  const q = e.target.value.trim().toLowerCase();
  document.querySelectorAll(".part-group").forEach((g) => {
    let any = false;
    g.querySelectorAll("a").forEach((a) => { const hit = !q || a.dataset.search.includes(q); a.parentElement.hidden = !hit; any ||= hit; });
    g.hidden = !any;
  });
});

async function showPart(lib, item) {
  if (!item) return;
  document.title = `${item.name}, ${lib.name} | SCAD Workshop`;
  document.querySelectorAll("#part-list a").forEach((a) => a.toggleAttribute("aria-current", a.dataset.id === item.id));
  const nav = $("#part-list"), cur = $(`#part-list a[data-id="${CSS.escape(item.id)}"]`);
  if (cur && nav.scrollHeight > nav.clientHeight + 1) {
    const top = cur.offsetTop - nav.offsetTop;
    if (top < nav.scrollTop || top > nav.scrollTop + nav.clientHeight - 80) nav.scrollTop = top - nav.clientHeight / 3;
  }
  const status = $("#part-status");
  status.replaceChildren(el("p", { class: "part-name", text: item.name }));
  // (a part with no summary of its own shows none: not its collection's again)
  if (item.description && item.description.trim() !== (lib.summary || "").trim()) status.append(el("p", { class: "help", text: item.description }));
  const made = madeFrom(item.made_from);
  if (made) {
    const link = `${made.href}?part=${lib.id}/${item.id}`;
    status.append(el("div", { class: "made-from", id: "made-from" },
      el("p", { class: "help", text: made.available ? made.text : `${made.text} That model isn't in the library now.` }),
      made.outdated ? el("p", { class: "help made-outdated", text: made.outdated }) : null,
      made.available ? el("p", { class: "made-actions" },
        el("a", { class: "button secondary", id: "made-open", href: link, text: "Open with these settings" }),
        platform.kind === "desktop" && !state.catalog.library?.read_only
          ? el("a", { class: "button secondary", id: "made-again", href: `${link}&again`, title: "Make it with the same settings and replace its files, keeping the name", text: "Make again" }) : null) : null));
  } else if (item.example) {
    // one of a folder of examples made into parts: its own model prints this one size; the main file's model,
    // when it has settings, prints any (UI-29)
    status.append(el("p", { class: "help" }, el("a", { href: `#/m/${item.generator}`, id: "example-open", text: "Open the example as a model" }),
      ...(item.main_model && state.catalog?.models?.some((m) => m.key === item.main_model)
        ? [", or ", el("a", { href: `#/m/${item.main_model}`, id: "example-main", text: `open ${state.catalog.models.find((m) => m.key === item.main_model).name}` }), " to make it at other sizes."] : ["."])));
  } else if (item.generator) status.append(el("p", { class: "help" }, el("a", { href: `#/m/${item.generator}`, text: "Open the parametric model" }), " to print it at any size."));
  // a part made from several files ("Make one part", milestone 5) can be split again
  if (item.merged?.length && platform.kind === "desktop" && !state.catalog.library?.read_only) {
    status.append(el("p", { class: "help" }, `Made one part from ${item.merged.length + 1} files. `,
      el("button", { type: "button", class: "link-btn", id: "part-split", text: "Split into separate parts",
        onclick: async () => {
          try {
            await platform.api("parts_merge", { source: lib.source_id || lib.id, ids: [item.id], split: true });
            await reloadCatalog();
            toast("Split into separate parts.");
          } catch (e) { toast(String(e.message || e)); }
        } })));
  }
  // ⋯: Show in folder, Choose picture…, Add content… (0.4.23, milestone 6)
  const more = $("#part-more"), partSource = lib.source_id || lib.id, ro = !!state.catalog.library?.read_only;
  more.hidden = platform.kind !== "desktop";
  more.open = false;
  $("#part-reveal").hidden = !item.files.length;
  $("#part-reveal").onclick = () => { more.open = false; revealFile({ source: partSource, rel: item.files[0].path, examples: !!item.example }); };
  const level = { source: partSource, level: "part", target: item.id, name: item.name };
  ui.set({ partItem: item.id }); // (the side viewer shows its own content too)
  $("#part-picture").disabled = $("#part-content").disabled = ro;
  $("#part-picture").onclick = () => { more.open = false; ui.set({ dialog: { type: "picture", ...level, current: item.meta?.icon || null } }); };
  $("#part-content").onclick = () => { more.open = false; ui.set({ dialog: { type: "content", ...level } }); };
  state.partEntry = { ref: `part:${lib.id}/${item.id}`, name: item.name };
  const dl = $("#part-downloads");
  const formats = item.files.map((f) => f.format);
  if (formats.length > 1 && new Set(formats).size === formats.length && !item.files.some((f) => f.label)) {
    // one Download with the format beside it (milestone 5): the last one chosen for this part, else Settings' rule
    const key = `part:${lib.id}/${item.id}`;
    const link = el("a", { class: "button primary", id: "part-download" });
    const show = (fmt) => {
      const f = item.files.find((x) => x.format === fmt) || item.files[0];
      link.href = f.url;
      link.setAttribute("download", f.path.split("/").pop());
      link.title = f.path.split("/").pop();
      link.textContent = `Download (${bytes(f.bytes)})`; // the picker beside it names the format (UI-33)
    };
    const sel = el("select", { class: "format-pick", id: "part-format", "aria-label": "Format",
      onchange: (e) => { rememberFormat(key, e.target.value); show(e.target.value); } },
      item.files.map((f) => el("option", { value: f.format, text: FORMAT_LABEL[f.format] || f.format })));
    sel.value = pickFormat(key, formats);
    show(sel.value);
    dl.replaceChildren(el("span", { class: "download-pick" }, link, sel));
    dl.classList.remove("many");
  } else if (item.files.length === 1 && !item.files[0].label) {
    // one file: the same Download, with its format named beside it (Review 6, UI-41)
    const f = item.files[0];
    const name = f.path.split("/").pop();
    dl.replaceChildren(el("span", { class: "download-pick" },
      el("a", { class: "button primary", id: "part-download", href: f.url, download: name, title: name, text: `Download (${bytes(f.bytes)})` }),
      el("span", { class: "format-label", id: "part-format-label", text: FORMAT_LABEL[f.format] || f.format.toUpperCase() })));
    dl.classList.remove("many");
  } else {
    dl.replaceChildren(...item.files.map((f, i) => el("a", {
      class: `button ${i === 0 && !["step", "shapr"].includes(f.format) ? "primary" : "secondary"}`, href: f.url,
      download: f.path.split("/").pop(), title: f.path.split("/").pop(),
      text: `${f.label ? f.label + ": " : ""}${FORMAT_LABEL[f.format] || f.format} (${bytes(f.bytes)})` })));
    dl.classList.toggle("many", item.files.length > 4);
  }
  state.viewerOwner = "library";
  $("#log").hidden = true;
  showOverlay(null);
  state.viewer.clear();
  $("#dims").hidden = true;
  const empty = $("#stage-empty");
  if (!item.preview_url) {
    empty.textContent = "No 3D preview: this part comes as CAD files (STEP or Shapr3D) for remixing.";
    empty.hidden = false;
    return;
  }
  empty.hidden = true;
  try {
    const url = platform.kind === "browser" ? item.preview_url : URL.createObjectURL(await (await platform.fetch(item.preview_url)).blob());
    fileColoursAvailable(false);
    const d = await state.viewer.load(url);
    if (url !== item.preview_url) URL.revokeObjectURL(url);
    if (state.viewerOwner === "library") {
      showDims(d);
      // a 3MF with colours of its own: shown unless Show the file's colours is unticked (UI-39)
      fileColoursAvailable(!!d.coloured);
    }
  } catch (e) {
    empty.textContent = `Preview unavailable: ${e.message}`;
    empty.hidden = false;
  }
}

// ---------------------------------------------------------------- licenses & credits

function showLicenses(focus) {
  showView("about");
  $("#about-title").textContent = "Licenses & credits";
  document.title = "Licenses & credits | SCAD Workshop";
  const c = state.catalog;
  const fams = (c?.families || []).filter((f) => f.status !== "missing-source").sort((a, b) => a.name.localeCompare(b.name));
  const row = (id, name, what, authors, lic, source) => el("tr", { id: `lic-${id}`, class: id === focus ? "focus" : null },
    el("th", { scope: "row" }, el("div", { class: "lic-name" }, el("b", { text: name }), el("span", { class: "muted", text: what }))),
    el("td", { text: authors || "Not stated" }),
    el("td", { text: spdx(lic) }),
    el("td", {}, el("span", { class: `badge ${lic?.public_use || "ok"}`, text: STATUS_TEXT[lic?.public_use || "ok"] }),
      lic?.notes ? el("span", { class: "lic-note", text: lic.notes }) : null),
    el("td", {}, source ? el("a", { href: source, target: "_blank", rel: "noopener", text: "Source" }) : "Supplied file"));
  const table = (rows) => el("div", { class: "lic-wrap" }, el("table", { class: "lic-table" },
    el("thead", {}, el("tr", {}, ["Project", "Authors", "License", "Status", "Source"].map((h) => el("th", { scope: "col", text: h })))),
    el("tbody", {}, rows)));
  const review = fams.filter((f) => f.license?.public_use && f.license.public_use !== "ok");
  showPage([
    el("p", { text: "Every model and part here is someone else's open work, credited below with its license. Generated files carry the same license as the project that made them." }),
    review.length ? el("p", { class: "lic-summary" },
      `${review.length} project${review.length > 1 ? "s need" : " needs"} a license check before files are shared or sold: `,
      ...review.flatMap((f, i) => [i ? ", " : "", el("a", { href: `#/licenses/${f.id}`, text: f.name })]), ".") : null,
    el("h2", { text: "Parametric Models" }),
    table(fams.map((f) => row(f.id, f.name, (f.models || []).join(", "), (f.authors || []).map((a) => a.name).join(", "), f.license, f.source))),
    el("h2", { text: "Parts Library" }),
    table(state.libraries.map((l) => row(l.id, l.name, `${l.item_count} parts`, (l.authors || []).map((a) => a.name).join(", "), l.license, l.source_url))),
    ...(c?.component_libraries?.length ? [el("h2", { text: "Components" }),
      table(c.component_libraries.filter((l) => l.active).map((l) => row(`components-${l.name}`, l.name, l.provider === "project" ? "Your copy" : "Comes with the app",
        (l.authors || []).join(", "), { spdx: l.license || "NOASSERTION", public_use: l.license ? "ok" : "review" }, l.repo ? `https://github.com/${l.repo}` : null)))] : []),
    el("h2", { text: "This site" }),
    el("ul", {},
      el("li", {}, platform.kind === "desktop" ? `OpenSCAD ${c?.engine || ""} (native, plus the WebAssembly build on Windows) makes every part on this computer. GNU GPL version 2 or later: ` : `OpenSCAD ${c?.engine || ""} (WebAssembly build) makes every part in your browser. GNU GPL version 2 or later: `,
        el("a", { href: "https://github.com/openscad/openscad", target: "_blank", rel: "noopener", text: "source code" }), ", ",
        el("a", { href: "engine/COPYING", target: "_blank", text: "license text" }), "."),
      el("li", { text: "3D preview: three.js (MIT). Interface: Preact (MIT) and htm (Apache-2.0)." }),
      el("li", { text: "Fonts for text on parts: Liberation Sans and Liberation Mono (SIL Open Font License 1.1)." }),
      el("li", { text: "Nothing you configure is sent to a server; once loaded, the site keeps working offline." }))]);
  const target = focus && document.getElementById(`lic-${focus}`);
  if (target) requestAnimationFrame(() => target.scrollIntoView({ block: "center" }));
  else window.scrollTo(0, 0);
}

// ---------------------------------------------------------------- viewer tools
// a narrow 3D view (the side viewer open beside it) folds the view buttons under ⋯
const hudTools = $(".hud-tools"), hudMore = $(".hud-more");
const foldHud = (open) => { hudTools.classList.toggle("open", open); hudMore.setAttribute("aria-expanded", String(open)); };
hudMore.addEventListener("click", () => foldHud(!hudTools.classList.contains("open")));
document.querySelectorAll(".hud-tools [data-view]").forEach((b) => b.addEventListener("click", () => { state.viewer?.view(b.dataset.view); foldHud(false); }));
document.querySelectorAll(".hud-tools [data-toggle]").forEach((b) => b.addEventListener("click", () => {
  const on = b.getAttribute("aria-pressed") !== "true";
  b.setAttribute("aria-pressed", String(on));
  if (b.dataset.toggle === "edges") state.viewer?.setEdges(on);
  if (b.dataset.toggle === "grid") state.viewer?.setGrid(on);
}));
const fil = $("#filament");
FILAMENTS.forEach(([name, hex]) => {
  const b = el("button", { type: "button", class: "swatch", role: "radio", "aria-checked": String(hex === savedColor),
    "aria-label": name, title: name, style: `--c:${hex}`,
    onclick: () => {
      fil.querySelectorAll(".swatch").forEach((s) => s.setAttribute("aria-checked", String(s === b)));
      state.viewer?.setColor(hex);
      store.prefs.set("gw-filament", hex);
    } });
  fil.append(b);
});

// ---------------------------------------------------------------- files out (download or save)
let toastTimer = null;
function toast(text, action) {
  const t = $("#toast");
  const hide = () => { t.hidden = true; document.body.classList.remove("toasting"); };
  t.replaceChildren(...[el("span", { text }), action ? el("button", { type: "button", class: "ghost", text: action.label, onclick: () => { hide(); action.run(); } }) : null].filter(Boolean));
  t.hidden = false;
  document.body.classList.add("toasting"); // the status bar's counts give way while it shows
  clearTimeout(toastTimer);
  toastTimer = setTimeout(hide, 8000);
}

/**
 * Hand a file to the user: a download in the browser, a save dialog in the desktop
 * app. Resolves to the saved path (desktop), "" (browser download) or null (cancelled).
 */
async function deliver(blob, name) {
  if (!platform.save) {
    const a = el("a", { href: URL.createObjectURL(blob), download: name });
    document.body.append(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 30000);
    return "";
  }
  try {
    const path = await platform.save(blob, name);
    if (path) toast(`Saved ${path.split(/[\\/]/).pop()}`, { label: "Show in folder", run: () => platform.reveal(path) });
    return path || null;
  } catch (e) {
    toast(`Couldn't save: ${e?.message || e}`);
    return null;
  }
}

// In the desktop app, download links (STL, parts, ZIPs) open a save dialog instead.
if (platform.save) {
  document.addEventListener("click", async (e) => {
    const a = e.target.closest?.("a[download]");
    if (!a || a.classList.contains("is-disabled") || a.getAttribute("href") === "#") return;
    e.preventDefault();
    const href = a.getAttribute("href");
    const name = a.getAttribute("download") || href.split("/").pop();
    try {
      const blob = href.startsWith("blob:") ? await (await fetch(href)).blob() : await (await platform.fetch(href)).blob();
      deliver(blob, name);
    } catch (err) {
      toast(`Couldn't read the file: ${err.message}`);
    }
  });
}

// ---------------------------------------------------------------- printer profile
// Values the user sets once (Settings) that generators asking for them start from.
const PROFILE_FIELDS = [
  { id: "bed", label: "Print bed size", unit: "mm", axes: ["width (X)", "depth (Y)"], kind: "vec2", min: 50, max: 2000 },
  { id: "nozzle", label: "Nozzle diameter", unit: "mm", kind: "number", min: 0.1, max: 2, step: 0.05 },
];
const getProfile = () => store.prefs.get("gw-profile", {}) || {};

function profileValue(path, profile = getProfile()) {
  const [field, axis] = path.split(".");
  const v = profile[field];
  if (v == null) return undefined;
  if (axis) return Array.isArray(v) ? v[{ x: 0, y: 1, z: 2 }[axis]] : undefined;
  return v;
}

function applyProfile(detail) {
  for (const p of detail.parameters) {
    if (!p.profile) continue;
    const v = profileValue(p.profile);
    const d = p.default;
    const fits = Array.isArray(d) ? Array.isArray(v) && v.length === d.length && v.every((x) => typeof x === "number")
      : typeof d === "number" && typeof v === "number";
    if (fits) { p.default = structuredClone(v); p.profiled = true; }
  }
}

function showSettings() {
  showView("about");
  document.title = "Settings | SCAD Workshop";
  $("#about-title").textContent = "Settings";
  const profile = getProfile();
  const uses = state.catalog?.profile_uses || {};
  const nameOf = (key) => { const m = state.catalog?.models.find((x) => x.key === key); return m ? `${m.family_name} ${m.name}` : key; };
  const saveProfile = (id, value) => {
    const next = { ...getProfile() };
    if (value == null) delete next[id]; else next[id] = value;
    store.prefs.set("gw-profile", next);
    if (state.model) state.model = null; // reopen models with the new defaults
    $("#profile-note").textContent = "Saved. Models open with these values from now on.";
  };
  const fields = PROFILE_FIELDS.map((f) => {
    const cur = profile[f.id];
    const used = [...new Set((uses[f.id] || []).map((u) => u.key))].map((k) => el("a", { href: `#/m/${k}`, text: nameOf(k) }));
    let inputs;
    if (f.kind === "vec2") {
      inputs = el("div", { class: "vector named", style: "--n:2" }, f.axes.map((ax, i) => el("label", { "data-axis": ax, style: `--ax:${ax.length}` }, ax,
        el("input", { type: "number", min: f.min, max: f.max, step: "any", value: cur ? cur[i] : "", "aria-label": `${f.label} ${ax}`, placeholder: "not set",
          onchange: (e) => {
            const ins = e.target.closest(".vector").querySelectorAll("input");
            const vals = [...ins].map((x) => (x.value === "" ? null : +x.value));
            saveProfile(f.id, vals.every((x) => x != null && x > 0) ? vals : null);
          } }))));
    } else {
      inputs = el("input", { type: "number", min: f.min, max: f.max, step: f.step || "any", value: cur ?? "", placeholder: "not set", "aria-label": f.label,
        onchange: (e) => saveProfile(f.id, e.target.value === "" ? null : +e.target.value) });
    }
    return el("div", { class: "field profile-field" },
      el("div", { class: "field-head" }, el("span", { class: "label", text: `${f.label} (${f.unit})` })), inputs,
      el("p", { class: "help-inline" }, used.length ? ["Used by ", ...used.flatMap((a, i) => [i ? ", " : "", a]), "."] : "No model uses this yet."));
  });
  const appearance = section("appearance", "Appearance", "The skin is the app's look: its colours, lettering, shapes and icons. Each skin has light and dark colours, chosen by the theme.",
    el("div", { class: "theme-pick", id: "skin-pick", role: "radiogroup", "aria-label": "Skin" }, el("b", { text: "Skin" }), SKINS.map(([id, label]) => el("label", {},
      el("input", { type: "radio", name: "skin", value: id, checked: ui.get().skin === id, onchange: () => setSkin(id) }), " ", label))),
    el("div", { class: "theme-pick", id: "theme-pick", role: "radiogroup", "aria-label": "Theme" }, el("b", { text: "Theme" }), THEMES.map(([id, label]) => el("label", {},
      el("input", { type: "radio", name: "theme", value: id, checked: ui.get().theme === id, onchange: () => setTheme(id) }), " ", label))));
  const printer = section("profile", "Printer profile", "Models that ask for these start with your values instead of the author's. Leave a value empty to keep the defaults. Magnet and tolerance settings aren't linked: each project measures them differently.",
    el("div", { class: "profile-grid" }, fields),
    el("p", { id: "profile-note", class: "muted", role: "status" }));
  const downloads = platform.kind === "desktop" ? section("downloads", "Downloads", "Which format a part's Download offers first when it comes in several, and a model's render. What you pick for one part or model is kept for it.",
    el("div", { class: "theme-pick", id: "format-pref", role: "radiogroup", "aria-label": "Preferred format" }, el("b", { text: "Preferred format" }), PREFERRED.map(([id, label]) => el("label", {},
      el("input", { type: "radio", name: "format-pref", value: id, checked: preferredFormat() === id, onchange: () => store.prefs.set("gw-format-pref", id) }), " ", label)))) : null;
  const sections = [appearance, printer, downloads, ...(platform.kind === "desktop" ? desktopSettings()
    : [section("data", "Your data", "Saved settings and this profile are kept in this browser only. Use Export for OpenSCAD on a model to keep a copy as a file.")])];
  showPage([el("div", { class: "library-settings" }, settingsToc(sections), sections)]);
}

/** One section of a settings page, laid out like Library settings: a heading, a grey intro, then its contents. */
function section(id, title, intro, ...children) {
  return el("section", { class: "ls-section", id: `ls-${id}` }, el("h2", { text: title }), intro ? el("p", { class: "muted", text: intro }) : null, ...children);
}
/** Jump links to a page's sections. */
function settingsToc(sections) {
  return el("nav", { class: "ls-toc", "aria-label": "On this page" }, sections.filter(Boolean).map((sec) => el("a", { href: location.hash,
    text: sec.querySelector("h2").textContent, onclick: (e) => { e.preventDefault(); sec.scrollIntoView({ behavior: "smooth" }); } })));
}

const RELEASES_URL = "https://github.com/DrFlGd/SCAD-Workshop/releases";

/** The text pages share #about-body; a Preact page mounted there is unmounted when another page replaces it. */
let mountedPage = null;
function showPage(children) {
  if (mountedPage) { render(null, mountedPage); mountedPage = null; }
  $("#about-body").replaceChildren(...children.filter((c) => c != null && c !== false));
}

function showLibrarySettings(section) {
  if (platform.kind !== "desktop") { location.hash = "#/settings"; return; }
  // #/library-settings/<section> opens at that section, once it's drawn
  if (section) {
    let tries = 0;
    const go = () => { const s = document.getElementById(`ls-${section}`); if (s) s.scrollIntoView(); else if (tries++ < 40) setTimeout(go, 100); };
    setTimeout(go, 0);
  }
  showView("about");
  document.title = "Library settings | SCAD Workshop";
  $("#about-title").textContent = "Library settings";
  const box = el("div", { id: "library-settings-page" });
  showPage([box]);
  render(html`<${LibrarySettingsPage} />`, box);
  mountedPage = box;
}

function desktopSettings() {
  const info = platform.info || {};
  const gb = (n) => (n == null ? "unknown" : bytes(n));
  const cacheLine = el("span", { text: gb(info.cache_bytes) });
  return [
    section("library", "Library", null,
    el("p", {}, "Library folder: ", el("code", { text: info.library?.path || info.workspace || "not available" }), ". ",
      el("a", { href: "#/library-settings", id: "settings-library-link", text: "Library settings" }),
      " has projects, categories, hidden and deleted items, the trash and updates.")),
    section("engine", "Engine", null,
    info.engine
      ? el("p", {}, `OpenSCAD ${info.engine}, native, up to ${info.concurrency || 1} render${info.concurrency > 1 ? "s" : ""} at once. `, el("br"), el("code", { text: info.engine_path || "" }))
      : el("p", { class: "lic-summary", text: info.engine_error || "OpenSCAD isn't available." }),
    info.os === "windows" ? (() => {
      const picks = store.prefs.get("gw-engine-pick", {}) || {};
      const wasm = Object.values(picks).filter((v) => v === "wasm").length;
      const line = el("p", {}, `On Windows, OpenSCAD is slow with projects made of many files, where the WebAssembly engine (the website's) is faster. The first time you make a model, both run and the faster one is remembered: ${wasm} of ${Object.keys(picks).length} models so far use WebAssembly. `,
        el("button", { type: "button", class: "ghost", text: "Forget", onclick: () => { store.prefs.set("gw-engine-pick", {}); line.firstChild.textContent = "Forgotten: the next render of each model tries both engines again. "; } }));
      return line;
    })() : null,
    mitcadLine(info),
    el("p", {}, "Finished renders are kept so repeating one is instant: ", cacheLine, ". ",
      el("button", { type: "button", class: "ghost", text: "Clear", onclick: async () => {
        try { await platform.workspace.clearCache(); const i = await platform.refreshInfo(); cacheLine.textContent = gb(i.cache_bytes); } catch (e) { toast(String(e)); }
      } }))),
    section("about", "About", null,
    el("p", { id: "app-version" }, `SCAD Workshop ${info.version || "(version unknown)"}. `,
      el("a", { href: RELEASES_URL, target: "_blank", rel: "noopener", text: "All releases and what changed" }))),
  ];
}

/** Settings › Engine: MitCad, for Fusion 360 files (experimental, 0.4.27): where it was found, or how to get it;
 *  Download gets the MitCad release the app's reader of Fusion files comes from (0.4.28), Choose… points the
 *  app at a mitcad-cli of your own (or its folder), Forget looks for it again. */
function mitcadLine(info) {
  const line = el("p", { id: "mitcad-line" });
  const draw = (m) => {
    const fail = (e) => toast(String(e.message || e));
    const choose = el("button", { type: "button", class: "ghost", id: "mitcad-choose", text: "Choose…", onclick: async () => {
      const p = await platform.library.pickFile("Choose MitCad's mitcad-cli", info.os === "windows" ? ["exe"] : []);
      if (!p) return;
      try { draw(await platform.api("mitcad_set", { path: p })); platform.refreshInfo(); } catch (e) { fail(e); }
    } });
    const forget = m.configured ? el("button", { type: "button", class: "ghost", id: "mitcad-forget", text: "Forget", onclick: async () => {
      try { draw(await platform.api("mitcad_set", { path: null })); platform.refreshInfo(); } catch (e) { fail(e); }
    } }) : null;
    const d = m.download;
    // offered until the app's own copy is the one in use
    const download = d && !m.downloaded ? el("button", { type: "button", id: "mitcad-download", text: `Download MitCad ${d.version} (${d.mb} MB)`,
      title: `The MitCad release the app's reader of Fusion files comes from, into the app's own folder (${d.page})`,
      onclick: async (ev) => {
        ev.target.disabled = true;
        try { draw(await runJob(platform.api("mitcad_install"), `Downloading MitCad ${d.version}`, { stoppable: true })); platform.refreshInfo(); }
        catch (e) { ev.target.disabled = false; fail(e); }
      } }) : null;
    // (replaceChildren takes nodes and text only: the parts flattened, the empty ones left out)
    line.replaceChildren(...[el("b", { text: "Fusion 360 files (experimental): " }),
      m.path ? [m.downloaded ? `MitCad ${d?.version || ""}, downloaded by the app: ` : "MitCad found: ", el("code", { text: m.path }), ". "]
        : [d && !m.configured ? "MitCad isn't installed." : m.error || "MitCad isn't available.", " "],
      m.path && !m.downloaded && d ? `The app is tested with MitCad ${d.version}. ` : null,
      download, download ? " " : null,
      !m.path && !d ? [el("a", { href: "https://github.com/mitcad/Mitcad/releases", target: "_blank", rel: "noopener", text: "Get MitCad" }), ". "] : null,
      choose, " ", forget].flat(2).filter((x) => x != null));
  };
  draw(info.mitcad || { error: "MitCad isn't available." });
  return line;
}

// ---------------------------------------------------------------- boot
/** The catalog and parts packs (on the website: the built site; in the app: the library). */
async function loadCatalog() {
  const catalog = await getJSON("data/catalog.json");
  if (platform.kind === "browser") {
    // models that need native OpenSCAD only appear in the desktop app
    state.desktopOnly = new Set(catalog.models.filter((m) => m.browser === false).map((m) => m.key));
    catalog.models = catalog.models.filter((m) => m.browser !== false);
    for (const c of catalog.categories) c.models = c.models.filter((k) => !state.desktopOnly.has(k));
  }
  const libraries = await Promise.all((catalog.libraries || []).map((l) => getJSON(`data/libraries/${l.id}.json`)));
  return { catalog, libraries };
}

/** Read the catalog again after the library changed (a project added, details edited). */
async function reloadCatalog() {
  const { catalog, libraries } = await loadCatalog();
  state.catalog = catalog;
  state.libraries = libraries;
  state.libDetail = {};
  for (const l of libraries) state.libDetail[l.id] = l;
  details.clear(); // model pages reload with the new details (open tabs keep their state)
  state.index = new LocalIndex(catalog, libraries);
  setContext({ catalog, libraries, index: state.index });
  ui.set((s) => ({ catalogVersion: s.catalogVersion + 1 }));
  if (platform.kind === "desktop") refreshCollections(); // (a merged library brings its collections)
  if (ui.get().view === "library" && location.hash.startsWith("#/parts/")) { state.openLib = null; route(); } // (its list too: parts made one or split)
  // the open model's header (name, credits, a broken flag) follows edits
  // (unless another model has been opened meanwhile: its header isn't this one's to draw)
  if (ui.get().view === "model" && state.model) {
    const key = state.model.key;
    loadModel(key).then(({ detail }) => { if (state.model?.key === key && ui.get().view === "model") renderModelHeader(detail); }, () => {});
  }
}

(async function boot() {
  try {
    const { catalog, libraries } = await loadCatalog();
    state.catalog = catalog;
    if (platform.kind !== "browser") document.body.dataset.platform = platform.kind;
    state.engine = platform.makeEngine(state.catalog);
    $("#engine").textContent = state.engine.label;
    $("#save-part").hidden = platform.kind !== "desktop" || !!catalog.library?.read_only;
    $("#model-collect").hidden = $("#part-collect").hidden = $("#save-part").hidden;
    state.libraries = libraries;
    for (const l of state.libraries) state.libDetail[l.id] = l;
    state.index = new LocalIndex(state.catalog, state.libraries);
    mountShell({ platform, catalog: state.catalog, libraries: state.libraries, index: state.index, engine: state.engine,
      loadModel, closeModelTab, openTabs, deliver, reloadCatalog, toast, route, refreshModel, currentValues, savePart,
      // collections (0.4.17): a model made at given settings, and the format Settings › Downloads picks
      renderModel, preferredFormatOf: (formats) => pickFormat(null, formats),
      // the Code tab's edits (an open model's page)
      codeEdits: editsOf, setCodeEdit, codeStatus, generate: () => { if (ui.get().view === "model") generate(); },
      saveCode: () => { if (ui.get().view === "model") openSaveDialog("save"); } });
    ui.set({ ready: true });
  } catch (e) {
    $("#view-browse").hidden = false;
    $("#view-browse").replaceChildren(el("p", { class: "empty", text: `Couldn't load the library: ${e.message}. Reload to try again.` }));
    return;
  }
  route();
})();
