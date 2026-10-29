// The side viewer (Phase 4): a collapsible panel on the right of the window with
// tabs along its edge. "Code" shows the model's OpenSCAD files (editable on the model's
// own page: the edits render in place of the files and are kept with saved settings,
// never written over the project's files), and the
// other tabs the project's reference documents: its README (HTML, made from the
// Markdown when the project was read), PDFs, readme.txt. It follows what's on
// screen: the open model, or in the library the selected item or the project page.
// Open or folded, the tab and the width are remembered (docs/DESKTOP_PLAN.md,
// "Reference documents and the side viewer").
import { html, useState, useEffect, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref, isDark, resolvedTheme } from "./state.js";
import { ctx } from "./context.js";
import { Icon } from "./icons.js";

const isDesktop = () => ctx.platform?.kind === "desktop";
const MIN_W = 320;

// ---------------------------------------------------------------- what to show

/** Where a model's documents come from: { source } (a library project), { library } (an app library), { family } (website). */
export async function projectRefFor(detail) {
  if (!detail) return null;
  if (detail.kind === "component") {
    const c = detail.component || {};
    return c.provider === "project" && c.source_id ? { source: c.source_id } : { library: c.library };
  }
  if (detail.pinned_from) {
    try { return await projectRefFor((await ctx.loadModel(detail.pinned_from)).detail); } catch { return null; }
  }
  const id = detail.key.split("/")[0];
  return isDesktop() ? { source: id } : { family: id };
}

/** The same for an item in the library index, without loading its model (null: load it to know). */
export function refForItem(it) {
  if (!it) return null;
  if (it.kind === "component") {
    const lib = (ctx.catalog.component_libraries || []).find((l) => l.active && l.name === it.project);
    return lib?.provider === "project" && lib.source_id ? { source: lib.source_id } : { library: it.project };
  }
  if (it.sourceId === "pinned") return null; // a pinned component: its library's, known from the model
  if (!isDesktop()) return it.kind === "part" ? { none: true } : { family: it.sourceId };
  return { source: it.sourceId };
}

const refKey = (r) => (r ? JSON.stringify(r) : "");
const docCache = new Map(); // refKey -> Promise<{ docs, folder, path, github }>
let docCacheVersion = -1;

/** A project's documents: { docs: [{ id, title, kind, src?, url? }], folder (library-relative), path, github }. */
export function listDocs(ref) {
  if (!ref || ref.none) return Promise.resolve({ docs: [] });
  const v = ui.get().catalogVersion;
  if (v !== docCacheVersion) { docCache.clear(); docCacheVersion = v; }
  const k = refKey(ref);
  if (!docCache.has(k)) {
    let p;
    if (ref.family) {
      const fam = (ctx.catalog.families || []).find((f) => f.id === ref.family);
      p = Promise.resolve({ docs: fam?.docs || [] });
    } else if (isDesktop()) {
      p = ctx.platform.api("docs_list", ref).catch(() => ({ docs: [] }));
    } else p = Promise.resolve({ docs: [] });
    docCache.set(k, p);
  }
  return docCache.get(k);
}

async function getDoc(ref, doc) {
  if (doc.url) {
    const r = await ctx.platform.fetch(doc.url);
    if (!r.ok) throw new Error(`Couldn't load ${doc.title}.`);
    const text = await r.text();
    return doc.kind === "html" ? { kind: "html", html: text, relative: false } : { kind: "text", text };
  }
  if (doc.kind === "pdf") {
    const buf = await ctx.platform.apiBytes("doc_get", { ...ref, doc: doc.id });
    return { kind: "pdf", blob: new Blob([buf], { type: "application/pdf" }) };
  }
  return ctx.platform.api("doc_get", { ...ref, doc: doc.id });
}

/** Open the viewer on a tab: "code", "doc" (the first document) or a document's id. */
export function openSide(tab, extra = {}) {
  const side = ui.get().side || {};
  setPref({ side: { ...side, open: true, tab } });
  ui.set(extra);
}

/** What's on screen: { modelKey?, ref? (null: from the model), open (a model page: its details are loaded anyway) }. */
function useTarget(s) {
  if (!s.ready) return null;
  if (s.view === "model" && s.activeTab) return { modelKey: s.activeTab, ref: null, open: true };
  if (s.view === "browse") {
    if (s.selection.length === 1) {
      const it = ctx.index.get(s.selection[0]);
      if (it && (it.kind === "generator" || it.kind === "component")) return { modelKey: it.key, ref: refForItem(it) };
      if (it && it.kind === "part") return { ref: refForItem(it) };
    }
    if (s.scope.startsWith("source:")) return { ref: { source: s.scope.slice(7) } };
  }
  // a part page: its pack's project
  if (s.view === "library" && s.partRef) return { ref: s.partRef };
  return null;
}

// ---------------------------------------------------------------- OpenSCAD highlighting

const KW = new Set("module function include use if else for let each assert echo intersection_for true false undef".split(" "));
const BUILTIN = new Set(("cube sphere cylinder polyhedron square circle polygon text import surface translate rotate scale resize mirror multmatrix "
  + "color offset hull minkowski union difference intersection linear_extrude rotate_extrude projection render children len concat lookup str chr "
  + "ord search version norm cross abs sign sin cos tan asin acos atan atan2 floor round ceil ln log pow sqrt exp min max rands is_undef is_bool "
  + "is_num is_string is_list is_function parent_module").split(" "));
const esc = (s) => s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]));

/** OpenSCAD source as one HTML string per line (comments, strings, numbers, keywords, built-ins, $variables). */
export function highlight(src) {
  const re = /\/\/[^\n]*|\/\*[\s\S]*?(?:\*\/|$)|"(?:[^"\\\n]|\\.)*"?|\d+(?:\.\d*)?(?:[eE][+-]?\d+)?|\.\d+|\$?[A-Za-z_]\w*|\s+|./g;
  const lines = [""];
  let last = "";
  const push = (cls, t) => {
    t.split("\n").forEach((part, i) => {
      if (i) lines.push("");
      if (part) lines[lines.length - 1] += cls ? `<span class="t${cls}">${esc(part)}</span>` : esc(part);
    });
  };
  let m;
  while ((m = re.exec(src))) {
    let t = m[0];
    let cls = null;
    if (t === "<" && (last === "include" || last === "use")) {
      // include <BOSL2/std.scad>
      const end = src.indexOf(">", m.index);
      const nl = src.indexOf("\n", m.index);
      if (end > 0 && (nl < 0 || end < nl)) { t = src.slice(m.index, end + 1); re.lastIndex = end + 1; cls = "s"; }
    } else if (t.startsWith("//") || t.startsWith("/*")) cls = "c";
    else if (t[0] === '"') cls = "s";
    else if (/^\.?\d/.test(t)) cls = "n";
    else if (t[0] === "$") cls = "v";
    else if (KW.has(t)) cls = "k";
    else if (BUILTIN.has(t)) cls = "b";
    if (!/^\s+$/.test(t)) last = t;
    push(cls, t);
  }
  return lines;
}

// ---------------------------------------------------------------- Code

const fileCache = new Map(); // sha or library path -> Promise<text>

function readFile(f) {
  const k = f.sha || f.rel;
  if (!fileCache.has(k)) {
    const p = f.sha ? ctx.platform.modelFile(f.sha)
      : fetch(ctx.platform.library.url(f.rel)).then((r) => { if (!r.ok) throw new Error(`Couldn't read ${f.rel}.`); return r.text(); });
    p.catch(() => fileCache.delete(k));
    fileCache.set(k, p);
  }
  return fileCache.get(k);
}

/** A model's files for the Code tab: [{ id, label, sha? | generated? | rel? }], and which to show first. */
function modelFiles(detail, extra) {
  const files = [];
  if (extra) files.push({ id: `file:${extra.rel}`, label: extra.label || extra.rel.split("/").pop(), rel: extra.rel, line: extra.line });
  if (!detail) return { files, first: files[0]?.id };
  const entries = Object.entries(detail.files || {});
  if (detail.kind === "component") {
    const c = detail.component || {};
    const own = `/libraries/${c.library}/${c.file}`;
    files.push({ id: "generated", label: "component.scad (these settings)", generated: true });
    const mine = entries.find(([p]) => p === own);
    if (mine) files.push({ id: mine[0], label: `${c.library}/${c.file}`, sha: mine[1], line: c.line });
    for (const [p, sha] of entries.sort((a, b) => a[0].localeCompare(b[0]))) if (p !== own) files.push({ id: p, label: p.replace(/^\/libraries\//, ""), sha });
  } else {
    const entry = detail.entry;
    const short = (p) => p.replace(/^\//, "");
    const e = entries.find(([p]) => p === entry);
    if (e) files.push({ id: e[0], label: short(e[0]), sha: e[1] });
    for (const [p, sha] of entries.sort((a, b) => a[0].localeCompare(b[0]))) if (p !== entry && /\.scad$/i.test(p)) files.push({ id: p, label: short(p), sha });
  }
  return { files, first: extra ? files[0].id : files[0]?.id };
}

/** A file of an open model's own that the Code tab can edit (not a library's file, not a component's). */
const editableFile = (f) => !!(f && f.sha && !f.generated && !f.id.startsWith("/libraries/"));

function CodeView({ detail, extra, editable = false }) {
  const { files, first } = modelFiles(detail, extra);
  const [pick, setPick] = useState(first);
  const [state, setState] = useState({ text: null, error: null }); // the file as it is
  const [find, setFind] = useState("");
  const wrap = useStore(ui, (s) => !!s.side?.wrap);
  useStore(ui, (s) => s.codeRev || 0); // edits changed (typed, a saved variant picked, Defaults)
  const box = useRef(null);
  const ta = useRef(null);
  const hl = useRef(null);
  const hit = useRef(-1);
  const file = files.find((f) => f.id === pick) || files[0];
  // editing: on a model's own page, its own files; the edited text lives on the page (app.js) until saved
  const canEdit = editable && !!detail && editableFile(file);
  const edits = editable && detail ? ctx.codeEdits?.(detail.key) || {} : {};
  const edited = canEdit ? edits[file.id] : undefined;
  const text = state.text == null ? null : edited ?? state.text;
  useEffect(() => { setPick(first); }, [detail?.key, extra?.rel]);
  useEffect(() => {
    if (!file) return;
    let live = true;
    setState({ text: null, error: null });
    const values = detail && (ctx.currentValues?.(detail.key) || Object.fromEntries((detail.parameters || []).map((p) => [p.name, p.default])));
    const p = file.generated ? ctx.platform.api("component_code", { key: detail.key, values }) : readFile(file);
    p.then((t) => live && setState({ text: t, error: null }), (e) => live && setState({ text: null, error: String(e.message || e) }));
    return () => { live = false; };
  }, [file?.id, detail?.key]);
  // a component's own file opens at the module
  useEffect(() => {
    if (state.text == null || !box.current || canEdit) return;
    hit.current = -1;
    const line = file?.line;
    const el = line ? box.current.querySelector(`.cl:nth-child(${line})`) : null;
    if (el) { el.classList.add("mark"); el.scrollIntoView({ block: "start" }); box.current.scrollTop -= 24; } else box.current.scrollTop = 0;
  }, [state.text]);
  // the editor's text follows edits made elsewhere (a saved variant, Defaults, Revert), not its own typing
  useEffect(() => {
    const t = ta.current;
    if (!t || state.text == null) return;
    const want = (ctx.codeEdits?.(detail.key) || {})[file.id] ?? state.text;
    if (t.value !== want) t.value = want;
  });
  // the highlighted copy under the editor scrolls with it, and wraps at the same width
  const follow = () => {
    const t = ta.current, h = hl.current;
    if (!t || !h) return;
    h.style.transform = `translate(${-t.scrollLeft}px, ${-t.scrollTop}px)`;
    h.style.width = wrap ? `${t.clientWidth}px` : "";
  };
  useEffect(() => {
    const t = ta.current;
    if (!t) return;
    follow();
    const ro = new ResizeObserver(follow);
    ro.observe(t);
    return () => ro.disconnect();
  });
  if (!file) return html`<p class="side-empty">No OpenSCAD files to show.</p>`;
  const lines = text != null ? highlight(text) : null;
  const findNext = (back = false) => {
    const q = find.trim().toLowerCase();
    if (!q || !lines || !box.current) return;
    const rows = text.split("\n");
    const n = rows.length;
    const holder = canEdit ? hl.current : box.current;
    for (let k = 1; k <= n; k++) {
      const i = (hit.current + (back ? -k : k) + n * 2) % n;
      if (rows[i].toLowerCase().includes(q)) {
        hit.current = i;
        holder.querySelectorAll(".cl.found").forEach((e) => e.classList.remove("found"));
        const el = holder.children[i];
        el?.classList.add("found");
        if (canEdit && el) ta.current.scrollTop = el.offsetTop - ta.current.clientHeight / 2;
        else el?.scrollIntoView({ block: "center" });
        return;
      }
    }
    ctx.toast?.(`“${find}” isn't in this file.`);
  };
  const type = (v) => ctx.setCodeEdit(detail.key, file.id, v === state.text ? null : v);
  const onKey = (e) => {
    const mod = e.ctrlKey || e.metaKey;
    if (e.key === "Tab" && !mod && !e.altKey && !e.shiftKey) {
      // indent (Shift+Tab still leaves the editor)
      e.preventDefault();
      if (!document.execCommand?.("insertText", false, "    ")) { e.target.setRangeText("    ", e.target.selectionStart, e.target.selectionEnd, "end"); type(e.target.value); }
    } else if (mod && e.key === "Enter") { e.preventDefault(); ctx.generate?.(); }
    else if (mod && (e.key === "s" || e.key === "S")) { e.preventDefault(); ctx.saveCode?.(); }
  };
  const status = detail && Object.keys(edits).length ? ctx.codeStatus?.(detail.key) : "none";
  const label = (f) => (edits[f.id] != null ? `${f.label} (edited)` : f.label);
  const foot = !lines ? null
    : !canEdit ? "read-only"
    : edited != null ? (status === "saved" ? "edited · saved with these settings" : "edited · not saved · Generate uses it")
    : "editable · the project's file stays as it is";
  const rowsHtml = (ls, pad) => ls.map((l, i) => `<div class="cl" data-n="${i + 1}">${l || " "}</div>`).join("") + (pad ? '<div class="cl" data-n=""> </div>' : "");
  return html`<div class="side-code">
    <div class="side-tools">
      ${files.length > 1 ? html`<select class="side-file" aria-label="File" value=${file.id} onChange=${(e) => setPick(e.target.value)} id="side-file">
        ${files.map((f) => html`<option value=${f.id}>${label(f)}</option>`)}</select>` : html`<span class="side-file-one" title=${file.label}>${label(file)}</span>`}
      <input type="search" class="side-find" placeholder="Find" aria-label="Find in this file" value=${find}
        onInput=${(e) => { setFind(e.target.value); hit.current = -1; }} onKeyDown=${(e) => { if (e.key === "Enter") { e.preventDefault(); findNext(e.shiftKey); } }} />
      <button type="button" class="ghost small" aria-pressed=${wrap ? "true" : "false"} title="Wrap long lines"
        onClick=${() => setPref({ side: { ...ui.get().side, wrap: !wrap } })}>Wrap</button>
      <button type="button" class="ghost small" title="Copy this file's code" disabled=${text == null} id="side-copy"
        onClick=${() => navigator.clipboard.writeText(text).then(() => ctx.toast?.("Code copied."), () => ctx.toast?.("The clipboard isn't available."))}>${Icon.code(14)} Copy</button>
      ${canEdit && edited != null ? html`<button type="button" class="ghost small" id="side-revert" title="Back to the project's own version of this file"
        onClick=${() => ctx.setCodeEdit(detail.key, file.id, null)}>Revert</button>` : null}
      ${editable && status === "unsaved" ? html`<button type="button" class="ghost small" id="side-save" title="Keep these edits with the settings, as a variant you can pick later (Ctrl+S). The original file isn't changed."
        onClick=${() => ctx.saveCode?.()}>Save…</button>` : null}
    </div>
    ${state.error ? html`<p class="side-empty">${state.error}</p>`
      : !lines ? html`<p class="side-empty">Loading…</p>`
      : canEdit ? html`<div class=${`code edit${wrap ? " wrap" : ""}`} ref=${box}>
          <div class="ed-hl" ref=${hl} aria-hidden="true" dangerouslySetInnerHTML=${{ __html: rowsHtml(lines, true) }}></div>
          <textarea class="ed-ta" ref=${ta} id="side-editor" spellcheck="false" autocapitalize="off" autocomplete="off" wrap=${wrap ? "soft" : "off"}
            aria-label=${`${file.label}: OpenSCAD code, editable. Ctrl+Enter generates, Ctrl+S saves, Shift+Tab leaves.`}
            onInput=${(e) => type(e.target.value)} onScroll=${follow} onKeyDown=${onKey}></textarea>
        </div>`
      : html`<div class=${`code${wrap ? " wrap" : ""}`} ref=${box} tabindex="0" aria-label=${`${file.label}, ${lines.length} lines, read-only`}
          dangerouslySetInnerHTML=${{ __html: rowsHtml(lines, false) }}></div>`}
    ${lines ? html`<p class="side-foot muted" id="side-foot">${lines.length.toLocaleString()} lines · ${foot}</p>` : null}
  </div>`;
}

// ---------------------------------------------------------------- documents

/** Styles for a document's frame, in the app's colours. */
function docCss() {
  const cs = getComputedStyle(document.documentElement);
  const v = (n) => cs.getPropertyValue(n).trim();
  const night = resolvedTheme() === "night";
  return `:root { color-scheme: ${isDark() ? "dark" : "light"}; }
body { margin: 0; padding: 16px 22px 48px; font: 15px/1.6 ${v("--font") || "system-ui, sans-serif"}; color: ${v("--ink")}; background: ${v("--white")}; overflow-wrap: anywhere; }
a { color: ${v("--focus")}; }
img { max-width: 100%; height: auto; ${night ? "filter: brightness(.82) sepia(.15);" : ""} }
h1, h2 { border-bottom: 1px solid ${v("--steel-2")}; padding-bottom: .25em; line-height: 1.25; }
h1 { font-size: 1.7em; } h2 { font-size: 1.35em; margin-top: 1.6em; }
pre { background: ${v("--panel")}; border: 1px solid ${v("--steel-2")}; padding: 10px 12px; border-radius: 6px; overflow: auto; font-size: 13px; line-height: 1.45; }
code { font-family: ui-monospace, SFMono-Regular, Consolas, "Liberation Mono", monospace; font-size: .88em; background: ${v("--panel")}; padding: 1px 4px; border-radius: 4px; }
pre code { background: none; padding: 0; font-size: inherit; }
table { border-collapse: collapse; margin: 12px 0; display: block; max-width: 100%; overflow: auto; font-size: 14px; }
th, td { border: 1px solid ${v("--steel-2")}; padding: 4px 8px; vertical-align: top; }
blockquote { margin: 0; padding: 2px 14px; border-left: 3px solid ${v("--steel-2")}; color: ${v("--graphite")}; }
hr { border: 0; border-top: 1px solid ${v("--steel-2")}; }
.found { outline: 2px solid ${v("--tool")}; }`;
}

/** The library folder's address (absolute), for a document whose links and pictures are relative to it. */
const folderUrl = (info) => (info?.folder != null && isDesktop() ? new URL(`${ctx.platform.library.url(info.folder)}/`, location.href).href : null);

/** Relative pictures and links made absolute against the project's folder (the frame's own <base>
 *  comes too late for the browser's preloader). Parsed inertly: nothing loads here. */
function absolutize(html, base) {
  const d = new DOMParser().parseFromString(`<!doctype html><body>${html}`, "text/html");
  for (const el of d.querySelectorAll("[src]")) el.setAttribute("src", new URL(el.getAttribute("src"), base).href);
  for (const a of d.querySelectorAll("a[href]")) {
    const h = a.getAttribute("href");
    if (!h.startsWith("#")) a.setAttribute("href", new URL(h, base).href);
  }
  return d.body.innerHTML;
}

function DocView({ refObj, doc, info, onOpenCode }) {
  const [state, setState] = useState({ data: null, error: null });
  const frame = useRef(null);
  const theme = useStore(ui, (s) => s.theme);
  // a link that still opened a page in the frame (a PDF's, say): a way back to the document
  const [away, setAway] = useState(false);
  const [round, setRound] = useState(0);
  const loads = useRef(0);
  useEffect(() => {
    let live = true;
    setState({ data: null, error: null });
    getDoc(refObj, doc).then((data) => live && setState({ data, error: null }), (e) => live && setState({ data: null, error: String(e.message || e) }));
    return () => { live = false; };
  }, [refKey(refObj), doc.id]);
  const data = state.data;
  // the frame's links: in-page anchors scroll, project .scad files open in Code, the web opens in the browser
  const wire = () => {
    const d = frame.current?.contentDocument;
    if (!d || d.__wired) return;
    d.__wired = true;
    d.addEventListener("click", (e) => {
      const a = e.target.closest?.("a[href]");
      if (!a) return;
      e.preventDefault();
      const raw = a.getAttribute("href") || "";
      if (raw.startsWith("#")) {
        const id = decodeURIComponent(raw.slice(1));
        const el = d.getElementById(id) || d.getElementsByName(id)[0];
        el?.scrollIntoView({ block: "start" });
        return;
      }
      let url;
      try { url = new URL(raw, d.baseURI); } catch { return; }
      const folder = folderUrl(info);
      if (folder && url.href.startsWith(folder)) {
        const rel = decodeURIComponent(url.href.slice(folder.length).split("#")[0]);
        if (/\.scad$/i.test(rel)) { onOpenCode({ rel: `${info.folder}/${rel}`, label: rel }); return; }
        if (info.github) { ctx.platform.openUrl(info.github + rel); return; }
        if (info.path) { ctx.platform.library.openPath(`${info.path}/${rel}`); return; }
        return;
      }
      if (/^https?:$/.test(url.protocol)) ctx.platform.openUrl ? ctx.platform.openUrl(url.href) : window.open(url.href, "_blank", "noopener");
    });
  };
  // (wired as soon as the document is there: "load" waits for every image, hundreds in NopSCADlib's README)
  useEffect(() => {
    if (state.data?.kind !== "html") return;
    let n = 0;
    const t = setInterval(() => {
      const d = frame.current?.contentDocument;
      if ((d && d.URL === "about:srcdoc" && d.body && d.readyState !== "loading") || ++n > 100) { clearInterval(t); if (d?.URL === "about:srcdoc") wire(); }
    }, 50);
    return () => clearInterval(t);
  }, [state.data, round]);
  // the app's colours follow the theme
  useEffect(() => {
    const st = frame.current?.contentDocument?.getElementById("app-style");
    if (st) st.textContent = docCss();
  }, [theme]);
  const [pdfUrl, setPdfUrl] = useState(null);
  useEffect(() => {
    if (data?.kind !== "pdf") return;
    const u = URL.createObjectURL(data.blob);
    setPdfUrl(u);
    return () => URL.revokeObjectURL(u);
  }, [data]);
  const onFrameLoad = () => {
    loads.current += 1;
    let d = null;
    try { d = frame.current?.contentDocument; } catch { /* another site's page */ }
    if (data?.kind === "html") { if (d && d.URL === "about:srcdoc") wire(); else setAway(true); }
    else if (loads.current > 1) setAway(true); // a PDF: its first load is the PDF itself
  };
  const back = () => { loads.current = 0; setAway(false); setRound((r) => r + 1); };
  const awayBar = away ? html`<div class="side-tools doc-away" role="status"><span>A link opened a web page here.</span>
    <button type="button" class="ghost small" id="side-doc-back" onClick=${back}>Back to ${doc.title}</button></div>` : null;
  if (state.error) return html`<p class="side-empty">${state.error}</p>`;
  if (!data) return html`<p class="side-empty">Loading…</p>`;
  if (data.kind === "text") return html`<pre class="doc-text">${data.text}</pre>`;
  if (data.kind === "pdf") {
    const external = isDesktop() && info?.path && doc.src ? html`<button type="button" class="ghost small" id="side-pdf-open"
      onClick=${() => ctx.platform.library.openPath(`${info.path}/${doc.src}`)}>Open in your PDF viewer</button>` : null;
    if (navigator.pdfViewerEnabled === false) {
      return html`<div class="side-empty"><p>This window can't show PDFs.</p>${external}</div>`;
    }
    return html`<div class="doc-pdf">${external ? html`<div class="side-tools">${external}</div>` : null}${awayBar}
      ${pdfUrl ? html`<iframe class="doc-frame" title=${doc.title} src=${pdfUrl} ref=${frame} key=${round} onLoad=${onFrameLoad}></iframe>` : null}</div>`;
  }
  const base = data.relative ? folderUrl(info) : null;
  const body = base ? absolutize(data.html, base) : data.html;
  const srcdoc = `<!doctype html><html><head><meta charset="utf-8"><style id="app-style">${docCss()}</style></head><body>${body}</body></html>`;
  // (no scripts run in the frame: sandbox without allow-scripts; same origin only so the page can style it and follow its links)
  return html`<div class="doc-box">${awayBar}<iframe class="doc-frame" title=${doc.title} sandbox="allow-same-origin" srcdoc=${srcdoc} ref=${frame} key=${round} onLoad=${onFrameLoad}></iframe></div>`;
}

// ---------------------------------------------------------------- the panel

export function SideView() {
  const s = useStore(ui, (st) => ({ ready: st.ready, view: st.view, activeTab: st.activeTab, selection: st.selection, scope: st.scope,
    side: st.side || {}, sideFile: st.sideFile, v: st.catalogVersion, partRef: st.partRef, partItem: st.partItem }));
  const target = useTarget(s);
  const [detail, setDetail] = useState(null);
  const [ref, setRef] = useState(null);
  const [info, setInfo] = useState(null);
  const modelKey = target?.modelKey || null;
  const tRef = target?.ref ? refKey(target.ref) : "";
  // in the library, a model's details load only when its code is shown (its documents are known from the index)
  const needDetail = !!modelKey && (target.open || !target.ref || (s.side.open && s.side.tab === "code"));
  // (what's shown stays while it's fetched again after a library change, e.g. a thumbnail saved:
  // only another model or project clears it, so the panel doesn't flicker or lose its place)
  const shownKey = useRef(null);
  useEffect(() => {
    let live = true;
    if (shownKey.current !== modelKey) { shownKey.current = modelKey; setDetail(null); }
    if (target?.ref) setRef(target.ref);
    if (!needDetail) { setDetail(null); shownKey.current = null; if (!target?.ref) setRef(null); return; }
    ctx.loadModel(modelKey).then(async ({ detail: d }) => {
      if (!live) return;
      setDetail((old) => (old && old.key === d.key && JSON.stringify(old.files) === JSON.stringify(d.files) ? old : d));
      if (!target.ref) { const r = await projectRefFor(d); if (live) setRef((old) => (refKey(old) === refKey(r) ? old : r)); }
    }, () => live && !target.ref && setRef(null));
    return () => { live = false; };
  }, [modelKey, tRef, needDetail, s.v]);
  const shownRef = useRef("");
  useEffect(() => {
    let live = true;
    if (shownRef.current !== refKey(ref)) { shownRef.current = refKey(ref); setInfo(null); }
    listDocs(ref).then((i) => live && setInfo((old) => (old && JSON.stringify(old) === JSON.stringify(i) ? old : i)));
    return () => { live = false; };
  }, [refKey(ref), s.v]);
  // a project file asked for (a README link, the Files tab) shows while its project is on screen
  const extra = s.sideFile && (!s.sideFile.ref || refKey(s.sideFile.ref) === refKey(ref)) ? s.sideFile : null;
  // content you added (milestone 6): the project's, and the open model's or part's own
  const here = new Set(["project", modelKey && !modelKey.startsWith("@") ? `item:${modelKey.slice(modelKey.indexOf("/") + 1)}` : null,
    s.view === "library" && s.partItem ? `part:${s.partItem}` : null]);
  const docs = (info?.docs || []).filter((d) => !d.for || here.has(d.for));
  const tabs = [...(modelKey || extra ? [{ id: "code", title: "Code", kind: "code" }] : []), ...docs];
  // the open panel lies over the right of the 3D view; the size bar and the colours move clear of it
  const over = target && tabs.length && s.side.open ? Math.max(MIN_W, s.side.width || 560) : 0;
  useEffect(() => {
    document.documentElement.style.setProperty("--side-over", over ? `min(${over}px, calc(100vw - 90px))` : "0px");
    window.dispatchEvent(new CustomEvent("gw-side-over"));
  }, [over]);
  if (!target || !tabs.length) return null;
  const want = s.side.tab === "doc" ? docs[0]?.id : s.side.tab;
  const current = tabs.find((t) => t.id === want) || tabs.find((t) => t.kind === (s.side.tab === "code" ? "code" : "html")) || tabs[0];
  const open = !!s.side.open;
  const width = Math.max(MIN_W, s.side.width || 560);
  const toggle = (id) => setPref({ side: { ...s.side, open: !(open && current.id === id), tab: id } });
  const close = () => setPref({ side: { ...s.side, open: false } });
  const startResize = (e) => {
    e.preventDefault();
    const x0 = e.clientX, w0 = width;
    const move = (ev) => ui.set({ side: { ...ui.get().side, width: Math.min(window.innerWidth - 120, Math.max(MIN_W, w0 + x0 - ev.clientX)) } });
    const up = () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up); setPref({ side: ui.get().side }); };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  const title = detail ? detail.name : current.title;
  // a model's page has its Code and README buttons in its header: the tabs along the edge show only
  // while the panel is open, to switch between them (one Code button, 0.4.23)
  const strip = open || !target.open;
  const tabIcon = (t) => (t.kind === "code" ? Icon.code(14) : t.kind === "pdf" || t.attachment ? Icon.file(14) : Icon.book(14));
  return html`<aside class=${`sideview${open ? " open" : ""}${strip ? "" : " bare"}`} style=${`--side-w:${width}px`} aria-label="Code and documents"
    onKeyDown=${(e) => { if (e.key === "Escape" && open) { e.stopPropagation(); close(); } }}>
    ${strip ? html`<div class="side-tabs" role="tablist" aria-orientation="vertical" aria-label="Show">
      ${tabs.map((t) => html`<button type="button" role="tab" class=${`side-tab${open && t.id === current.id ? " on" : ""}`} key=${t.id}
        aria-selected=${open && t.id === current.id ? "true" : "false"} data-side-tab=${t.id} data-kind=${t.kind}
        title=${open && t.id === current.id ? "Fold away" : `Show ${t.kind === "code" ? "the OpenSCAD code" : t.title}`} onClick=${() => toggle(t.id)}>
        ${tabIcon(t)}<span>${t.title}</span></button>`)}
    </div>` : null}
    ${open ? html`<section class="side-panel" role="tabpanel" aria-label=${current.title} data-side-panel=${current.id}>
      <div class="side-resize" role="separator" aria-orientation="vertical" aria-label="Resize" title="Drag to resize" onPointerDown=${startResize}></div>
      <header class="side-head">
        <div class="side-title"><b>${current.kind === "code" ? "Code" : current.title}</b>
          <span class="muted">${current.kind === "code" ? (detail ? title : extra?.label || "") : detail ? `${detail.family_name || ""}` : ""}</span></div>
        <button type="button" class="ghost small side-close" aria-label="Fold away" title="Fold away (Esc)" onClick=${close}>${Icon.close(14)}</button>
      </header>
      <div class="side-body">
        ${current.kind === "code" ? (extra || detail ? html`<${CodeView} detail=${extra ? null : detail} extra=${extra} key=${extra ? extra.rel : detail?.key}
              editable=${!!(target.open && !extra && detail && detail.kind !== "component" && !detail.pinned_from)} />` : html`<p class="side-empty">Loading…</p>`)
          : current.attachment ? html`<${AttachmentView} refObj=${ref} doc=${current} info=${info} key=${`${refKey(ref)}/${current.id}`} />`
          : html`<${DocView} refObj=${ref} doc=${current} info=${info} key=${`${refKey(ref)}/${current.id}`}
              onOpenCode=${(f) => openSide("code", { sideFile: { ...f, ref } })} />`}
      </div>
    </section>` : null}
  </aside>`;
}

/** Content you added (milestone 6): a picture or video shown here, a link or another file opened outside,
 * a PDF or text as a document; each with Remove. */
function AttachmentView({ refObj, doc, info }) {
  const ro = !!ctx.catalog?.library?.read_only;
  const remove = async () => {
    if (!confirm(`Remove ${doc.title}? ${doc.url ? "" : "Its file goes to the library's backups/removed folder."}`)) return;
    try {
      await ctx.platform.api("attachment_remove", { source: refObj.source, level: doc.level, target: doc.target, index: doc.index });
      setPref({ side: { ...(ui.get().side || {}), open: false } }); // (rather than showing another document in its place)
      await ctx.reloadCatalog();
      ctx.toast(`Removed ${doc.title}.`);
    } catch (e) { ctx.toast(`Couldn't remove it: ${e?.message || e}`); }
  };
  const open = () => (doc.url ? ctx.platform.openUrl(doc.url) : ctx.platform.library.openPath(doc.path));
  const tools = html`<div class="side-tools attachment-tools">
    <span class="muted">${doc.url ? doc.url : `Added ${String(doc.added || "").slice(0, 10)}`}</span>
    ${doc.missing ? null : html`<button type="button" class="ghost small" id="attachment-open" onClick=${open}>${doc.url ? "Open in the browser" : "Open in its app"}</button>`}
    ${ro ? null : html`<button type="button" class="ghost small danger-text" id="attachment-remove" onClick=${remove}>Remove</button>`}</div>`;
  if (doc.missing) return html`<div>${tools}<p class="side-empty">Its file isn't in the library any more.</p></div>`;
  const body = doc.kind === "image" ? html`<div class="attachment-media"><img src=${doc.href} alt=${doc.title} /></div>`
    : doc.kind === "video" ? html`<div class="attachment-media"><video src=${doc.href} controls preload="metadata"></video></div>`
    : doc.kind === "pdf" || doc.kind === "text" ? html`<${DocView} refObj=${refObj} doc=${doc} info=${info} onOpenCode=${() => {}} />`
    : html`<p class="side-empty">${doc.url ? "A link: it opens in your browser." : "This kind of file opens in its own app."}</p>`;
  return html`<div class="attachment-view">${tools}${body}</div>`;
}

/** "Code" and "README" (a project's documents) buttons, for the inspector and the project page. */
export function SideButtons({ modelKey, refObj, small = true }) {
  const [docs, setDocs] = useState([]);
  useEffect(() => {
    let live = true;
    setDocs([]);
    (async () => {
      const it = modelKey ? ctx.index.get(`${modelKey.startsWith("@") ? "comp" : "gen"}:${modelKey}`) : null;
      const r = refObj || refForItem(it) || (modelKey ? await projectRefFor((await ctx.loadModel(modelKey)).detail) : null);
      const i = await listDocs(r);
      if (live) setDocs((i.docs || []).filter((d) => !d.for || d.for === "project"));
    })().catch(() => {});
    return () => { live = false; };
  }, [modelKey, refKey(refObj)]);
  const cls = `ghost${small ? " small" : ""}`;
  return html`${modelKey ? html`<button type="button" class=${cls} data-side-open="code" onClick=${() => openSide("code", { sideFile: null })}>${Icon.code(14)} Code</button>` : null}
    ${docs.map((d) => html`<button type="button" class=${cls} data-side-open=${d.id} onClick=${() => openSide(d.id)}>
      ${d.kind === "pdf" ? Icon.file(14) : Icon.book(14)} ${d.title}</button>`)}`;
}

/** The same buttons for the model page's header (plain DOM, app.js). */
export function sideModelButtons(box, detail) {
  const mk = (id, label, icon, tab) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "ghost small";
    b.dataset.sideOpen = id;
    b.innerHTML = `${icon}<span>${label}</span>`;
    b.addEventListener("click", () => openSide(tab, { sideFile: null }));
    return b;
  };
  const ICON_CODE = '<svg class="icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 7l-5 5 5 5M16 7l5 5-5 5"/></svg>';
  const ICON_DOC = '<svg class="icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2z"/><path d="M4 21V5"/></svg>';
  // (a Fusion 360 file has no code to show; 0.4.27)
  if (detail.engine !== "mitcad") box.prepend(mk("code", "Code", ICON_CODE, "code"));
  projectRefFor(detail).then(listDocs).then((i) => {
    if (!box.isConnected) return;
    const first = (i.docs || []).filter((d) => !d.for || d.for === "project")[0];
    // and content you added to this model (milestone 6): the edge tabs only show while the panel is open
    const own = (i.docs || []).filter((d) => d.for === `item:${detail.id}`);
    let at = box.querySelector("[data-side-open=code]") || null;
    for (const d of [first, ...own].filter(Boolean)) {
      const b = mk(d.id, d === first && d.kind === "html" ? "README" : d.title, ICON_DOC, d.id);
      if (at) at.after(b); else box.prepend(b);
      at = b;
    }
    box.hidden = false;
  }, () => {});
  box.hidden = false;
}
