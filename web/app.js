// SCAD Workshop's page script: routing, and the open model's state and what's done with it
// (its values, saved settings and code edits, rendering and the 3D preview, downloads, saving
// parts, making a batch). Every page and dialog is drawn by Preact components in web/ui/.
import { Viewer } from "./viewer.js";
import { createPlatform } from "./platform.js";
import { makeZip } from "./zip.js";
import { changedValues, withChanges, toOpenSCAD, fromOpenSCAD } from "./settings-codec.js";
import { editsKey } from "./lib/edits.js";
import { $, el, humanize, fmt, same, bytes, plural, safeHTML, compileCondition, FORMAT_LABEL, partName, friendlyName } from "./lib/util.js";
import { LocalIndex } from "./ui/index-local.js";
import { mountShell } from "./ui/shell.js";
import { ModelHead, ModelNotes } from "./ui/modelhead.js";
import { Batch } from "./ui/batch.js";
import { SaveDialog } from "./ui/savedsettings.js";
import { ModelForm, compileConditions } from "./ui/modelform.js";
import { setContext } from "./ui/context.js";
import { html, render } from "./lib/html.js";
import { LibrarySettingsPage } from "./ui/libsettings.js";
import { SettingsPage, applyProfile, pickFormat, rememberFormat } from "./ui/settings.js";
import { PartsRail } from "./ui/parts.js";
import { LicensesPage } from "./ui/licenses.js";
import { unflagItems } from "./ui/actions.js";
import { saveComponentThumb, sourceById, rescanLocal } from "./ui/library.js";
import { ui, recordRecent, setTabs, layoutFor, isDark } from "./ui/state.js";
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
  if (m) openModel(`${m[1]}/${m[2]}`, params.get("saved"), params.get("part") ? { part: params.get("part"), again: params.has("again") } : null);
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
  if (state.model?.key === key) { updateStatusForEdits(); drawForm(); }
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

/** A link or a tab to a model that isn't in the library: say so, with nothing to make (Review 7, item 3). */
function showMissing(key) {
  const tabs = ui.get().tabs;
  if (tabs.includes(key)) { tabStates.delete(key); setTabs(tabs.filter((k) => k !== key), null); }
  showView("about");
  document.title = "Not in the library | SCAD Workshop";
  $("#about-title").textContent = "This model isn't in the library";
  showPage([
    el("p", { id: "model-missing" }, "Nothing in the library is called ", el("code", { text: key }),
      ". It may have been removed, or its project read again under other names."),
    el("p", {}, el("a", { href: ui.get().lastBrowse || "#/", text: "Back to the library" })),
  ]);
}

/** One of a folder of examples made into parts (F-15): not listed as a model, but its page opens. */
const isExampleModel = (key) => (state.catalog?.examples_todo || []).some((e) => e.key === key)
  || state.libraries.some((l) => l.items.some((i) => i.example && i.generator === key));

async function loadDetail(key) {
  // a model that's no longer in the library (an open tab, a list made before a project was read again):
  // say so without asking for it
  if (!key.startsWith("@") && state.index && !state.index.get(`gen:${key}`) && !isExampleModel(key)) {
    throw Object.assign(new Error(`${key} isn't in the library`), { status: 404 });
  }
  if (!details.has(key)) {
    // (asked for in a way that answers "not there" without a failed request)
    const d = await platform.api("model_get", { key });
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
  // the author's notes fold away under About this model; their warnings stay visible above the form
  const holder = document.createElement("div");
  holder.append(...(detail.description_html ? safeHTML(detail.description_html) : []));
  const alerts = [...holder.querySelectorAll("[class^=alert]")];
  alerts.forEach((a) => a.remove());
  const hasNotes = !!holder.textContent.trim();
  // every page has a one-line summary under its name: a component (or a model without one) takes its notes' first sentence
  let summary = detail.summary;
  if (!summary && hasNotes) {
    const text = holder.textContent.replace(/\s+/g, " ").trim();
    const first = (text.match(/^.{20,}?[.!?](?=\s|$)/) || [text])[0];
    summary = first.length > 180 ? `${first.slice(0, 177).trimEnd()}…` : first;
  }
  render(html`<${ModelHead} detail=${detail} summary=${summary || ""} hasNotes=${hasNotes} act=${headActions} />`, $("#model-head"));
  const it = state.index?.get(`gen:${detail.key}`);
  const own = { can: !!it?.sourceId && detail.kind !== "component" && !detail.pinned_from, readOnly: !!state.catalog?.library?.read_only,
    save: (v) => saveOwnNotes(it, detail, v) };
  render(html`<${ModelNotes} key=${detail.key} detail=${detail} notes=${[...holder.childNodes]} alerts=${alerts} own=${own} act=${{
    unflag: async () => { if (it) await unflagItems([it]); },
    keepPin: async () => {
      try { await platform.api("pin_accept", { key: detail.key }); await reloadCatalog(); refreshModel(detail.key); } catch (e) { toast(String(e.message || e)); }
    } }} />`, $("#model-notes-root"));
}

/** What the head's buttons do (web/ui/modelhead.js). */
const headActions = {
  reveal: (args) => revealFile(args),
  copyCode: () => copyComponentCode(),
  pin: () => ui.set({ dialog: { type: "pin", key: state.model.key, values: structuredClone(state.values) } }),
  batch: () => openBatch(),
  help: () => setHelp(!document.body.classList.contains("show-help")),
};

/** Your own notes on a model (0.4.23), kept in its metadata (my_notes). */
async function saveOwnNotes(it, detail, v) {
  try {
    await platform.api("meta_set", { source: it.sourceId, level: "item", target: detail.id, patch: { my_notes: v.trim() ? v : null } });
    await reloadCatalog();
    toast(v.trim() ? "Notes saved." : "Notes removed.");
  } catch (e) { toast(String(e.message || e)); }
}

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

async function openModel(key, savedId = null, made = null) {
  if (!state.catalog) return;
  showView("model");
  // a made part's "Open with these settings" or "Make again": its recipe, applied once the model is open
  const recipe = made && madeRecipe(made, key);
  if (savedId || made) history.replaceState(null, "", `#/m/${key}`); // the link has done its job
  if (state.model?.key === key) {
    addTab(key);
    if (recipe) {
      state.viewerOwner = "model";
      $("#stage-empty").hidden = true;
      applyRecipe(recipe);
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
    if (e.status === 404) { showMissing(key); return; }
    showView("model");
    setStatus("error", e.message);
    return;
  }
  if (state.model) { cancelJob(); stashTab(); }
  addTab(key);
  recordRecent(`${key.startsWith("@") ? "comp" : "gen"}:${key}`); // components' keys start with @
  state.viewerOwner = "model";
  $("#stage-empty").hidden = true;
  $("#log").hidden = true;
  $("#param-search").value = "";
  formQuery = "";
  renderModelHeader(detail);
  state.usesColour = (await checkUsesColour(detail)) ? key : null;
  const kept = tabStates.get(key);
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
  state.viewer.clear();
  $("#dims").hidden = true;
  setDownload(null);
  state.values = Object.fromEntries(detail.parameters.map((p) => [p.name, structuredClone(p.default)]));
  saved.active = null;
  buildForm();
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
// The form is web/ui/modelform.js; the author's notes and warnings above it can have display
// conditions too (shown or hidden as the settings change).
let formQuery = "";
function buildForm() {
  const { names } = compileConditions(state.model);
  state.nodeConditions = [...document.querySelectorAll("#model-alerts [data-display-condition], #model-notes-body [data-display-condition]")]
    .map((node) => ({ test: compileCondition(node.dataset.displayCondition, names), node }));
  drawForm();
  applyNodeConditions();
}

/** The form, drawn again when a setting, the saved settings or Find a setting change. */
function drawForm() {
  if (!state.model) return;
  render(html`<${ModelForm} key=${state.model.key} model=${state.model} values=${state.values} q=${formQuery} set=${setValue}
    bar=${{ presets: state.model.presets || [], list: saved.list, active: saved.active || "default", note: saved.note,
      edited: Object.keys(editsOf(state.model.key)).length, act: barActions }} />`, $("#params"));
}

function setValue(p, v) {
  state.values[p.name] = v;
  drawForm();
  applyNodeConditions();
  updateStatusForEdits();
}

function applyNodeConditions() {
  for (const c of state.nodeConditions || []) {
    const show = c.test(state.values);
    c.node.hidden = !show;
    c.node.style.display = show ? "" : "none";
  }
}

/** Help under each setting (⋯ on the model page), kept between sessions. */
function setHelp(on) {
  document.body.classList.toggle("show-help", on);
  ui.set({ help: on });
  store.prefs.set("gw-help", on);
}
setHelp(!!store.prefs.get("gw-help", false));

/** Replace every setting (missing names fall back to defaults) and update the form. */
function setAllValues(values) {
  for (const p of state.model.parameters) state.values[p.name] = structuredClone(p.name in values ? values[p.name] : p.default);
  drawForm();
  applyNodeConditions();
  updateStatusForEdits();
}

$("#param-search").addEventListener("input", (e) => { formQuery = e.target.value; drawForm(); });

// ---------------------------------------------------------------- saved settings and share links
// saved.active: "default" | "preset:<i>" | "saved:<id>" (what the picker shows)
const saved = { list: [], active: null, note: null };

/** What the saved-settings bar above the form does (web/ui/savedsettings.js). */
const barActions = {
  choose: (value) => chooseSettings(value),
  save: () => openSaveDialog("save"),
  copy: () => copySettings(),
  rename: () => openSaveDialog("rename"),
  remove: () => deleteSaved(),
  exportAll: () => exportSettings(),
  importFile: () => $("#settings-import").click(),
};

async function loadSaved() {
  const key = state.model.key;
  try { saved.list = await store.settings.list(key); } catch { saved.list = []; }
  if (state.model?.key === key) drawForm();
}

const activeSaved = () => saved.list.find((r) => `saved:${r.id}` === saved.active) || null;

let noteTimer = null;
function note(text, kind = "") {
  saved.note = { text, kind };
  drawForm();
  clearTimeout(noteTimer);
  noteTimer = setTimeout(() => { saved.note = null; drawForm(); }, 9000);
}

const skippedText = (n) => (n ? ` ${plural(n, "setting")} didn't match this version of the model and stayed at the default.` : "");

function chooseSettings(value) {
  const key = state.model.key;
  // Defaults and saved settings bring their own code (saved without edits: the original), presets only values
  const replacesCode = value === "default" || value.startsWith("saved:");
  if (replacesCode && codeStatus(key) === "unsaved" && !confirm("Go back to the original code? Your edits to the code aren't saved.")) {
    drawForm();
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
  drawForm();
  generate();
}

let saveOpened = 0;
function openSaveDialog(mode) {
  if (!state.model) return;
  const cur = activeSaved();
  const changes = Object.keys(changedValues(state.model, state.values)).length;
  const files = Object.keys(editsOf(state.model.key)).length;
  const spec = {
    mode, name: mode === "rename" ? cur?.name || "" : cur ? cur.name : suggestName(),
    updating: mode === "save" && cur ? cur.name : null,
    note: mode === "rename" ? "" : (files && !changes ? `Keeps your edits to ${plural(files, "file")} of code (the original stays as it is)`
      : `Keeps the ${plural(changes, "setting")} you changed from the defaults` + (files ? ` and your edits to ${plural(files, "file")} of code (the original stays as it is)` : "")) + ".",
  };
  saved.dialogMode = mode;
  render(html`<${SaveDialog} key=${++saveOpened} spec=${spec} save=${saveSettings} />`, $("#save-dialog"));
  $("#save-dialog").showModal();
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

async function saveSettings(asNew, name) {
  const dlg = $("#save-dialog");
  const mode = saved.dialogMode;
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
  drawForm();
  generate();
  const n = Object.keys(s.changes).length;
  note(`Pasted “${s.name}”: ${plural(n, "setting")} changed from the defaults.${s.skipped ? ` ${plural(s.skipped, "value")} didn't match this model and ${s.skipped === 1 ? "was" : "were"} skipped.` : ""}`, s.skipped ? "warn" : "ok");
});

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

function setDownload(blob, name) {
  state.lastStl = blob ? { blob, name: name || "model.stl" } : null;
  return applyDownloadFormat();
}

// ---------------------------------------------------------------- download formats (milestone 5)
// A model's render downloads as STL or 3MF (the core writes the 3MF from the render); a part with
// several formats has one Download button and a format beside it. The choice is kept per model or
// part, and Settings › Downloads gives the default.
let formatJob = 0;
async function applyDownloadFormat() {
  const a = $("#download"), sel = $("#download-format");
  sel.hidden = false;
  // a 2D model downloads its drawing as SVG or DXF (0.4.23)
  const formats = isFlat() ? ["svg", "dxf"] : ["stl", "3mf"];
  if ([...sel.options].map((o) => o.value).join() !== formats.join()) sel.replaceChildren(...formats.map((f) => el("option", { value: f, text: FORMAT_LABEL[f] })));
  const fmt = state.model ? pickFormat(state.model.key, formats) : formats[0];
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
      blob = (await state.engine.render({ ...forRender(m), format: "dxf" }, renderValues(m, state.lastResultValues)).promise).blob;
    } catch (e) {
      toast(`Couldn't make the DXF: ${e?.message || e}`);
      blob = null;
    }
  }
  name = name.replace(/\.(stl|svg)$/i, `.${isFlat() ? fmt : "stl"}`);
  if (blob && fmt === "3mf") {
    try {
      // OpenSCAD's own 3MF; without one (the code was edited since), the app writes one from the render
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
  if (!m || !state.engine?.info?.engine || !state.lastResultValues) return null;
  const sent = forRender(m);
  if (editsKey(sent.edits) !== (state.renderedEdits || "")) return null;
  return (await state.engine.render({ ...sent, format: "3mf" }, renderValues(m, state.lastResultValues)).promise).blob;
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
  if (!sha || detail.kind === "component") return false;
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
  return state.downloadFormat === "3mf" && !!state.engine?.info?.engine;
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
    const own = await state.engine.render({ ...sent, format: "3mf" }, edits ? changedValues(detail, full) : full).promise.catch(() => null);
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
  drawForm();
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
  if (state.catalog?.library?.read_only) return;
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
// Batch generate (⋯ on the model page): the dialog is web/ui/batch.js; this makes what it lists.
let batchOpened = 0;

/** Batch generate… (⋯ on the model page). */
function openBatch() {
  if (!state.model) return;
  render(html`<${Batch} key=${state.model.key} model=${state.model} values=${state.values} saved=${saved.list} opened=${++batchOpened}
    keepable=${!state.catalog.library?.read_only && !isFlat()} make=${makeBatch} />`, $("#batch"));
  $("#batch").showModal();
}

/**
 * Make every part Batch generate lists (`combos`: { values, file, part }), then download them as one
 * ZIP or (keep) save each to the Parts Library. `report(text, done)`: progress, done from 0 to 1.
 * Returns { promise (what happened, in words), cancel }.
 */
function makeBatch(combos, keep, report) {
  const run = { cancelled: false, jobs: new Set(), cancel() { this.cancelled = true; for (const j of this.jobs) j.cancel(); } };
  run.promise = (async () => {
    const model = state.model;
    cancelJob();
    const files = [];
    const names = new Set();
    const started = Date.now();
    const width = Math.max(1, Math.min(state.engine.concurrency || 1, combos.length));
    const results = new Array(combos.length);
    // 3MF chosen beside Download: each made as OpenSCAD's own 3MF, colours and objects kept (F-28)
    // a 2D model: its drawings, in the format chosen beside Download (0.4.23)
    const flat = isFlat() ? state.downloadFormat : null;
    if (flat && keep) return "A 2D model's drawings can't be kept in the Parts Library yet: download them as a ZIP.";
    const as3mf = !flat && partsAs3mf();
    const ext = flat || (as3mf ? "3mf" : "stl");
    let next = 0, done = 0;
    const progress = () => report(width > 1 ? `Making ${combos.length} parts, ${width} at a time: ${done} done`
      : `Making ${Math.min(done + 1, combos.length)} of ${combos.length}: ${combos[Math.min(done, combos.length - 1)].part}`, done / combos.length);
    const stopped = () => Object.assign(new Error("Batch cancelled."), { cancelled: true });
    const worker = async () => {
      while (next < combos.length) {
        if (run.cancelled) throw stopped();
        const i = next++;
        const { values } = combos[i];
        const job = state.engine.render(as3mf || flat ? { ...forRender(model), format: ext } : forRender(model), renderValues(model, values));
        run.jobs.add(job);
        try { results[i] = { values, result: await job.promise }; } finally { run.jobs.delete(job); }
        done++;
        progress();
      }
    };
    // the preview shows the last one: made as an STL too when the parts are 3MFs
    const previewOf = async (r) => as3mf || flat === "dxf" ? { values: r.values, result: await state.engine.render(forRender(model), renderValues(model, r.values)).promise } : r;
    const showLast = async () => {
      const last = await previewOf(results[combos.length - 1]);
      state.viewerOwner = "model";
      await showResult(last.result, last.values, true, editsKey(editsOf(model.key)));
    };
    try {
      progress();
      await Promise.all(Array.from({ length: width }, worker));
      if (keep) {
        // each to local/Made parts/<model>/ with its recipe, named from what varies
        let first = null;
        for (let i = 0; i < combos.length; i++) {
          if (run.cancelled) throw stopped();
          report(`Saving ${i + 1} of ${combos.length} to the Parts Library…`);
          const r = await savePartFile(model, results[i].result.blob, results[i].values, { name: combos[i].part, category: model.kind === "component" ? null : model.category || null, format: ext });
          first ||= r;
        }
        const href = await madePartHref(first);
        const text = `Done: ${plural(combos.length, "part")} saved to the Parts Library under Made parts › ${first.folder}, in ${Math.round((Date.now() - started) / 1000)} s.`;
        toast(`Saved ${plural(combos.length, "part")} to the Parts Library.`, href ? { label: "Show", run: () => { $("#batch").close(); location.hash = href; } } : null);
        await showLast();
        report(text, 1);
        setStatus("ok", `${plural(combos.length, "part")} saved to the Parts Library. The preview shows the last one.`);
        return text;
      }
      for (let i = 0; i < combos.length; i++) {
        let name = combos[i].file;
        name = name.replace(/[^A-Za-z0-9._-]+/g, "_").slice(0, 140);
        while (names.has(name)) name += "_";
        names.add(name);
        files.push({ name: `${name}.${ext}`, data: new Uint8Array(await results[i].result.blob.arrayBuffer()) });
      }
      await showLast();
      const zip = makeZip(files);
      const took = Math.round((Date.now() - started) / 1000);
      const where = await deliver(zip, `${model.family}-${model.id}-batch-${files.length}.zip`);
      setStatus("ok", `Batch of ${files.length} ${where === null ? "made" : where ? "saved" : "downloaded"}. The preview shows the last one.`);
      return `Done: ${files.length} files (${bytes(zip.size)}) in ${took} s. ` +
        (where === null ? "Not saved." : where ? `Saved to ${where}.` : "Your download has started.");
    } catch (e) {
      return e.cancelled ? `Cancelled. ${keep ? "Parts already saved stay in the Parts Library." : "Nothing was downloaded."}` : `Stopped: ${e.message}`;
    }
  })();
  return run;
}

// ---------------------------------------------------------------- part libraries
async function openLibrary(libId, itemId) {
  showView("library");
  let lib = state.libDetail[libId];
  if (!lib) {
    try {
      lib = state.libDetail[libId] = await getJSON(`data/libraries/${libId}.json`);
    } catch {
      state.openLib = null;
      drawPartsRail(null);
      return;
    }
  }
  if (state.openLib !== libId) {
    state.openLib = libId;
    // the pack's project documents (its README), as on model pages, and in the side viewer's tabs (UI-25)
    const sourceId = lib.source_id || state.libraries.find((l) => l.id === libId)?.source_id || null;
    ui.set({ partRef: sourceId ? { source: sourceId } : null });
  }
  const item = lib.items.find((i) => i.id === itemId) || lib.items.find((i) => i.preview_url) || lib.items[0];
  if (!itemId && item) history.replaceState(null, "", `#/parts/${libId}/${item.id}`);
  drawPartsRail(lib, item);
  showPart(lib, item);
}

/** The rail beside the 3D view on a parts pack's page (web/ui/parts.js). */
function drawPartsRail(lib, item = null) {
  render(html`<${PartsRail} key=${lib?.id || ""} lib=${lib} item=${item} partRef=${ui.get().partRef} reveal=${revealFile} />`, $("#parts-rail"));
}

/** A part's preview in the shared 3D view (the rail beside it: drawPartsRail). */
async function showPart(lib, item) {
  if (!item) return;
  document.title = `${item.name}, ${lib.name} | SCAD Workshop`;
  ui.set({ partItem: item.id }); // (the side viewer shows its own content too)
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
    const url = URL.createObjectURL(await (await platform.fetch(item.preview_url)).blob());
    fileColoursAvailable(false);
    const d = await state.viewer.load(url);
    URL.revokeObjectURL(url);
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
  const box = el("div", { id: "licenses-page" });
  showPage([box]);
  render(html`<${LicensesPage} focus=${focus} />`, box);
  mountedPage = box;
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

function showSettings() {
  showView("about");
  document.title = "Settings | SCAD Workshop";
  $("#about-title").textContent = "Settings";
  const box = el("div", { id: "settings-page" });
  showPage([box]);
  // (a changed profile: models open with its values from then on)
  render(html`<${SettingsPage} onProfileChange=${() => { if (state.model) state.model = null; }} />`, box);
  mountedPage = box;
}

/** The text pages share #about-body; a Preact page mounted there is unmounted when another page replaces it. */
let mountedPage = null;
function showPage(children) {
  if (mountedPage) { render(null, mountedPage); mountedPage = null; }
  $("#about-body").replaceChildren(...children.filter((c) => c != null && c !== false));
}

function showLibrarySettings(section) {
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

// ---------------------------------------------------------------- boot
/** The library's catalog and parts packs. */
async function loadCatalog() {
  const catalog = await getJSON("data/catalog.json");
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
  refreshCollections(); // (a merged library brings its collections)
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
    document.body.dataset.platform = platform.kind;
    state.engine = platform.makeEngine(state.catalog);
    $("#engine").textContent = state.engine.label;
    $("#save-part").hidden = !!catalog.library?.read_only;
    $("#model-collect").hidden = $("#save-part").hidden;
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
