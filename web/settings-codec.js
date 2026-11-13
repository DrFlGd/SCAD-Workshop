// Settings as data: what differs from the defaults, and OpenSCAD's own parameter-set files (the .json the Customizer reads and writes).

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

/** Settings that differ from the model's defaults. */
export function changedValues(model, values) {
  const out = {};
  for (const p of model.parameters) if (p.name in values && !same(values[p.name], p.default)) out[p.name] = values[p.name];
  return out;
}

/** Does v have the same shape as the parameter's default (so the form can show it)? */
function fits(p, v) {
  const d = p.default;
  if (Array.isArray(d)) return Array.isArray(v) && v.length === d.length && v.every((x, i) => typeof x === typeof d[i]);
  if (typeof d === "number") return typeof v === "number" && Number.isFinite(v);
  return typeof v === typeof d;
}

/**
 * Defaults + changes. Returns { values, applied, skipped } where skipped lists
 * names the model no longer has or values of the wrong kind (left at default).
 */
export function withChanges(model, changes) {
  const values = Object.fromEntries(model.parameters.map((p) => [p.name, structuredClone(p.default)]));
  const byName = new Map(model.parameters.map((p) => [p.name, p]));
  let applied = 0;
  const skipped = [];
  for (const [k, v] of Object.entries(changes || {})) {
    const p = byName.get(k);
    if (p && fits(p, v)) { values[k] = structuredClone(v); applied++; } else skipped.push(k);
  }
  return { values, applied, skipped };
}

// ---------------------------------------------------------------- OpenSCAD parameter files
// OpenSCAD stores every value as a string: "2", "true", "[0, 14, 0, 0.6]", or bare text.
function scadText(v) {
  if (Array.isArray(v)) return `[${v.map((x) => (typeof x === "string" ? JSON.stringify(x) : scadText(x))).join(", ")}]`;
  return String(v);
}

function fromScadText(p, s) {
  const d = p.default;
  if (typeof s !== "string") return s;
  if (typeof d === "number") { const n = Number(s); return s.trim() !== "" && Number.isFinite(n) ? n : undefined; }
  if (typeof d === "boolean") return s === "true" ? true : s === "false" ? false : undefined;
  if (Array.isArray(d)) { try { return JSON.parse(s); } catch { return undefined; } }
  return s;
}

/** sets: [{ name, values }] with full values; fixed values the site forces are included. */
export function toOpenSCAD(model, sets) {
  const parameterSets = {};
  for (const set of sets) {
    const row = {};
    const all = { ...set.values, ...(model.fixed || {}) };
    for (const k of Object.keys(all).sort()) row[k] = scadText(all[k]);
    parameterSets[set.name] = row;
  }
  return JSON.stringify({ fileFormatVersion: "1", parameterSets }, null, 4) + "\n";
}

/** Parse an OpenSCAD parameter file -> [{ name, changes, skipped }] relative to the site's defaults. */
export function fromOpenSCAD(model, text) {
  let data;
  try { data = JSON.parse(text); } catch { throw new Error("This isn't a JSON file."); }
  const sets = data?.parameterSets;
  if (!sets || typeof sets !== "object") throw new Error("No parameter sets found. Expected an OpenSCAD Customizer file.");
  const byName = new Map(model.parameters.map((p) => [p.name, p]));
  const fixed = model.fixed || {};
  return Object.entries(sets).map(([name, row]) => {
    const parsed = {};
    let unknown = 0;
    for (const [k, s] of Object.entries(row || {})) {
      if (k.startsWith("$") || k in fixed) continue; // render quality and site-fixed values aren't user settings
      const p = byName.get(k);
      if (!p) { unknown++; continue; }
      const v = fromScadText(p, s);
      if (v !== undefined) parsed[k] = v; else unknown++;
    }
    const { values, skipped } = withChanges(model, parsed);
    return { name, changes: changedValues(model, values), skipped: unknown + skipped.length };
  });
}
