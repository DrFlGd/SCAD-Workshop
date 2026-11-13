# Third-party sources

The app ships OpenSCAD, five OpenSCAD libraries, three.js, Preact, htm and fonts (below). The tests use two third-party projects, stored unmodified in `tests/old-library/` (their licences are in its README; those govern), one third-party file in `tests/fixtures/`, and three projects that CI's bench adds from GitHub at pinned commits (not stored here). `tests/projects/` was written for the tests (CC0 1.0). Until Review 7 (item 12) this repository also held the website's generators and parts packs (`vendor/`, `libraries/`, `adapters/`); they're listed in this file at tag `v0.4.30`.

## Test projects

| Project | Authors | Upstream @ commit | License | Where |
| --- | --- | --- | --- | --- |
| Gridfinity Basket | LeKoYa | [LeKoYa/gridfinity-basket-openscad](https://github.com/LeKoYa/gridfinity-basket-openscad) @ `549dc40` | MIT | `tests/old-library/sources/gridfinity-basket` |
| Honeycomb Storage Wall | Xander, Edwin Eefting, geru (continuation) | [geru/3d-scad-hsw-customizable](https://github.com/geru/3d-scad-hsw-customizable) @ `b606f2e` | CC-BY-4.0 | `tests/old-library/sources/hsw` |
| openGrid (its Printables page, as a PDF) | David D | [Printables 1214361](https://www.printables.com/model/1214361-opengrid-walldesk-mounting-framework-and-ecosystem), supplied by the owner 2026-10-03 | CC-BY-4.0 | `tests/fixtures/printables-opengrid.pdf` |
| Gridfinity Extended | ostat | [ostat/gridfinity_extended_openscad](https://github.com/ostat/gridfinity_extended_openscad) @ `94d5a3e` | GPL-3.0-only | added by CI's bench, not stored |
| Gridfinity Rebuilt | kennetek | [kennetek/gridfinity-rebuilt-openscad](https://github.com/kennetek/gridfinity-rebuilt-openscad) @ `910e22d` | MIT | added by CI's bench, not stored |
| Label Generator for Gridfinity | Laurens Guijt | [laurensguijt/Label-Generator-Gridfinity](https://github.com/laurensguijt/Label-Generator-Gridfinity) @ `350b406` | GPL-3.0 | added by CI's bench, not stored |

The acceptance test also adds projects from GitHub (Gridfinity Rebuilt by vector76, chrisspen/gears, LEGO.scad, BOSL2); they aren't stored here either.

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
- `desktop/core/data/components.json` holds start values for modules whose docs have no example (written for this project).

## App runtime

| Component | Use | License |
| --- | --- | --- |
| [OpenSCAD](https://github.com/openscad/openscad) 2026.10.02 | Renders every model; the unmodified official snapshot for Linux and Windows, bundled with the app (`engine.json`, `tools/fetch_native_engine.py`) | GPL-2.0-or-later (`assets/engine/COPYING`) |
| [three.js](https://github.com/mrdoob/three.js) r186 | 3D preview (`web/vendor/three`) | MIT |
| [Preact](https://github.com/preactjs/preact) 10.27.2 @ `0dbe636` | Interface components (`web/vendor/preact`; copied by `tools/vendor_preact.py`, only import paths changed) | MIT |
| [htm](https://github.com/developit/htm) @ `d62dcfd` | JSX-like templates without a build step (`web/vendor/htm`) | Apache-2.0 |
| [Archivo](https://github.com/google/fonts/tree/main/ofl/archivo) | Interface font of the Classic skin (bundled into the app by `tools/build_desktop.py --fetch-fonts`) | SIL OFL 1.1 |
| [Overpass](https://github.com/google/fonts/tree/main/ofl/overpass) and [Overpass Mono](https://github.com/google/fonts/tree/main/ofl/overpassmono) | Fonts of the Drafting Sheet skin (bundled the same way) | SIL OFL 1.1 |
| [Sofia Sans](https://github.com/google/fonts/tree/main/ofl/sofiasans), [Condensed](https://github.com/google/fonts/tree/main/ofl/sofiasanscondensed) and [Extra Condensed](https://github.com/google/fonts/tree/main/ofl/sofiasansextracondensed) | Fonts of the Parts Bin skin (bundled the same way) | SIL OFL 1.1 |
| [Atkinson Hyperlegible](https://github.com/google/fonts/tree/main/ofl/atkinsonhyperlegible) and [M PLUS Rounded 1c](https://github.com/google/fonts/tree/main/ofl/mplusrounded1c) (ExtraBold, cut to Latin letters) | Fonts of the Layer Line skin (bundled the same way) | SIL OFL 1.1 |
| Liberation Sans / Mono | Fonts for text() in every render (`assets/fonts`) | SIL OFL 1.1 |
| [web-openscad-editor](https://github.com/yawkat/web-openscad-editor) | Its `editor.toml` format is read for form metadata; its build approach (dependency discovery, content-addressed files, fonts) informed the website's build, which the app's reading of projects came from. No code copied. | MIT |
