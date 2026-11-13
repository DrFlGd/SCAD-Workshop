// The browser: toolbar, filter chips, three views (grid, list, table),
// selection and keyboard handling, and the inspector panel.
import { html, useState, useEffect, useMemo, useRef, useCallback } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, setPref, setLayout, toggleFav } from "./state.js";
import { ctx } from "./context.js";
import { scopeInfo } from "./sidebar.js";
import { Icon } from "./icons.js";
import { Inspector } from "./inspector.js";
import { Home } from "./home.js";
import { LICENSE_TEXT, AGE_TEXT, KINDS, STATUS_TEXT } from "./index-local.js";
import { SourcePanel, sourceById } from "./library.js";
import { CollectionPage } from "./collections.js";
import { plural } from "../lib/util.js";

const LAYOUTS = [["grid", "Grid", Icon.grid], ["list", "List", Icon.list], ["table", "Table", Icon.table]];
const SORTS = [["default", "Best match / project"], ["name", "Name"], ["project", "Project"], ["updated", "Recently updated"], ["settings", "Most settings"], ["license", "License"], ["kind", "Kind"]];
const GROUPS = [["none", "No grouping"], ["project", "Project"], ["cat", "Category"], ["kind", "Kind"], ["license", "License"]];
export const COLUMNS = {
  project: { label: "Project", get: (i) => i.project, sort: "project" },
  categoryLabel: { label: "Category", get: (i) => i.subcategory ? `${i.categoryLabel} › ${i.subcategory}` : i.categoryLabel },
  kind: { label: "Kind", get: (i) => (i.kind === "part" ? "Part" : i.kind === "component" ? "Component" : "Parametric model"), sort: "kind" },
  license: { label: "License", get: (i) => i.license?.spdx && i.license.spdx !== "NOASSERTION" ? i.license.spdx : "Not stated", sort: "license", badge: true },
  settings: { label: "Settings", get: (i) => (i.settings ?? ""), sort: "settings", num: true },
  updated: { label: "Updated", get: (i) => i.updated || "", sort: "updated", nowrap: true },
  authors: { label: "Authors", get: (i) => i.authors.join(", ") },
  tags: { label: "Tags", get: (i) => i.tags.join(", ") },
};
/** Built-in columns plus one per open metadata field ("f:material"). */
function allColumns() {
  const extra = Object.fromEntries((ctx.index?.fieldNames || []).map((n) => [`f:${n}`, { label: n, get: (i) => i.fields[n] ?? "", sort: `f:${n}` }]));
  return { ...COLUMNS, ...extra };
}
const GROUP_KEY = {
  project: (i) => i.project, cat: (i) => i.categoryLabel, kind: (i) => KINDS[i.kind],
  license: (i) => LICENSE_TEXT[i.licenseStatus] || i.licenseStatus,
};
const SIZES = { s: 132, m: 172, l: 236 };
const wide = () => matchMedia("(min-width: 1100px)").matches;

export function openItem(item) {
  if (!item) return;
  location.hash = item.href;
}

/** Query for the current place, search and filters. */
export function useResults() {
  const s = useStore(ui, (st) => ({ scope: st.scope, q: st.q, filters: st.filters, sort: st.sort, favs: st.favs, recent: st.recent, ready: st.ready, v: st.catalogVersion, showHidden: st.showHidden }));
  return useMemo(() => {
    if (!s.ready || s.scope === "home") return null;
    const info = scopeInfo(s.scope);
    const sort = s.sort === "default" ? (info.sort || "relevance") : s.sort;
    const res = ctx.index.query({ text: s.q, scope: { ...info.query, hidden: s.showHidden }, filters: s.filters, sort });
    return { info, ...res };
  }, [s.scope, s.q, s.filters, s.sort, s.favs, s.recent, s.ready, s.v, s.showHidden]);
}

function groupItems(items, group) {
  if (!group || group === "none") return [{ key: null, items }];
  const out = new Map();
  for (const it of items) {
    const k = GROUP_KEY[group](it) || "Other";
    if (!out.has(k)) out.set(k, []);
    out.get(k).push(it);
  }
  return [...out].map(([key, items]) => ({ key, items }));
}

// ---------------------------------------------------------------- small parts
function Thumb({ item, cls = "thumb" }) {
  if (item.thumb) return html`<span class=${cls}><img src=${item.thumb} alt="" loading="lazy" decoding="async" /></span>`;
  if (item.kind === "component") return html`<span class=${`${cls} thumb-none thumb-comp`} aria-hidden="true">${Icon.cog(28)}<small>${item.module}()</small></span>`;
  const letters = item.name.split(/\s+/).map((w) => w[0]).join("").slice(0, 2);
  return html`<span class=${`${cls} thumb-none`} aria-hidden="true">${letters}</span>`;
}

/** (Review 7, item 8) the part badge only where parts and models mix: Recent, Favourites, search, a project */
function Badges({ item, favs, mixed }) {
  return html`${favs.includes(item.id) ? html`<span class="badge-fav" title="Favourite">${Icon.star(12, true)}</span>` : null}
    ${item.broken ? html`<span class="badge-mini broken" title=${item.broken.note ? `Flagged as broken: ${item.broken.note}` : "Flagged as broken"}>broken</span>` : null}
    ${item.hidden ? html`<span class="badge-mini hidden" title="Hidden">hidden</span>` : null}
    ${item.kind === "part" && mixed ? html`<span class="badge-mini">part</span>` : null}
    ${item.needs?.length ? html`<span class="badge-mini" title=${`Set ${item.needs.join(", ")} first`}>needs values</span>` : null}`;
}

/** Close a dropdown on a click outside it or on Escape. */
function useDismiss(open, setOpen, ref) {
  useEffect(() => {
    if (!open) return;
    const click = (e) => { if (!ref.current?.contains(e.target)) setOpen(false); };
    const key = (e) => { if (e.key === "Escape") { e.stopPropagation(); setOpen(false); } };
    document.addEventListener("pointerdown", click);
    document.addEventListener("keydown", key, true);
    return () => { document.removeEventListener("pointerdown", click); document.removeEventListener("keydown", key, true); };
  }, [open]);
}

function FilterMenu({ name, label, values, selected, render = (v) => v, order = null, onChange, total = Infinity }) {
  const [open, setOpen] = useState(false);
  const ref = useRef();
  useDismiss(open, setOpen, ref);
  const entries = Object.entries(values).sort(order
    ? (a, b) => order.indexOf(a[0]) - order.indexOf(b[0])      // a fixed order, e.g. newest first
    : (a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  // a value every item has can't narrow anything (UI review, 0.4.26)
  if ((!entries.length || (entries.length === 1 && entries[0][1] >= total)) && !selected.length) return null;
  const toggle = (v) => onChange(selected.includes(v) ? selected.filter((x) => x !== v) : [...selected, v]);
  return html`<div class="filter" ref=${ref}>
    <button type="button" class=${`chip-btn${selected.length ? " on" : ""}`} aria-expanded=${open ? "true" : "false"} onClick=${() => setOpen(!open)}
      data-filter=${name}>${label}${selected.length ? `: ${selected.length}` : ""} ▾</button>
    ${open ? html`<div class="filter-panel" role="group" aria-label=${label}>
      ${entries.map(([v, n]) => html`<label class="filter-opt"><input type="checkbox" checked=${selected.includes(v.toLowerCase())}
        onChange=${() => toggle(v.toLowerCase())} /> <span>${render(v)}</span><span class="filter-n">${n}</span></label>`)}
      ${selected.length ? html`<button type="button" class="link-btn" onClick=${() => onChange([])}>Clear</button>` : null}
    </div>` : null}
  </div>`;
}

// ---------------------------------------------------------------- virtual rows
function useWindow(scrollRef, count, rowH, enabled) {
  const [win, setWin] = useState({ start: 0, end: count });
  useEffect(() => {
    if (!enabled) { setWin({ start: 0, end: count }); return; }
    const el = scrollRef.current;
    if (!el) return;
    const update = () => {
      const start = Math.max(0, Math.floor(el.scrollTop / rowH) - 8);
      const end = Math.min(count, Math.ceil((el.scrollTop + el.clientHeight) / rowH) + 8);
      setWin((w) => (w.start === start && w.end === end ? w : { start, end }));
    };
    update();
    el.addEventListener("scroll", update, { passive: true });
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => { el.removeEventListener("scroll", update); ro.disconnect(); };
  }, [count, rowH, enabled]);
  return enabled ? win : { start: 0, end: count };
}

// ---------------------------------------------------------------- views
function GridView({ groups, size, sel, favs, mixed, onPick, onOpen, cols }) {
  return groups.map((g) => html`<section class="result-group" key=${g.key || "all"}>
    ${g.key ? html`<h2 class="group-head">${g.key} <span>${g.items.length}</span></h2>` : null}
    <div class="grid-view" style=${`--card:${SIZES[size]}px`}>
      ${g.items.map((it) => html`<div role="option" aria-selected=${sel.includes(it.id) ? "true" : "false"} tabindex="-1"
        class="card" data-item=${it.id} data-hidden=${it.hidden ? "" : null} key=${it.id} onClick=${(e) => onPick(it, e)} onDblClick=${() => onOpen(it)}>
        <${Thumb} item=${it} />
        <span class="card-name">${it.name}</span>
        <span class="card-sub">${it.sub || it.project}</span>
        <span class="card-badges"><${Badges} item=${it} favs=${favs} mixed=${mixed} /></span>
      </div>`)}
    </div>
  </section>`);
}

function ListView({ items, sel, favs, mixed, onPick, onOpen, scrollRef }) {
  const ROW = 52;
  const virt = items.length > 300;
  const w = useWindow(scrollRef, items.length, ROW, virt);
  return html`<div class="list-view" style=${virt ? `padding-top:${w.start * ROW}px;padding-bottom:${(items.length - w.end) * ROW}px` : ""}>
    ${items.slice(w.start, w.end).map((it) => html`<div role="option" aria-selected=${sel.includes(it.id) ? "true" : "false"} tabindex="-1"
      class="row" data-item=${it.id} data-hidden=${it.hidden ? "" : null} key=${it.id} onClick=${(e) => onPick(it, e)} onDblClick=${() => onOpen(it)}>
      <${Thumb} item=${it} cls="thumb thumb-sm" />
      <span class="row-name">${it.name} <${Badges} item=${it} favs=${favs} mixed=${mixed} /></span>
      <span class="row-project">${it.project}</span>
      <span class="row-cat">${it.subcategory || it.categoryLabel}</span>
      <span class="row-tags">${it.tags.slice(0, 4).join(", ")}</span>
    </div>`)}
  </div>`;
}

function TableView({ items, sel, favs, mixed, columns: wanted, sort, onSort, onPick, onOpen, scrollRef }) {
  const ROW = 38;
  const COLUMNS = allColumns();
  const columns = wanted.filter((c) => COLUMNS[c]);
  const virt = items.length > 300;
  const w = useWindow(scrollRef, items.length, ROW, virt);
  const head = (id, label, sortKey, num) => html`<th scope="col" class=${num ? "num" : ""} aria-sort=${sort === sortKey ? "ascending" : null}>
    ${sortKey ? html`<button type="button" class="th-btn" onClick=${() => onSort(sortKey)}>${label}${sort === sortKey ? " ▲" : ""}</button>` : label}</th>`;
  return html`<div class="table-wrap"><table class="table-view">
    <thead><tr>${head("name", "Name", "name")}${columns.map((c) => head(c, COLUMNS[c].label, COLUMNS[c].sort, COLUMNS[c].num))}</tr></thead>
    <tbody>
      ${virt && w.start ? html`<tr aria-hidden="true" style=${`height:${w.start * ROW}px`}><td colspan=${columns.length + 1}></td></tr>` : null}
      ${items.slice(w.start, w.end).map((it) => html`<tr role="option" aria-selected=${sel.includes(it.id) ? "true" : "false"} tabindex="-1"
        data-item=${it.id} data-hidden=${it.hidden ? "" : null} key=${it.id} onClick=${(e) => onPick(it, e)} onDblClick=${() => onOpen(it)}>
        <td class="td-name"><${Thumb} item=${it} cls="thumb thumb-xs" /> ${it.name} <${Badges} item=${it} favs=${favs} mixed=${mixed} /></td>
        ${columns.map((c) => html`<td class=${COLUMNS[c].num ? "num" : COLUMNS[c].nowrap ? "nowrap" : ""}>${COLUMNS[c].badge
          ? html`${COLUMNS[c].get(it)} ${it.licenseStatus !== "ok" ? html`<span class=${`badge ${it.licenseStatus}`}>${LICENSE_TEXT[it.licenseStatus]}</span>` : null}`
          : COLUMNS[c].get(it)}</td>`)}
      </tr>`)}
      ${virt && items.length > w.end ? html`<tr aria-hidden="true" style=${`height:${(items.length - w.end) * ROW}px`}><td colspan=${columns.length + 1}></td></tr>` : null}
    </tbody>
  </table></div>`;
}

/** The icon chosen for a group (a project's or a category's), if any. */
function groupIcon(group, g) {
  const first = g.items[0];
  if (!first) return null;
  if (group === "project") return (ctx.catalog.sources || []).find((x) => x.id === first.projectId)?.icon || (ctx.catalog.families || []).find((f) => f.id === first.projectId)?.icon || null;
  if (group === "cat") return (ctx.catalog.categories || []).find((c) => c.id === first.category)?.icon || null;
  return null;
}

// what a grouping's tiles are called ("‹ All projects")
const GROUP_NOUN = { project: "projects", cat: "categories", kind: "kinds", license: "licenses" };

function CondensedView({ groups, group, size }) {
  const projects = group === "project";
  return html`<div class="grid-view condensed" style=${`--card:${SIZES[size]}px`}>
    ${groups.map((g) => {
      const icon = groupIcon(group, g);
      const thumbs = g.items.filter((i) => i.thumb).slice(0, 4);
      const src = projects ? sourceById(g.items[0]?.projectId) : null;
      return html`<button type="button" class="card group-tile" data-group=${g.key || ""} key=${g.key || "all"}
        onClick=${() => ui.set({ openGroup: g.key, selection: [], anchor: null, sourceTab: "items" })}>
        <span class=${`thumb${icon ? "" : " collage"}${!icon && thumbs.length < 2 ? " single" : ""}`}>${icon ? html`<img src=${icon} alt="" />`
          : thumbs.length ? thumbs.map((t) => html`<img src=${t.thumb} alt="" loading="lazy" />`) : html`<span class="thumb-none">${(g.key || "?").slice(0, 2)}</span>`}</span>
        ${src?.update?.state === "available" ? html`<span class="card-badges"><span class="badge-mini update" title="A newer version is ready to review">update</span></span>` : null}
        <span class="card-name">${g.key || "Other"}</span>
        <span class="card-sub">${plural(g.items.length, "item")}</span>
      </button>`;
    })}
  </div>`;
}

// ---------------------------------------------------------------- browser
export function Browser() {
  const s = useStore(ui, (st) => ({
    scope: st.scope, q: st.q, filters: st.filters, layout: st.layout, size: st.size, sort: st.sort, group: st.group,
    columns: st.columns, selection: st.selection, anchor: st.anchor, favs: st.favs, inspector: st.inspector, ready: st.ready,
    condensed: st.condensed, openGroup: st.openGroup, sourceTab: st.sourceTab, v: st.catalogVersion, showHidden: st.showHidden,
    groupBefore: st.groupBefore,
  }));
  const res = useResults();
  const scrollRef = useRef();
  const listRef = useRef();
  const [cols, setCols] = useState(4);
  const items = res?.items || [];
  // the items on screen, in the order they're shown (by group when grouped; none on the condensed
  // overview; only the open group's inside one): what selecting, arrow keys and Ctrl+A work on
  const shownRef = useRef([]);

  // grid columns, for arrow-key movement
  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const card = SIZES[ui.get().size] + 12;
      setCols(Math.max(1, Math.floor((el.clientWidth + 12) / card)));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [s.layout, s.size, s.scope]);

  // keep the selection (and the anchor for Shift-click and arrow keys) to items still on screen
  useEffect(() => {
    const st = ui.get();
    const shown = new Set(shownRef.current.map((i) => i.id));
    const keep = st.selection.filter((id) => shown.has(id));
    const patch = {};
    if (keep.length !== st.selection.length) patch.selection = keep;
    if (st.anchor && !shown.has(st.anchor)) patch.anchor = keep[keep.length - 1] || null;
    if (Object.keys(patch).length) ui.set(patch);
  }, [items, s.condensed, s.openGroup, s.group, s.layout]);

  const onPick = useCallback((it, e) => {
    const st = ui.get();
    if (!wide() && !e?.ctrlKey && !e?.metaKey && !e?.shiftKey) { openItem(it); return; } // no inspector on small screens
    let selection = [it.id];
    if (e?.ctrlKey || e?.metaKey) selection = st.selection.includes(it.id) ? st.selection.filter((x) => x !== it.id) : [...st.selection, it.id];
    else if (e?.shiftKey && st.anchor) {
      const ids = shownRef.current.map((i) => i.id);
      const a = ids.indexOf(st.anchor), b = ids.indexOf(it.id);
      if (a >= 0 && b >= 0) selection = ids.slice(Math.min(a, b), Math.max(a, b) + 1);
    }
    ui.set({ selection, anchor: e?.shiftKey ? st.anchor : it.id });
    listRef.current?.querySelector(`[data-item="${CSS.escape(it.id)}"]`)?.focus({ preventScroll: true });
  }, [items]);

  const onKey = (e) => {
    if (e.target.closest("input, select, textarea, .filter-panel")) return;
    const st = ui.get();
    if (st.condensed && st.group !== "none" && !st.openGroup) return; // group tiles are plain buttons (Tab, Enter)
    const ids = shownRef.current.map((i) => i.id);
    const cur = Math.max(0, ids.indexOf(st.anchor ?? ids[0]));
    const step = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: s.layout === "grid" ? cols : 1, ArrowUp: s.layout === "grid" ? -cols : -1 }[e.key];
    if (step && ids.length) {
      e.preventDefault();
      const next = Math.min(ids.length - 1, Math.max(0, (st.anchor ? cur + step : 0)));
      const id = ids[next];
      const selection = e.shiftKey ? [...new Set([...st.selection, id])] : [id];
      ui.set({ selection, anchor: id });
      const node = listRef.current?.querySelector(`[data-item="${CSS.escape(id)}"]`);
      node?.focus({ preventScroll: true });
      node?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && st.anchor) { e.preventDefault(); openItem(ctx.index.get(st.anchor)); }
    else if (e.key === " " && st.anchor) { e.preventDefault(); ui.set({ quicklook: st.anchor }); }
    else if ((e.key === "f" || e.key === "F") && st.selection.length) { e.preventDefault(); toggleFav(st.selection); }
    else if (e.key === "Escape") ui.set({ selection: [], anchor: null });
    else if ((e.ctrlKey || e.metaKey) && e.key === "a") { e.preventDefault(); ui.set({ selection: ids }); }
  };

  if (!s.ready) return html`<div class="browse-loading">Loading…</div>`;
  if (s.scope === "home") return html`<${Home} />`;
  if (s.scope.startsWith("coll:")) return html`<${CollectionPage} id=${s.scope.slice(5)} key=${s.scope} />`;

  const info = res.info;
  const setFilter = (k) => (vals) => ui.set({ filters: { ...ui.get().filters, [k]: vals } });
  const allGroups = groupItems(items, s.group);
  const condensing = s.condensed && s.group !== "none";
  const overview = condensing && !s.openGroup;
  const groups = condensing && s.openGroup ? allGroups.filter((g) => g.key === s.openGroup) : allGroups;
  const shownItems = condensing && s.openGroup ? groups.flatMap((g) => g.items) : items;
  // the table lists items as sorted; the other views by group
  shownRef.current = overview ? [] : s.layout === "table" ? shownItems : groups.flatMap((g) => g.items);
  // a project opened from its tile shows the project's page (details, files, versions, updates)
  const openProject = condensing && s.openGroup && s.group === "project" ? groups[0]?.items[0]?.projectId : null;
  const panelSource = info.source || (openProject && sourceById(openProject) ? openProject : null);
  const sourceHidden = panelSource && s.sourceTab && s.sourceTab !== "items";
  const props = { sel: s.selection, favs: s.favs, mixed: new Set(items.map((i) => i.kind)).size > 1, onPick, onOpen: openItem, scrollRef };
  const title = s.q ? `“${s.q}”` : info.label;
  const typed = Object.entries(res.parsed?.filters || {});
  const noun = GROUP_NOUN[s.group] || "groups";
  const countText = overview && items.length ? `${plural(allGroups.length, noun === "categories" ? "category" : noun.slice(0, -1), noun)} · ${plural(items.length, "item")}`
    : plural(shownItems.length, "item");
  const condense = () => {
    if (!condensing) {
      // one tile per group: nothing selected on the overview. With no grouping, group by project, and
      // remember that, so turning Condense off goes back to no grouping
      if (s.group === "none") setPref({ group: "project", condensed: true, groupBefore: "none" });
      else setPref({ condensed: true, groupBefore: null });
      ui.set({ openGroup: null, sourceTab: "items", selection: [], anchor: null });
    } else {
      // back to the list: the grouping you had before, and what was selected inside an open group stays selected
      setPref({ condensed: false, ...(s.groupBefore ? { group: s.groupBefore, groupBefore: null } : {}) });
      ui.set({ openGroup: null, sourceTab: "items" });
    }
  };
  // the inspector is shown while something is selected (UI review, 0.4.26)
  const inspecting = s.inspector && s.selection.length > 0;
  // a project with a few items needs no sorting, grouping or filters (UI review, 0.4.26)
  const few = !!info.source && !s.q && items.length <= 12 && !Object.values(s.filters).some((v) => v?.length) && !res.hidden && !s.showHidden;
  return html`<div class=${`browser${inspecting ? " with-inspector" : ""}`}>
    <div class="browse-main">
      <div class="browse-head">
        <div class="browse-title">
          <h1>${title}</h1>
          <span class="browse-count">${countText}${s.q ? ` · ${res.ms < 1 ? "<1" : Math.round(res.ms)} ms` : ""}</span>
        </div>
        <div class="browse-views">
          ${few ? null : html`<div class="seg" role="group" aria-label="View">
            ${LAYOUTS.map(([v, l, ic]) => html`<button type="button" aria-pressed=${s.layout === v ? "true" : "false"} title=${`${l} view`}
              aria-label=${`${l} view`} data-layout=${v} onClick=${() => setLayout(v)}>${ic(16)}</button>`)}
          </div>`}
          <button type="button" class="tool-btn inspector-btn" aria-pressed=${s.inspector ? "true" : "false"} title="Show or hide the inspector"
            aria-label="Inspector" onClick=${() => setPref({ inspector: !s.inspector })}>${Icon.panel(16)}</button>
        </div>
      </div>
      ${condensing && s.openGroup ? html`<p class="group-crumb"><button type="button" class="link-btn" data-crumb
        onClick=${() => ui.set({ openGroup: null, selection: [], anchor: null, sourceTab: "items" })}>‹ All ${noun}</button> / <b>${s.openGroup}</b></p>` : null}
      ${panelSource ? html`<${SourcePanel} id=${panelSource} key=${panelSource} />` : null}
      ${info.note ? html`<p class="browse-note">${info.note}</p>` : null}
      ${sourceHidden || few ? null : html`<div class="browse-tools">
        <label class="tool-select"><span class="visually-hidden">Sort</span>
          <select value=${s.sort} onChange=${(e) => setPref({ sort: e.target.value })} aria-label="Sort">
            ${SORTS.map(([v, l]) => html`<option value=${v}>${v === "default" ? "Sort: " + (s.q ? "best match" : "default") : "Sort: " + l}</option>`)}
          </select></label>
        <label class="tool-select"><span class="visually-hidden">Group</span>
          <select value=${s.group} onChange=${(e) => { setPref({ group: e.target.value, groupBefore: null }); ui.set({ openGroup: null, sourceTab: "items" }); }} aria-label="Group">
            ${GROUPS.map(([v, l]) => html`<option value=${v}>${v === "none" ? l : "Group: " + l}</option>`)}
          </select></label>
        <button type="button" class="tool-btn condense-btn" aria-pressed=${condensing ? "true" : "false"} data-condense
          title=${s.group === "none" ? "Group by project and show each project as one tile" : `Show each group as one tile (${noun})`}
          onClick=${condense}>${Icon.stack(15)}<span>Condense</span></button>
        ${s.layout === "grid" ? html`<div class="seg size-seg" role="group" aria-label="Thumbnail size">
          ${["s", "m", "l"].map((k) => html`<button type="button" aria-pressed=${s.size === k ? "true" : "false"} onClick=${() => setPref({ size: k })}
            title=${{ s: "Small", m: "Medium", l: "Large" }[k] + " thumbnails"}>${k.toUpperCase()}</button>`)}
        </div>` : null}
      </div>`}
      ${sourceHidden || few ? null : html`<div class="filters">
        ${!info.query.kind ? html`<${FilterMenu} total=${items.length} name="kind" label="Kind" values=${res.facets.kind} selected=${s.filters.kind || []}
          render=${(v) => KINDS[v] || v} onChange=${setFilter("kind")} />` : null}
        ${!info.query.category ? html`<${FilterMenu} total=${items.length} name="cat" label=${info.query.kind === "component" ? "Topic" : "Category"} values=${res.facets.cat} selected=${s.filters.cat || []} onChange=${setFilter("cat")} />` : null}
        ${!info.query.project ? html`<${FilterMenu} total=${items.length} name="project" label=${info.query.kind === "component" ? "Library" : "Project"} values=${res.facets.project} selected=${s.filters.project || []} onChange=${setFilter("project")} />` : null}
        <${FilterMenu} total=${items.length} name="license" label="License" values=${res.facets.license} selected=${s.filters.license || []}
          render=${(v) => LICENSE_TEXT[v] || v} onChange=${setFilter("license")} />
        ${Object.keys(res.facets.sub || {}).length > 1 || s.filters.sub?.length ? html`<${FilterMenu} total=${items.length} name="sub" label="Type" values=${res.facets.sub}
          selected=${s.filters.sub || []} onChange=${setFilter("sub")} />` : null}
        <${FilterMenu} total=${items.length} name="updated" label="Updated" values=${res.facets.updated} selected=${s.filters.updated || []}
          render=${(v) => AGE_TEXT[v] || v} order=${Object.keys(AGE_TEXT)} onChange=${setFilter("updated")} />
        ${res.facets.status?.broken || s.filters.status?.length ? html`<${FilterMenu} total=${items.length} name="status" label="Status" values=${res.facets.status}
          render=${(v) => STATUS_TEXT[v] || v} order=${Object.keys(STATUS_TEXT)} selected=${s.filters.status || []} onChange=${setFilter("status")} />` : null}
        ${typed.map(([k, vals]) => html`<span class="chip-btn on typed">${k}:${vals.join(",")}</span>`)}
        ${Object.entries(res.facets.fields || {}).sort(([a], [b]) => a.localeCompare(b)).slice(0, 8).map(([k, vals]) => {
          // a field of dates ("Published") is grouped as Updated is (UI-38)
          const dates = Object.keys(vals).every((v) => v in AGE_TEXT);
          return html`<${FilterMenu} total=${items.length} name=${`f:${k}`} label=${k} values=${vals} selected=${s.filters[`f:${k}`] || []} onChange=${setFilter(`f:${k}`)}
            render=${dates ? (v) => AGE_TEXT[v] || v : undefined} order=${dates ? Object.keys(AGE_TEXT) : undefined} />`;
        })}
        ${s.layout === "table" ? html`<${ColumnChooser} columns=${s.columns} />` : null}
        ${res.hidden || s.showHidden ? html`<label class="chip-btn hidden-toggle" title="Items and projects you hid">
          <input type="checkbox" checked=${!!s.showHidden} onChange=${(e) => ui.set({ showHidden: e.target.checked })} data-show-hidden />
          Show hidden${res.hidden ? ` (${res.hidden})` : ""}</label>` : null}
      </div>`}
      ${sourceHidden ? null : html`<div class="browse-scroll" ref=${scrollRef} onKeyDown=${onKey}>
        <div class="results" ref=${listRef} role="listbox" aria-multiselectable="true" aria-label=${`${title}: results`} tabindex="0"
          data-layout=${s.layout}>
          ${!items.length ? html`<p class="empty">${s.q ? `Nothing matches “${s.q}”. Try fewer words, or “bin”, “baseplate”, “label”.` : info.label === "Favourites" ? "No favourites yet. Select something and press F, or use the star in the inspector." : info.label === "Recent" ? "Nothing opened yet." : "Nothing here."}</p>` : null}
          ${condensing && !s.openGroup && items.length ? html`<${CondensedView} groups=${allGroups} group=${s.group} size=${s.size} />` : null}
          ${(!condensing || s.openGroup) && s.layout === "grid" && items.length ? html`<${GridView} groups=${groups} size=${s.size} cols=${cols} ...${props} />` : null}
          ${(!condensing || s.openGroup) && s.layout === "list" && items.length ? groups.map((g) => html`<section class="result-group" key=${g.key || "all"}>
            ${g.key ? html`<h2 class="group-head">${g.key} <span>${g.items.length}</span></h2>` : null}
            <${ListView} items=${g.items} ...${props} /></section>`) : null}
          ${(!condensing || s.openGroup) && s.layout === "table" && items.length ? html`<${TableView} items=${shownItems} columns=${s.columns} sort=${s.sort}
            onSort=${(k) => setPref({ sort: k })} ...${props} />` : null}
        </div>
      </div>`}
    </div>
    ${inspecting ? html`<${Inspector} />` : null}
  </div>`;
}

function ColumnChooser({ columns }) {
  const [open, setOpen] = useState(false);
  const ref = useRef();
  useDismiss(open, setOpen, ref);
  const COLUMNS = allColumns();
  const toggle = (c) => setPref({ columns: columns.includes(c) ? columns.filter((x) => x !== c) : Object.keys(COLUMNS).filter((k) => k === c || columns.includes(k)) });
  return html`<div class="filter" ref=${ref}>
    <button type="button" class="chip-btn" aria-expanded=${open ? "true" : "false"} onClick=${() => setOpen(!open)}>Columns ▾</button>
    ${open ? html`<div class="filter-panel">${Object.entries(COLUMNS).map(([k, c]) => html`<label class="filter-opt">
      <input type="checkbox" checked=${columns.includes(k)} onChange=${() => toggle(k)} /> <span>${c.label}</span></label>`)}</div>` : null}
  </div>`;
}
