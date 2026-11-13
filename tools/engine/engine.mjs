// OpenSCAD WebAssembly runner for the build tools (Node; the site build reads models'
// settings with it until Review 7's item 12). One fresh engine instance per run: the WASM
// build aborts its runtime after a render, and a fresh instance also gives each
// model an empty virtual file system.
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const FONT_ENV = { FONTCONFIG_PATH: "/fonts", FONTCONFIG_FILE: "/fonts/fonts.conf", HOME: "/tmp" };

// Capture each instance's exported memory so runs can report peak engine memory.
let lastMemory = null;
const origInstantiate = WebAssembly.instantiate;
WebAssembly.instantiate = async (...a) => {
  const r = await origInstantiate(...a);
  const inst = r.instance || r;
  lastMemory = Object.values(inst.exports || {}).find((x) => x instanceof WebAssembly.Memory) || lastMemory;
  return r;
};

export async function loadEngine(engineDir) {
  const mod = await import(pathToFileURL(path.join(engineDir, "openscad.js")).href);
  const OpenSCAD = mod.default;
  const wasmBinary = fs.readFileSync(path.join(engineDir, "openscad.wasm"));
  let version = null;

  async function instance(logs) {
    const inst = await OpenSCAD({
      noInitialRun: true,
      wasmBinary,
      print: (l) => logs.push(String(l)),
      printErr: (l) => logs.push(String(l)),
    });
    if (inst.ENV) Object.assign(inst.ENV, FONT_ENV);
    inst.FS.mkdirTree("/tmp/fontconfig");
    return inst;
  }

  /**
   * files: Map(virtualPath -> Uint8Array); args: OpenSCAD argv; outputs: paths to read back.
   */
  async function run(files, args, outputs = []) {
    const logs = [];
    const t0 = performance.now();
    const inst = await instance(logs);
    const memory = lastMemory;
    for (const [p, data] of files) {
      inst.FS.mkdirTree(path.posix.dirname(p));
      inst.FS.writeFile(p, data);
    }
    let code;
    try {
      code = inst.callMain(args);
    } catch (e) {
      code = typeof e === "number" ? e : -1;
      logs.push(`EXCEPTION: ${e && e.message ? e.message : e}`);
      if (process.env.ENGINE_DEBUG && e && e.stack) logs.push(String(e.stack).split("\n").slice(0, 12).join("\n"));
    }
    const out = {};
    for (const o of outputs) {
      try { out[o] = inst.FS.readFile(o); } catch { out[o] = null; }
    }
    // WebAssembly memory only grows, so its size after the run is the peak
    const heap = memory ? memory.buffer.byteLength : null;
    return { code, logs, outputs: out, ms: performance.now() - t0, heapBytes: heap };
  }

  async function getVersion() {
    if (version) return version;
    const r = await run(new Map(), ["--version"]);
    version = (r.logs.find((l) => /OpenSCAD version/i.test(l)) || "unknown").replace(/^OpenSCAD version\s*/i, "").trim();
    return version;
  }

  return { run, getVersion };
}

/** Load every file a built model needs from the site's content-addressed store. */
export function modelFiles(site, model, common) {
  const files = new Map();
  for (const [p, sha] of Object.entries({ ...(common || {}), ...model.files })) {
    files.set(p, fs.readFileSync(path.join(site, "fs", sha)));
  }
  return files;
}

/** OpenSCAD parameter-set file: strings unquoted, everything else as JSON text. */
export function parameterSet(values) {
  const set = {};
  for (const [k, v] of Object.entries(values)) set[k] = typeof v === "string" ? v : JSON.stringify(v);
  return JSON.stringify({ fileFormatVersion: "1", parameterSets: { site: set } });
}

export const isError = (l) => /^ERROR:|^EXCEPTION:/.test(l);
