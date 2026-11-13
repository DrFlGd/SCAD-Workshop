// Command palette (Ctrl+K): actions and anything in the index, from one box.
import { html, useState, useLayoutEffect, useRef, useMemo } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setLayout, setPref, setTheme, THEMES, setSkin, SKINS } from "./state.js";
import { ctx, scopeHash } from "./context.js";
import { Icon } from "./icons.js";

function commands() {
  const { catalog, platform, index } = ctx;
  const partCats = [...new Map(index.items.filter((i) => i.kind === "part" && !i.hidden).map((i) => [i.category, i.categoryLabel])).entries()];
  const go = (hash) => () => { location.hash = hash; };
  const list = [
    { label: "Go to Home", run: go("#/") },
    { label: "Go to Parametric Models", run: go(scopeHash("all")) },
    { label: "Go to the Parts Library", run: go(scopeHash("parts")) },
    ...((catalog.components || []).length ? [{ label: "Go to Components", hint: "library modules as forms", run: go(scopeHash("components")) },
      ...(catalog.component_groups || []).map((g) => ({ label: `Go to ${g.label} components`, run: go(scopeHash(`ccat:${g.id}`)) }))] : []),
    { label: "Go to Favourites", run: go(scopeHash("favs")) },
    { label: "Go to Recent", run: go(scopeHash("recent")) },
    ...catalog.categories.map((c) => ({ label: `Go to ${c.label}`, run: go(scopeHash(`cat:${c.id}`)) })),
    ...partCats.map(([id, label]) => ({ label: `Go to ${label} parts`, run: go(scopeHash(`pcat:${id}`)) })),
    { label: "Open Settings", hint: "printer profile", run: go("#/settings") },
    { label: "Open Licenses & credits", run: go("#/licenses") },
    { label: "Switch to Grid view", run: () => setLayout("grid") },
    { label: "Switch to List view", run: () => setLayout("list") },
    { label: "Switch to Table view", run: () => setLayout("table") },
    ...SKINS.map(([id, label]) => ({ label: `Skin: ${label}`, run: () => setSkin(id) })),
    ...THEMES.map(([id, label]) => ({ label: id === "system" ? "Theme: follow the system" : `Theme: ${label}`, run: () => setTheme(id) })),
    { label: ui.get().inspector ? "Hide the inspector" : "Show the inspector", run: () => setPref({ inspector: !ui.get().inspector }) },
  ];
  if (!catalog.library?.read_only) list.push({ label: "Add a project", hint: "GitHub, ZIP or folder", run: () => ui.set({ dialog: { type: "add-project" } }) });
  if (!catalog.library?.read_only) list.push({ label: "New collection", hint: "parts and models for one use", run: () => ui.set({ dialog: { type: "collect", entries: [] } }) });
  for (const c of ui.get().collections || []) list.push({ label: `Collection: ${c.name}`, run: () => { location.hash = `#/browse/coll/${c.id}`; } });
  list.push({ label: "Open Library settings", hint: "projects, libraries, categories, trash", run: go("#/library-settings") });
  list.push({ label: "Open the library folder", run: () => platform.workspace.open() });
  list.push({ label: "Clear the render cache", run: () => platform.workspace.clearCache() });
  return list;
}

const matches = (label, q) => {
  const l = label.toLowerCase();
  return q.toLowerCase().split(/\s+/).filter(Boolean).every((w) => l.includes(w));
};

export function Palette() {
  const open = useStore(ui, (s) => s.palette);
  return open ? html`<${PaletteBox} />` : null; // fresh state each time it opens
}

function PaletteBox() {
  const [q, setQ] = useState("");
  const [cur, setCur] = useState(0);
  const input = useRef();
  useLayoutEffect(() => { input.current?.focus(); }, []);
  const rows = useMemo(() => {
    const acts = commands().filter((c) => !q || matches(c.label, q)).slice(0, q ? 6 : 8).map((c) => ({ ...c, type: "action" }));
    const items = q ? ctx.index.query({ text: q, limit: 8 }).items.map((i) => ({ label: i.name, hint: i.kind === "component" ? `${i.project} ${i.module}()` : i.project, thumb: i.thumb, type: "item", run: () => { location.hash = i.href; } })) : [];
    return [...items, ...acts];
  }, [q]);
  const close = () => ui.set({ palette: false });
  const run = (r) => { close(); r?.run(); };
  const onKey = (e) => {
    if (e.key === "Escape") { e.preventDefault(); close(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); setCur((c) => Math.min(rows.length - 1, c + 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setCur((c) => Math.max(0, c - 1)); }
    else if (e.key === "Enter") { e.preventDefault(); run(rows[cur]); }
  };
  let lastType = null;
  return html`<div class="palette-backdrop" onPointerDown=${(e) => { if (e.target === e.currentTarget) close(); }}>
    <div class="palette" role="dialog" aria-modal="true" aria-label="Command palette">
      <div class="palette-input">${Icon.search(18)}
        <input ref=${input} value=${q} placeholder="Type a command or search…" aria-label="Command or search"
          onInput=${(e) => { setQ(e.target.value); setCur(0); }} onKeyDown=${onKey} role="combobox" aria-expanded="true" aria-controls="palette-list"
          aria-activedescendant=${rows.length ? `pal-${cur}` : null} />
      </div>
      <ul class="palette-list" id="palette-list" role="listbox">
        ${rows.map((r, i) => {
          const head = r.type !== lastType ? html`<li class="palette-head" role="presentation">${r.type === "item" ? "Open" : "Actions"}</li>` : null;
          lastType = r.type;
          return html`${head}<li id=${`pal-${i}`} role="option" aria-selected=${i === cur ? "true" : "false"} class="palette-row"
            onPointerEnter=${() => setCur(i)} onClick=${() => run(r)}>
            ${r.type === "item" ? html`<span class="thumb thumb-xs">${r.thumb ? html`<img src=${r.thumb} alt="" />` : null}</span>` : null}
            <span class="palette-label">${r.label}</span>${r.hint ? html`<span class="palette-hint">${r.hint}</span>` : null}</li>`;
        })}
        ${!rows.length ? html`<li class="palette-empty">Nothing matches.</li>` : null}
      </ul>
      <div class="palette-foot"><span>↑↓ move</span><span>Enter run</span><span>Esc close</span></div>
    </div>
  </div>`;
}
