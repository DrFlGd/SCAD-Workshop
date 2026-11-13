// Sidebar: Home, Recent, Favourites; Parametric Models, Components (desktop:
// library modules by topic) and the Parts Library; Add a project
// (desktop); Settings, Library settings, Licenses. The section being browsed shows
// its categories (each opening to its projects) in a panel attached to the menu,
// which scrolls on its own and folds away to a thin strip. In the small-window
// drawer the categories open under each section instead.
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref } from "./state.js";
import { ctx, scopeHash } from "./context.js";
import { Icon } from "./icons.js";
import { readOnly, sourceById } from "./library.js";
import { categoryLabel } from "./index-local.js";
import { collectionById } from "./collections.js";

/** The name of a project as the index lists it (moved items included). */
function projectName(id, kind) {
  const it = ctx.index.items.find((i) => i.projectId === id && (!kind || i.kind === kind));
  return it?.project || sourceById(id)?.name || ctx.catalog.families?.find((f) => f.id === id)?.name || ctx.libraries?.find((l) => l.id === id)?.name || id;
}

/** A Components topic's name ("Gears & pulleys"). */
export function groupLabel(id) {
  return (ctx.catalog.component_groups || []).find((g) => g.id === id)?.label || categoryLabel(ctx.catalog, id);
}

/** Everything that needs a look: licenses not cleared for sharing, and models flagged as broken. */

export function scopeInfo(scope) {
  const { catalog, index } = ctx;
  const s = ui.get();
  if (scope === "home") return { label: "Home" };
  if (scope === "search") return { label: "Search", query: {} };
  if (scope === "all") return { label: "Parametric Models", query: { kind: "generator" } };
  if (scope === "parts") return { label: "Parts Library", query: { kind: "part" } };
  if (scope === "components") return { label: "Components", query: { kind: "component" },
    note: "Modules from OpenSCAD libraries, as forms: set the values, preview, download. Pin one to keep it under Parametric Models with your settings." };
  if (scope === "favs") return { label: "Favourites", query: { ids: s.favs }, sort: "given" };
  if (scope === "recent") return { label: "Recent", query: { ids: s.recent.map((r) => r.id) }, sort: "given" };
  const [kind, id] = scope.split(":");
  if (kind === "cat") return { label: categoryLabel(catalog, id), query: { kind: "generator", category: id } };
  if (kind === "pcat") return { label: `${categoryLabel(catalog, id)} parts`, query: { kind: "part", category: id } };
  if (kind === "project") return { label: projectName(id, "generator"), query: { kind: "generator", project: id } };
  if (kind === "lib") return { label: projectName(id, "part"), query: { kind: "part", project: id } };
  if (kind === "ccat") return { label: groupLabel(id), query: { kind: "component", category: id } };
  if (kind === "clib") return { label: `${projectName(id, "component")} components`, query: { kind: "component", project: id } };
  if (kind === "coll") return { label: collectionById(id)?.name || "Collection", query: { ids: [] }, sort: "given" };
  if (kind === "source") return { label: sourceById(id)?.name || projectName(id), query: { project: id }, source: id };
  return { label: scope, query: {} };
}

function count(query) {
  return ctx.index.query({ scope: query }).total;
}

/**
 * One section's categories with their projects: [{ id, label, n, projects: [{ id, name, n }] }],
 * in the library's category order ("Other" last). Hidden items don't count.
 */
function tree(kind) {
  const cats = new Map();
  const perProject = new Map();
  for (const i of ctx.index.items) {
    if (i.kind !== kind || i.hidden) continue;
    perProject.set(i.projectId, (perProject.get(i.projectId) || 0) + 1);
    let c = cats.get(i.category);
    if (!c) cats.set(i.category, (c = { id: i.category, label: i.categoryLabel, n: 0, projects: new Map() }));
    c.n++;
    if (!c.projects.has(i.projectId)) c.projects.set(i.projectId, { id: i.projectId, name: i.project });
  }
  const order = (kind === "component" ? ctx.catalog.component_groups || [] : ctx.catalog.category_choices || ctx.catalog.categories || []).map((c) => c.id);
  const rank = (id) => (id === "other" ? 1e6 : order.includes(id) ? order.indexOf(id) : 1e5);
  return [...cats.values()]
    .sort((a, b) => rank(a.id) - rank(b.id) || a.label.localeCompare(b.label))
    .map((c) => ({ ...c, projects: [...c.projects.values()].map((p) => ({ ...p, n: perProject.get(p.id) })).sort((a, b) => a.name.localeCompare(b.name)) }));
}

function Row({ scope, label, n, icon, depth = 0, expand, open, onToggle, current, within, section, toggleEnd }) {
  const active = current === scope;
  const toggle = expand ? html`<button type="button" class="nav-toggle" aria-expanded=${open ? "true" : "false"} aria-label=${`${open ? "Collapse" : "Expand"} ${label}`}
      onClick=${onToggle}>${Icon.chevron(12)}</button>` : null;
  return html`<li class="nav-row" style=${`--depth:${depth}`} data-depth=${depth} data-section-row=${section || null}>
    ${toggleEnd ? null : toggle || html`<span class="nav-toggle-space"></span>`}
    <a href=${scopeHash(scope)} class=${`nav-link${active ? " active" : within ? " within" : ""}`} aria-current=${active ? "page" : null}
      onClick=${() => ui.set({ navOpen: false })} data-scope=${scope}>
      ${icon ? html`<span class="nav-icon">${icon}</span>` : null}<span class="nav-label">${label}</span>${n != null ? html`<span class="nav-count">${n}</span>` : null}
    </a>
    ${toggleEnd ? toggle : null}
  </li>`;
}

/** The three sections: their "everything" place, category and project places, label and icon. */
const SECTION = {
  generator: { cat: "cat", proj: "project", all: "all", label: "Parametric Models", icon: () => Icon.grid(15) },
  component: { cat: "ccat", proj: "clib", all: "components", label: "Components", icon: () => Icon.cog(15) },
  part: { cat: "pcat", proj: "lib", all: "parts", label: "Parts Library", icon: () => Icon.box(15) },
};

/** The section a browse place belongs to (generator, component, part), or null. */
export function sectionOf(scope) {
  if (!scope) return null;
  const head = scope.split(":")[0];
  return Object.keys(SECTION).find((k) => SECTION[k].all === scope || SECTION[k].cat === head || SECTION[k].proj === head) || null;
}

/** A section's categories, each opening to its projects. */
function Categories({ kind, current, open, toggle, depth = 0 }) {
  const { cat: catScope, proj: projScope } = SECTION[kind];
  return html`<ul class="nav-list" data-section=${kind}>
    ${tree(kind).map((c) => {
      const key = `${kind}:${c.id}`;
      return html`<${Row} scope=${`${catScope}:${c.id}`} label=${c.label} n=${c.n} current=${current} key=${key} depth=${depth}
          expand=${true} open=${open[key]} onToggle=${toggle(key)}
          within=${current?.startsWith(`${projScope}:`) && c.projects.some((p) => current === `${projScope}:${p.id}`)} />
        ${open[key] ? c.projects.map((p) => html`<${Row} scope=${`${projScope}:${p.id}`} label=${p.name} depth=${depth + 1}
          n=${p.n} current=${current} key=${`${key}/${p.id}`} />`) : null}`;
    })}
  </ul>`;
}

/** The categories of the section being browsed, attached to the menu; folds to a strip. */
function CategoryPanel({ kind, current, open, toggle, shown }) {
  const { label } = SECTION[kind];
  if (!shown) {
    return html`<aside class="catpanel folded" aria-label=${`${label}: categories`}>
      <button type="button" class="catpanel-toggle" id="catpanel-toggle" aria-expanded="false" title="Show the categories"
        aria-label=${`Show the ${label} categories`} onClick=${() => setPref({ catPanel: true })}>${Icon.chevronsRight(14)}</button>
      <button type="button" class="catpanel-rail" tabindex="-1" aria-hidden="true" onClick=${() => setPref({ catPanel: true })}>${label}</button>
    </aside>`;
  }
  return html`<aside class="catpanel" aria-label=${`${label}: categories`}>
    <div class="catpanel-head"><span>${label}</span>
      <button type="button" class="catpanel-toggle" id="catpanel-toggle" aria-expanded="true" title="Hide the categories"
        aria-label=${`Hide the ${label} categories`} onClick=${() => setPref({ catPanel: false })}>${Icon.chevronsLeft(14)}</button></div>
    <div class="catpanel-list"><${Categories} kind=${kind} current=${current} open=${open} toggle=${toggle} /></div>
  </aside>`;
}

const pageOfHash = () => location.hash.replace(/^#\/?/, "").split(/[/?]/)[0];

export function Sidebar() {
  const s = useStore(ui, (st) => ({ scope: st.scope, view: st.view, favs: st.favs.length, recent: st.recent.length, ready: st.ready, navOpen: st.navOpen, v: st.catalogVersion, catPanel: st.catPanel,
    collections: st.collections || [] }));
  const [open, setOpen] = useState(() => ({}));
  // the page shown (#/settings, #/library-settings…), followed when the address changes by any route
  // (a link on another page, the command palette), not only when the menu itself is clicked
  const [page, setPage] = useState(pageOfHash);
  useEffect(() => {
    const follow = () => setPage(pageOfHash());
    window.addEventListener("hashchange", follow);
    return () => window.removeEventListener("hashchange", follow);
  }, []);
  if (!s.ready) return null;
  const current = s.view === "browse" ? s.scope : null;
  const toggle = (k) => () => setOpen({ ...open, [k]: !open[k] });
  const has = {
    generator: true,
    component: ctx.index.items.some((i) => i.kind === "component"),
    part: ctx.index.items.some((i) => i.kind === "part" && !i.hidden),
  };
  const browsing = sectionOf(current);
  // in the drawer (small windows) each section opens to its categories in place
  const sectionRow = (kind) => {
    const { all, label, icon } = SECTION[kind];
    const key = `section:${kind}`;
    const inline = s.navOpen && (open[key] ?? browsing === kind);
    return html`<${Row} scope=${all} label=${label} n=${count({ kind })} current=${current} icon=${icon()} section=${kind}
        within=${browsing === kind} expand=${s.navOpen} open=${inline} toggleEnd=${true}
        onToggle=${() => setOpen({ ...open, [key]: !inline })} />
      ${inline ? html`<li class="nav-inline"><${Categories} kind=${kind} current=${current} open=${open} toggle=${toggle} depth=${1} /></li>` : null}`;
  };
  // (Add a project sits under the sections it adds to, not under Collections: Review 6, UI-42)
  return html`${s.navOpen ? html`<div class="nav-backdrop" onClick=${() => ui.set({ navOpen: false })}></div>` : null}
  <nav class=${`sidebar${s.navOpen ? " open" : ""}`} aria-label="Library"
    onKeyDown=${(e) => { if (e.key === "Escape" && ui.get().navOpen) ui.set({ navOpen: false }); }}>
    <ul class="nav-list">
      <${Row} scope="home" label="Home" icon=${Icon.home(15)} current=${current} />
      <${Row} scope="recent" label="Recent" n=${s.recent} icon=${Icon.clock(15)} current=${current} />
      <${Row} scope="favs" label="Favourites" n=${s.favs} icon=${Icon.star(15)} current=${current} />
    </ul>
    <ul class="nav-list nav-sections">
      ${Object.keys(SECTION).filter((k) => has[k]).map(sectionRow)}
    </ul>
    ${!readOnly() ? html`<ul class="nav-list nav-add"><li>
      <button type="button" class="nav-link" id="add-project" title="Add a project from GitHub, a ZIP or a folder. Its models and parts appear under Parametric Models and the Parts Library."
        onClick=${() => ui.set({ dialog: { type: "add-project" }, navOpen: false })}><span class="nav-icon">${Icon.plus(15)}</span><span class="nav-label">Add a project</span></button>
    </li></ul>` : null}
    <ul class="nav-list nav-collections" aria-label="Collections">
      <li class="nav-head"><span>Collections</span>${readOnly() ? null : html`<button type="button" class="nav-head-add" id="new-collection" title="New collection" aria-label="New collection"
        onClick=${() => ui.set({ dialog: { type: "collect", entries: [] }, navOpen: false })}>${Icon.plus(13)}</button>`}</li>
      ${s.collections.map((c) => html`<${Row} scope=${`coll:${c.id}`} label=${c.name} n=${c.items.length} icon=${Icon.stack(15)} current=${current} key=${c.id} />`)}
      ${s.collections.length ? null : html`<li class="nav-empty muted">Lists of parts and models for one use, with quantities.</li>`}
    </ul>
    <ul class="nav-list nav-foot">
      <li><a class=${`nav-link${page === "settings" ? " active" : ""}`} href="#/settings" onClick=${() => ui.set({ navOpen: false })}><span class="nav-icon">${Icon.gear(15)}</span><span class="nav-label">Settings</span></a></li>
      <li><a class=${`nav-link${page === "library-settings" ? " active" : ""}`} href="#/library-settings" id="nav-library-settings" onClick=${() => ui.set({ navOpen: false })}><span class="nav-icon">${Icon.shelf(15)}</span><span class="nav-label">Library settings</span></a></li>
      <li><a class=${`nav-link${page === "licenses" ? " active" : ""}`} href="#/licenses" onClick=${() => ui.set({ navOpen: false })}><span class="nav-icon">${Icon.scale(15)}</span><span class="nav-label">Licenses & credits</span></a></li>
    </ul>
  </nav>
  ${browsing && has[browsing] ? html`<${CategoryPanel} kind=${browsing} current=${current} open=${open} toggle=${toggle} shown=${s.catPanel !== false} key=${browsing} />` : null}`;
}
