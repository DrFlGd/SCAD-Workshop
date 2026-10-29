// Desktop app side of web/platform.js: native OpenSCAD and the library folder,
// reached through the app's commands (desktop/core/src/api.rs, called through
// Tauri's "api" / "api_bytes" commands in desktop/src-tauri/src/main.rs).

import { EngineClient } from "./engine-client.js";

const tauri = globalThis.__TAURI__;
const invoke = (cmd, args, opts) => tauri.core.invoke(cmd, args, opts);
/** One of the app's commands, answered as JSON. */
const api = (cmd, args = {}) => invoke("api", { cmd, args });
/** One of the app's commands, answered as bytes (ArrayBuffer). */
const apiBytes = (cmd, args = {}) => invoke("api_bytes", { cmd, args });

const MIME = { json: "application/json", stl: "model/stl", "3mf": "model/3mf", png: "image/png", svg: "image/svg+xml", dxf: "image/vnd.dxf" };
const newId = () => (crypto.randomUUID ? crypto.randomUUID() : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`);

const cancelledError = () => Object.assign(new Error("Render cancelled."), { cancelled: true });

/**
 * Native OpenSCAD, plus (on Windows) the same WebAssembly engine as the website.
 * Native OpenSCAD on Windows is slow with projects made of many `use`d files
 * (Gridfinity Extended: ~10 s vs ~1 s in WebAssembly), while it's much faster
 * for heavy geometry. So on Windows the first render of each model races both
 * engines, keeps the faster one's result and remembers the winner for that model.
 */
class DesktopEngine {
  constructor(info, catalog, prefs) {
    this.common = catalog.common_files || {};
    this.info = info;
    this.prefs = prefs;
    this.concurrency = info.concurrency || 1;
    this.label = info.engine ? `OpenSCAD ${info.engine}, native` : "OpenSCAD not available";
    this.n = 0;
    this.hybrid = info.os === "windows";
    this.wasm = null;
  }

  wasmEngine() {
    this.wasm ||= new EngineClient({
      commonFiles: this.common,
      readFile: async (sha) => new Uint8Array(await apiBytes("blob", { sha })),
    });
    return this.wasm;
  }

  picks() { return this.prefs.get("gw-engine-pick", {}) || {}; }

  remember(key, engine) {
    if (this.picks()[key] !== engine) this.prefs.set("gw-engine-pick", { ...this.picks(), [key]: engine });
  }

  render(model, values, onEvent = () => {}) {
    // (a Fusion 360 file goes to MitCad, through the native path; 0.4.27)
    if (!this.hybrid || model.browser === false || !this.info.engine || model.engine === "mitcad") return this.renderNative(model, values, onEvent);
    const pick = this.picks()[model.key];
    if (pick === "native") return this.renderNative(model, values, onEvent);
    if (pick === "wasm") return this.withFallback(this.wasmEngine().render(model, values, onEvent), model, values, onEvent);
    return this.race(model, values, onEvent);
  }

  /** WebAssembly first; if it fails (memory, crash), native. */
  withFallback(job, model, values, onEvent) {
    let current = job;
    const promise = job.promise.then((r) => ({ ...r, engine: "wasm" }), (e) => {
      if (e.cancelled) throw e;
      current = this.renderNative(model, values, onEvent);
      return current.promise;
    });
    return { promise, cancel: () => current.cancel() };
  }

  race(model, values, onEvent) {
    const jobs = { native: this.renderNative(model, values, onEvent), wasm: this.wasmEngine().render(model, values, () => {}) };
    let settled = false, rejectRace;
    const promise = new Promise((resolve, reject) => {
      rejectRace = reject;
      const errors = {};
      for (const [name, job] of Object.entries(jobs)) {
        job.promise.then((result) => {
          if (settled) return;
          settled = true;
          for (const [other, j] of Object.entries(jobs)) if (other !== name) j.cancel();
          if (!result.cached) this.remember(model.key, name); // a cache hit says nothing about speed
          resolve({ ...result, engine: name });
        }, (e) => {
          if (settled || e.cancelled) return;
          errors[name] = e;
          if (Object.keys(errors).length === 2) { settled = true; reject(errors.native); }
        });
      }
    });
    return {
      promise,
      cancel() {
        if (settled) return;
        settled = true;
        for (const j of Object.values(jobs)) j.cancel();
        rejectRace(cancelledError());
      },
    };
  }

  renderNative(model, values, onEvent = () => {}) {
    const job = `j${Date.now().toString(36)}${(this.n++).toString(36)}`;
    const started = performance.now();
    let meta = null;
    let rejectRun;
    let settled = false;
    const promise = new Promise((resolve, reject) => {
      rejectRun = reject;
      if (!this.info.engine && model.engine !== "mitcad") {
        reject(Object.assign(new Error(this.info.engine_error || "OpenSCAD isn't available."), { logs: [] }));
        return;
      }
      const channel = new tauri.core.Channel();
      channel.onmessage = (m) => { if (m?.type === "done") meta = m; else onEvent(m); };
      const req = {
        model: model.key, entry: model.entry,
        files: { ...this.common, ...model.files },
        values: { ...values, ...(model.fixed || {}) },
        defines: (model.parameters || []).filter((p) => p.define).map((p) => p.name),
        call: model.call || null, // a component: the app writes the call with these values
        format: model.format || null, // "3mf": OpenSCAD's own, objects apart and colours kept (0.4.19); "svg"/"dxf": a drawing
        engine: model.engine || null, // "mitcad": a Fusion 360 file, generated with MitCad (experimental, 0.4.27)
      };
      // edited files (the Code tab): stored by their hash in the app's data folder, used in place of the originals
      const edited = Object.entries(model.edits || {});
      Promise.all(edited.map(async ([p, text]) => { req.files[p] = await api("blob_put", { text }); }))
        .then(() => (settled ? Promise.reject(cancelledError()) : invoke("render", { job, req, onEvent: channel }))).then((buf) => {
        settled = true;
        const ms = meta?.ms ?? performance.now() - started;
        // (a 2D model asked for as STL comes back as its SVG drawing, 0.4.23)
        const format = meta?.format || model.format || "stl";
        resolve({ blob: new Blob([buf], { type: MIME[format] || "model/stl" }), format, ms, cached: meta ? !!meta.cached : ms < 40, logs: meta?.logs || [], engine: "native",
          warning: meta?.warning || null }); // the model came out incomplete (MitCad's failed features, 0.4.29)
      }, (e) => {
        settled = true;
        reject(Object.assign(new Error(e?.message || String(e)), { logs: e?.logs || [], cancelled: !!e?.cancelled }));
      });
    });
    return {
      promise,
      cancel() {
        if (settled) return;
        invoke("render_cancel", { job }).catch(() => {});
        rejectRun(cancelledError());
      },
    };
  }
}

export async function createPlatform() {
  const info = await api("app_info").catch((e) => ({ engine_error: String(e) }));
  let prefs = await api("prefs_get").catch(() => ({}));
  let timer = null;
  const settings = {
    async persistent() { return true; },
    list: (model) => api("settings_list", { model }),
    get: (id) => api("settings_get", { id }),
    async save(rec) {
      const now = new Date().toISOString();
      const old = rec.id ? await api("settings_get", { id: rec.id }) : null;
      const row = old ? { ...old, ...rec, updated: now }
        : { kind: "settings", id: newId(), ...rec, created: now, updated: now };
      await api("settings_put", { record: row });
      return row;
    },
    remove: (id) => api("settings_remove", { id }),
  };
  const store = {
    kind: "desktop",
    prefs: {
      get(key, fallback = null) { return key in prefs ? prefs[key] : fallback; },
      set(key, value) {
        prefs = { ...prefs, [key]: value };
        clearTimeout(timer);
        timer = setTimeout(() => api("prefs_set", { prefs }).catch(() => {}), 250);
      },
    },
    settings,
  };
  /** Wait for a background job (adding or reading a project); onStage gets progress text. */
  async function waitJob(job, onStage = () => {}) {
    for (;;) {
      await new Promise((r) => setTimeout(r, 400));
      const j = (await api("jobs")).find((x) => x.id === job);
      if (!j) throw new Error("The job disappeared.");
      onStage(j.stage);
      if (j.done) {
        if (j.error) throw new Error(j.error);
        return j.result;
      }
    }
  }
  const libraryUrl = info.library_url || "library://localhost/";
  // web links (a project's GitHub page, the releases page) open in the default browser, not in the app's window
  document.addEventListener("click", (e) => {
    const a = e.target.closest?.("a[href]");
    if (!a || e.defaultPrevented || a.hasAttribute("download")) return;
    let url;
    try { url = new URL(a.href, location.href); } catch { return; }
    if (!/^https?:$/.test(url.protocol) || url.origin === location.origin || url.href.startsWith(libraryUrl)) return;
    e.preventDefault();
    invoke("open_url", { url: url.href }).catch((err) => console.warn("open_url", err));
  });
  return {
    kind: "desktop",
    info,
    store,
    api,
    apiBytes,
    /** A model file's text (by its content hash), for the side viewer's Code tab. */
    modelFile: async (sha) => new TextDecoder().decode(await apiBytes("blob", { sha })),
    /** A web link in the default browser. */
    openUrl: (url) => invoke("open_url", { url }).catch((err) => console.warn("open_url", err)),
    makeEngine: (catalog) => new DesktopEngine(info, catalog, store.prefs),
    /** App files and the library's catalog, model pages and files (by relative path). */
    async fetch(path) {
      try {
        const buf = await apiBytes("read", { path: path.replace(/^\.?\//, "") });
        const ext = path.split(".").pop().toLowerCase();
        return new Response(buf, { status: 200, headers: { "content-type": MIME[ext] || "application/octet-stream" } });
      } catch (e) {
        return new Response(String(e), { status: 404 });
      }
    },
    /** Save dialog + write. Resolves to the saved path, or null if cancelled. */
    async save(blob, name) {
      const bytes = new Uint8Array(await blob.arrayBuffer());
      return invoke("save_file", bytes, { headers: { "x-name": encodeURIComponent(name) } });
    },
    reveal: (path) => invoke("reveal", { path }),
    /** Files and folders dragged onto the window (Tauri's drag-drop events): `cb(paths, at)` on a drop,
     * `over(true/false)` while dragging (milestone 5, as Model Library does). */
    onDrop(cb, over) {
      const listen = (name, fn) => tauri.event?.listen?.(name, fn)?.catch?.(() => {});
      const at = (p) => (p && typeof p.x === "number" ? { x: p.x / (devicePixelRatio || 1), y: p.y / (devicePixelRatio || 1) } : null);
      listen("tauri://drag-drop", (e) => { over?.(false); cb(e.payload?.paths || [], at(e.payload?.position)); });
      listen("tauri://drag-enter", () => over?.(true));
      listen("tauri://drag-leave", () => over?.(false));
    },
    async refreshInfo() { Object.assign(info, await api("app_info")); return info; },
    workspace: {
      choose: async () => {
        const path = await invoke("pick_folder", { title: "Open or create a library folder" });
        if (!path) return null;
        await api("library_open", { path });
        return path;
      },
      open: () => invoke("open_path", { path: info.library?.path || info.workspace }),
      clearCache: () => api("cache_clear"),
    },
    /** The library: projects, jobs, metadata (desktop only). */
    library: {
      url: (rel) => libraryUrl + rel.split("/").map(encodeURIComponent).join("/"),
      pickFolder: (title) => invoke("pick_folder", { title }),
      pickFile: (title, extensions) => invoke("pick_file", { title, extensions }),
      pickSavePath: (title, name, extensions) => invoke("pick_save_path", { title, name, extensions }),
      openPath: (path) => invoke("open_path", { path }),
      waitJob,
      jobs: () => api("jobs"),
    },
  };
}
