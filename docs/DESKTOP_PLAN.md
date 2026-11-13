# SCAD Workshop: desktop app plan

The design of the app and what's next. The project was called Claude Grid Workshop until Phase 1.5; the repository was renamed SCAD-Workshop on 2026-10-13. Since 0.4.4 the website is retired and the app ships no starter projects (owner, 2026-10-14). Fusion 360 files were tried from 0.4.27 to 0.4.30 and removed (owner, 2026-11-12).

Every earlier release's plan, design and notes (the phases, the owner's requests of 2026-10-12 and direction of 2026-10-14, milestones 1 to 6, Reviews 1 to 6, the library format, reading settings, Fusion files) are in [history/PLAN_HISTORY.md](history/PLAN_HISTORY.md); what was built when is in [CHANGELOG.md](../CHANGELOG.md).

## Now: Review 7 (of 0.4.26, 2026-11-12)

After the Fusion support was removed, the owner asked for a review of the app as of 0.4.26 (features, interface, library, code clean-up) and took all 15 items (2026-11-13). The review: https://claude.ai/artifact/1kDG8QLusBk13oNwvXJNSM. In the order they're being built:

| # | Item | Status |
| --- | --- | --- |
| 3 | A model that isn't in the library gets a page saying so, not the model page's buttons | Done |
| 5 | The 7 NopSCADlib components that fail with their start values | Done |
| 7 | Library settings' text about the removed "Use as the group's icon" | Done |
| 8 | The "part" badge only where parts mix with other kinds | Done |
| 9 | Settings › Library merged into Library settings | Done |
| 14 | Leftovers: unused commands, clippy, stale comments, old branches | Done, except deleting the branches `archive/server-v1`, `ci-screenshots`, `vendor-markdown` and `wip-0.4.3` (the session can't; the owner's to do) |
| 15 | Docs: README, FEATURES.md, the agent notes, this plan's history | Done |
| 2 | Batch generate varies each part of a paired setting (a bin's Width in units), guesses from the first group, leaves out hidden and debug settings | Done |
| 4 | More components start on their own (NopSCADlib types with their other values guessed; MCAD) | |
| 6 | Component pictures made by CI and shipped with the app | |
| 1 | Windows renders about 8 times slower than Linux: measure on the Windows runner, fix, then drop the race against the WebAssembly engine | |
| 10 | The website half of the front end removed (`platform.js`'s browser branch, `store-browser.js`) | |
| 11 | Converters for libraries older than format 2 removed (owner: old libraries can be added again instead) | |
| 12 | Small test projects in `tests/` instead of the 81 MB of website-era projects and the site build | |
| 13 | `app.js` moved to Preact, a page at a time | |

## Backlog

Not scheduled; each comes back when the owner asks.

- **Several libraries open at once** (owner, 2026-10-17: moved to the backlog; item 9 of the requests of 2026-10-12 in the history).
- **Open a .scad file without adding it** (on hold since 2026-10-14).
- **The rest of Phases 4 and 5** that isn't built yet (the Phases table in the history): multi-colour and multi-part 3MF export, batch from CSV, 3MF arranged on the bed, send to slicer, live reload of a watched `.scad` file, among others.
- **Future and experimental options** (below): direct slicing, agent integration, a plugin architecture.

## Goal

A desktop "Swiss Army knife" for parametric 3D design on **Windows and Linux**:

1. **Generate**: find any generator or library part quickly ("bevel gear", "gridfinity bin", "hinge") and get a printable file in seconds.
2. **Ingest**: add new projects from a GitHub link (with update checks) or from a file, ZIP or folder, stored locally.
3. **Index**: one searchable, browsable index of everything available, organised by what it makes.
4. **Library**: keep models, both ones you generated and premade ones you imported, in collections for a use case (a rugged case, the parts for one RC car).
5. **Browse**: a library-style interface (grid, list and table views, search with filters, an inspector that edits metadata) that stays quick with thousands of items.


## Decisions taken

| Question | Decision |
| --- | --- |
| Wrapper or native UI | Wrapper: **Tauri 2**, reusing the existing `web/` front end |
| Platforms | Windows and Linux (no macOS for now) |
| Repository | This repo: `desktop/` beside the website |
| Website | Kept as the light version, sharing the front end (until 0.4.3; retired by the owner on 2026-10-14) |

## Architecture

```
web/  (shared UI: catalog, search, forms, viewer, library pages)
  │
  ├── in a browser ──> engine: OpenSCAD WebAssembly worker  storage: IndexedDB
  │
  └── in Tauri ──────> engine: native OpenSCAD (render queue)  storage: library folder
                         │
desktop/src-tauri (Rust) ├── jobs: run openscad, N in parallel, cancel, cache
                         ├── sources: download, pin, check for updates
                         ├── ingest: entrypoints, includes, settings, licenses, thumbnails
                         ├── index: SQLite + full-text search
                         └── library: collections, imports, recipes, export
```

**Seams in the front end** make one UI work in both places:

- `engine`: `render(model, files, settings) -> STL/3MF` plus progress and cancel. Today's `engine-client.js` becomes the browser implementation; the desktop one calls a Tauri command.
- `store`: saved settings, projects and library. Browser: IndexedDB. Desktop: files in the library folder.
- `index` (added for the new interface, section 5): `query({ text, filters, sort, group, offset, limit }) -> { total, items, facets }`, `get(id)`, `update(id, patch)`. Browser: in memory over `catalog.json`, read-only. Desktop: SQLite in Rust, so the page never loads the whole catalog.

The page detects Tauri at start-up and picks the implementation. Everything above the seams is shared.

### Native engine

- The pinned OpenSCAD snapshot (same version as `engine.json`, today 2026.10.02) ships inside the app. On Windows that's the snapshot's ZIP folder. On Linux it's the AppImage contents, extracted at build time so there's no AppImage inside an AppImage. Both are bundled as Tauri resources and launched by path. CI downloads them and checks their SHA-256, like the WebAssembly engine.
- **Render queue:** one OpenSCAD process per job, up to (cores − 1) at once; cancel kills the process.
- **Cache:** results are stored in the app's data folder on the computer (not in the library), keyed by a hash of (engine version, model files, settings). Repeating a render is instant.
- **Fonts and libraries:** native OpenSCAD sees system fonts, and `OPENSCADPATH` points at the bundled libraries and those in the library folder.
- **Benchmark:** `tools/engine/cli.mjs bench` gets a native mode, so CI records native vs WebAssembly times side by side.

### Library folder

Everything you add, make or edit lives in one **library folder**. You choose it on first run and can change it in Settings. The default is `~/SCAD Workshop`: today's workspace folder becomes the library in Phase 2, without moving anything. The folder is self-contained and portable:

- move or copy it to another disk or computer, sync it (Syncthing, OneDrive, Dropbox) or put it in git, then point any build of the app at it with **Open library…**;
- it holds the files and all their metadata; nothing about the library lives only inside the app;
- paths inside it are relative, and its JSON files are small, pretty-printed with sorted keys and written atomically (write, then rename), so sync tools and git diffs stay clean;
- it holds no database. What the app needs to search is rebuilt from the folder when it opens, and the render cache is kept on the computer, because database files and file-sync tools don't mix well.

```
My SCAD Library/
  library.json                        name, id, format version, library-wide defaults
  sources/<source>/                   one folder per project you added
    source.json                       origin (GitHub repo + pinned commit, Printables page, ZIP or folder),
                                      what ingest detected (license, authors, dates), update state
    metadata.json                     your edits: project, folder and file level (see "Metadata levels")
    files/<version>/...               the downloaded or extracted files, untouched, one folder per version
    derived/<version>.json            what reading the project found: models, settings, parts, problems
                                      (rebuildable, kept so a moved library opens at once)
    thumbs/, previews/                thumbnails and part previews (rebuildable)
  local/<project>/                    your own projects: drop .scad or model files here and the app picks them up
                                      (edited in place, not versioned)
  collections/<name>/collection.json  items by id (not copies), quantities, notes, status, icon
  recipes/<id>.json (+ output file)   saved settings (output format and last output: Phase 4)
```

**How files get in:**

- **The starter library:** the generators and parts that used to be built into the app (Gridfinity, openGrid, the openGrid parts pack and the rest) are packaged as library projects and shipped with the installer as a starter library beside the program. On first start the app copies them into your library, where they are ordinary projects: editable, relicensable, removable. A later app version with changed projects offers them as updates, like a GitHub update; your edits are kept. The website still builds from the repository as before.

- **GitHub:** not a git clone. The app downloads the repository at one commit (GitHub's tarball) into `files/<commit>/`: plain files exactly as upstream has them, which open in OpenSCAD directly, with no git needed and each version pinned. An update downloads the new commit beside the old one. The old one is kept while a saved recipe uses it; "Clean up" removes unused versions.
- **Printables or any ZIP:** extracted unchanged into `files/<version>/`. What the README and license file say goes into `source.json` as detected metadata (reading Printables' PDFs: Phase 4).
- **A folder elsewhere on disk:** copied in by default. A project you are still developing can instead be **linked** (left where it is). A linked folder isn't portable, so if it's missing after a move it shows under "Needs attention" to relink.

**With the app, on this computer** (the app's data folder, not the library): printer profiles, the render cache, the unpacked OpenSCAD engine, window layout and view preferences, and the list of libraries you've opened. Apart from the printer profiles, all of it can be rebuilt, so deleting it loses nothing else.

**Moving the library or installing a new build:** install, choose **Open library…** and pick the folder. A newer app upgrades an older library format when it opens it (after backing up the JSON files it changes). An older app opens a newer library read-only and says why. **Merge library…** copies another library's sources, collections and recipes into the open one: a source with the same origin and version is kept once, and conflicting metadata is shown side by side to choose from.

**From Phase 1:** today's workspace holds `settings/saved/`, `settings/prefs.json` and `cache/`. On the first start of the Phase 2 app, saved settings move to `recipes/`, and the printer profile, view preferences and the cache move to the app's data folder.

### Metadata levels

Metadata is layered, so one edit can cover a whole project, part of it, or a single item:

| Level | Stored in | Example |
| --- | --- | --- |
| Library | `library.json` | default license and author for your own `local/` projects |
| Project | `sources/<source>/metadata.json` → `project` | license CC-BY-4.0 for the whole project; author; category; icon |
| Folder in a project | same file → `groups["<folder>"]` | tag "lids" for everything in `lids/` |
| Item (one name, all its formats) | same file → `items["<item>"]` | a different license or name for one part |

- **Which value wins:** your edits beat detected values, and among your edits the most specific level wins (item, then folder, then project). Detected values (README, LICENSE, Printables PDF, family manifest) come next, and library defaults only fill fields nothing else set.
- **Changing a whole project** is one edit at project level. Items with their own value keep it, and the inspector says how many do and offers to clear them.
- **Provenance:** every field shows its value and where it came from ("License: CC-BY-4.0, set on the project; detected: none"). "Revert" removes your value at that level.
- **Updates:** detected values live in `source.json` and are refreshed by a source update; your edits live in `metadata.json` and are never touched by one. Edits that no longer match anything (a renamed file or setting) go to "Needs attention".
- **Starter projects** are edited the same way as any other project: their edits live in their own `metadata.json`.
- **Cross-project groups** (categories, collections) have their own name, description and icon. Editing them doesn't change the license or author of the items inside; for that, select the items and bulk-edit them at item level.
- **Form overlays** (renamed settings, conditions, presets; section 5) are stored per generator in the same project file.

## 1. Sources (ingest)

**From GitHub:** paste a repo URL (optionally a branch, tag or subfolder).

- The app downloads the tarball for a specific commit through GitHub's API. That's no git dependency, and it is pinned exactly.
- **Update checks:** on start-up, at most daily, the app asks GitHub for the latest commit on the tracked branch. Unauthenticated limits (60 requests an hour) are plenty; a personal token can be added for many sources. When there's a newer commit, the app shows:
  - the commit messages since the pinned one;
  - settings added, removed or changed default, for each generator;
  - a re-render of your saved recipes from that source on the new version, compared with the old by size and volume (as `tools/parity.py` does).

  You accept or skip. The old version stays until nothing uses it, so saved recipes always rebuild exactly.
- Other git hosts (GitLab, Codeberg) follow later.

**From a file, ZIP or folder:** copied into `sources/`, with a form for name, author, origin URL and license.

**From a model-site download (Printables first):** a Printables ZIP holds the model files plus PDFs carrying the page's details: creator, license, published and updated dates, description, tags and print settings. Ingest reads the PDF text and fills the form from it, so nothing has to be typed:

- fields are picked out by their labels; the exact layout gets confirmed against sample ZIPs when this is built;
- every value records where it came from ("from the Printables PDF"), so the inspector's provenance and "Revert to source" work as for any other source;
- the origin URL lets the app check the page later for a newer upload, as GitHub sources are checked for new commits;
- other sites follow the same pattern with their own extractor (Thingiverse ZIPs carry a README and LICENSE text file; MakerWorld to check).

Items with STL/3MF only go into the model library (section 4); ZIPs that include `.scad` files also become sources with generators.

**Ingest steps** (generalising today's `tools/build_site.py`):

1. Find entry files: `.scad` files that produce geometry at top level and/or have Customizer settings. Files that only define modules and functions are library files.
2. Follow `include`, `use` and `import`. Paths that point at a bundled library (`<BOSL2/std.scad>`) resolve to it; anything missing is listed on the source page.
3. Read settings with OpenSCAD's own Customizer export, as now. Apply `editor.toml` when present.
4. Read the README (description), LICENSE (identified as SPDX where possible) and any parameter-set JSON files (as presets).
5. Render a thumbnail at default settings.
6. Write the index entries.

The ingest logic gets **one implementation in Rust**, used by the app and, through a small CLI, by the website build in CI. `build_site.py` then keeps only the website-specific steps, so the two can't drift apart.

**Safety:** OpenSCAD can't run programs, but `import()` can read any file the user can read. Renders therefore run with the job's folder as the working directory, and ingest warns about absolute paths. Downloads have a size limit, with a prompt above it.

## 2. Bundled libraries

Pinned and hash-checked like `vendor/`:

| Library | Source (checked 2026-10-03) | License |
| --- | --- | --- |
| BOSL2 | BelfrySCAD/BOSL2 @ e173fa0 (2026-10-02) | BSD-2-Clause |
| MCAD | openscad/MCAD @ bd0a7ba (2021-10-25) | LGPL-2.1 (some files more permissive) |
| NopSCADlib | nophead/NopSCADlib @ 00ec289 (2026-09-24) | GPL-3.0 |
| Round-Anything | Irev-Dev/Round-Anything @ 061fef7 (2023-08-07) | MIT |
| threads-scad | rcolyer/threads-scad @ 4ae9aeb (2021-12-01) | CC0-1.0 |

More can be added the same way, or ingested as ordinary sources.

**As built in 0.3:**

- **With the app, not in the library.** The libraries ship inside the installer as app resources (`libs/`), pinned in `libraries.json` (commit, license, and a SHA-256 over the file list so a download and a git checkout check the same way), fetched by `tools/fetch_libraries.py`. They're updated with the app, so they don't show as projects and don't need update checks; Library settings lists them with their version.
- **Which copy is used.** A library you added as a project (role "library", say your own BOSL2 checkout) wins over the bundled copy with the same name, both for Components and for `include <BOSL2/...>` in your projects. Library settings → Libraries has a switch per library to go back to the bundled copy; it's stored in `library.json`, so it moves with the library. Projects that use the library are read again when it changes.
- **Includes resolve everywhere.** A project that includes `<BOSL2/...>`, `<MCAD/...>`, `<NopSCADlib/...>`, `<Round-Anything/...>` or `<threads-scad/...>` finds them on every install, even without the library added as a project.

### Components

A third section in the left menu, between Parametric Models and the Parts Library: **Components**, the modules inside libraries (gears, threads, bearings, hinges, shapes, fasteners) as forms. Grouped by topic (collapsed, like the other sections), with the libraries inside each topic.

- **What a component is:** one public module that makes geometry on its own (not an operator that needs children). BOSL2: modules and function-modules whose docs say they make geometry ("SynTags: Geom") and that don't need children. Other libraries: public modules (not starting with `_`, not tests) that don't need children; for NopSCADlib, the documented ones (`//!` comments).
- **Rendering:** a generated file: the library's includes, then one call with the values you set, named (`bevel_gear(teeth=36, mate_teeth=36, circ_pitch=5, shaft_diam=5)`). Settings you leave empty aren't passed, so the module's own default applies. The file is written for each render; the library's files are laid out once and shared. "Copy code" gives the same OpenSCAD text.
- **Settings types:** a number, checkbox, text or vector when the module's default (or the docs example) is one; otherwise an OpenSCAD expression field (`[10, 20, 5]`, `BOTTOM`, `circle(5)`), empty by default. `anchor` and `orient` are choices of BOSL2's directions and the module's named anchors. Detail (`$fa`, `$fs`, `$fn`) is its own group.
- **Start values:** the first doc example that is a plain call of the module; every such example is a "Start from" preset.
- **Desktop only** (the website stays the light version).
- **Keys:** `@<library>/<module>` (`@bosl2/bevel_gear`), by library name, so favourites, recent and pins survive switching between your copy and the bundled one.

**Pinning:** "Pin as a model" on a component makes it a Parametric Model: a name, a category, the settings to show (the rest hidden at their current values or defaults), and the current values as its defaults. Pins are files in the library (`sources/pinned/pins/<id>.json`, shaped like a family manifest's model entry with `component` instead of `entrypoint`), listed under a "Pinned components" project, and edited, hidden, deleted and moved like any other model.

**Form editor:** "Edit form" on a model page (desktop) changes how its settings look: label, help text, box names (axes), min/max/step, choices, show-when condition, hidden, section (`group`), printer-profile link, default, and presets. The edits are stored with the item's metadata (`items.<id>.form` in the project's `metadata.json`), in the same shape as a family manifest: `{ ui: { <setting>: {...} }, hidden: [...], defaults: {...}, presets: [...] }`, and applied with the same code the website build uses for manifests, so a form made in the app can be pasted into `catalog/families/*.json` unchanged.

### Module → generator

Most mechanical parts (gears, threads, hinges, bearings) are modules inside libraries, not ready-made generator files. The app makes them usable directly:

- **BOSL2:** its public modules have structured doc comments (`// Module:`, `Synopsis:`, `Topics:`, `Arguments:`, `Example:`), 964 documented functions and modules in the current version. The parser turns this into:
  - an index entry, with the synopsis and the BOSL2 topics as tags (`bevel_gear()` has topics Gears, Parts);
  - a form, one field per argument with its description as help; types and defaults come from the signature and the description ("Default: 90");
  - presets, from the doc examples (`bevel_gear(mod=3, teeth=35, mate_teeth=35, face_width=20)`).
- **Other libraries:** the form comes from the module signature (`module name(a=1, b=[2,3])`): names, defaults and types inferred from the defaults. Comments directly above the module become its description.
- **Rendering:** a generated wrapper file (`include <BOSL2/std.scad>` + `include <BOSL2/gears.scad>` + one call with the form's values), rendered like any other model.
- A module can be **pinned as a generator**, with a nicer name, chosen settings and hidden extras. Pinned generators are stored as small JSON files in the library folder, in the same shape as `catalog/families/*.json`.

## 3. Index and search

**One index** for everything:

- generators (built-in and ingested);
- library modules;
- pinned generators;
- library items (premade and generated).

**Each entry has:**

- kind, name, description and tags;
- a category path;
- settings (names and descriptions, so "tooth count" finds gears);
- source and version, and license;
- creator and origin URL;
- dates: updated upstream (the pinned commit, or the date on a downloaded page), added to the library, last used;
- thumbnail and size;
- any further fields a source supplies (print settings, material, a model site's own category), kept as open key/value metadata so new filters don't need a schema change.

**Categories:** a starting taxonomy you can edit:

- Storage › Gridfinity / openGrid / Boxes / Labels
- Mechanical › Gears / Threads / Hinges / Bearings / Springs
- Fasteners › Screws / Nuts / Inserts
- Enclosures
- Text and signs
- Shapes and helpers

Ingest suggests a category from keywords, BOSL2 topics, file and module names, and the README; you can change it.

**Search:** SQLite FTS5 with:

- prefix matching;
- a synonym list (cog → gear, box → enclosure / case, nut → fastener);
- filters by category, kind, source, license, "in my library", and "runs fast";
- results ranked by match, then by how often you use them.

**Browse:** see section 5. The home page becomes recent, favourites and category tiles; the current family/project grouping becomes one of the browser's views.

**Target flow (acceptance test for the bevel-gear case):** search "bevel gear" → BOSL2 `bevel_gear()` is the first result → form opens with a preset from the docs → change teeth and module → preview → download STL. All in under a minute on a normal laptop.

## 4. Model library

**Collections** are lists, not folders of copies: a `collection.json` refers to items by id, so one part can be in several collections. Imported files live in `sources/` like any other project (see "Library folder"). Each item is either:

- **premade:** STL, 3MF, OBJ or STEP (files with the same name are one item; see "Files and formats" below), plus origin URL, author and license; or
- **recipe:** a generated model, stored as source, version, model and settings, with its last output file. Recipes can be re-opened in their form, changed, or re-rendered after a source update.

**Per item:** quantity, notes, print settings (material, nozzle, infill and so on, free text to start with), status (to print / printed), and tags.

**Import:** drop files, a ZIP or a folder. Duplicates are found by content hash. Sizes and thumbnails are measured on import. STEP gets a preview through OpenCascade's WebAssembly build (occt-import-js; license to confirm before bundling).

**Export:** a collection as a ZIP (files × quantity, with a parts list), or as a 3MF with parts arranged on the plate (Phase 5).

**Search:** library items are in the same index as generators, so "case latch" finds your saved latch and the generator that made it.

### Files and formats

**One item per name.** Files in the same folder whose names differ only by extension (`latch.stl`, `latch.3mf`, `latch.step`, `latch.f3d`) are one item, wherever they come from: the built-in parts libraries (as `tools/import_library.py` does now), imported folders and ZIPs, Printables downloads, and your own generated outputs. Matching ignores case. Near misses (`latch_v2.stl` beside `latch.3mf`) are listed after an import so you can merge them, and any selection can be merged or split by hand.

- The item lists the formats it has (STL · 3MF · STEP). **Download** has a format picker. The default is your preferred format from Settings (for example "3MF where there is one, otherwise STL"), and the last choice is remembered.
- The preview and thumbnail use the best format the viewer reads: 3MF, then STL, then OBJ; STEP goes through OpenCascade.
- A collection export (ZIP) applies one rule for items with several formats ("3MF where available"), or lets you choose per item.
- Generators get the same picker for their output: STL or 3MF. Saved settings and recipes remember the format.

**Multi-colour and multi-part 3MF**, when a model calls for it:

- **When:** the model colours its parts with `color()` (a label with raised text in a second colour, an inlay), or makes several separate parts at once (the Rugged Box's case, lid and latches; a batch of labels). The render reports how many objects and colours it produced, and the picker offers "3MF, 2 colours" or "3MF, 12 parts" only then. One-part, one-colour models stay a plain choice of STL or 3MF.
- **How:** OpenSCAD keeps top-level objects separate when lazy union is on, and its 3MF export can write colours. Each object becomes a named object in the 3MF with its colour as a material, so Bambu Studio, OrcaSlicer and PrusaSlicer can give each part its own filament. Both need checking against the pinned snapshot, in the WebAssembly and the native engine, before the app relies on them.
- **Projects with a part picker:** many generators make one part at a time, chosen by a setting ("part to print: base / lid / latch"). A family manifest can name that setting, and the download then offers "All parts in one 3MF": the app renders each choice and combines them, laid out side by side.
- **Preview:** the 3D view shows each object in its own colour, with a parts list to show, hide or recolour them, so what you see is what the slicer gets.
- **Batch:** the batch download gains "one 3MF with every variant as a separate object".
- Arranging the parts on the print bed for your printer profile comes in Phase 5.

## 5. Interface

Today's home page is one long scrolling page grouped by type and project. It works for 58 models from 20 projects. It won't work for hundreds of generators, thousands of library modules, and collections of imported parts. The desktop app moves to a workspace layout like a file manager or photo library. The model page (form + 3D view) stays, as the "workbench".

### App layout

```
┌──────────────────────────────────────────────────────────────────────────┐
│ [≡]  Search everything (Ctrl+K)                          jobs ◔   ⚙       │
├─────────────┬──────────────────────────────────────────┬─────────────────┤
│ Home        │  Gridfinity › Bins            ⊞ ☰ ▤  Sort ▾ │ Inspector       │
│ Recent      │  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐        │ [thumbnail]     │
│ Favourites  │  │    │ │    │ │    │ │    │ │    │        │ Basic Bin       │
│ ─────────   │  └────┘ └────┘ └────┘ └────┘ └────┘        │ Gridfinity Ext. │
│ Generators ▸│  Basic  Drawer Tray   Sieve  Lid           │ tags, category, │
│ Modules    ▸│                                            │ license, source │
│ Parts      ▸│  ┌────┐ ┌────┐ ...                         │ [Open] [★] [⋯]  │
│ Collections▸│                                            │                 │
│ Sources    ▸│                                            │ saved settings: │
│ Smart lists▸│                                            │ ▢ ▢ ▢           │
├─────────────┴──────────────────────────────────────────┴─────────────────┤
│ 3 renders running · update available for 2 sources                        │
└──────────────────────────────────────────────────────────────────────────┘
```

**Sidebar** (collapsible):

- Home, Recent and Favourites.
- Generators, library modules, ready-made parts, collections and sources, each as a category tree with counts. (As built in 0.2: **Parametric Models** and the **Parts Library**, each by category, collapsed until opened, with projects inside; projects have no list of their own, they're reached by category, by condensing on project, or from Library settings.)
- Smart lists (saved searches).
- "Needs attention": sources with missing includes, failed thumbnails, unknown licenses or updates waiting. Today that list lives only in SOURCE_AUDIT.md.

**Browser** (centre) shows whatever is selected in the sidebar or searched.

**Inspector** (right, toggleable) shows the selected item's details and edits them.

**Workbench:** opening an item opens it in tabs, so several models can stay open with their settings and preview.

**Status bar:** background jobs (renders, batch queue, ingest, thumbnails, update checks) with a drawer to see or cancel them.

### Browser views

All views share selection, sorting and grouping. The app remembers the view per place, so parts can default to large thumbnails and modules to a table.

| View | For | Shows |
| --- | --- | --- |
| **Grid** | Looking for a shape | Thumbnails at three sizes (slider), name, small kind badge |
| **List** | Scanning names | One row each: small thumbnail, name, project, category, tags |
| **Table** | Managing metadata | Sortable, resizable columns you choose: kind, source, license, version, added, last used, size, triangle count, render time |
| **Grouped** | Today's look | Sections by category, project or source, with items as chips or thumbnails |

**Sort:** name, recently used, recently added, most used, source, size.

**Group by:** category, project/source, kind, license or none.

**Condensed groups:** whenever a grouping is chosen, a "Condense" switch beside the Group menu shows each group as a single tile instead of a section. It works in every view and every place: generators, parts, collections and search results.

- The tile shows the group's icon, name and item count. Opening it shows that group's items, with a path back ("Generators › Gridfinity Extended").
- Searching still finds items inside condensed groups; each matching group shows how many of its items match.
- **Group icon:** by default a collage of the first few items' thumbnails. Once metadata is editable (see "Metadata editing" below), you can choose it: one item's thumbnail, a render of a saved setting, or an image of your own. The same icon then appears for that project or category on the home page and in the sidebar.

**Quick look:** Space opens a large preview that you can spin, without leaving the list. For a generator it shows its default render; for a part, the part.

**Multi-select:** Shift/Ctrl-click, or drag a box, then act on all of them:

- tag, set category, favourite;
- add to collection;
- batch render;
- export, or hide.

**Keyboard:** arrows move, Enter opens, Space for quick look, `/` searches, Ctrl+K opens the command palette.

**Scale:** lists are virtualised and thumbnails load lazily, so browsing stays fast with thousands of items.

### Search

- **One search box** for everything: generators, modules, parts, collections, sources, saved settings and setting names ("tooth count" finds gears). Results are grouped by kind with counts.
- **Filters** appear as chips with counts: kind, category, tags, source, license, updated (last month / 3 months / year / older), desktop-only, has thumbnail, recently used. Tags can also be typed: `tag:gear source:BOSL2 kind:module updated:month`.
- **More metadata filters as sources supply them:** creator, license family (CC BY, CC BY-NC, GPL…), date added, and any open metadata field (section 3). A filter menu lists the fields present in the current results, so a field arriving from a new kind of source (a Printables PDF, say) becomes filterable, sortable and a table column without a code change.
- **Forgiving matching:** prefix matching, typo tolerance (SQLite FTS5 plus trigram), and the synonym list from section 3.
- **Size search** for parts and generators that report dimensions: "fits within 84 × 42 mm", or for Gridfinity "2 × 1 units".
- **Saved searches** become smart lists in the sidebar. Recent searches are kept.
- **Command palette** (Ctrl+K): the same box also runs actions such as "Add GitHub source…", "New collection", "Clear render cache" or "Open settings".

### Metadata editing

Everything from a source can be adjusted without touching the source files. Edits are stored in the library folder beside the project they belong to (`sources/<source>/metadata.json`, at project, folder or item level; see "Metadata levels" in the architecture section), so they survive upstream updates and travel with the library.

**In the inspector** (one item, many at once, or a whole group):

- **Level:** selecting a project or folder (in the sidebar, a group heading or a condensed group tile) edits that level, so "license for the whole project" is one change. Each field says which level its value comes from.
- **Item details:**
  - display name, description (Markdown), category (tree picker), tags (autocomplete), favourite, personal notes;
  - author, origin URL and license, with a note of where the information came from.
- **Thumbnail:** re-render from the current or a saved setting, choose the camera angle and colour, or use an image.
- **Groups:** a project's, category's or collection's name, description and icon (the tile shown when groups are condensed, also used on the home page and in the sidebar).
- **Provenance:** each field shows whether it came from the source or from you, with "Revert to source".
- **Undo:** for every edit.

**Form editor** (on the workbench, "Edit form"): what the family manifests' `ui` blocks do today, without editing JSON:

- rename settings;
- set box names for list values (narrow/wide/length/position);
- hide, reorder or group settings;
- add show-when conditions ("only when utensil count ≥ N");
- mark settings as linked to the printer profile;
- add presets.

These are saved as overlays in the same form as `catalog/families` `ui`. A good one can be copied into the repo for everyone.

**Category editor:** rename, move and merge categories, and drag items between them.

**After a source update**, overlays that no longer apply (a renamed or removed setting) show up under "Needs attention" instead of failing silently.

**Exchange:** metadata can be exported and imported, to share a curated catalog or move it between machines.

### Thumbnails

- Rendered automatically at ingest and import, with the same viewer camera and colour for every item, so the grid looks consistent.
- Saved settings and recipes get their own thumbnails, so the inspector shows the variants you've made.
- Premade STL/3MF files are rendered directly; STEP files go through OpenCascade (section 4).
- Thumbnails are stored in `cache/thumbs/` and rebuilt when the source or the engine changes.

### Other parts of the interface

- **Source page:**
  - the README, rendered;
  - license, files, generators and modules found;
  - pinned version, available update with a summary of what changed, and ingest problems.
- **Collection page:**
  - items with quantities and printed/to-print checkboxes, plus a progress bar ("12 of 30 printed");
  - drag in from the browser;
  - export.
- **Compare variants:** two saved settings side by side, or overlaid in the 3D view, with the differing settings highlighted.
- **Drag and drop:**
  - drop a `.scad` file, a ZIP, a folder or an STL/3MF on the window to import it;
  - drag a finished part out to the file manager or a slicer.
- **Theme:** light, dark and night (dim and warm, for a dark workshop), following the system by default.
- **Accessibility:** everything reachable by keyboard, with visible focus, labelled controls and enough contrast. The layout works down to a 900 px wide window.
- **Website:** gets the browser views and search, in read-only form over the built-in catalog. Editing, sources and collections stay desktop-only.

### Reference documents and the side viewer

(Requested 2026-10-05; Phase 4, first part in 0.4.0. The owner's answers to the questions below are folded in.)

Every project gets a **reference document**: the project's own documentation, kept with it in the library and readable offline. It opens in the **side viewer**, which also shows a model's OpenSCAD code.

**Side viewer.** A collapsible panel on the right edge of the window. Tabs along its edge choose what it shows: **Code** (the model's OpenSCAD files, read-only for now) and each of the project's documents (README, a PDF, readme.txt). Choosing "Code" or "README" on a model page, in the inspector or on a project's page opens the panel on that tab; clicking the open tab, or ×, folds it back to the tabs. It follows what's on screen: the open model, or in the library the selected item or the project page. Open or folded, the tab chosen and the panel's width (drag its edge) are remembered. When open it lies over the right of the window (the 3D view, the inspector) rather than squeezing the form.

- **Code:** the model's files, entry file first, with line numbers and simple highlighting (comments, strings, numbers, keywords), a file picker when it has several, and Copy. A component shows the file made for its current settings (as Copy code), then the library file that defines the module, opened at the module. A README link to one of the project's `.scad` files opens it here.
- **Documents are project-level** (owner): a model or component shows its project's (or library's) documents; for NopSCADlib, every part has its section in the one README, so you scroll to it. No per-part links.
- **Three kinds, stored as** (owner):
  - **HTML** — a Markdown README (GitHub projects, and any project with `README.md`) is converted when the project is read (pulldown-cmark: CommonMark plus GitHub's tables, strikethrough, task lists and footnotes; headings get GitHub's anchors so the README's own contents links work) and kept in the library at `sources/<id>/docs/<version>/readme.html`. Raw HTML in the README is kept, minus scripts, frames, forms, styles and event handlers; it's shown in a frame that can't run scripts, in the app's colours. Images and links stay relative to the project's folder, so they work offline. A `README.html` is used as it is (cleaned the same way).
  - **PDF** — kept raw (Printables downloads): the project's PDFs at the top level or one folder down, shown in the webview's own PDF viewer where it has one (Windows), with "Open in your PDF viewer" always offered (Linux's webview has none).
  - **Text** — `readme.txt` (or `README` with no extension), shown as it is.

  Tabs in that order: README, PDFs, readme.txt.
- **The app's libraries** (BOSL2, NopSCADlib, …): their README is converted when the app is built (`libs/docs/<Name>/readme.html`), with images and links pointing to GitHub at the pinned commit (online only: the app ships their OpenSCAD files, not their pictures).
- **Built-in models** (the starter library and the website): the family's README from its vendored folder, converted by the website build (`data/docs/<family>.html`, images and links to GitHub at the pinned commit) and copied into the starter projects. The website has the side viewer too: Code from the model's files, the README from the build.
- **Projects read by 0.3** have no stored documents yet: they're found when first asked for, and the HTML is made then.

**Settings borrowed from the library** (owner, 2026-10-06; 0.4.1, put ahead of editing the code). Settings lifted from module calls borrow the library's own description and choice list when the library passes its documented Customizer variable to the module (`block(width=block_width, type=block_type, …)` in LEGO.scad gives "Width of the block, in studs" and the block type drop-down); fractions like `height=1/3` become the library's height drop-down (1/3, 1/2, 1…) rather than a number.

**Editing the code** (owner, 2026-10-06; 0.4.1). The Code tab is editable on a model's page:

- **Generate uses the edits.** Edited files replace the originals for that render only (the engines get the edited text under the same path, so includes resolve as before; native OpenSCAD reads it from the app's data folder by its hash, the WebAssembly engine gets the bytes). While the code is edited, the form passes only the settings changed from their defaults, so a default changed in the code applies unless the form overrides it. The status says when the preview is older than the code.
- **Saving works like saved settings, never touching the original.** "Save" (in the Code tab or the settings bar) stores the edits inside a saved-settings record, next to the values: `{ id, model, name, values, edits: { "<path>": { text, base } } }` in the library's `recipes/` (the browser's storage on the website). No copy of the model appears in the library; the variant is picked from the same list as saved settings (marked "code edited"). Picking it brings back its values and its code; Defaults goes back to the original code; a preset changes only values.
- **Updates don't touch it.** The project's files stay as downloaded, so updates arrive as usual. `base` is the hash of the file the edit started from: when the project's file has changed since, picking the variant says so (the edited copy is still used as saved).
- **Not editable:** components and pinned components (their file is made from the form for each render), and models outside their page (the inspector's Code is read-only).

**Later (owner):** a **scratch pad** of notes per part, saved with it in the library (kept as edits, so updates don't overwrite them).

### Front-end structure

`web/app.js` is already about 1,400 lines of hand-built DOM. The new interface (virtual lists, multi-select, an inspector, tabs) needs structure, so before building it:

- Split the front end into modules: shell, browser, inspector, search, workbench, settings.
- Add a small shared state store.
- Use **Preact + htm** for the new views. Both are vendored ES modules of a few KB each and need no build step, so the website still deploys as static files.

Plain web components would also work. Preact makes list virtualisation and editable inspectors much less code.

**Before building:** a clickable mockup of the layout and the four views, to settle the look before code.

## Future and experimental options

Asked for by the owner on 2026-10-06 (the plugin architecture a little later the same day). None is scheduled, none belongs to a numbered phase, and each needs a decision before building (the open questions are listed). They are ideas to try, not promises; anything that ships is marked experimental in the app until it has been used for a while.

### Direct slicing in software

Slice a rendered model inside the app and show the result (layers, print time, filament), without leaving for a separate slicer.

- **How it might work:** run a command-line slicer (PrusaSlicer, OrcaSlicer or CuraEngine) as the native engine runs OpenSCAD: the app finds or bundles it, passes the model and a print profile, and reads back G-code or a 3MF with its statistics. Printer profiles already belong to the software (decision 8), so the bed size and tolerances are there to start from.
- **Relation to Phase 5's "send to slicer":** that item only opens the file in the slicer. Direct slicing is the step after it and shares its setting for where each slicer is installed.
- **Open questions:** which slicer (licensing and bundling size against asking the user to install one); how much of a slicer's profile system to expose; a preview of layers or only numbers; desktop only (the website can't run a slicer).

### Multiple libraries

Requested by the owner (item 9 of the requests of 2026-10-12, in the history), then moved to the backlog (2026-10-17).

### Agent integration to edit SCAD files

Let an AI agent (Claude, for instance) read and edit a model's OpenSCAD from inside the app, with the result rendering as usual.

- **How it might work:** the agent's changes are kept the way the Code tab's edits already are (0.4.1): stored as a variant in the saved-settings record, never written to the project's files and never added as a model, so library updates aren't affected. The app would offer the agent the model's code, its settings and the render result; the agent proposes an edit, the user sees it in the Code tab as a change to accept or revert, and Generate renders it. Possible routes are an MCP server exposing the library, or a panel that talks to an API.
- **Open questions:** the route (MCP server, built-in chat panel, or both); what the agent may touch (only the open model, or the whole library); keys and where requests go (a network call from an otherwise offline app, so off until the user turns it on); how an agent's edit is told apart from the user's in a variant's history.

### Plugin architecture

A way for outside code to add abilities to the app without changing the app itself. Direct slicing and agent integration could each be a plugin rather than part of the core, which would keep the core small and let people choose what to install.

- **Example uses:** a slicer plugin per slicer (PrusaSlicer, OrcaSlicer, Bambu Studio, CuraEngine) behind one "slice" interface; inference-engine plugins for the agent integration, so the agent can be Claude, another hosted model or one run locally; model editors that open a part's code or mesh in another tool and bring the result back (a code editor, a mesh sculpting tool, a CAD program); also import and export formats, extra sources to ingest from, and extra views of a model.
- **How it might work:** a plugin is a folder with a manifest that says what it adds (a command, a panel in the side viewer, an action on a model, a setting page), and the app asks the plugin to do the work through a small, versioned interface. Two shapes are possible and may both be used: separate programs the app starts and talks to over standard input and output (any language, and a crash can't take the app down), and web-page panels in a sandboxed frame for things that need an interface. This matches how the native engine and the library's commands are already separated from the front end (`App::call`).
- **Decisions to make first:** what the interface offers a plugin (read the library, render, store a variant) and what needs the user's say-so each time (network, files outside the library, running programs); how plugins are installed, updated and removed (a folder in the library, or from a GitHub link the way projects are added); whether plugins belong to the app or travel with a library; and whether they run on the website (only the web-panel kind could).
- **Order:** the interface should be settled with the first plugin that needs it (a slicer is the simplest), not designed in the abstract. Direct slicing and agent integration would then be built as plugins from the start.

## Testing

- **Linux:** automated UI tests through `tauri-driver` (WebDriver) in CI (this environment can't build the app; see Phase 1 notes).
- **Windows:** CI builds the installer and runs the same WebDriver tests on a Windows runner. The owner does hands-on testing.
- **Acceptance test:** `tests/desktop_page.py` drives the app's page against its real backend (`workshop-cli serve`); every user-visible feature adds a check.

## Risks and how they're handled

| Risk | Handling |
| --- | --- |
| Linux webview (WebKitGTK) has weaker WebGL than Chromium | Test the viewer early in Phase 1; if it's a problem, Linux falls back to an Electron build of the same front end |
| OpenSCAD snapshot packaging differs per OS | Phase 1 starts with a spike that bundles and runs it on both; pinned and hash-checked |
| Unsigned installers | Fine for private use (Windows shows a SmartScreen prompt the first time) |
| Ingested projects with unusual layouts | Ingest reports what it couldn't resolve; per-source overrides in the same JSON form as `catalog/families` |
| License mix in a collection | Each item keeps its license; export includes a credits file |
| 3MF colour or object export behaves differently in the WebAssembly and native builds | Check both engines in CI with a coloured and a multi-part model; fall back to one STL per colour or part in a ZIP |

## Still open

- Licenses and authors for Anylid, openGrid Shelf, Just Fit and Minimalist Kitchen (owner to supply).
- Multiboard and openGrid Connector sources (owner to supply).
- STEP/Shapr3D files for openGrid tiles: on hold.
