// A model's settings form (#params): the saved-settings bar, then its settings in their groups, each
// as its kind asks (a box, a drop-down, a slider with its number, a vector's numbers, a number or
// text), with display conditions, changed marks and Reset, an author's suggestions beside a
// setting, and Find a setting. app.js keeps the values and draws the form again when they change.
// (Review 7, item 13: moved from app.js to Preact.)
import { html, useRef, useState, useLayoutEffect, useMemo } from "../lib/html.js";
import { humanize, same, safeHTML, compileCondition } from "../lib/util.js";
import { SettingsBar } from "./savedsettings.js";

/** Author text (sanitised HTML) in an element of its own. */
function SafeHTML({ tag = "div", html: text, ...attrs }) {
  const box = useRef(null);
  useLayoutEffect(() => { box.current?.replaceChildren(...safeHTML(text)); }, [text]);
  return html`<${tag} ref=${box} ...${attrs}></${tag}>`;
}
const textOf = (h) => { const d = document.createElement("div"); d.append(...safeHTML(h)); return d.textContent; };

/**
 * A number or text box that shows `value` without taking over while it's typed in (a half-typed
 * number such as "1." stays as it is). `onValue(text, input)` on every change.
 */
function Box({ value, onValue, ...attrs }) {
  const box = useRef(null);
  useLayoutEffect(() => {
    const el = box.current;
    const text = value == null ? "" : typeof value === "object" ? JSON.stringify(value) : String(value);
    if (el && document.activeElement !== el && el.value !== text) el.value = text;
  });
  return html`<input ref=${box} ...${attrs} onInput=${(e) => onValue(e.target.value, e.target)} />`;
}

/** A drop-down whose choice is an index (its options keep their labels as values, as tests and old forms read them). */
function Pick({ index, ...attrs }) {
  const box = useRef(null);
  useLayoutEffect(() => { if (box.current && box.current.selectedIndex !== index) box.current.selectedIndex = index; });
  return html`<select ref=${box} ...${attrs}></select>`;
}

// a step the default sits on, else any (a default between steps stays valid)
function fitStep(p) {
  if (typeof p.default !== "number" || !(p.step > 0)) return p.step;
  const n = (p.default - (p.min ?? 0)) / p.step;
  return Math.abs(n - Math.round(n)) < 1e-6 ? p.step : "any";
}
export const axesOf = (p) => p.axes || (p.default.length <= 3 ? ["X", "Y", "Z"] : p.default.map((_, i) => `${i + 1}`));

/** One setting. */
function Field({ p, value, needed, hidden, searchHidden, set }) {
  const id = `p-${p.name}`;
  // (components read before 0.4.6 have their code names as labels)
  const label = p.label && p.label !== p.name ? p.label : humanize(p.name);
  // a default outside its own range widens it (reported when the project is read)
  const lo = typeof p.default === "number" && p.min != null ? Math.min(p.min, p.default) : p.min;
  const hi = typeof p.default === "number" && p.max != null ? Math.max(p.max, p.default) : p.max;
  const help = p.description_html ? html`<${SafeHTML} class="help" id=${`${id}-help`} html=${p.description_html} />`
    : p.description ? html`<p class="help" id=${`${id}-help`}>${p.description}</p>` : null;
  const describedby = help ? `${id}-help` : null;
  const changed = !same(value, p.default);
  // short labels; the full explanation and the name in the code on hover (or with Help on)
  const tip = useMemo(() => p.description || (p.description_html ? textOf(p.description_html) : null), [p]);
  const code = label !== p.name ? `In the code: ${p.name}` : null;
  const title = tip || code ? [tip, code].filter(Boolean).join("\n\n") : null;
  const head = (labelNode) => html`<div class="field-head">${labelNode}${p.profiled
    ? html`<a class="profile-tag" href="#/settings" title="Starts from your printer profile (Settings)">profile</a>` : null}${p.help_link
    ? html`<a class="help-link" href=${p.help_link} target="_blank" rel="noopener" title="Documentation" aria-label=${`Documentation for ${label}`}>?</a>` : null}<button
      type="button" class="reset" hidden=${!changed} title=${`Reset to ${JSON.stringify(p.default)}`} aria-label=${`Reset ${label}`}
      onClick=${() => set(structuredClone(p.default))}>↺</button></div>`;
  const labelled = head(html`<label for=${id} title=${title}>${label}</label>`);
  const cls = ["field"];
  let body;
  if (p.widget === "checkbox") {
    cls.push("check");
    body = [labelled, html`<input type="checkbox" id=${id} aria-describedby=${describedby} checked=${!!value} onChange=${(e) => set(e.target.checked)} />`];
  } else if (p.widget === "dropdown") {
    // choices longer than half the column get the whole row under the label
    if (p.options.some((o) => String(o.label).length > 24)) cls.push("wide");
    body = [labelled, html`<${Pick} id=${id} aria-describedby=${describedby} index=${Math.max(0, p.options.findIndex((o) => same(o.value, value)))}
      onChange=${(e) => set(p.options[e.target.selectedIndex].value)}>${p.options.map((o) => html`<option>${o.label}</option>`)}</${Pick}>`];
  } else if (p.widget === "slider" && !Array.isArray(p.default)) {
    const step = p.step != null ? fitStep(p) : (Number.isInteger(p.default) && Number.isInteger(p.min ?? 0) && Number.isInteger(p.max ?? 0) ? 1 : "any");
    body = [labelled, html`<div class="slider">
      <${Box} type="range" min=${lo} max=${hi} step=${step === "any" ? (hi - lo) / 100 : step} aria-hidden="true" tabindex="-1" value=${value} onValue=${(v) => set(+v)} />
      <${Box} type="number" id=${id} min=${lo} max=${hi} step=${step} aria-describedby=${describedby} value=${value}
        onValue=${(v, el) => { if (v !== "" && el.checkValidity()) set(+v); }} /></div>`];
  } else if (Array.isArray(p.default)) {
    const axes = axesOf(p);
    const editable = p.default.every((v) => typeof v !== "object");
    body = [head(html`<span class="label" id=${id} title=${title}>${label}</span>`),
      html`<div class=${p.default.length >= 3 && axes.some((a) => a.length > 2) ? "vector named" : "vector"} role="group" aria-labelledby=${id}
        style=${`--n:${Math.min(p.default.length, 4)}`}>${p.default.map((d, i) => {
        const t = typeof d === "boolean" ? "checkbox" : typeof d === "number" ? "number" : "text";
        const ax = axes[i] || `${i + 1}`;
        const x = value?.[i];
        const put = (v) => { const next = structuredClone(value); next[i] = v; set(next); };
        return html`<label data-axis=${ax} style=${`--ax:${ax.length}`}>${ax}${t === "checkbox"
          ? html`<input type="checkbox" data-i=${i} aria-label=${`${label} ${ax}`} disabled=${!editable} checked=${!!x} onInput=${(e) => put(e.target.checked)} />`
          : html`<${Box} type=${t} data-i=${i} aria-label=${`${label} ${ax}`} step="any" min=${p.min} max=${p.max} disabled=${!editable} value=${x}
              onValue=${(v) => put(t === "number" ? (v === "" ? x : +v) : v)} />`}</label>`;
      })}</div>`];
  } else {
    // optional settings (components) can be left empty: the module's own default applies
    const t = p.type === "number" ? "number" : "text";
    const expr = p.type === "expression";
    if (expr) cls.push("field-expr");
    body = [labelled, html`<${Box} type=${t} id=${id} step=${p.step != null ? fitStep(p) : "any"} min=${lo} max=${hi} maxlength=${t === "text" ? (expr ? 2000 : 200) : null}
      aria-describedby=${describedby} placeholder=${p.placeholder || (p.optional ? "(default)" : null)} class=${expr ? "expr" : null} spellcheck=${expr ? "false" : null}
      value=${value} onValue=${(v, el) => {
        if (t === "number") {
          if (v === "" && p.optional) set(null);
          else if (v !== "" && el.checkValidity()) set(+v);
        } else set(v === "" && p.optional ? null : v);
      }} />`];
  }
  if (needed) cls.push("needed");
  if (changed) cls.push("changed");
  if (hidden) cls.push("cond-hidden");
  if (searchHidden) cls.push("search-hidden");
  // the author's suggestions for this setting (a drop-down beside it)
  const suggest = p.presets ? html`<${Pick} class="presets" aria-label=${`${p.presets.label} for ${label}`}
    index=${p.presets.values.findIndex((x) => same(x.value, value)) + 1}
    onChange=${(e) => { const pick = p.presets.values[e.target.selectedIndex - 1]; if (pick) set(structuredClone(pick.value)); }}>
    <option>${p.presets.label}…</option>${p.presets.values.map((v) => html`<option>${v.label}</option>`)}</${Pick}>` : null;
  return html`<div class=${cls.join(" ")} data-name=${p.name} data-search=${`${p.name} ${label} ${p.description || ""}`.toLowerCase()}>
    ${body}${suggest}${help}</div>`;
}

/** The settings' display conditions, compiled once per model: name → test(values). */
export function compileConditions(model) {
  const names = model.parameters.map((p) => p.name).filter((n) => /^[A-Za-z_$][\w$]*$/.test(n));
  return { names, tests: new Map(model.parameters.filter((p) => p.show_if).map((p) => [p.name, compileCondition(p.show_if, names)])) };
}

/**
 * The form. `values`: the settings now; `q`: Find a setting; `bar`: the saved-settings bar's props
 * (web/ui/savedsettings.js); `set(p, value)`: a setting changed.
 */
export function ModelForm({ model, values, q, bar, set }) {
  const { parameters, groups, tabs = {} } = model;
  const conditions = useMemo(() => compileConditions(model), [model]);
  // groups opened or closed here; the others as they start: the first two (or all of three or
  // fewer), not those the author folds, and one with a switch while it's on
  const [open, setOpen] = useState({});
  const needs = model.needs || [];
  const query = (q || "").trim().toLowerCase();
  return html`<${SettingsBar} ...${bar} />${groups.map((g, gi) => {
    const tab = tabs[g] || {};
    const fields = parameters.filter((p) => p.group === g && !p.hidden && p.name !== tab.control);
    const control = tab.control && parameters.find((p) => p.name === tab.control);
    if (!fields.length && !control) return null;
    const shown = fields.map((p) => {
      const label = (p.label && p.label !== p.name ? p.label : humanize(p.name));
      const hit = !query || `${p.name} ${label} ${p.description || ""}`.toLowerCase().includes(query);
      return { p, hidden: !(conditions.tests.get(p.name)?.(values) ?? true), searchHidden: !hit };
    });
    // a group whose settings are all switched off by conditions disappears too; one with nothing found is hidden
    const allOff = !control && shown.length > 0 && shown.every((f) => f.hidden);
    const found = shown.some((f) => !f.searchHidden && !f.hidden);
    const isOpen = control ? !!values[control.name] && open[g] !== false
      : query && found ? true : g in open ? open[g] : !tab.collapsed && (gi < 2 || groups.length <= 3);
    const changedHere = shown.filter((f) => !same(values[f.p.name], f.p.default)).length;
    return html`<details class=${`group${allOff ? " cond-hidden" : ""}`} data-group=${g} open=${isOpen} hidden=${!!query && !found}
      onToggle=${(e) => { if (e.target.open !== isOpen) setOpen({ ...open, [g]: e.target.open }); }}>
      <summary>${control ? html`<input type="checkbox" class="group-switch" aria-label=${`Use ${g.toLowerCase()}`} checked=${!!values[control.name]}
          onClick=${(e) => e.stopPropagation()} onChange=${(e) => { setOpen({ ...open, [g]: e.target.checked }); set(control, e.target.checked); }} />` : null}<span
        class="group-title">${g === "Parameters" ? "Settings" : g}</span>${tab.help_link ? html`<a class="help-link" href=${tab.help_link} target="_blank" rel="noopener"
        title="Documentation" aria-label=${`Documentation for ${g}`} onClick=${(e) => e.stopPropagation()}>?</a>` : null}<span class="changed-count">${changedHere ? `${changedHere} changed` : ""}</span></summary>
      <div class="group-body">
        ${tab.description_html ? html`<${SafeHTML} class="group-note" html=${tab.description_html} />` : null}
        ${shown.map(({ p, hidden, searchHidden }) => html`<${Field} key=${p.name} p=${p} value=${values[p.name]} needed=${needs.includes(p.name)}
          hidden=${hidden} searchHidden=${searchHidden} set=${(v) => set(p, v)} />`)}
      </div>
    </details>`;
  })}`;
}
