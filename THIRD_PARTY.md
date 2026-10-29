# Third-party sources

Everything under `vendor/` is third-party work, vendored unmodified at the commits below. Each directory keeps its own LICENSE/README; those govern. Generated models may carry obligations (attribution, share-alike, non-commercial) — the site must show attribution and license per model.

| Project | Authors | Upstream @ commit | License | Path |
| --- | --- | --- | --- | --- |
| Cullenect Label | CullenJWebb | [CullenJWebb/Cullenect-Labels](https://github.com/CullenJWebb/Cullenect-Labels) @ `d6dc0bb` | MIT | `vendor/cullenect-label` |
| Gridfinity Basket | LeKoYa | [LeKoYa/gridfinity-basket-openscad](https://github.com/LeKoYa/gridfinity-basket-openscad) @ `549dc40` | MIT | `vendor/gridfinity-basket` |
| Gridfinity Extended | ostat | [ostat/gridfinity_extended_openscad](https://github.com/ostat/gridfinity_extended_openscad) @ `94d5a3e` | GPL-3.0-only | `vendor/gridfinity-extended` |
| Gridfinity Rebuilt | kennetek | [kennetek/gridfinity-rebuilt-openscad](https://github.com/kennetek/gridfinity-rebuilt-openscad) @ `910e22d` | MIT | `vendor/gridfinity-rebuilt` |
| Gridfinity Rugged Box | smkent | [smkent/monoscad](https://github.com/smkent/monoscad) @ `069891b` | CC-BY-SA-4.0 AND MIT | `vendor/monoscad` |
| GridFlock | yawkat | [yawkat/GridFlock](https://github.com/yawkat/GridFlock) @ `9788b3c` | MIT AND CC-BY-4.0 | `vendor/gridflock` |
| Honeycomb Storage Wall | Xander, Edwin Eefting, geru (continuation) | [geru/3d-scad-hsw-customizable](https://github.com/geru/3d-scad-hsw-customizable) @ `b606f2e` | CC-BY-4.0 | `vendor/hsw` |
| openGrid | David D (openGrid design), Andy Levesque / QuackWorks | [AndyLevesque/QuackWorks](https://github.com/AndyLevesque/QuackWorks) @ `e0c1cb7` | CC-BY-NC-SA-4.0 | `vendor/quackworks` |
| Underware for openGrid | Pedro Leite, Underware design by Hands on Katie | [pleite/Monokini](https://github.com/pleite/Monokini) @ `fa601de` | NOASSERTION | `vendor/monokini` |
| gridflock-rebuilt-dependency (library) | — | [kennetek/gridfinity-rebuilt-openscad](https://github.com/kennetek/gridfinity-rebuilt-openscad) @ `910e22d` | MIT | `vendor/gridflock/gridfinity-rebuilt-openscad` |
| rugged-box-rebuilt-dependency (library) | — | [kennetek/gridfinity-rebuilt-openscad](https://github.com/kennetek/gridfinity-rebuilt-openscad) @ `0b7bf6e` | MIT | `vendor/monoscad/libraries/gridfinity-rebuilt-openscad` |
| bosl2 (library) | — | [BelfrySCAD/BOSL2](https://github.com/BelfrySCAD/BOSL2) @ `e173fa0` | BSD-2-Clause; retain file-level notices | `vendor/libraries/BOSL2` |
| Gridfinity Bin for Pred Labels | 3DLG; base by ABDELat; gridfinity-rebuilt by kennetek | owner-supplied file (2026-10-03) | CC-BY-NC-SA (base, per header) | `vendor/pred-label-bin` |
| Gridfinity Kitchen | ostat (Gridfinity Extended) | owner-supplied ZIP (2026-10-03) + 1 file from ostat/gridfinity_extended_openscad @ `ee7d25f` | MIT | `vendor/gridfinity-kitchen` |

## Libraries shipped with the desktop app

Bundled unmodified at these commits (`libraries.json`, fetched and hash-checked by `tools/fetch_libraries.py`; only the OpenSCAD and data files, with each library's license and readme). Their modules are the app's Components; each component shows its library's license and authors.

| Library | Authors | Upstream @ commit | License |
| --- | --- | --- | --- |
| BOSL2 | Revar Desmera, Adrian Mariano and contributors | [BelfrySCAD/BOSL2](https://github.com/BelfrySCAD/BOSL2) @ `e173fa0` | BSD-2-Clause |
| MCAD | OpenSCAD MCAD contributors | [openscad/MCAD](https://github.com/openscad/MCAD) @ `bd0a7ba` | LGPL-2.1 (some files more permissive; see their headers) |
| NopSCADlib | Chris Palmer (nophead) | [nophead/NopSCADlib](https://github.com/nophead/NopSCADlib) @ `00ec289` | GPL-3.0-or-later |
| Round-Anything | Kurt Hutten | [Irev-Dev/Round-Anything](https://github.com/Irev-Dev/Round-Anything) @ `061fef7` | MIT |
| threads-scad | Ryan A. Colyer | [rcolyer/threads-scad](https://github.com/rcolyer/threads-scad) @ `4ae9aeb` | CC0-1.0 |

Notes
- Models made from NopSCADlib (GPL) and MCAD (LGPL) components: the license applies to the code; check it before sharing the generated files commercially.
- `catalog/components.json` holds start values for modules whose docs have no example (written for this project).

## MitCad's `.f3d` reader (compiled into the desktop app)

| Project | Authors | Upstream @ commit | License | Path |
| --- | --- | --- | --- | --- |
| MitCad (the `.f3d` reader crates only) | MitCad contributors | [mitcad/Mitcad](https://github.com/mitcad/Mitcad) @ `eba5e3b` | MIT | `desktop/core/mitcad` |

Eight Rust crates of MitCad's reader for Fusion 360 files, copied at the pinned commit by `tools/fetch_mitcad.py` with one patch (`desktop/core/mitcad/patches/`, waiting to go upstream); see `desktop/core/mitcad/README.md`. MitCad itself (the application and `mitcad-cli`, which the app runs to generate from an `.f3d`) is installed by the user and isn't shipped.

## Site runtime

| Component | Use | License |
| --- | --- | --- |
| [OpenSCAD](https://github.com/openscad/openscad) 2026.10.02 WebAssembly build | Renders every model in the browser; unmodified official snapshot (`engine.json`) | GPL-2.0-or-later (`assets/engine/COPYING`) |
| [three.js](https://github.com/mrdoob/three.js) r186 | 3D preview (`web/vendor/three`) | MIT |
| [Preact](https://github.com/preactjs/preact) 10.27.2 @ `0dbe636` | Interface components (`web/vendor/preact`; copied by `tools/vendor_preact.py`, only import paths changed) | MIT |
| [htm](https://github.com/developit/htm) @ `d62dcfd` | JSX-like templates without a build step (`web/vendor/htm`) | Apache-2.0 |
| [Archivo](https://github.com/google/fonts/tree/main/ofl/archivo) | Interface font of the Classic skin (bundled into the app by `tools/build_desktop.py --fetch-fonts`) | SIL OFL 1.1 |
| [Overpass](https://github.com/google/fonts/tree/main/ofl/overpass) and [Overpass Mono](https://github.com/google/fonts/tree/main/ofl/overpassmono) | Fonts of the Drafting Sheet skin (bundled the same way) | SIL OFL 1.1 |
| [Sofia Sans](https://github.com/google/fonts/tree/main/ofl/sofiasans), [Condensed](https://github.com/google/fonts/tree/main/ofl/sofiasanscondensed) and [Extra Condensed](https://github.com/google/fonts/tree/main/ofl/sofiasansextracondensed) | Fonts of the Parts Bin skin (bundled the same way) | SIL OFL 1.1 |
| [Atkinson Hyperlegible](https://github.com/google/fonts/tree/main/ofl/atkinsonhyperlegible) and [M PLUS Rounded 1c](https://github.com/google/fonts/tree/main/ofl/mplusrounded1c) (ExtraBold, cut to Latin letters) | Fonts of the Layer Line skin (bundled the same way) | SIL OFL 1.1 |
| Liberation Sans / Mono | Fonts for text() in the browser engine (`assets/fonts`) | SIL OFL 1.1 |
| [web-openscad-editor](https://github.com/yawkat/web-openscad-editor) | Its `editor.toml` format is read for form metadata; its build approach (dependency discovery, content-addressed files, fonts) informed `tools/build_site.py`. No code copied. | MIT |

## Part libraries

| Library | Author | Source | License | Path |
| --- | --- | --- | --- | --- |
| openGrid parts | David D | [Printables 1214361](https://www.printables.com/model/1214361-opengrid-walldesk-mounting-framework-and-ecosystem) | CC-BY-4.0 | `libraries/opengrid-official` |
| Label Generator for Gridfinity | Laurens Guijt | [laurensguijt/Label-Generator-Gridfinity](https://github.com/laurensguijt/Label-Generator-Gridfinity) @ `350b406` | GPL-3.0 | `vendor/label-generator-gridfinity` |
| Gridfinity Storage Box Label | Maurice Kevenaar | owner-supplied file (2026-10-03) | CC-BY-4.0 | `vendor/kevenaar-box-label` |
| Gridfinity Screw Label | Nadia Santalla | owner-supplied file (2026-10-03) | GPL-3.0-or-later | `vendor/santalla-screw-label` (+ `adapters/santalla-screw-label`) |
| bosl (library, v1) | Revar Desmera | [revarbat/BOSL](https://github.com/revarbat/BOSL) @ `4ce427a` | BSD-2-Clause | `vendor/libraries/BOSL` |

Notes
- openGrid code: CC-BY-NC-SA-4.0 (non-commercial); file headers license generated parts CC-BY-4.0.
- Underware/Monokini: root LICENSE says AGPL-3.0, README and SCAD headers say CC-BY-NC-SA-4.0. Unresolved — kept disabled for public use.
- Gridfinity Extended (GPL-3.0): offer the SCAD source with generated files (source bundle feature).
- Gridfinity Rugged Box (CC-BY-SA-4.0): share-alike applies to derivatives.
- Honeycomb Storage Wall: original work by Xander and Edwin Eefting (Printables 380870, 530149), continued by geru.
