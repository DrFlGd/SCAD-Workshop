// Pinning a component as a model, and the form editor (desktop). Both write
// manifest-shaped data (docs/DESKTOP_PLAN.md, "Components"): a pin is like a
// family manifest's model entry with "component" instead of "entrypoint"; a form
// edit is a model's `ui`, `hidden`, `defaults` and `presets`, stored with the
// item's metadata and applied with the same code the website build uses.
import { html, useState, useEffect } from "../lib/html.js";
import { ui, pushUndo } from "./state.js";
import { ctx } from "./context.js";
import { CategoryPicker } from "./catpick.js";
import { Icon } from "./icons.js";
import { api } from "./library.js";
import { itemTarget } from "./metaedit.js";
import { humanize } from "../lib/util.js";

const same = (a, b) => JSON.stringify(a ?? null) === JSON.stringify(b ?? null);
const close = () => ui.set({ dialog: null });
const toastErr = (what) => (e) => ctx.toast(`${what}: ${e.message || e}`);

/** The Parametric Models category a component's topic suggests. */
const TOPIC_CATEGORY = { gears: "mechanical", threads: "mechanical", bearings: "mechanical", hinges: "mechanical", motors: "mechanical",
  fasteners: "fasteners", enclosures: "enclosures", text: "labels" };

// ---------------------------------------------------------------- pin
export function PinDialog({ spec }) {
  const [detail, setDetail] = useState(null);
  const [name, setName] = useState("");
  const [category, setCategory] = useState("other");
  const [shown, setShown] = useState({});
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    ctx.loadModel(spec.key).then(({ detail, values }) => {
      setDetail({ detail, values: { ...values, ...(spec.values || {}) } });
      setName(`${detail.name}`);
      setCategory(TOPIC_CATEGORY[detail.category] || "other");
      // show the main settings and anything set away from its default
      const vals = { ...values, ...(spec.values || {}) };
      setShown(Object.fromEntries(detail.parameters.map((p) => [p.name, p.group === "Settings" || !same(vals[p.name], p.default)])));
    }, toastErr("Couldn't load the component"));
  }, [spec.key]);
  if (!detail) return html`<div class="dialog"><p class="muted">Loading…</p></div>`;
  const { detail: d, values } = detail;
  const save = async (e) => {
    e.preventDefault();
    setBusy(true);
    try {
      const params = d.parameters;
      const defaults = {}, fixed = {}, hidden = [];
      for (const p of params) {
        const v = values[p.name];
        if (shown[p.name]) { if (!same(v, p.default)) defaults[p.name] = v; }
        else {
          hidden.push(p.name);
          if (!same(v, p.default) || v != null) fixed[p.name] = v; // hidden at the value it has now
        }
      }
      for (const k of Object.keys(fixed)) if (fixed[k] == null) delete fixed[k];
      let cat = category;
      if (typeof cat === "object") {
        // a new category named in the picker
        const label = cat.create.trim();
        cat = label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "other";
        if (!(ctx.catalog.category_choices || []).some((c) => c.id === cat)) await api("category_set", { id: cat, label });
      }
      const r = await api("pin_create", { component: d.key, name: name.trim(), category: cat, defaults, hidden, fixed });
      await ctx.reloadCatalog();
      close();
      ctx.toast(`Pinned as ${name.trim()} under Parametric Models.`);
      location.hash = `#/m/${r.key}`;
    } catch (err) { toastErr("Couldn't pin it")(err); } finally { setBusy(false); }
  };
  const groups = [...new Set(d.parameters.map((p) => p.group))];
  return html`<form class="dialog pin-dialog" onSubmit=${save} aria-label="Pin as a model">
    <div class="dialog-head"><h2>${Icon.pin(18)} Pin as a model</h2><button type="button" class="ghost" aria-label="Close" onClick=${close}>${Icon.close(16)}</button></div>
    <p class="muted">${d.component?.library} <code>${d.component?.module}()</code> becomes a model under Parametric Models, with the values it has now as its defaults. Settings you don't show stay at those values.</p>
    <label class="field-block"><span>Name</span><input id="pin-name" value=${name} onInput=${(e) => setName(e.target.value)} required /></label>
    <label class="field-block"><span>Category</span>
      <${CategoryPicker} id="pin-category" value=${typeof category === "object" ? category.create : category} onChange=${setCategory} /></label>
    <fieldset class="pin-settings"><legend>Settings to show</legend>
      ${groups.map((g) => html`<div class="pin-group"><b>${g}</b>
        ${d.parameters.filter((p) => p.group === g).map((p) => html`<label class="check" key=${p.name}>
          <input type="checkbox" data-pin-show=${p.name} checked=${!!shown[p.name]} onChange=${(e) => setShown({ ...shown, [p.name]: e.target.checked })} />
          ${p.label || p.name} <span class="muted">${values[p.name] == null ? "" : `= ${typeof values[p.name] === "object" ? JSON.stringify(values[p.name]) : values[p.name]}`}</span></label>`)}
      </div>`)}
    </fieldset>
    <div class="dialog-actions"><button type="button" class="ghost" onClick=${close}>Cancel</button>
      <button type="submit" class="primary" disabled=${busy || !name.trim()} id="pin-go">Pin</button></div>
  </form>`;
}

// ---------------------------------------------------------------- form editor
const PROFILE = [["", "None"], ["bed", "Print bed size"], ["bed.x", "Bed width (X)"], ["bed.y", "Bed depth (Y)"], ["nozzle", "Nozzle diameter"]];

const choicesText = (opts) => (opts ? opts.map((o) => (typeof o === "object" ? `${JSON.stringify(o.value)} = ${o.label ?? ""}` : JSON.stringify(o))).join("\n") : "");

/** A setting as the project's file has it, as editable text: what each field of the form editor starts
 * from (2026-10-16: the values are in the fields, not greyed hints that go when you type). */
function readRow(p) {
  return {
    show: true, label: p.label && p.label !== p.name ? p.label : humanize(p.name), description: p.description ?? "",
    axes: Array.isArray(p.default) ? (p.axes || ["X", "Y", "Z"]).join(", ") : "", min: p.min ?? "", max: p.max ?? "", step: p.step ?? "",
    choices: choicesText(p.options), when: p.show_if ?? "", profile: "", group: p.group ?? "",
    default: p.default === undefined ? "" : JSON.stringify(p.default),
  };
}

/** A setting's form, as editable text: { label, description, axes, min, max, step, choices, when, profile, group, default, show },
 * each field what the form sets or else what the file has. */
function rowFor(p, form) {
  const u = form.ui?.[p.name] || {};
  const base = readRow(p);
  const has = (k) => u[k] !== undefined;
  return {
    show: !(form.hidden || []).includes(p.name),
    label: has("label") ? u.label : base.label, description: has("description") ? u.description : base.description,
    axes: has("axes") ? u.axes.join(", ") : base.axes,
    min: has("min") ? u.min : base.min, max: has("max") ? u.max : base.max, step: has("step") ? u.step : base.step,
    choices: has("options") ? choicesText(u.options) : base.choices,
    when: u["display-condition"]?.js ?? base.when, profile: u.profile ?? "", group: has("group") ? u.group : base.group,
    default: form.defaults && p.name in form.defaults ? JSON.stringify(form.defaults[p.name]) : base.default,
  };
}

const num = (t) => (t === "" || t == null ? undefined : Number(t));
const parseValue = (t) => { try { return JSON.parse(t); } catch { return t; } };

/** The rows back into a manifest-shaped form ({ ui, hidden, defaults, presets }), keeping only what's set. */
function formFrom(params, rows, presets) {
  const ui_ = {}, hidden = [], defaults = {};
  for (const p of params) {
    const r = rows[p.name];
    if (!r) continue;
    if (!r.show) hidden.push(p.name);
    // only what differs from the file is kept, so a later version's own labels and ranges still come through
    const base = readRow(p);
    const differs = (k) => String(r[k]).trim() !== String(base[k]).trim();
    const u = {};
    if (differs("label")) u.label = r.label.trim();
    if (differs("description")) u.description = r.description.trim();
    if (differs("axes") && r.axes.trim()) u.axes = r.axes.split(",").map((s) => s.trim()).filter(Boolean);
    for (const k of ["min", "max", "step"]) { const v = num(String(r[k]).trim()); if (differs(k) && v !== undefined && !Number.isNaN(v)) u[k] = v; }
    // choices cleared where the file or its code gave some: typed in (0.4.24)
    if (differs("choices") && !r.choices.trim()) u.options = [];
    else if (differs("choices") && r.choices.trim()) {
      u.options = r.choices.split("\n").map((l) => l.trim()).filter(Boolean).map((l) => {
        const m = l.match(/^(.*?)\s=\s(.*)$/);
        return m ? { value: parseValue(m[1].trim()), label: m[2].trim() || m[1].trim() } : { value: parseValue(l), label: l.replace(/^"|"$/g, "") };
      });
    }
    if (differs("when") && r.when.trim()) u["display-condition"] = { js: r.when.trim() };
    if (r.profile) u.profile = r.profile;
    if (differs("group") && r.group.trim()) u.group = r.group.trim();
    if (Object.keys(u).length) ui_[p.name] = u;
    if (differs("default") && r.default.trim()) defaults[p.name] = parseValue(r.default.trim());
  }
  const form = {};
  if (Object.keys(ui_).length) form.ui = ui_;
  if (hidden.length) form.hidden = hidden;
  if (Object.keys(defaults).length) form.defaults = defaults;
  if (presets.length) form.presets = presets;
  return Object.keys(form).length ? form : null;
}

export function FormEditor({ spec }) {
  const [state, setState] = useState(null);
  const [open, setOpen] = useState(null);
  const [busy, setBusy] = useState(false);
  const [presetName, setPresetName] = useState("");
  useEffect(() => {
    ctx.loadModel(spec.key).then(({ detail }) => {
      const params = detail.base_parameters || detail.parameters;
      const form = detail.form || {};
      setState({ detail, params, rows: Object.fromEntries(params.map((p) => [p.name, rowFor(p, form)])), presets: form.presets || [] });
    }, toastErr("Couldn't load the model"));
  }, [spec.key]);
  if (!state) return html`<div class="dialog"><p class="muted">Loading…</p></div>`;
  const { detail, params, rows, presets } = state;
  const item = ctx.index.get(`gen:${detail.key}`);
  const set = (name, k, v) => setState({ ...state, rows: { ...rows, [name]: { ...rows[name], [k]: v } } });
  const form = formFrom(params, rows, presets);
  const save = async (value) => {
    setBusy(true);
    try {
      const t = itemTarget(item);
      const r = await api("meta_set_many", { edits: [{ ...t, patch: { form: value } }] });
      const prev = r.previous?.[0];
      if (prev) pushUndo(`form of ${detail.name}`, () => api("meta_set_many", { edits: [{ ...t, patch: { form: prev.patch.form ?? null } }] }).then(() => ctx.refreshModel?.(detail.key)));
      await ctx.reloadCatalog();
      ctx.refreshModel?.(detail.key);
      close();
      ctx.toast(value ? "Form saved." : "Form back to how the project has it.", { label: "Undo", run: () => import("./metaedit.js").then((m) => m.undoNow()).then(() => ctx.refreshModel?.(detail.key)) });
    } catch (e) { toastErr("Couldn't save the form")(e); } finally { setBusy(false); }
  };
  const addPreset = () => {
    const values = ctx.currentValues?.(detail.key);
    if (!values || !presetName.trim()) return;
    const changed = Object.fromEntries(Object.entries(values).filter(([k, v]) => { const p = params.find((x) => x.name === k); return p && !same(v, p.default); }));
    setState({ ...state, presets: [...presets, { label: presetName.trim(), values: changed }] });
    setPresetName("");
  };
  const copy = async () => {
    try { await navigator.clipboard.writeText(JSON.stringify(form || {}, null, 2)); ctx.toast("Copied: paste it into the model's entry in its project's workshop.json."); }
    catch { ctx.toast("The clipboard isn't available."); }
  };
  const input = (name, k, attrs = {}) => html`<input value=${rows[name][k]} onInput=${(e) => set(name, k, e.target.value)} data-form=${`${name}.${k}`} ...${attrs} />`;
  return html`<div class="dialog form-editor" aria-label="Edit form">
    <div class="dialog-head"><h2>${Icon.edit(18)} Edit the form: ${detail.name}</h2><button type="button" class="ghost" aria-label="Close" onClick=${close}>${Icon.close(16)}</button></div>
    <p class="muted">Change how the settings look. The model's file isn't touched: this is saved with the model's details in your library. Copy as JSON gives the same description, for a project's workshop.json.</p>
    <div class="fe-list">
      ${params.map((p) => {
        const r = rows[p.name];
        const isOpen = open === p.name;
        const changed = !same(readRow(p), { ...r });
        return html`<div class=${`fe-row${isOpen ? " open" : ""}${r.show ? "" : " fe-hidden"}`} key=${p.name} data-fe=${p.name}>
          <div class="fe-head">
            <label class="check" title="Show this setting"><input type="checkbox" checked=${r.show} onChange=${(e) => set(p.name, "show", e.target.checked)} data-form=${`${p.name}.show`} /></label>
            <button type="button" class="fe-name" onClick=${() => setOpen(isOpen ? null : p.name)} aria-expanded=${isOpen ? "true" : "false"}>
              <b>${r.label || humanize(p.name)}</b> <code>${p.name}</code> <span class="muted">${r.group}</span>${changed ? html` <span class="badge-mini">edited</span>` : null}</button>
            ${changed ? html`<button type="button" class="link-btn small" data-fe-reset=${p.name} title="Put back what the model's file has"
              onClick=${() => setState({ ...state, rows: { ...rows, [p.name]: readRow(p) } })}>Put back</button>` : null}
          </div>
          ${isOpen ? html`<div class="fe-body">
            <label><span>Label</span>${input(p.name, "label")}</label>
            <label><span>Help</span>${input(p.name, "description")}</label>
            <label><span>Section</span>${input(p.name, "group")}</label>
            <label><span>Default</span>${input(p.name, "default")}</label>
            ${p.type === "number" || p.widget === "slider" || Array.isArray(p.default) ? html`<div class="fe-three">
              <label><span>Min</span>${input(p.name, "min")}</label>
              <label><span>Max</span>${input(p.name, "max")}</label>
              <label><span>Step</span>${input(p.name, "step")}</label></div>` : null}
            ${Array.isArray(p.default) ? html`<label><span>Box names</span>${input(p.name, "axes")}</label>` : null}
            <label><span>Choices <small class="muted">${p.choices_from === "code" ? "read from the code; clear them to type values in" : "one per line: value = label; none to type values in"}</small></span><textarea rows="3" value=${r.choices}
              onInput=${(e) => set(p.name, "choices", e.target.value)} data-form=${`${p.name}.choices`}></textarea></label>
            <label><span>Show when <small class="muted">such as style == "round"</small></span>${input(p.name, "when", { class: "expr" })}</label>
            <label><span>Printer profile</span><select value=${r.profile} onChange=${(e) => set(p.name, "profile", e.target.value)} data-form=${`${p.name}.profile`}>
              ${PROFILE.map(([v, l]) => html`<option value=${v} selected=${v === r.profile}>${l}</option>`)}</select></label>
          </div>` : null}
        </div>`;
      })}
    </div>
    <section class="fe-presets"><h3>Presets</h3>
      ${presets.length ? html`<ul>${presets.map((ps, i) => html`<li key=${i}>${ps.label} <span class="muted">${Object.keys(ps.values).length} values</span>
        <button type="button" class="link-btn" onClick=${() => setState({ ...state, presets: presets.filter((_, j) => j !== i) })}>Remove</button></li>`)}</ul>` : html`<p class="muted">None. Presets appear in the Defaults list above the model's form.</p>`}
      ${ctx.currentValues?.(detail.key) ? html`<div class="fe-add-preset"><input placeholder="Name for the current settings" value=${presetName} onInput=${(e) => setPresetName(e.target.value)} id="fe-preset-name" />
        <button type="button" class="ghost" onClick=${addPreset} disabled=${!presetName.trim()} id="fe-preset-add">Add as a preset</button></div>` : null}
    </section>
    <div class="dialog-actions">
      <button type="button" class="ghost" onClick=${copy} title="Copy the form as JSON, for a project's workshop.json">Copy as JSON</button>
      ${detail.form ? html`<button type="button" class="ghost danger-text" onClick=${() => save(null)} disabled=${busy} id="fe-reset">Reset the form</button>` : null}
      <span class="spacer"></span>
      <button type="button" class="ghost" onClick=${close}>Cancel</button>
      <button type="button" class="primary" onClick=${() => save(form)} disabled=${busy} id="fe-save">Save</button>
    </div>
  </div>`;
}
