// Licenses & credits (#/licenses, #/licenses/<id>): every project, parts pack and component
// library with its authors and license, then the app's own parts. (Review 7, item 13: moved from
// app.js to Preact.)
import { html, useEffect } from "../lib/html.js";
import { ctx } from "./context.js";
import { spdx, STATUS_TEXT } from "../lib/util.js";

function Row({ id, name, what, authors, license, source, focus }) {
  const use = license?.public_use || "ok";
  return html`<tr id=${`lic-${id}`} class=${id === focus ? "focus" : null}>
    <th scope="row"><div class="lic-name"><b>${name}</b><span class="muted">${what}</span></div></th>
    <td>${authors || "Not stated"}</td>
    <td>${spdx(license)}</td>
    <td><span class=${`badge ${use}`}>${STATUS_TEXT[use]}</span>${license?.notes ? html`<span class="lic-note">${license.notes}</span>` : null}</td>
    <td>${source ? html`<a href=${source} target="_blank" rel="noopener">Source</a>` : "Supplied file"}</td>
  </tr>`;
}

function Table({ rows, focus }) {
  return html`<div class="lic-wrap"><table class="lic-table">
    <thead><tr>${["Project", "Authors", "License", "Status", "Source"].map((h) => html`<th scope="col">${h}</th>`)}</tr></thead>
    <tbody>${rows.map((r) => html`<${Row} key=${r.id} ...${r} focus=${focus} />`)}</tbody>
  </table></div>`;
}

/** The page; `focus`: the id of a row to show (scrolled to and marked). */
export function LicensesPage({ focus }) {
  const c = ctx.catalog;
  const fams = (c?.families || []).filter((f) => f.status !== "missing-source").sort((a, b) => a.name.localeCompare(b.name));
  const review = fams.filter((f) => f.license?.public_use && f.license.public_use !== "ok");
  const names = (authors) => (authors || []).map((a) => a.name).join(", ");
  const components = (c?.component_libraries || []).filter((l) => l.active);
  useEffect(() => {
    const target = focus && document.getElementById(`lic-${focus}`);
    if (target) requestAnimationFrame(() => target.scrollIntoView({ block: "center" }));
    else window.scrollTo(0, 0);
  }, [focus]);
  return html`
    <p>Every model and part here is someone else's open work, credited below with its license. Generated files carry the same license as the project that made them.</p>
    ${review.length ? html`<p class="lic-summary">${review.length} project${review.length > 1 ? "s need" : " needs"} a license check before files are shared or sold: ${review.flatMap((f, i) => [i ? ", " : "", html`<a href=${`#/licenses/${f.id}`}>${f.name}</a>`])}.</p>` : null}
    <h2>Parametric Models</h2>
    <${Table} focus=${focus} rows=${fams.map((f) => ({ id: f.id, name: f.name, what: (f.models || []).join(", "), authors: names(f.authors), license: f.license, source: f.source }))} />
    <h2>Parts Library</h2>
    <${Table} focus=${focus} rows=${(ctx.libraries || []).map((l) => ({ id: l.id, name: l.name, what: `${l.item_count} parts`, authors: names(l.authors), license: l.license, source: l.source_url }))} />
    ${components.length ? html`<h2>Components</h2>
      <${Table} focus=${focus} rows=${components.map((l) => ({ id: `components-${l.name}`, name: l.name, what: l.provider === "project" ? "Your copy" : "Comes with the app",
        authors: (l.authors || []).join(", "), license: { spdx: l.license || "NOASSERTION", public_use: l.license ? "ok" : "review" }, source: l.repo ? `https://github.com/${l.repo}` : null }))} />` : null}
    <h2>This app</h2>
    <ul>
      <li>OpenSCAD ${c?.engine || ""} makes every part on this computer. GNU GPL version 2 or later: <a href="https://github.com/openscad/openscad" target="_blank" rel="noopener">source code</a>, <a href="engine/COPYING" target="_blank">license text</a>.</li>
      <li>3D preview: three.js (MIT). Interface: Preact (MIT) and htm (Apache-2.0).</li>
      <li>Fonts for text on parts: Liberation Sans and Liberation Mono (SIL Open Font License 1.1).</li>
      <li>Nothing you configure is sent to a server; the app works offline, except for adding and updating projects from the web.</li>
    </ul>`;
}
