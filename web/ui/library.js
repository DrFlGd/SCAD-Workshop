// The library in the desktop app: adding projects, project pages (details,
// documents, files, versions, updates), project problems (shown in Library settings), thumbnails for newly
// read models, and the library section of Settings. On the website none of
// this shows (there is no library there).
import { html, useState, useEffect, useMemo, useRef } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, addJob } from "./state.js";
import { ctx, scopeHash } from "./context.js";
import { Icon } from "./icons.js";
import { Viewer } from "../viewer.js";
import { bytes, plural } from "../lib/util.js";
import { hideProject, deleteProject, hideItems } from "./actions.js";
import { SideButtons, openSide } from "./sideview.js";

export const api = (cmd, args) => ctx.platform.api(cmd, args);
export const readOnly = () => !!ctx.catalog?.library?.read_only;

/** A project (source) entry from the catalog. */
export const sourceById = (id) => (ctx.catalog?.sources || []).find((s) => s.id === id) || null;

/** Start a backend job and follow it in the status bar. Resolves with its result. */
export async function runJob(startPromise, label, { stoppable = false } = {}) {
  const { job } = await startPromise;
  // a job that can stop gets Cancel in the status bar (the app stops it at the next file)
  const done = addJob(label, stoppable ? () => api("job_cancel", { id: job }).catch(() => {}) : undefined);
  try {
    return await ctx.platform.library.waitJob(job, (stage) => done.update(`${label} · ${stage}`));
  } finally {
    done();
  }
}

const KIND_TEXT = { github: "GitHub", zip: "ZIP file", folder: "Folder (copied)", linked: "Linked folder", local: "Your project (local/)", bundled: "From an earlier app", pinned: "Pinned components" };
export const kindText = (s) => KIND_TEXT[s?.kind] || s?.kind || "";

// ---------------------------------------------------------------- adding a project

/** Add a project (source_add's arguments), read it, say what it brought, open its page and make its thumbnails. */
export async function addProject(args, label) {
  try {
    const res = await runJob(api("source_add", args), `Adding ${label}`);
    await ctx.reloadCatalog();
    const n = res.read?.models ?? 0, parts = res.read?.parts ?? 0;
    const problems = (res.read?.problems || []).length;
    const src = sourceById(res.source), name = src?.name || label;
    if (res.version) {
      // another download of a model page a project has: a version of that project, offered as an update (F-23),
      // or only listed under Versions when it's older than the one in use (Review 6, F-26)
      const v = (src?.versions || []).find((x) => x.id === res.version);
      const date = res.older ? v?.date : src?.update?.latest?.date;
      ctx.toast(res.older
        ? `Added it to ${name} as an earlier version${date ? `, of ${when(date)}` : ""}: it's under Versions on its page.`
        : `Added it to ${name} as a new version${date ? `, of ${when(date)}` : ""}. Its page has Use the new version.`);
      location.hash = scopeHash(`source:${res.source}`);
      return res;
    }
    // a folder of examples is asked about on the project's page before they're listed (F-21)
    const ex = src?.examples?.folder && !src.examples.as ? src.examples : null;
    const listed = ex ? Math.max(0, n - ex.count) : n;
    ctx.toast(args.role === "library" || src?.role === "library" ? `Added ${name}: a library, its modules are in Components.`
      : `Added ${name}: ${[listed || !parts ? plural(listed, "model") : "", parts ? plural(parts, "part") : ""].filter(Boolean).join(", ")}${problems ? `; ${plural(problems, "problem")} to look at` : ""}.`
        + (ex ? ` Choose what its ${ex.folder}/ folder's ${ex.count} examples should be, on its page.` : "")
        + dupText(res.duplicates));
    location.hash = scopeHash(`source:${res.source}`);
    makeThumbnails();
    return res;
  } catch (err) {
    const msg = String(err.message || err);
    // the same project is in the library already: offer to open that one
    const same = msg.match(/already in the library.*\(([^()]+)\)\.$/)?.[1];
    ctx.toast(`Couldn't add it: ${msg}`, same && sourceById(same) ? { label: "Open it", run: () => { location.hash = scopeHash(`source:${same}`); } } : undefined);
    return null;
  }
}

/** " 3 of its files are in Gridfinity Rebuilt already." (milestone 5: they're added anyway) */
function dupText(d) {
  if (!d?.length) return "";
  const where = [...new Set(d.map((x) => x.project))];
  return ` ${d.length === 1 ? `${d[0].file.split("/").pop()} is` : `${d.length} of its files are`} in ${listText(where)} already.`;
}

export function AddProject() {
  const [tab, setTab] = useState("github");
  const [url, setUrl] = useState("");
  const [path, setPath] = useState("");
  const [link, setLink] = useState(false);
  const [asLibrary, setAsLibrary] = useState(false);
  // a link to a tag, release or commit: follow newer releases, or every change on the branch (F-11)
  const ref = tab === "github" ? (url.match(/github\.com\/[^/]+\/[^/]+\/(?:(?:tree|blob|commit)|releases\/tag)\/([^/?#]+)/) || [])[1] : null;
  const versionLike = !!ref && /^v?\d+(\.\d+)+/i.test(ref);
  const [followPick, setFollow] = useState(null);
  const follow = followPick || (versionLike || /\/releases\/tag\//.test(url) ? "releases" : "branch");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const close = () => ui.set({ dialog: null });
  const pick = async () => {
    const lib = ctx.platform.library;
    const p = tab === "zip" ? await lib.pickFile("Choose a ZIP file or a .scad file", ["zip", "scad"]) : await lib.pickFolder("Choose a project folder");
    if (p) setPath(p);
  };
  const submit = async (e) => {
    e.preventDefault();
    setError("");
    // the field itself, not state: a link pasted and submitted at once may not have re-rendered yet
    const url = e.currentTarget.querySelector("#add-url")?.value ?? "";
    // one .scad file is a project of its own, copied in as dropped files are (2026-10-16)
    const scad = tab === "zip" && /\.scad$/i.test(path);
    const args = tab === "github" ? { kind: "github", url, ...(ref ? { follow } : {}) }
      : scad ? { kind: "files", paths: [path], name: path.split(/[\\/]/).pop().replace(/\.scad$/i, "") }
      : tab === "zip" ? { kind: "zip", path } : { kind: "folder", path, link };
    if (asLibrary) args.role = "library";
    if (tab === "github" ? !url.trim() : !path) { setError(tab === "github" ? "Paste a GitHub link first." : "Choose a file or folder first."); return; }
    setBusy(true);
    const label = tab === "github" ? url.replace(/^https?:\/\/(www\.)?github\.com\//, "") : path.split(/[\\/]/).filter(Boolean).pop();
    close();
    await addProject(args, label);
    setBusy(false);
  };
  const tabBtn = (id, label) => html`<button type="button" role="tab" aria-selected=${tab === id ? "true" : "false"} class=${`seg-btn${tab === id ? " on" : ""}`}
    onClick=${() => { setTab(id); setPath(""); setError(""); }} data-add-tab=${id}>${label}</button>`;
  return html`<form class="dialog add-project" onSubmit=${submit} aria-label="Add a project">
    <div class="dialog-head"><h2>Add a project</h2><button type="button" class="ghost" aria-label="Close" onClick=${close}>${Icon.close(16)}</button></div>
    <div class="seg tabs-seg" role="tablist">${tabBtn("github", "GitHub link")}${tabBtn("zip", "File")}${tabBtn("folder", "Folder")}</div>
    ${tab === "github" ? html`<label class="field-block"><span>Repository link</span>
        <input type="url" id="add-url" value=${url} onInput=${(e) => setUrl(e.target.value)} placeholder="https://github.com/owner/repository" autofocus />
        <small class="muted">A branch, tag, release or commit (/tree/…, /releases/tag/…, /commit/…) and a subfolder work too. The app downloads that version as plain files (no git needed) and checks for updates later.</small></label>
      ${ref ? html`<fieldset class="field-block add-follow" id="add-follow"><legend>Updates</legend>
        <label class="check"><input type="radio" name="follow" value="releases" checked=${follow === "releases"} onChange=${() => setFollow("releases")} />
          Newer releases: stay on ${ref.length > 20 ? "this version" : ref} until a newer release comes out</label>
        <label class="check"><input type="radio" name="follow" value="branch" checked=${follow === "branch"} onChange=${() => setFollow("branch")} />
          Every change to the project, as its branch moves on</label></fieldset>` : null}`
      : html`<div class="field-block"><span>${tab === "zip" ? "A ZIP file, or one .scad file" : "Folder"}</span>
        <div class="pick-row"><code class="pick-path" id="add-path" title=${path || null}>${path || "Nothing chosen"}</code><button type="button" class="ghost" onClick=${pick} id="add-pick">Choose…</button></div>
        ${tab === "folder" ? html`<label class="check"><input type="checkbox" checked=${link} onChange=${(e) => setLink(e.target.checked)} id="add-link" />
          Link instead of copying (for a project you're still editing; it won't move with the library)</label>` : null}</div>`}
    <label class="check"><input type="checkbox" checked=${asLibrary} onChange=${(e) => setAsLibrary(e.target.checked)} id="add-library" />
      It's a library other projects include (like BOSL2): don't list its files as models</label>
    ${error ? html`<p class="form-error" role="alert">${error}</p>` : null}
    <div class="dialog-actions"><button type="button" class="ghost" onClick=${close}>Cancel</button>
      <button type="submit" class="primary" disabled=${busy} id="add-go">Add</button></div>
  </form>`;
}

// ---------------------------------------------------------------- a project's page
export function when(iso) {
  if (!iso) return "";
  const d = new Date(iso);
  return isNaN(d) ? iso : d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

/**
 * A made part's recipe (milestone 4), for its page and the inspector: "Made from Basic Bin (Gridfinity
 * Rebuilt), 2 settings changed.", whether that model is still here, and a note when its project has
 * been updated since the part was made.
 */
export function madeFrom(r) {
  if (!r?.model) return null;
  const m = ctx.index?.get(`${r.model.startsWith("@") ? "comp" : "gen"}:${r.model}`);
  const n = Object.keys(r.values || {}).length;
  const what = m ? `${m.name} (${m.project})` : r.model_name || r.model;
  const text = `Made from ${what}, ${n ? `${plural(n, "setting")} changed` : "with its default settings"}${r.edits ? " and your code edits" : ""}.`;
  const src = m?.sourceId ? sourceById(m.sourceId) : null;
  let outdated = null;
  // changed since: the files the model uses (F-20), so an edit elsewhere in your own project doesn't count;
  // recipes from before 0.4.13 (and components) compare the project's version
  const changed = r.files_id && m?.filesId ? r.files_id !== m.filesId : !!(src && r.project_version && src.version && src.version !== r.project_version);
  if (src && changed) {
    const then = (src.versions || []).find((v) => v.id === r.project_version);
    outdated = src.kind === "local" || src.kind === "linked" ? `The files ${m.name} uses have changed since this was made. Make again to use them.`
      : `Made with ${then?.date ? `the version of ${when(then.date)}` : "an earlier version"}; ${src.name || m.project} has been updated since. Make again to use it.`;
  }
  return { text, available: !!m, outdated, href: `#/m/${r.model}` };
}

function UpdateBanner({ src }) {
  const u = src.update || {};
  const [busy, setBusy] = useState(false);
  if (u.state !== "available") return null;
  const ch = u.changes || {};
  const latest = u.latest || {};
  const act = async (cmd, label) => {
    setBusy(true);
    try {
      await api(cmd, { id: src.id });
      await ctx.reloadCatalog();
      ctx.toast(label);
      // the new version's models are rendered at their defaults again, with new thumbnails
      if (cmd === "update_apply") makeThumbnails();
    } catch (e) { ctx.toast(String(e.message || e)); } finally { setBusy(false); }
  };
  return html`<div class="update-banner" role="region" aria-label="Update">
    <p><b>Update ready</b>${latest.date ? `: the version of ${when(latest.date)}` : ""}${latest.message ? ` (“${latest.message.split("\n")[0].replace(/[.\s]+$/, "")}”)` : ""}</p>
    ${latest.commits?.length ? html`<details><summary>${plural(latest.commits.length, "change")} upstream</summary>
      <ul class="commit-list">${latest.commits.map((c) => html`<li><code>${c.sha}</code> ${c.message} <span class="muted">${c.author || ""} ${when(c.date)}</span></li>`)}</ul></details>` : null}
    <ul class="change-list">
      ${ch.added?.length ? html`<li>New: ${ch.added.join(", ")}</li>` : null}
      ${ch.removed?.length ? html`<li>Removed: ${ch.removed.join(", ")}</li>` : null}
      ${(ch.settings || []).map((m) => html`<li>${m.model}: ${[
        m.added?.length ? `new settings ${m.added.join(", ")}` : "",
        m.removed?.length ? `removed ${m.removed.join(", ")}` : "",
        m.defaults?.length ? `new defaults for ${m.defaults.map((d) => d.name).join(", ")}` : ""].filter(Boolean).join("; ")}</li>`)}
      ${ch.parts_added?.length ? html`<li>New parts: ${ch.parts_added.join(", ")}</li>` : null}
      ${ch.parts_removed?.length ? html`<li>Parts removed: ${ch.parts_removed.join(", ")}</li>` : null}
      ${ch.parts_changed?.length ? html`<li>Changed parts: ${ch.parts_changed.join(", ")}</li>` : null}
      ${!ch.added?.length && !ch.removed?.length && !(ch.settings || []).length && !ch.parts_added?.length && !ch.parts_removed?.length && !ch.parts_changed?.length
        ? html`<li>No changes to models, parts or settings found.</li>` : null}
    </ul>
    <p class="muted">The current version stays until you accept, and your edits are kept either way.</p>
    <div class="button-row"><button type="button" class="primary" disabled=${busy} onClick=${() => act("update_apply", "Updated.")} data-update="apply">Use the new version</button>
      <button type="button" class="ghost" disabled=${busy} onClick=${() => act("update_skip", "Skipped this update.")} data-update="skip">Skip it</button></div>
  </div>`;
}

/**
 * A folder of examples (Review 3, F-15): many files that each only call the project's main file
 * (LEGO.scad's examples/, a piece per file). Asked once on the project's page: list them as models
 * (as before), make them parts (each made once at its own values, in the Parts Library), or leave
 * them out. The answer is kept in the project's metadata.json, so it holds across updates.
 */
const EXAMPLES_AS = { models: "listed as models", parts: "made into parts", leave: "left out" };
function ExamplesChoice({ src }) {
  const ex = src.examples;
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  if (!ex?.folder || src.role === "library") return null;
  const choose = async (as) => {
    setBusy(true);
    try {
      await runJob(api("examples_set", { source: src.id, as }), `Reading ${src.name} again`);
      await ctx.reloadCatalog();
      setOpen(false);
      ctx.toast(as === "parts" ? `The ${ex.count} examples are being made into parts, in the background.` : `The examples are ${EXAMPLES_AS[as]}.`);
      makeThumbnails();
    } catch (e) { ctx.toast(String(e.message || e)); } finally { setBusy(false); }
  };
  const folder = `${ex.folder}/`;
  if (ex.as && !open) {
    return html`<p class="info-note" id="examples-choice">Its ${folder} folder (${plural(ex.count, "file")}): ${EXAMPLES_AS[ex.as] || ex.as}.
      ${readOnly() ? null : html` <button type="button" class="link-btn" onClick=${() => setOpen(true)}>Change</button>`}</p>`;
  }
  if (readOnly()) return null;
  const btn = (as, label, cls = "ghost") => html`<button type="button" class=${cls} disabled=${busy} data-examples=${as} onClick=${() => choose(as)}>${label}</button>`;
  return html`<div class="update-banner" role="region" aria-label="Examples" id="examples-choice">
    <p><b>${folder} has ${ex.count} examples</b>: each file only calls ${ex.main.split("/").pop()} to make one thing, so each is a part more than a model to set up.</p>
    <p class="muted">Parts are made once, in the background, and listed in the Parts Library; each can still be opened as a model to change its settings.</p>
    <div class="button-row">${btn("parts", "Make them parts", "primary")}${btn("models", "List them as models")}${btn("leave", "Leave them out")}
      ${ex.as ? html`<button type="button" class="ghost" onClick=${() => setOpen(false)}>Cancel</button>` : null}</div>
  </div>`;
}

/** Parts that look like one ("latch_v2.stl" beside "latch.3mf", milestone 5): made one on request. */
function NearMisses({ src }) {
  const [busy, setBusy] = useState(false);
  const near = src.near_misses || [];
  if (!near.length || readOnly()) return null;
  const merge = async (ids) => {
    setBusy(true);
    try {
      await api("parts_merge", { source: src.id, ids });
      await ctx.reloadCatalog();
      ctx.toast("Made one part. Its page can split it again.");
    } catch (e) { ctx.toast(String(e.message || e)); } finally { setBusy(false); }
  };
  // they really are different parts: not offered again (UI-35)
  const apart = async (ids) => {
    setBusy(true);
    try {
      await api("parts_merge", { source: src.id, ids, apart: true });
      await ctx.reloadCatalog();
      ctx.toast("Kept apart. They won't be offered as one part again.");
    } catch (e) { ctx.toast(String(e.message || e)); } finally { setBusy(false); }
  };
  return html`<div class="info-note near-misses" id="near-misses">
    <p><b>${near.length === 1 ? "These look like one part" : `${near.length} groups of files look like one part each`}</b>: their names differ only by a version or copy mark, and they have no format in common.</p>
    <ul>${near.map((g) => html`<li>${listText(g.names)} <button type="button" class="link-btn" disabled=${busy} data-merge=${g.ids.join(",")} onClick=${() => merge(g.ids)}>Make one part</button>
      <button type="button" class="link-btn" disabled=${busy} data-apart=${g.ids.join(",")} onClick=${() => apart(g.ids)}>Keep apart</button></li>`)}</ul>
  </div>`;
}

/** Its files that are in another project too (UI-34): found when it's added, listed for as long as they are. */
function Duplicates({ src }) {
  const v = useStore(ui, (s) => s.catalogVersion);
  const [found, setFound] = useState([]);
  useEffect(() => {
    let live = true;
    setFound([]);
    if (src.kind !== "bundled" && src.kind !== "pinned") api("source_duplicates", { id: src.id }).then((d) => live && setFound(d || []), () => {});
    return () => { live = false; };
  }, [src.id, v]);
  const rows = found.map((d) => ({ ...d, it: ctx.index?.get(`part:${src.id}/${d.item}`) })).filter((d) => d.it && !d.it.hidden);
  if (!rows.length || readOnly()) return null;
  return html`<div class="info-note duplicates" id="duplicates">
    <p><b>${rows.length === 1 ? "A file here is" : `${rows.length} files here are`} in another project too</b>: the same file, byte for byte.</p>
    <ul>${rows.map((d) => html`<li>${d.file.split("/").pop()}: also in ${d.project}
      <button type="button" class="link-btn" data-dup-open=${d.source} onClick=${() => { location.hash = scopeHash(`source:${d.source}`); }}>Open the other</button>
      <button type="button" class="link-btn" data-dup-hide=${d.item} onClick=${() => hideItems([d.it])}>Hide this copy</button></li>`)}</ul>
  </div>`;
}

export function SourcePanel({ id }) {
  const v = useStore(ui, (s) => s.catalogVersion);
  // the parts you've made from models (local/Made parts/): no source of its own to show, read or keep versions of (UI-28)
  const made = id === "local-made-parts";
  const tab = useStore(ui, (s) => (made && s.sourceTab === "versions" ? "items" : s.sourceTab || "items"));
  const setTab = (t) => ui.set({ sourceTab: t });
  const [info, setInfo] = useState(null);
  const [busy, setBusy] = useState("");
  const src = sourceById(id);
  useEffect(() => {
    setInfo(null);
    if (src && tab !== "items") api("source_get", { id }).then(setInfo, (e) => setInfo({ error: String(e) }));
  }, [id, tab, v]);
  if (!src) return html`<p class="empty">This project isn't in the library.</p>`;
  const act = async (label, fn) => {
    setBusy(label);
    try { await fn(); } catch (e) { ctx.toast(String(e.message || e)); } finally { setBusy(""); }
  };
  const ro = readOnly();
  const origin = src.origin || {};
  const url = origin.url || src.meta?.origin;
  const problems = src.problems || [];
  const tabs = [["items", "Items"], ["about", "About"], ["notes", "Notes"], ["files", "Files"], ...(made ? [] : [["versions", "Versions"]])];
  // one main action beside the project's name (Check for updates, or Read again); the rest under ⋯ (Review 1, UI-3)
  const reread = html`<button type="button" class=${src.kind === "github" ? "" : "ghost"} disabled=${!!busy || ro} data-act="reread" onClick=${() => act("read", async () => {
    await runJob(api("source_read", { id }), `Reading ${src.name}`);
    await ctx.reloadCatalog();
    makeThumbnails();
  })}>${busy === "read" ? "Reading…" : "Read again"}</button>`;
  return html`<section class="source-panel" data-source=${id}>
    <div class="source-head">
      ${src.icon ? html`<img class="source-icon" src=${src.icon} alt="" />` : null}
      <div>
        <p class="source-kind">${kindText(src)}${origin.site?.name === "printables" ? " from Printables" : ""}${src.role === "library" ? " · library" : ""}${src.version_date && !made ? ` · version of ${when(src.version_date)}` : ""}</p>
        ${made ? html`<p class="source-sub">Parts you've made from models. Each keeps the license of the project it was made from.</p>` : html`<p class="source-sub">${[
          url ? html`<a href=${url} target="_blank" rel="noopener">${url.replace(/^https?:\/\//, "")}</a>`
            : origin.path || origin.file ? html`<span title=${origin.path || origin.file}>${(origin.path || origin.file).split(/[\\/]/).filter(Boolean).slice(-2).join("/")}</span>` : null,
          src.meta?.license?.spdx ? (src.meta.license.spdx === "NOASSERTION" ? "License not stated" : src.meta.license.spdx) : null,
          // what its updates follow (F-11)
          src.kind === "github" && origin.track ? html`<span data-follows=${origin.track}>${origin.track === "releases" ? "updates: new releases" : `updates: changes on its ${String(origin.track).replace(/^branch:/, "") || "default"} branch`}</span>` : null,
        ].filter(Boolean).flatMap((x, i) => (i ? [" · ", x] : [x]))}</p>`}
      </div>
      <div class="source-actions">
        ${src.kind === "github" ? html`<button type="button" class="ghost" disabled=${!!busy || ro} data-act="check" onClick=${() => act("check", async () => {
          const r = await runJob(api("updates_check", { id }), `Checking ${src.name}`);
          await ctx.reloadCatalog();
          ctx.toast(r.updates?.length ? "An update is ready to review." : "No newer version.");
        })}>${busy === "check" ? "Checking…" : "Check for updates"}</button>`
        : src.kind !== "bundled" && !made ? reread : null}
        <details class="menu" id="source-more"><summary class="ghost" aria-label="More" title="More">⋯</summary><div class="menu-panel" onClick=${(e) => {
          if (e.target.closest("button")) e.currentTarget.closest("details").open = false;
        }}>
          <button type="button" disabled=${ro} onClick=${() => ui.set({ dialog: { type: "edit", level: "project", source: id } })} data-act="edit">Edit details…</button>
          ${src.kind === "github" ? reread : null}
          <button type="button" disabled=${ro} onClick=${() => hideProject(id, !src.hidden)} data-act="hide-project"
            title=${src.hidden ? "List it again" : "Keep it and everything in it out of browsing and search"}>${src.hidden ? "Unhide" : "Hide"}</button>
          <button type="button" disabled=${ro} data-act="picture" onClick=${() => ui.set({ dialog: { type: "picture", source: id, level: "project", name: src.name, current: src.meta?.icon || null } })}
            title="Show another picture for the project: one of its own, an item's, or your own">Choose picture…</button>
          <button type="button" disabled=${ro} data-act="content" onClick=${() => ui.set({ dialog: { type: "content", source: id, level: "project", name: src.name } })}
            title="Add a file or a link to the project: a build video, an assembly guide, photos">Add content…</button>
          <button type="button" data-act="reveal" title="Open the project's files in the system's file manager" onClick=${() => api("source_get", { id }).then((i) => i.folder && ctx.platform.library.openPath(i.folder))}>Show in folder</button>
          ${src.kind === "local" || src.kind === "linked" ? html`<button type="button" disabled=${ro} data-act="save-project-file"
            title="Write this project's description (its models, names and forms, with your edits) into its folder as workshop.json, so it goes with the files"
            onClick=${() => act("save-file", async () => {
              const r = await api("workshop_save", { source: id });
              ctx.toast(`Saved workshop.json in the project's folder (${plural(r.models, "model")}).`);
            })}>Save as project file</button>` : null}
          ${src.models ? html`<button type="button" disabled=${ro} data-act="check-defaults" onClick=${() => makeThumbnails({ source: id })}
            title="Render every model at its default settings again and note which don't render">Check defaults</button>` : null}
          ${src.kind !== "bundled" && src.kind !== "local" ? html`<button type="button" disabled=${ro} onClick=${() => act("role", async () => {
            await runJob(api("source_role", { id, role: src.role === "library" ? "project" : "library" }), `Reading ${src.name}`);
            await ctx.reloadCatalog();
          })}>${src.role === "library" ? "Treat as a project (list its models)" : "Treat as a library (others include it)"}</button>` : null}
          <hr />
          <button type="button" class="danger-text" disabled=${ro} data-act="remove" onClick=${() => act("remove", async () => {
            if (await deleteProject(id)) location.hash = "#/";
          })}>Delete project…</button>
        </div></details>
      </div>
    </div>
    <${UpdateBanner} src=${src} />
    <${ExamplesChoice} src=${src} />
    <${NearMisses} src=${src} />
    <${Duplicates} src=${src} />
    ${src.kind === "bundled" && src.update?.state === "upstream" ? html`<p class="lic-summary" id="upstream-newer">A newer version is on GitHub, from ${when(src.update.latest?.date)}${src.update.latest?.ahead_by ? ` (${plural(src.update.latest.ahead_by, "change")} since the app's copy)` : ""}.
      ${origin.not_updatable ? html` This copy can't be updated in place because ${origin.not_updatable}: <a href=${origin.repository} target="_blank" rel="noopener">add it from GitHub</a> to follow it.`
        : html` <a href="#/library-settings/projects">Get it from GitHub</a> (Library settings) to follow it.`}</p>` : null}
    ${src.update?.error ? html`<p class="lic-summary" title=${src.update.error}>Couldn't check for updates${src.update.error_date ? ` on ${when(src.update.error_date)}` : ""}: ${updateErrorText(src.update.error)}</p>` : null}
    ${problems.length ? html`<details class="problems"><summary>${plural(problems.length, "problem")} with this project</summary>
      <ul>${problems.map((p) => html`<li>${p.message}</li>`)}</ul></details>` : null}
    <div class="seg tabs-seg source-tabs" role="tablist">${tabs.map(([t, l]) => html`<button type="button" role="tab" class=${`seg-btn${tab === t ? " on" : ""}`}
      aria-selected=${tab === t ? "true" : "false"} onClick=${() => setTab(t)} data-source-tab=${t}>${l}</button>`)}</div>
    ${tab === "about" ? html`<div class="readme">
      ${src.meta?.summary ? html`<p>${src.meta.summary}</p>` : null}
      ${origin.site ? html`<${SiteDetails} src=${src} info=${info} />` : null}
      <p class="muted">The project's README and other documents open beside the page (the tabs on the right).</p>
      <div class="button-row source-docs"><${SideButtons} refObj=${{ source: id }} small=${false} /></div>
      ${info?.license_text ? html`<details><summary>License text</summary><pre class="license-text">${info.license_text}</pre></details>` : null}</div>` : null}
    ${tab === "notes" ? html`<${ProjectNotes} src=${src} ro=${ro} />` : null}
    ${tab === "files" ? html`<div class="file-list">${!info ? html`<p class="muted">Loading…</p>` : html`
      <p class="muted">${plural(info.files.length, "file")}${info.files.length >= 2000 ? " (first 2000)" : ""} in <code>${info.folder}</code>. OpenSCAD files open in the Code tab on the right.</p>
      <ul>${info.files.map(([p, n]) => html`<li>${/\.scad$/i.test(p) && info.folder_rel != null
        ? html`<button type="button" class="link-btn" data-code-file=${p} onClick=${() => openSide("code", { sideFile: { rel: `${info.folder_rel}/${p}`, label: p, ref: { source: id } } })}>${p}</button>`
        : html`<span>${p}</span>`}<span class="muted">${bytes(n)}</span></li>`)}</ul>`}</div>` : null}
    ${tab === "versions" ? html`<div class="versions">${!info ? html`<p class="muted">Loading…</p>` : html`
      <ul>${(info.source.versions || []).slice().reverse().map((ver) => html`<li class=${ver.id === info.source.version ? "current" : ""}>
        <code>${ver.id}</code> ${when(ver.date)} ${ver.message ? html`<span class="muted">${ver.message}</span>` : null}
        ${ver.id === info.source.version ? html`<b> (in use)</b>` : null}</li>`)}</ul>
      ${(info.source.versions || []).length > 1 ? html`<button type="button" class="ghost" disabled=${ro} onClick=${() => act("clean", async () => {
        const r = await api("source_clean", { id });
        ctx.toast(r.removed.length ? `Removed ${plural(r.removed.length, "old version")}.` : "Nothing to remove.");
        setTab("versions"); await ctx.reloadCatalog();
      })}>Remove versions not in use</button>` : null}`}</div>` : null}
  </section>`;
}

/** Your own notes on a project (0.4.23): free text in its metadata.json (my_notes), saved when you click away. */
function ProjectNotes({ src, ro }) {
  const saved = src.meta?.my_notes || "";
  const [text, setText] = useState(saved);
  // what's typed is also saved when the tab closes before the box lost focus, and isn't replaced when the
  // library is read again meanwhile
  const last = useRef({ text: saved, shown: saved });
  useEffect(() => {
    if (last.current.text === last.current.shown) { last.current.text = saved; setText(saved); }
    last.current.shown = saved;
  }, [src.id, saved]);
  last.current = { ...last.current, id: src.id, saved };
  const save = async (t = text) => {
    if (t === last.current.saved || t === last.current.sent) return;
    last.current.sent = last.current.shown = t;
    try {
      await api("meta_set", { source: last.current.id, level: "project", patch: { my_notes: t.trim() ? t : null } });
      await ctx.reloadCatalog();
      ctx.toast(t.trim() ? "Notes saved." : "Notes removed.");
    } catch (e) { ctx.toast(String(e.message || e)); }
  };
  useEffect(() => () => save(last.current.text), []);
  return html`<div class="own-notes" id="project-notes">
    <p class="muted">Your notes on this project: what you printed, settings that worked, what to change next time. They're kept with the project in the library, and search finds its models by them. Saved when you click away.</p>
    <textarea id="project-notes-text" rows="12" aria-label="Your notes" value=${text} disabled=${ro}
      onInput=${(e) => { last.current.text = e.target.value; setText(e.target.value); }} onBlur=${(e) => save(e.target.value)}></textarea>
  </div>`;
}

/** What a model site's page said (a Printables download's PDF, 0.4.16): who made it, when, and its description. */
function SiteDetails({ src, info }) {
  const f = src.meta?.fields || {};
  const by = (src.meta?.authors || []).map((a) => a.url ? html`<a href=${a.url} target="_blank" rel="noopener">${a.name}</a>` : a.name);
  const facts = [
    by.length ? html`<span>by ${by.flatMap((x, i) => (i ? [", ", x] : [x]))}</span>` : null,
    f.Published ? `published ${when(f.Published)}` : null,
    f.Updated && f.Updated !== f.Published ? `updated ${when(f.Updated)}` : null,
    f["Printables category"] ? `in ${f["Printables category"]}` : null,
  ].filter(Boolean);
  return html`<div class="site-details" id="site-details">
    <p class="muted">From the model's page on Printables${facts.length ? ": " : "."}${facts.flatMap((x, i) => (i ? [" · ", x] : [x]))}</p>
    ${info?.site_description ? html`<div class="site-description" dangerouslySetInnerHTML=${{ __html: info.site_description }}></div>` : null}
  </div>`;
}

// ---------------------------------------------------------------- project problems (Library settings › Projects)
export const ATTN_TEXT = { update: "Update ready", pin_library: "Its library changed", missing: "Missing files", settings: "Settings couldn't be read", unread: "Not read", reread: "Read with an older version",
  "missing-folder": "Folder missing", license: "License", libraries: "A library it uses changed", scan: "Note", broken: "Flagged as broken",
  render: "Doesn't render at its defaults", limits: "Defaults outside their limits" };

// ---------------------------------------------------------------- thumbnails for new models
let thumbRunning = false;
let thumbAgain = false;
let thumbViewer = null;

function thumbViewerFor() {
  if (thumbViewer) return thumbViewer;
  const wrap = document.createElement("div");
  wrap.style.cssText = "position:fixed;left:-10000px;top:0;width:480px;height:360px;pointer-events:none";
  const canvas = document.createElement("canvas");
  canvas.style.cssText = "width:100%;height:100%;display:block";
  wrap.append(canvas);
  document.body.append(wrap);
  thumbViewer = new Viewer(canvas);
  thumbViewer.renderer.setPixelRatio(1);
  thumbViewer.setEdges(false);
  thumbViewer.setGrid(false);
  return thumbViewer;
}

/** Draw an STL as a thumbnail (same camera and framing as tools/thumbnails.py). */
async function drawThumb(url) {
  const v = thumbViewerFor();
  v.setColor(ctx.platform.store.prefs.get("gw-filament") || "#f2b705");
  await v.load(url);
  v.view("iso");
  const size = v.bbox.getSize(v.camera.position.clone());
  const r = Math.max(size.length() / 2, 1);
  const dist = (r / Math.sin((v.camera.fov / 2) * Math.PI / 180)) * 0.95;
  const dir = v.camera.position.clone().sub(v.controls.target).normalize();
  v.camera.position.copy(v.controls.target).addScaledVector(dir, dist);
  v.camera.near = dist / 200; v.camera.far = dist * 50; v.camera.updateProjectionMatrix();
  v.controls.update();
  v.resize();
  v.renderer.render(v.scene, v.camera);
  return v.renderer.domElement.toDataURL("image/webp", 0.86);
}

/** A 2D model's drawing (an SVG) on white, fitted into a thumbnail's 480×360, as WebP (0.4.23). */
async function drawingThumb(url) {
  const img = new Image();
  img.src = url;
  await img.decode();
  const c = document.createElement("canvas");
  c.width = 480; c.height = 360;
  const g = c.getContext("2d");
  g.fillStyle = "#fff";
  g.fillRect(0, 0, c.width, c.height);
  const k = Math.min((c.width - 40) / (img.naturalWidth || 1), (c.height - 40) / (img.naturalHeight || 1));
  const w = img.naturalWidth * k, h = img.naturalHeight * k;
  g.drawImage(img, (c.width - w) / 2, (c.height - h) / 2, w, h);
  return c.toDataURL("image/webp", 0.86);
}

/** A picture (PNG) fitted into a thumbnail's 480×360, as WebP. */
async function pictureThumb(blob) {
  const img = await createImageBitmap(blob);
  const c = document.createElement("canvas");
  c.width = 480; c.height = 360;
  const k = Math.min(c.width / img.width, c.height / img.height);
  const w = img.width * k, h = img.height * k;
  c.getContext("2d").drawImage(img, (c.width - w) / 2, (c.height - h) / 2, w, h);
  img.close?.();
  return c.toDataURL("image/webp", 0.86);
}

// one thumbnail at a time: they share one hidden viewer
let thumbLock = Promise.resolve();
function drawThumbLocked(url) {
  const p = thumbLock.then(() => drawThumb(url));
  thumbLock = p.catch(() => {});
  return p;
}


/** Keep a component's render as its thumbnail (thumbs/components/ in the library), shown at once. */
export async function saveComponentThumb(key, blob) {
  const it = ctx.index?.get(`comp:${key}`);
  if (readOnly() || !it || it.thumb) return;
  const url = URL.createObjectURL(blob);
  try {
    const data = await drawThumbLocked(url);
    const rel = await api("component_thumb", { key, data });
    it.thumb = ctx.platform.library.url(rel);
    ui.set((s) => ({ catalogVersion: s.catalogVersion + 1 }));
  } catch (e) {
    console.warn("component thumbnail", key, e);
  } finally { URL.revokeObjectURL(url); }
}

/** An update check's error in a few words; the whole text is on hover. */
function updateErrorText(e) {
  const s = String(e || "");
  if (/rate limit|403/i.test(s)) return "GitHub's limit for checks was reached. Add a GitHub token in Library settings, or try later.";
  if (/404|not found/i.test(s)) return "the repository wasn't found on GitHub (renamed, moved or made private?).";
  if (/tls|certificate|dns|connect|timed? ?out|network|resolve/i.test(s)) return "couldn't reach GitHub.";
  return s.length > 140 ? `${s.slice(0, 137)}…` : s;
}

/** OpenSCAD's message for a render that failed, cut to its first error line. */
function renderMessage(e) {
  const s = String(e?.message || e || "").trim();
  const line = s.split("\n").find((l) => /^ERROR|error/i.test(l.trim())) || s.split("\n")[0] || "OpenSCAD made nothing";
  return line.trim().replace(/^ERROR:\s*/, "").slice(0, 600);
}

/** Make thumbnails for library models and parts that have none, or whose project changed since
 * they were made (renders defaults), keeping
 * whether each model rendered at its defaults (sources/<id>/checks.json). A model that failed
 * isn't tried again until its project or the engine changes, or { source } asks for every model
 * of that project to be checked again (Check defaults). With { components: true }, makes the
 * components' thumbnails instead. */
/** Models with no check at their defaults for the version in use (projects from before 0.4.6, starter projects), F-13. */
export function uncheckedModels() {
  return ctx.index.items.filter((i) => i.kind === "generator" && i.sourceId && !i.check && !i.hidden && sourceById(i.sourceId)?.kind !== "pinned");
}

export async function makeThumbnails({ components = false, source = null, unchecked = false } = {}) {
  if (readOnly()) return;
  // asked while thumbnails are being made (a project added meanwhile): go round again after
  if (thumbRunning) { if (!components && !source && !unchecked) thumbAgain = true; return; }
  if (components) return componentThumbnails();
  if (!source && !unchecked && (ctx.catalog?.examples_todo || []).length) return makeExampleParts();
  const checking = !!(source || unchecked);
  const todo = unchecked ? uncheckedModels() : source
    ? ctx.index.items.filter((i) => i.kind === "generator" && i.sourceId === source)
    : ctx.index.items.filter((i) => (!i.thumb || i.check?.outdated) && (i.kind === "generator" || i.preview) && sourceById(i.sourceId || i.projectId)?.kind !== "bundled"
      && !(i.check?.ok === false && !i.check.stale));
  if (!todo.length) return;
  thumbRunning = true;
  const done = addJob(`${checking ? "Checking defaults" : "Making thumbnails"} · 0 of ${todo.length}`);
  let made = 0, failed = 0;
  try {
    for (const [n, it] of todo.entries()) {
      done.update(`${checking ? "Checking defaults" : "Making thumbnails"} · ${n + 1} of ${todo.length}`);
      if (!ctx.index.get(it.id)) continue; // deleted meanwhile
      try {
        let blob, flat = false;
        if (it.kind === "generator") {
          const { detail, values } = await ctx.loadModel(it.key);
          const t0 = performance.now();
          const check = (ok, message) => api("defaults_put", { source: it.sourceId, model: it.key.split("/")[1], ok, message, seconds: (performance.now() - t0) / 1000 }).catch(() => {});
          try {
            let r = await ctx.engine.render(detail, values, () => {}).promise.catch((e) => ({ error: e }));
            // its defaults don't render: it works if one of its first three presets does (0.4.24)
            for (const ps of r.error ? (detail.presets || []).slice(0, 3) : []) {
              const tried = await ctx.engine.render(detail, { ...values, ...ps.values }, () => {}).promise.catch(() => null);
              if (tried) { r = { ...tried, preset: ps.label }; break; }
            }
            if (r.error) throw r.error;
            blob = r.blob;
            flat = r.format === "svg"; // a 2D model: its drawing (0.4.23)
            await check(true, r.preset ? `Renders with its preset “${r.preset}”, not its defaults.` : undefined);
          } catch (e) {
            failed++;
            await check(false, renderMessage(e));
            throw e;
          }
          if (it.thumb && checking) { made++; continue; } // checking only: its thumbnail is kept
        } else {
          // a 3MF's own picture (the one its slicer saved), when it has one (milestone 5)
          const pic = /^mesh\/.*\.3mf$/i.test(it.preview) ? await ctx.platform.fetch(it.preview.replace(/^mesh\//, "picture/")) : null;
          const picBlob = pic?.ok ? await pic.blob() : null;
          if (picBlob?.size) {
            const data = await pictureThumb(picBlob);
            await api("thumb_put", { source: it.sourceId || it.projectId, name: `part-${it.key.split("/")[1]}`, data });
            made++;
            continue;
          }
          const r = await ctx.platform.fetch(it.preview);
          if (!r.ok) throw new Error(await r.text());
          blob = await r.blob();
        }
        const url = URL.createObjectURL(blob);
        let data;
        try { data = flat ? await drawingThumb(url) : await drawThumb(url); } finally { URL.revokeObjectURL(url); }
        const name = it.kind === "generator" ? it.key.split("/")[1] : `part-${it.key.split("/")[1]}`;
        await api("thumb_put", { source: it.sourceId || it.projectId, name, data });
        made++;
      } catch (e) {
        console.warn("thumbnail", it.id, e);
      }
    }
  } finally {
    done();
    thumbRunning = false;
  }
  if (made || failed) await ctx.reloadCatalog();
  if (thumbAgain) { thumbAgain = false; setTimeout(() => makeThumbnails(), 0); }
  if (source) ctx.toast(failed ? `${plural(failed, "model")} of ${todo.length} didn't render at ${failed === 1 ? "its" : "their"} defaults; see Library settings.` : `All ${plural(todo.length, "model")} render at their defaults.`);
  return { made, failed };
}

/** Examples made into parts (F-15): each rendered once at its defaults, kept as the project's part. Thumbnails follow. */
async function makeExampleParts() {
  const todo = ctx.catalog.examples_todo;
  thumbRunning = true;
  const done = addJob(`Making parts from examples · 0 of ${todo.length}`);
  let made = 0;
  try {
    for (const [n, ex] of todo.entries()) {
      done.update(`Making parts from examples · ${n + 1} of ${todo.length}`);
      try {
        const { detail, values } = await ctx.loadModel(ex.key);
        const r = await ctx.engine.render(detail, values, () => {}).promise;
        if (r.format === "svg") throw new Error("a 2D example: not a part");
        const blob = r.blob;
        const data = await new Promise((resolve, reject) => { const r = new FileReader(); r.onload = () => resolve(r.result); r.onerror = () => reject(r.error); r.readAsDataURL(blob); });
        await api("example_put", { source: ex.source, model: ex.model, data });
        made++;
      } catch (e) {
        console.warn("example part", ex.key, e);
        await api("defaults_put", { source: ex.source, model: ex.model, ok: false, message: renderMessage(e) }).catch(() => {});
      }
    }
  } finally {
    done();
    thumbRunning = false;
  }
  await ctx.reloadCatalog();
  // then their thumbnails (and anything else waiting); ones that didn't render aren't tried again until asked
  setTimeout(() => makeThumbnails(), 0);
  return { made };
}

async function componentThumbnails() {
  const todo = ctx.index.items.filter((i) => i.kind === "component" && !i.thumb && !i.needs?.length);
  if (!todo.length) return 0;
  thumbRunning = true;
  let cancelled = false;
  const done = addJob(`Making component previews · 0 of ${todo.length}`, () => { cancelled = true; });
  let made = 0;
  try {
    // a few at a time: the native engine runs several renders at once
    const queue = [...todo];
    const worker = async () => {
      while (queue.length && !cancelled) {
        const it = queue.shift();
        done.update(`Making component previews · ${todo.length - queue.length} of ${todo.length}`);
        try {
          const { detail, values } = await ctx.loadModel(it.key);
          const blob = (await ctx.engine.render(detail, values, () => {}).promise).blob;
          const url = URL.createObjectURL(blob);
          let data;
          try { data = await drawThumbLocked(url); } finally { URL.revokeObjectURL(url); }
          const rel = await api("component_thumb", { key: it.key, data });
          it.thumb = ctx.platform.library.url(rel);
          made++;
        } catch (e) { console.warn("component preview", it.key, e); }
      }
    };
    await Promise.all(Array.from({ length: Math.max(1, Math.min(4, ctx.engine.concurrency || 1)) }, worker));
  } finally {
    done();
    thumbRunning = false;
  }
  ui.set((s) => ({ catalogVersion: s.catalogVersion + 1 }));
  if (thumbAgain) { thumbAgain = false; setTimeout(() => makeThumbnails(), 0); }
  return made;
}

// ---------------------------------------------------------------- merge conflicts
export function MergeConflicts({ result }) {
  const [picks, setPicks] = useState(() => result.conflicts.map(() => "ours"));
  const close = () => ui.set({ dialog: null });
  const show = (v) => (v == null ? "(none)" : typeof v === "object" ? (v.spdx || v.label || JSON.stringify(v)) : String(v));
  const apply = async () => {
    const choices = result.conflicts.filter((_, i) => picks[i] === "theirs").map((c) => ({ ...c, value: c.theirs }));
    try { await api("merge_resolve", { choices }); await ctx.reloadCatalog(); ctx.toast("Merged."); close(); } catch (e) { ctx.toast(String(e)); }
  };
  return html`<div class="dialog merge-dialog" aria-label="Merge differences">
    <div class="dialog-head"><h2>Merged, with ${plural(result.conflicts.length, "difference")}</h2><button type="button" class="ghost" aria-label="Close" onClick=${close}>${Icon.close(16)}</button></div>
    <p>${plural(result.copied.length, "new project")}, ${plural(result.combined.length, "project")} in both libraries combined, ${plural(result.recipes, "saved setting")} copied. Where both libraries edited the same detail differently, pick which to keep:</p>
    <table class="table-view merge-table"><thead><tr><th>Where</th><th>Detail</th><th>This library</th><th>The other</th></tr></thead><tbody>
      ${result.conflicts.map((c, i) => html`<tr><td>${c.source ? sourceById(c.source)?.name || c.source : "Library"}${c.key ? ` › ${c.key}` : ""}</td><td>${c.field}</td>
        <td><label><input type="radio" name=${`m${i}`} checked=${picks[i] === "ours"} onChange=${() => setPicks(picks.map((p, j) => (j === i ? "ours" : p)))} /> ${show(c.ours)}</label></td>
        <td><label><input type="radio" name=${`m${i}`} checked=${picks[i] === "theirs"} onChange=${() => setPicks(picks.map((p, j) => (j === i ? "theirs" : p)))} /> ${show(c.theirs)}</label></td></tr>`)}
    </tbody></table>
    <div class="dialog-actions"><button type="button" class="primary" onClick=${apply}>Keep these</button></div>
  </div>`;
}

/**
 * Scan for changes: projects in the library's local/ folder and linked folders, and the stored
 * copies of added projects, read again where their files changed; projects whose files are gone
 * are listed in Library settings › Projects. Quiet (start-up, the window coming back into focus): a message
 * only when something changed.
 */
let scanning = null;
let lastScan = 0;
export async function rescanLocal(quiet = true) {
  if (readOnly()) return;
  if (scanning) return scanning;
  lastScan = Date.now();
  scanning = (async () => {
    try {
      const r = await runJob(api("source_rescan", {}), "Scanning for changes");
      const names = (ids) => ids.map((id) => sourceById(id)?.name || id);
      const parts = [];
      if (r.read?.length || r.added?.length) {
        await ctx.reloadCatalog();
        makeThumbnails();
      }
      const reread = (r.read || []).filter((id) => !(r.added || []).includes(id));
      if (r.added?.length) parts.push(`Added ${listText(names(r.added))}`);
      if (reread.length) parts.push(`Read ${listText(names(reread))} again`);
      if (r.missing?.length) parts.push(`${listText(names(r.missing))} ${r.missing.length === 1 ? "is" : "are"} missing (see Library settings › Projects)`);
      if (parts.length) ctx.toast(`${parts.join(". ")}.`);
      else if (!quiet) ctx.toast("No changes found.");
    } catch (e) {
      if (!quiet) ctx.toast(String(e.message || e));
    } finally {
      scanning = null;
    }
  })();
  return scanning;
}

/** "A", "A and B", "A, B and 3 more" */
export function listText(names) {
  if (names.length <= 2) return names.join(" and ");
  if (names.length <= 4) return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  return `${names.slice(0, 3).join(", ")} and ${names.length - 3} more`;
}

/** Scan again when the app's window comes back into focus (files may have been changed elsewhere), at most once a minute. */
export function scanOnFocus() {
  window.addEventListener("focus", () => {
    if (Date.now() - lastScan < 60_000 || ui.get().dialog) return;
    rescanLocal(true);
  });
}

/** Check GitHub projects for updates at most once a day (desktop). */
export function dailyUpdateCheck() {
  if (readOnly()) return;
  const prefs = ctx.platform.store.prefs;
  const last = prefs.get("gw-update-check", 0) || 0;
  if (Date.now() - last < 864e5) return;
  if (!(ctx.catalog?.sources || []).some((s) => s.kind === "github")) return;
  prefs.set("gw-update-check", Date.now());
  runJob(api("updates_check", {}), "Checking for updates").then(async (r) => {
    if (r.updates?.length) {
      await ctx.reloadCatalog();
      ctx.toast(`${plural(r.updates.length, "update")} ready to review.`, { label: "Show", run: () => { location.hash = "#/library-settings"; } });
    }
  }, () => {});
}

export { useMemo };
