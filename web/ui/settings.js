// Settings (#/settings): the app's own choices, kept on this computer: the skin and theme, the
// printer profile models start from, the format a Download offers first, the engine and the app's
// version. What's kept in the library folder is in Library settings (libsettings.js).
// (Review 7, item 13: the first page of app.js moved to Preact.)
import { html, useState } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setTheme, THEMES, setSkin, SKINS } from "./state.js";
import { ctx } from "./context.js";
import { bytes } from "../lib/util.js";

const prefs = () => ctx.platform.store.prefs;

export const RELEASES_URL = "https://github.com/DrFlGd/SCAD-Workshop/releases";

// ---------------------------------------------------------------- printer profile
// Values set once here that models asking for them (a parameter's `profile`) start from.
export const PROFILE_FIELDS = [
  { id: "bed", label: "Print bed size", unit: "mm", axes: ["width (X)", "depth (Y)"], kind: "vec2", min: 50, max: 2000 },
  { id: "nozzle", label: "Nozzle diameter", unit: "mm", kind: "number", min: 0.1, max: 2, step: 0.05 },
];
export const getProfile = () => prefs().get("gw-profile", {}) || {};

/** A profile value by its path ("bed", "bed.x"), or undefined when it isn't set. */
export function profileValue(path, profile = getProfile()) {
  const [field, axis] = path.split(".");
  const v = profile[field];
  if (v == null) return undefined;
  if (axis) return Array.isArray(v) ? v[{ x: 0, y: 1, z: 2 }[axis]] : undefined;
  return v;
}

/** A model's defaults with the profile's values put in where its parameters ask for them. */
export function applyProfile(detail) {
  for (const p of detail.parameters) {
    if (!p.profile) continue;
    const v = profileValue(p.profile);
    const d = p.default;
    const fits = Array.isArray(d) ? Array.isArray(v) && v.length === d.length && v.every((x) => typeof x === "number")
      : typeof d === "number" && typeof v === "number";
    if (fits) { p.default = structuredClone(v); p.profiled = true; }
  }
}

// ---------------------------------------------------------------- download formats (milestone 5)
export const PREFERRED = [["stl", "STL where there is one"], ["3mf", "3MF where there is one, otherwise STL"], ["step", "STEP where there is one, otherwise STL"]];
export const preferredFormat = () => prefs().get("gw-format-pref") || "stl";
/** The format to offer first among `formats` (["stl", "3mf", ...]): the one last chosen for `key`, else the rule above. */
export function pickFormat(key, formats) {
  const last = (prefs().get("gw-formats", {}) || {})[key];
  if (last && formats.includes(last)) return last;
  const pref = preferredFormat();
  return formats.includes(pref) ? pref : formats.includes("stl") ? "stl" : formats[0];
}
export function rememberFormat(key, fmt) {
  prefs().set("gw-formats", { ...(prefs().get("gw-formats", {}) || {}), [key]: fmt });
}

// ---------------------------------------------------------------- the page
const SECTIONS = [["appearance", "Appearance"], ["profile", "Printer profile"], ["downloads", "Downloads"], ["engine", "Engine"], ["about", "About"]];

/** One section, laid out like Library settings': a heading, a grey intro, then its contents. */
function Section({ id, intro, children }) {
  const title = SECTIONS.find(([s]) => s === id)[1];
  return html`<section class="ls-section" id=${`ls-${id}`}><h2>${title}</h2>${intro ? html`<p class="muted">${intro}</p>` : null}${children}</section>`;
}

/** A row of radio buttons with a bold label. */
function Choice({ id, label, name, options, value, onPick }) {
  return html`<div class="theme-pick" id=${id} role="radiogroup" aria-label=${label}><b>${label}</b>${options.map(([v, text]) => html`<label>
    <input type="radio" name=${name} value=${v} checked=${value === v} onChange=${() => onPick(v)} /> ${text}</label>`)}</div>`;
}

function Appearance() {
  const s = useStore(ui, (st) => ({ skin: st.skin, theme: st.theme }));
  return html`<${Section} id="appearance" intro="The skin is the app's look: its colours, lettering, shapes and icons. Each skin has light and dark colours, chosen by the theme.">
    <${Choice} id="skin-pick" label="Skin" name="skin" options=${SKINS} value=${s.skin} onPick=${setSkin} />
    <${Choice} id="theme-pick" label="Theme" name="theme" options=${THEMES} value=${s.theme} onPick=${setTheme} />
  </${Section}>`;
}

function ProfileField({ f, value, save }) {
  const models = ctx.catalog?.models || [];
  const nameOf = (key) => { const m = models.find((x) => x.key === key); return m ? `${m.family_name} ${m.name}` : key; };
  const used = [...new Set(((ctx.catalog?.profile_uses || {})[f.id] || []).map((u) => u.key))];
  const input = f.kind === "vec2"
    ? html`<div class="vector named" style="--n:2">${f.axes.map((ax, i) => html`<label data-axis=${ax} style=${`--ax:${ax.length}`}>${ax}<input type="number"
        min=${f.min} max=${f.max} step="any" value=${value ? value[i] : ""} aria-label=${`${f.label} ${ax}`} placeholder="not set"
        onChange=${(e) => {
          const vals = [...e.target.closest(".vector").querySelectorAll("input")].map((x) => (x.value === "" ? null : +x.value));
          save(f.id, vals.every((x) => x != null && x > 0) ? vals : null);
        }} /></label>`)}</div>`
    : html`<input type="number" min=${f.min} max=${f.max} step=${f.step || "any"} value=${value ?? ""} placeholder="not set" aria-label=${f.label}
        onChange=${(e) => save(f.id, e.target.value === "" ? null : +e.target.value)} />`;
  return html`<div class="field profile-field">
    <div class="field-head"><span class="label">${f.label} (${f.unit})</span></div>
    ${input}
    <p class="help-inline">${used.length ? ["Used by ", ...used.flatMap((k, i) => [i ? ", " : "", html`<a href=${`#/m/${k}`}>${nameOf(k)}</a>`]), "."] : "No model uses this yet."}</p>
  </div>`;
}

function PrinterProfile({ onChange }) {
  const [profile, setProfile] = useState(getProfile);
  const [note, setNote] = useState("");
  const save = (id, value) => {
    const next = { ...getProfile() };
    if (value == null) delete next[id]; else next[id] = value;
    prefs().set("gw-profile", next);
    setProfile(next);
    onChange?.();
    setNote("Saved. Models open with these values from now on.");
  };
  return html`<${Section} id="profile" intro="Models that ask for these start with your values instead of the author's. Leave a value empty to keep the defaults. Magnet and tolerance settings aren't linked: each project measures them differently.">
    <div class="profile-grid">${PROFILE_FIELDS.map((f) => html`<${ProfileField} key=${f.id} f=${f} value=${profile[f.id]} save=${save} />`)}</div>
    <p id="profile-note" class="muted" role="status">${note}</p>
  </${Section}>`;
}

function Downloads() {
  const [pref, setPref] = useState(preferredFormat);
  const pick = (id) => { prefs().set("gw-format-pref", id); setPref(id); };
  return html`<${Section} id="downloads" intro="Which format a part's Download offers first when it comes in several, and a model's render. What you pick for one part or model is kept for it.">
    <${Choice} id="format-pref" label="Preferred format" name="format-pref" options=${PREFERRED} value=${pref} onPick=${pick} />
  </${Section}>`;
}

function Engine() {
  const [info, setInfo] = useState(ctx.platform.info || {});
  const clear = async () => {
    try {
      await ctx.platform.workspace.clearCache();
      setInfo({ ...(await ctx.platform.refreshInfo()) });
    } catch (e) { ctx.toast(String(e)); }
  };
  const n = info.concurrency || 1;
  return html`<${Section} id="engine">
    ${info.engine
      ? html`<p>OpenSCAD ${info.engine}, native, up to ${n} render${n > 1 ? "s" : ""} at once. <br /><code>${info.engine_path || ""}</code></p>`
      : html`<p class="lic-summary">${info.engine_error || "OpenSCAD isn't available."}</p>`}
    <p>Finished renders are kept so repeating one is instant: <span>${info.cache_bytes == null ? "unknown" : bytes(info.cache_bytes)}</span>. <button type="button" class="ghost" onClick=${clear}>Clear</button></p>
  </${Section}>`;
}

function About() {
  const info = ctx.platform.info || {};
  return html`<${Section} id="about">
    <p id="app-version">SCAD Workshop ${info.version || "(version unknown)"}. <a href=${RELEASES_URL} target="_blank" rel="noopener">All releases and what changed</a></p>
  </${Section}>`;
}

/** The page. `onProfileChange`: the printer profile changed (models open with it from then on). */
export function SettingsPage({ onProfileChange }) {
  return html`<div class="library-settings">
    <nav class="ls-toc" aria-label="On this page">${SECTIONS.map(([id, label]) => html`<a href="#/settings"
      onClick=${(e) => { e.preventDefault(); document.getElementById(`ls-${id}`)?.scrollIntoView({ behavior: "smooth" }); }}>${label}</a>`)}</nav>
    <${Appearance} />
    <${PrinterProfile} onChange=${onProfileChange} />
    <${Downloads} />
    <${Engine} />
    <${About} />
  </div>`;
}
