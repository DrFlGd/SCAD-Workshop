// Inspector: details of the selected item (or what can be done with several).
import { html, useState, useEffect } from "../lib/html.js";
import { useStore } from "../lib/store.js";
import { ui, toggleFav } from "./state.js";
import { ctx, scopeHash } from "./context.js";
import { Icon } from "./icons.js";
import { LICENSE_TEXT } from "./index-local.js";
import { bytes, FORMAT_LABEL, plural } from "../lib/util.js";
import { readOnly, api, when, madeFrom } from "./library.js";
import { ItemActions } from "./actions.js";
import { SideButtons } from "./sideview.js";

const canEdit = () => !readOnly();

function SavedSettings({ item }) {
  const [rows, setRows] = useState(null);
  useEffect(() => {
    let live = true;
    setRows(null);
    ctx.platform.store.settings.list(item.key).then((r) => live && setRows(r), () => live && setRows([]));
    return () => { live = false; };
  }, [item.id]);
  if (!rows?.length) return null;
  return html`<section class="insp-section">
    <h3>Your saved settings</h3>
    <ul class="insp-saved">${rows.map((r) => html`<li key=${r.id}>
      <a href=${`#/m/${item.key}?saved=${encodeURIComponent(r.id)}`}>${r.name}</a>
      <span>${plural(Object.keys(r.values || {}).length, "change")}</span></li>`)}</ul>
  </section>`;
}

function Downloads({ item }) {
  return html`<section class="insp-section"><h3>Files</h3><div class="insp-files">
    ${item.files.map((f) => html`<a class="button ghost" href=${f.url} download=${f.path.split("/").pop()} title=${f.path.split("/").pop()}>
      ${Icon.download(14)} ${f.label ? f.label + ": " : ""}${FORMAT_LABEL[f.format] || f.format} <span class="muted">${bytes(f.bytes)}</span></a>`)}
  </div></section>`;
}

/** A component: a library module. Its details come from the library's docs; pin it to keep your own version. */
function ComponentInspector({ it, fav }) {
  const lib = (ctx.catalog.component_libraries || []).find((l) => l.active && l.name === it.project);
  const lic = it.license?.spdx && it.license.spdx !== "NOASSERTION" ? it.license.spdx : "Not stated";
  return html`<aside class="inspector" aria-label="Inspector" data-item=${it.id}>
    <button type="button" class="insp-thumb" onClick=${() => ui.set({ quicklook: it.id })} title="Quick look (Space)">
      ${it.thumb ? html`<img src=${it.thumb} alt="" />` : html`<span class="thumb-none">${Icon.cog(42)}</span>`}
    </button>
    <div>
      <h2 class="insp-title">${it.name}</h2>
      <p class="insp-sub"><code>${it.module}()</code> from <a href=${scopeHash(`clib:${it.projectId}`)}>${it.project}</a>${lib?.provider === "project" ? " (your copy)" : ""}</p>
    </div>
    <div class="insp-actions">
      <a class="button primary" href=${it.href} data-open=${it.id}>Open</a>
      <button type="button" class="ghost" aria-pressed=${fav ? "true" : "false"} aria-label=${fav ? "Remove from favourites" : "Add to favourites"}
        title="Favourite (F)" onClick=${() => toggleFav([it.id])}>${Icon.star(17, fav)}</button>
      ${it.needs?.length ? null : html`<button type="button" class="ghost" onClick=${() => ui.set({ quicklook: it.id })} title="Quick look (Space)">${Icon.eye(16)} Quick look</button>`}
    </div>
    <div class="insp-actions insp-side"><${SideButtons} modelKey=${it.key} /></div>
    ${it.summary ? html`<p class="insp-summary">${it.summary}</p>` : null}
    ${it.needs?.length ? html`<p class="insp-flag" role="note">Needs values before it can be made: ${it.needs.join(", ")}.</p>` : null}
    <dl class="insp-dl">
      <dt>Kind</dt><dd>Component (a module of ${it.project})</dd>
      <dt>Topic</dt><dd>${it.categoryLabel}</dd>
      ${it.tags.length ? html`<dt>Library topics</dt><dd>${it.tags.join(", ")}</dd>` : null}
      <dt>License</dt><dd>${lic} <a class="insp-more" href="#/licenses">credits</a></dd>
      ${it.settings != null ? html`<dt>Settings</dt><dd>${it.settings}</dd>` : null}
      ${lib ? html`<dt>Library</dt><dd>${lib.title || lib.name}${lib.commit ? html` <span class="muted">@ ${lib.commit.slice(0, 7)}</span>` : null}${lib.provider === "bundled" ? html` <span class="muted">(comes with the app)</span>` : null}</dd>` : null}
    </dl>
    ${canEdit() ? html`<div class="insp-actions">
      <button type="button" class="ghost" data-act="pin" onClick=${() => ui.set({ dialog: { type: "pin", key: it.key } })}>${Icon.pin(15)} Pin as a model…</button>
    </div>` : null}
  </aside>`;
}

export function Inspector() {
  const s = useStore(ui, (st) => ({ selection: st.selection, favs: st.favs, scope: st.scope, group: st.group, v: st.catalogVersion }));
  const sel = s.selection.map((id) => ctx.index.get(id)).filter(Boolean);
  if (!sel.length) {
    return html`<aside class="inspector" aria-label="Inspector"><div class="insp-empty">
      <p>Select something to see its details here.</p>
      <p class="muted">Double-click or Enter opens it. Space shows a quick 3D look. F adds it to your favourites. Ctrl/Shift-click selects several.</p>
    </div></aside>`;
  }
  if (sel.length > 1) {
    const gens = sel.filter((i) => i.kind === "generator" || i.kind === "component");
    const editable = sel.filter((i) => i.kind !== "component");
    const allFav = sel.every((i) => s.favs.includes(i.id));
    return html`<aside class="inspector" aria-label="Inspector">
      <h2 class="insp-title">${sel.length} selected</h2>
      <div class="insp-thumbs">${sel.slice(0, 12).map((i) => i.thumb ? html`<img src=${i.thumb} alt=${i.name} title=${i.name} />` : null)}</div>
      <div class="insp-actions column">
        <button type="button" class="ghost" onClick=${() => toggleFav(sel.map((i) => i.id))}>${Icon.star(15, allFav)} ${allFav ? "Remove from favourites" : "Add to favourites"}</button>
        ${gens.length ? html`<button type="button" class="ghost" onClick=${() => ctx.openTabs(gens.slice(0, 8).map((i) => i.key))}>
          Open ${gens.length > 8 ? "the first 8" : gens.length === 1 ? "it" : `all ${gens.length}`} in tabs</button>` : null}
        <button type="button" class="ghost" onClick=${() => ui.set({ selection: [] })}>Clear selection</button>
      </div>
      ${canEdit() && editable.length ? html`<button type="button" class="ghost" data-act="bulk-edit" onClick=${() => ui.set({ dialog: { type: "edit", level: "bulk", items: editable.map((i) => i.id) } })}>
        ${Icon.edit(15)} Edit details of ${plural(editable.length, "item")}</button>
        <${ItemActions} items=${editable} />` : canEdit() ? null : html`<p class="muted insp-later">This library is read-only.</p>`}
    </aside>`;
  }
  const it = sel[0];
  if (it.kind === "component") return html`<${ComponentInspector} it=${it} fav=${s.favs.includes(it.id)} />`;
  const fav = s.favs.includes(it.id);
  const siblings = it.kind === "generator" ? ctx.index.items.filter((x) => x.kind === "generator" && x.projectId === it.projectId && x.id !== it.id) : [];
  const made = it.kind === "part" ? madeFrom(it.madeFrom) : null; // a part made from a model here (milestone 4)
  const lic = it.license?.spdx && it.license.spdx !== "NOASSERTION" ? it.license.spdx : "Not stated";
  return html`<aside class="inspector" aria-label="Inspector" data-item=${it.id}>
    <button type="button" class="insp-thumb" onClick=${() => ui.set({ quicklook: it.id })} title="Quick look (Space)">
      ${it.thumb ? html`<img src=${it.thumb} alt="" />` : html`<span class="thumb-none">${it.name.slice(0, 2)}</span>`}
    </button>
    <div>
      <h2 class="insp-title">${it.name}</h2>
      <p class="insp-sub"><a href=${scopeHash(`source:${it.projectId}`)}>${it.project}</a>${it.authors.length ? ` · ${it.authors.slice(0, 2).join(", ")}` : ""}</p>
    </div>
    <div class="insp-actions">
      <a class="button primary" href=${it.href} data-open=${it.id}>${it.kind === "part" ? "Open part" : "Open"}</a>
      <button type="button" class="ghost" aria-pressed=${fav ? "true" : "false"} aria-label=${fav ? "Remove from favourites" : "Add to favourites"}
        title="Favourite (F)" onClick=${() => toggleFav([it.id])}>${Icon.star(17, fav)}</button>
      <button type="button" class="ghost" onClick=${() => ui.set({ quicklook: it.id })} title="Quick look (Space)">${Icon.eye(16)} Quick look</button>
    </div>
    <div class="insp-actions insp-side">${it.kind === "part"
      ? html`<${SideButtons} refObj=${{ source: it.sourceId }} />`
      : html`<${SideButtons} modelKey=${it.key} />`}</div>
    ${it.broken ? html`<p class="insp-flag broken" role="note">${Icon.alert(14)} Flagged as broken${it.broken.date ? ` on ${when(it.broken.date)}` : ""}${it.broken.note ? html`: <span>${it.broken.note}</span>` : "."}</p>` : null}
    ${it.hidden ? html`<p class="insp-flag" role="note">${Icon.eye(14)} Hidden${it.hidden === "item" ? "" : it.hidden === "project" ? " with its project" : " with its folder"}: it only shows with “Show hidden”.</p>` : null}
    ${it.summary ? html`<p class="insp-summary">${it.summary}</p>` : null}
    <dl class="insp-dl">
      <dt>Kind</dt><dd>${it.kind === "part" ? "Part" : "Parametric model"}</dd>
      <dt>Category</dt><dd>${it.subcategory ? `${it.categoryLabel} › ${it.subcategory}` : it.categoryLabel}</dd>
      <dt>License</dt><dd>${lic} ${it.licenseStatus !== "ok" ? html`<span class=${`badge ${it.licenseStatus}`}>${LICENSE_TEXT[it.licenseStatus]}</span>` : null}
        <a class="insp-more" href=${`#/licenses/${it.sourceId || it.projectId}`}>details</a></dd>
      ${it.settings != null ? html`<dt>Settings</dt><dd>${it.settings}</dd>` : null}
      ${it.dims ? html`<dt>Size</dt><dd>${it.dims.map((d) => Math.round(d * 10) / 10).join(" × ")} mm</dd>` : null}
      ${it.tags.length ? html`<dt>Tags</dt><dd>${it.tags.join(", ")}</dd>` : null}
      ${it.updated ? html`<dt>Updated</dt><dd>${when(it.updated)} <span class="muted">${it.updatedFrom === "supplied" ? "(files supplied)" : it.updatedFrom === "upstream" ? "(version in use)" : ""}</span></dd>` : null}
      ${Object.entries(it.fields || {}).map(([k, v]) => html`<dt>${k}</dt><dd>${String(v)}</dd>`)}
    </dl>
    ${it.kind === "generator" ? html`<${SavedSettings} item=${it} />` : null}
    ${it.kind === "part" && it.files?.length ? html`<${Downloads} item=${it} />` : null}
    ${made ? html`<p class="insp-note" id="insp-made-from">${made.text} ${made.available ? html`<a href=${`${made.href}?part=${it.key}`}>Open with these settings</a>` : "That model isn't in the library now."}</p>
      ${made.outdated ? html`<p class="insp-note">${made.outdated}</p>` : null}`
    : it.generator ? html`<p class="insp-note"><a href=${`#/m/${it.generator}`}>Open the parametric model</a> to make it at any size.</p>` : null}
    ${siblings.length ? html`<section class="insp-section"><h3>More from ${it.project}</h3>
      <div class="insp-chips">${siblings.slice(0, 16).map((x) => html`<button type="button" class="chip" onClick=${() => ui.set({ selection: [x.id], anchor: x.id })}>${x.name}</button>`)}</div>
    </section>` : null}
    ${canEdit() ? html`<div class="insp-actions">
      <button type="button" class="ghost" data-act="edit" onClick=${() => ui.set({ dialog: { type: "edit", level: "item", item: it.id } })}>${Icon.edit(15)} Edit details</button>
    </div>
    <${ItemActions} items=${[it]} />` : null}
  </aside>`;
}
