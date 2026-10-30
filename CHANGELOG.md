# Changelog

The history of SCAD Workshop in one place: what was built, when, why, and what was learned on the way. It combines the git log, the GitHub release notes, the "notes" sections of [docs/DESKTOP_PLAN.md](docs/DESKTOP_PLAN.md), the early backlog in [docs/FEATURES.md](docs/FEATURES.md), [docs/SOURCE_AUDIT.md](docs/SOURCE_AUDIT.md), and the owner's requests from the working sessions.

It's written for whoever picks the work up next, people or agents: start with **For agents** below, then read the newest release. Dates are the owner's time (America/New_York). Newest first.

**Keeping it up to date:** add a line under **Unreleased** with each change you push (what changed for the user, and anything the next person needs to know). When CI publishes a release, move those lines under a heading for that version. CI writes the GitHub release notes from the commit subjects; this file is where the reasons and lessons go. Design detail belongs in the plan's notes sections; link to it rather than repeating it.

---

## For agents

### What this is

A desktop app (Windows, Linux; Tauri 2) for making 3D-printable parts with OpenSCAD: native OpenSCAD, a portable library folder, projects added from GitHub, ZIPs and folders, Components from bundled OpenSCAD libraries. Installers on the [releases page](https://github.com/DrFlGd/SCAD-Workshop/releases). The repository was Claude-Grid-Workshop until 2026-10-13.

Since 0.4.4 (owner, 2026-10-14): the **website is retired** (no longer deployed; `web/platform.js` and `build_site.py` remain because the desktop build still uses the site build for its WebAssembly engine pieces and the test projects), and the app ships **no starter projects**, only the component libraries. The website-era generators and parts packs (`vendor/`, `catalog/`, `libraries/`) are **test projects**: CI builds them into a starter library (`build/desktop/starter`) and the tests point the app at it (`--starter` for `workshop-cli`, `WORKSHOP_STARTER` for the built app).

The owner ("DrFlGd") tests hands-on, mostly on Windows, and steers by phases. The roadmap and every design decision are in `docs/DESKTOP_PLAN.md` (Phases table; "Still open"; one notes section per finished stage).

### Where things are

| Path | What |
| --- | --- |
| `web/` | The front end (no build step). `app.js`: routing, model page (form, preview, batch, saved settings), settings and license pages. `web/ui/`: Preact + htm islands (sidebar, browser, inspector, tabs, palette, quick look, dialogs, Library settings, pin dialog and form editor, `sideview.js`: the side viewer for code and documents). `platform.js` / `platform-desktop.js`: the browser vs desktop seam. `ui/index-local.js`: search. |
| `desktop/core/` | Rust, no GUI: `api.rs` (the app's command table), `render.rs` (native OpenSCAD queue and cache), `library.rs` (library folder; its format is docs/LIBRARY_FORMAT.md), `format.rs` (its JSON files and their format numbers), `check.rs` (Check library), `meta.rs` (layered metadata), `catalog.rs` (catalog from the library), `components.rs` (library modules as forms), `docs.rs` (reference documents: README to HTML, PDFs, text), `ingest.rs` / `sitebuild.rs` / `scan.rs` / `project.rs` (reading projects; also the website build), `sources.rs` (GitHub, ZIP, folder), `merge.rs`. `src/bin/workshop-cli.rs`: CLI and `serve` (the app's backend over HTTP, for tests). |
| `desktop/src-tauri/` | The Tauri app: a thin layer over `api.rs`, plus dialogs and the `library://` protocol. Version in `tauri.conf.json`. |
| `catalog/` | Family manifests for the built-in generators (`families/*.json`, with `ui` overlays), parts packs (`libraries/`), start values for components (`components.json`). |
| `vendor/`, `adapters/`, `sources/` | Third-party OpenSCAD projects, vendored unmodified and hash-checked (`sources/SHA256SUMS`); small adapters; upstream pins. See THIRD_PARTY.md and docs/SOURCE_AUDIT.md. |
| `libraries.json` | The OpenSCAD libraries the desktop app ships with (BOSL2, MCAD, NopSCADlib, Round-Anything, threads-scad), pinned by commit and content hash. |
| `engine.json` | Pinned OpenSCAD 2026.10.02: the WebAssembly build and the native Linux AppImage / Windows ZIP. |
| `tools/` | `build_site.py`, `build_desktop.py`, `fetch_engine.py`, `fetch_native_engine.py`, `fetch_libraries.py`, `thumbnails.py`, `set_version.py`, `import_library.py`, `parity.py`, … |
| `tests/` | `interface.py` (website, Playwright), `desktop_page.py` (acceptance test: page + real backend), `desktop_ui.py` (WebDriver on the built app), `github_mock.py`, `tauri_shim.js`, `screenshots.py`. |
| `.github/workflows/` | `site.yml` (website build, renders, tests, Pages), `desktop.yml` (site data → Linux and Windows apps → portability → release), `vendor.yml` (vendored crates for offline builds). |

### Conventions

- **Phases and releases.** The repository keeps `0.<phase>.0` in `desktop/src-tauri/tauri.conf.json`, `desktop/Cargo.toml` and `desktop/Cargo.lock` (`tools/set_version.py 0.4.0` stamps all three). CI picks the next free `0.<phase>.<n>` from the release tags and publishes `v<version>` only when the Linux and Windows apps, the acceptance test and the portability check all pass. Start a new phase by bumping the minor number.
- **Plan first.** For a new phase or a big feature, the owner wants the design written into `docs/DESKTOP_PLAN.md` before code, and a notes section ("Phase N notes", "0.N notes") afterwards recording what was built and where it differs. Some roadmap items are marked "talk through the implementation before building it": do that.
- **Commits:** a subject saying what changed for the user, a body with the details. The session tools add attribution trailers (`Co-Authored-By`, `Claude-Session`).
- **Text:** plain words, no jargon in the interface; British spellings in the UI (colour, favourites, normalise). Names the owner chose: **Parametric Models**, **Components**, **Parts Library**, **Library settings**, **SCAD Workshop**.
- **Tests grow with each phase.** Add checks to the acceptance test (`desktop_page.py`) for every user-visible feature, and to `desktop_ui.py` when it needs the real app.

### Working in a cloud session (lessons learned)

The development sandbox has a network allowlist and is reset between sessions (the clone, the scratchpad and everything built is gone). What works:

- **Rust:** crates.io is blocked. `vendor.yml` pushes `desktop/core`'s dependencies to the `crates-vendor` branch whenever `desktop/core/Cargo.toml` changes. Clone that branch, then build offline: a workspace with `core` linked to `desktop/core`, `Cargo.lock` from the branch, and `cargo <cmd> --offline --config <vendor-config.toml with directory = the branch's vendor/>` (put the flags right after the subcommand, before any `--`). New dependencies: push the `Cargo.toml` change and wait for `vendor.yml`.
- **The Tauri app can't be built locally** (no Tauri crates, npm's Tauri CLI and apt are blocked). Test the app's backend with `workshop-cli serve` + `tests/tauri_shim.js` instead (that's what `desktop_page.py` does); the real app only builds in CI.
- **Native OpenSCAD:** the `engine-cache-native` branch holds the pinned AppImage. `python3 tools/fetch_native_engine.py --file <AppImage> --out build/desktop/engine --extract build/native`. It needs `libOpenGL.so.0` and `libEGL.so.1`, which the sandbox lacks: compile a stub with a no-op function for every `gl*` symbol `nm -D --undefined-only` lists in `openscad`, `libGLU`, `libQt6Gui` and `libQt6OpenGL`, and another for every `egl*` symbol in `openscad` and the AppImage's own `.so` files, put them in `build/native/stub/` (don't stub `libGLX`: the system's is needed), and run with `LD_LIBRARY_PATH=build/native/stub`. `workshop-cli serve --engine build/native/squashfs-root` then renders natively, so the page tests can check renders.
- **WebAssembly engine:** the `engine-cache` branch holds `engine.zip`: `python3 tools/fetch_engine.py --out build/engine --zip <engine.zip>`.
- **Libraries:** `codeload.github.com` is blocked locally (CI downloads tarballs fine), but `git clone` works: clone the five libraries, check out the pinned commits, then `python3 tools/fetch_libraries.py --out build/desktop/libs --from-git <folder of clones>`.
- **Full local build:** `tools/build_site.py --engine build/engine --out _site --native-engine build/native/squashfs-root/AppRun --cli <workshop-cli>` (about 2 minutes), then `tools/build_desktop.py --site _site --out build/desktop --cli <workshop-cli>`. Local sites have no thumbnails unless you also run `tools/thumbnails.py`, so the website test's thumbnail check fails locally; that's expected.
- **Acceptance test locally:** clone `vector76/gridfinity_openscad` (full history: it needs old commit `af95c658`), `chrisspen/gears` and `BelfrySCAD/BOSL2` into one folder, run `tests/github_mock.py --repos <folder> --port 8795`, then `tests/desktop_page.py --cli … --ui build/desktop/ui --app-site build/desktop/site --starter build/desktop/starter --engine build/native/squashfs-root --home <tmp> --out <shots> --github-api http://127.0.0.1:8795`. After changing `web/`, copy it into `build/desktop/ui` (`index.html` as `build_desktop.py` writes it: without the Google Fonts lines).
- **The GitHub stand-in and the proxy:** the app's HTTP client sends even `http://127.0.0.1` through `HTTPS_PROXY`, which refuses plain HTTP; start `workshop-cli serve` with `env -u HTTPS_PROXY -u https_proxy` when it uses `tests/github_mock.py`. The app can't reach real GitHub from the session (the proxy's certificate isn't in its trust store); CI uses real GitHub. To try a starter project's conversion locally, make clones named `owner__repo` from `vendor/<x>/` and let the stand-in answer any unknown commit with HEAD.
- **GitHub from the session:** `gh` works through the REST API only (`gh api repos/DrFlGd/SCAD-Workshop/...`); GraphQL commands such as `gh release view` are refused, and editing releases isn't permitted. Pushes to `main` are fine. CI reports land on the `desktop-ci-linux` and `desktop-ci-windows` branches (logs, JSON reports, screenshots, and the acceptance test's library as `p2/library.zip`); clone those to read results when job logs aren't downloadable.
- **Careful with `pkill -f`:** a pattern that matches your own command line kills your shell.
- **One push at a time:** `desktop.yml` cancels a run when `main` moves, so a commit pushed before the previous run released is released together with it. Check the release exists (`gh api repos/DrFlGd/SCAD-Workshop/releases`) before pushing the next one.
- **Hosted runners sometimes aren't acquired** ("The job was not acquired by Runner…"): that's GitHub, not the code; re-run (`gh run rerun <id>`).

### Branches

`main` (everything), `crates-vendor` (vendored Rust crates), `engine-cache` / `engine-cache-native` (pinned OpenSCAD downloads), `desktop-ci-linux` / `desktop-ci-windows` (CI reports, force-pushed), `ci-screenshots` (the retired website's last reports), `archive/server-v1` (the first, server-based website, on hold).

### Known limitations now

- Many components can't start on their own: 326 of the 796 open with required settings empty (most are NopSCADlib printed parts whose type is a property list built by a function, and MCAD modules without defaults). BOSL2 modules whose first doc example isn't a plain call start from a later example or wait for values (55 of 201).
- Installers aren't code-signed (Windows SmartScreen asks once), and there's no auto-update: install the new release over the old one.
- Native OpenSCAD on Windows is slow with projects split into many `use`d files; the app races it against the WebAssembly engine per model (see 0.1 / Phase 1 below).
- Licenses and authors for Anylid, openGrid Shelf, Just Fit and Minimalist Kitchen are still to be supplied by the owner; Multiboard and the openGrid Connector have no sources yet; Underware's license is contradictory (kept for private use).
- GitHub Pages still serves the last website build until it's switched off in the repository's Settings › Pages (owner).

### Requested and waiting (roadmap)

- **Now (owner, 2026-10-14):** see "Owner direction of 2026-10-14" in `docs/DESKTOP_PLAN.md`: (1) rename clean-up, (2) skins from Model Library, (3) consistent projects whatever their source (one description, defaults that render, `workshop.json`; update checks kept), (4) make parts from a generator, (5) the rest of Phase 4; a review of the interface and the library's file structure after each. Opening a `.scad` file without adding it is on hold.
- **Phase 4 (0.4), model library and file formats:** the side viewer and reference documents are done (0.4.0); settings borrowed from the library and editing code in the viewer (0.4.1); still to do: collections; importing STL/3MF/OBJ/STEP; Printables ZIPs with their PDF metadata; a scratch pad of notes per part (owner, later); one item per file name with a download format picker; multi-colour and multi-part 3MF; recipes, quantities, notes, status; ZIP export; more metadata filters (creator, license family, date added).
- **Phase 5:** batch from CSV, sweeps, 3MF arranged on the bed, send to slicer, live reload of a watched `.scad` file.
- **Owner requests of 2026-10-12 (order to be chosen):** done in 0.4.3: category pickers that list every category; grouping (Condense) with a selection; scan the library for changes; library backups. Still to do: pick the picture shown for any item; add your own content (videos, documents, pictures, links) to a model, component or part; a standard project layout (`workshop.json`, read when present); open a `.scad` file without adding it; save generator output (one or a batch) as ready-made parts; several libraries open at once, each item labelled with its library. The Birdhouse bug was fixed in 0.4.2. Each is described in "Owner requests of 2026-10-12" in `docs/DESKTOP_PLAN.md`.
- **Future and experimental (unscheduled, owner's ideas of 2026-10-06):** direct slicing in the software; agent integration to edit SCAD files; and a plugin architecture (different slicers, agentic inference engines, model editors) that the first three might be built on. Each has its open questions in "Future and experimental options" in `docs/DESKTOP_PLAN.md`.
- Details and the "done when" checks for each: the Phases table in `docs/DESKTOP_PLAN.md`.

---

## Unreleased

- **Fusion files use our MitCad fork (in progress).** The owner chose to build our own MitCad (2026-10-30) and forked it as DrFlGd/Mitcad. Its `f3d-sketch-projections` branch reads where each projected sketch curve comes from (another sketch's curve, or a face's or an edge's stored curve) and makes MitCad's rebuild follow it: the Rugged Box V2 rebuilds with all 250 projections following their sources (none before) and its Fillet2 rebuilds; the older rugged box 42 of 42 (34), the latching box 33 of 37 (11; the other four project the lid where its joint turns it, which MitCad doesn't replay). The reader in `desktop/core/mitcad/` now comes from the fork at MitCad 0.36.0 (`tools/fetch_mitcad.py --repo`; the v5 parameter patch is in the fork, so `patches/` is empty; 0.36's reader opens a package's documents one at a time, so `f3d.rs`'s `choose` changed), and the app downloads MitCad 0.36.0. The new `mitcad-cli` workflow builds the fork's `mitcad-cli` for Windows and Linux (MitCad has no CI of its own) and publishes it as a prerelease, `mitcad-cli-<tag>-<commit>`. Download MitCad fetches the official release, then that build (PIN.json `cli`, recorded with `fetch_mitcad.py --cli <tag>`), puts it in place of the release's `mitcad-cli` and leaves a `.mitcad-cli-<tag>` stamp; a download from before gets only the new program. Waiting: the first workflow run (it builds MitCad's libraries, hours) and its tag in PIN.json.

## 0.4.29 — 2026-10-29 · When MitCad drops features

Released first time. After the owner's first test of Fusion files on 0.4.28: the Rugged Box broke when its size changed and the page didn't say so.

- **A Fusion model that MitCad can't rebuild whole says so.** When features fail with the chosen settings (the owner's Rugged Box at width 400: the ribs and hinges are projected sketches MitCad keeps fixed), the status line now says how many and which, in red, instead of "Preview matches your settings"; cached renders keep the warning. Findings in docs/DESKTOP_PLAN.md, "0.4.29 notes"; the MitCad limits themselves are waiting on the owner's choice (report upstream, fix in our reader, or park).

## 0.4.28 — 2026-10-29 · Download MitCad, and the app's reading goes to MitCad

Released first time. The owner asked (2026-10-29) to download and patch MitCad from the app to test the Rugged Box file, and was open to the app reading the file and passing the model to MitCad; that second route is what was built, since MitCad can't be patched on a user's machine and its `--dump` option makes it unnecessary.

- **Download MitCad from the app, and the app's reading of Fusion files goes to MitCad.** Settings › Engine › Download MitCad gets MitCad 0.35.0 (229 MB) into the app's own folder, checks its hash and unpacks it; the app uses that copy from then on. Generate now gives MitCad the design as the app's reader reads it (`mitcad-cli import-f3d --dump`), so the reader's patch reaches MitCad without changing MitCad: the owner's Rugged Box imports with all its user parameters and the lid follows them. The box bottom keeps its saved shape (MitCad can't rebuild its Fillet2 from the file's history), and the render log now names each item MitCad kept as the file's shape. Also fixed: Settings › Engine's MitCad line printed `null` and `[object HTMLElement]`. For the next person: the reader is pinned to MitCad 0.35.0's commit and `PIN.json` records that release's downloads; move both with `tools/fetch_mitcad.py --commit <sha> --release <tag>` (design: docs/DESKTOP_PLAN.md, "The app's reading handed to MitCad"). MitCad was run for real in the cloud session this time (releases download there now; `cargo test` with `WORKSHOP_MITCAD_CLI`).

## 0.4.27 — 2026-10-29 · Fusion 360 files through MitCad (experimental)

Released after one CI fix. The owner asked (2026-10-29) whether MitCad could bring parametric `.f3d` files into the app, and chose MitCad as a whole backend renderer over converting to OpenSCAD, FreeCAD or CadQuery. The owner's decision the same day: keep the reader patch in our tree and test internally before offering it to MitCad.

- **Fusion 360 files through MitCad (experimental).** An `.f3d` in a project with user parameters is now a Parametric Model: its settings are the file's user parameters (read with MitCad's reader, compiled in: `desktop/core/mitcad/`, pinned and patched by `tools/fetch_mitcad.py`), its thumbnail the preview Fusion saved, and Generate rebuilds it with MitCad's `mitcad-cli`, which you install yourself (Settings › Engine says where it was found, with Choose… and Forget). Design and findings: docs/DESKTOP_PLAN.md, "Fusion 360 files through MitCad". For the next person: the reader patch (older writers' user parameters) needs to go upstream before a released MitCad can generate from such files; `cargo test` reads a real file when `WORKSHOP_F3D_SAMPLE` names one; the render path was checked with a stand-in `mitcad-cli`, not MitCad itself (no MitCad in the cloud session). CI fix: the `build/` ignore rule had kept the reader's `f3d-design/src/build/` sources out of the repository, so the first push to main didn't compile; `.gitignore` now excepts them.

## 0.4.26 — 2026-10-17 · The UI review

Released with 101 of 101 checks after two CI fixes. The UI review after milestone 6 (owner, 2026-10-17: check the UI isn't overengineered); the review is in the plan's "UI review after milestone 6".

- **Fewer controls where they can't matter:** a project of 12 items or fewer shows no view, sort, grouping, Condense, size or filter controls; a filter whose one value every item shares is hidden; the details panel shows only while something is selected; no "desktop" badge in the desktop app; a collection's "Copies in the ZIP" only when a quantity is above 1.
- **One way to do each thing:** Choose picture… replaces the details panel's "Use as the group's icon"; the Grouped view goes (Group and Condense do it); Reset all settings goes (choose Defaults); About this model is a button beside Code and README.
- **Leftovers gone:** Home's "Projects to start with" and the Add a project tile in a condensed list.
- CI: a small project has no browse controls, no details panel without a selection, three views, About this model in the header, no Reset all; the condensed-group picture is chosen with Choose picture….
- CI fix found by the first run: the Printables filters check reads the Parts Library's filters when its project is small, then goes back to the project's page for Read again. The second run: the notes search check waits for its result while the library reads the notes just saved.

## 0.4.25 — 2026-10-17 · Pictures and your own content

Milestone 6: pictures and your own content (owner's items 1 and 2 of 2026-10-12), released with 100 of 100 checks after one fix; details in the plan's "0.4.25 notes".

- **Choose picture…** (a project's ⋯ menu, a model's ⋯ menu, a part's ⋯ menu): the render, a picture from the project's files, an item's thumbnail (for a project), or one of your own, copied into the library. It replaces Edit details' Icon list.
- **Add content…** (the same menus): a file (an assembly guide, a build video, photos) copied into the library, or a link. It's shown in the side panel beside the project's documents, and Remove sets the file aside in `backups/removed/`.
- A part's page has a ⋯ menu beside Add to collection.
- The library format: `pictures/` and `attachments/` in a project's folder, `icon` and `attachments` in `metadata.json`; Check library lists content or a picture whose file is gone (`docs/LIBRARY_FORMAT.md`).
- CI: a project's picture from its files and one you add; a PDF and a link added to a model, and Remove.
- Fixes found by the first run: a PDF you added didn't open in the side panel; removing content left the side panel open over the page; the update check waits for the last thumbnail, which can be written just after its model's check.

## 0.4.24 — 2026-10-17 · Reading settings reliably

Reading settings reliably (owner, 2026-10-17), with 99 of 99 checks; details in the plan's "0.4.24 notes".

- **Choices read from the code:** a text setting the model compares with words (`style == "round"`) is a drop-down of those words, the default first. If reading them goes wrong the model keeps OpenSCAD's list and the project lists a "reading" problem.
- **Typed in:** clear a setting's Choices in Edit form and it becomes a field you type in, and stays one when the project is read again.
- **Arithmetic as numbers:** a component's setting written as arithmetic (`2*3`, `[10, 20/2]`) is a number or a list of numbers, not text.
- **Defaults that don't render:** a model's page shows its first preset that does, and says so; Check defaults records it as rendering with that preset.
- `tests/settings/`: models that were read wrongly, each with what should be read; `cargo test` reads them all. Add a case there when one turns up.
- CI: a word-compared setting is a drop-down; cleared in Edit form it's typed in; a model whose defaults fail shows its preset.

## 0.4.23 — 2026-10-17 · Future features

The future features of 2026-10-16 (owner), released with 98 of 98 checks after a CI fix and a fix; details in the plan's "0.4.23 notes".

- **Show in folder** for a model (its .scad file), a part (its file) and a project (its folder), so the original files are quick to find.
- **2D models:** a model that makes a flat shape is shown as a drawing and downloads as SVG or DXF; its thumbnail is the drawing and Check defaults counts it as rendering. Save as part is off for drawings for now.
- **Your notes** on a model (⋯ › Your notes) and a project (a Notes tab): free text kept in the project's `metadata.json` (`my_notes`), found by search.
- **One Code button:** a model's page has Code and README in its header; the right-edge tabs appear only while the panel is open.
- **Batch generate** replaces Make several.
- **Edit form** fields are filled in with what was read from the file and stay editable; only what you change is saved, and Put back restores it.
- **Add a project:** the File tab takes one .scad file as a project of its own; the Suggested row is gone.
- Several libraries open at once moves to the backlog (owner, 2026-10-17).
- CI: a 2D model, notes, Show in folder and the header-only Code button are checked; the Add a project check fills in a GitHub link itself now there are no suggestions.
- CI fix found by the first run: the notes check read the project's `metadata.json` while the model's "Notes saved." toast was still showing; it waits for the file itself.
- Fix found by the second run: a project's notes typed while the library was being read again (just after saving a model's notes) were put back to what was saved before, and weren't saved. What you type is kept, and saved when the Notes tab closes too.

## 0.4.22 — 2026-10-16 · The library format

The library format (owner, 2026-10-16), before milestone 6; 95 of 95 checks.

- **The library format is written down:** `docs/LIBRARY_FORMAT.md` describes every folder and file in a library, which hold your work and which the app can make again, each JSON file's fields, and the ids collections use. The library's README.txt says the same in short.
- **Library format 2:** every JSON file the app writes says its `format` (`desktop/core/src/format.rs` names them all, and `write_json` stamps them), and a file written by a newer version is read but never written over. A library from 0.4.21 or before is upgraded when it's opened, after the files it changes are copied as they were to `backups/format-1-<date>.zip`. An older app then opens the library read-only and says why.
- **One recipe shape** for saved settings and made parts: model, project and its version, the files' fingerprint, changed settings, code edits, file type (`output`), dates. Older names (`version`, a made part's `format` and `date`) are renamed by the upgrade and still read.
- **Check library** (Library settings): lists what isn't as the app keeps it, as "Can be repaired" (a file not yet in the format, an unfinished copy from an add that stopped, what was made from a version whose files are gone, a missing README) and "To look at" (a file that isn't valid JSON or is newer than the app, a project folder without source.json, missing files, saved settings or a collection entry for something that isn't there). Repair never changes a project's files or anything you made.
- CI: the files the tests' app wrote all say their format and recipes have one shape; Check library repairs a planted unfinished copy and lists a collection entry that refers to nothing; a format 1 library is upgraded on opening with its backup.
- CI fix found by the first run: two checks read a recipe's old names (`version`, a made part's `format`); they read `project_version` and `output`.

## 0.4.21 — 2026-10-16 · The logo

The logo (93 of 93 checks).

- **The app has its logo** (owner's design, 2026-10-16): a drawing sheet with a dimensioned cube. The app icon, installer and window use it, drawn again as a vector (`desktop/src-tauri/icons/src/logo.svg`) with a simpler form for 16 to 48 pixels (`logo-small.svg`: no grid, dimensions or hidden lines, a larger cube). The sidebar shows the small form in the skin's colour, and it's the favicon. `tools/make_icons.py` makes the icon files from the two drawings.
- CI: the logo is in the sidebar and is the favicon.

## 0.4.20 — 2026-10-16 · Review 6's items

Review 6's items (0.4.20), released with 92 of 92 checks after three CI fixes.

- **A model's colours show without choosing 3MF** (UI-39): after the preview of a model that calls `color()`, the app makes OpenSCAD's 3MF behind it, shows its colours in the 3D view, and with STL chosen says so ("It has 3 parts in 3 colours: choose 3MF to keep them"). **Show the file's colours** sits in the 3D view for models and parts alike, on unless you untick it; the filament swatches hide while it's on.
- **Made parts keep the chosen format** (F-28): Save as part, Make several and Make again write a .3mf from OpenSCAD (colours and objects kept) when 3MF is chosen beside Download; the recipe records the format.
- **Printables downloads** (F-26): the same files downloaded again aren't added as another version ("has these files already"); a download dated before the version in use is only listed under Versions; Read again dates versions from the page's PDF; an update's summary lists parts added, removed and changed.
- **Collections** (UI-40, F-27): several copies are counted as they're printed ("1 of 3", with − and +), a finished row is quieter rather than struck through, and Notes shows its hint only while typing. A model entry keeps the files it was added with and says "Made from an earlier version" with **Use the current version** when its model changes; Export says how many changed.
- One **Updated** filter on a Printables project, and **Published** grouped by age (UI-38); a part with one file has **Download (size)** with its format beside it (UI-41); **Add a project** is under the sections in the sidebar, and Home lists collections (UI-42).
- The library's README describes what `metadata.json` holds and how collections refer to items (F-29).
- Roadmap: the library format (a written spec, a version in each JSON file, one recipe shape, Check library) comes before milestone 6 (owner, 2026-10-16); see the plan's "Owner direction of 2026-10-14".
- Roadmap (owner, 2026-10-16): reading settings reliably follows the library format (OpenSCAD's own parameter list, types from evaluation, failing defaults repaired from the project's examples, drop-downs for text settings with known values); future features: Show in folder, "Batch generate" in place of Make several, 2D models shown and downloaded as SVG or DXF, no suggested projects in Add a project, and a single .scad file as a project; then Notes on projects and models, one Code button on a model's page, and the Edit form filled in with the values read from the file.
- CI: colours shown and named with STL chosen; a part saved as 3MF; the same Printables files not added twice; one Updated filter; printed copies counted; a collection's model marked when it changed; Home's collections; a one-file part's Download.
- CI fixes found by 0.4.20's run: the filters check opens the project's Items tab (the About tab, opened earlier, has none), and Windows gets its Edge driver for WebView2's version (or the runner's own) when msedgedriver-tool can't be built (a yanked dependency).
- The app reads and writes ZIP and 3MF files with version 9 of the `zip` crate: on 2026-10-16 every earlier version was withdrawn from crates.io after symlink and extraction fixes, so builds without a lock file stopped resolving it.

## 0.4.19 — 2026-10-14 · Milestone 5, last part: multi-colour 3MF from OpenSCAD's own export

Multi-colour 3MF.

- **A model's 3MF keeps its colours and separate parts**: with the app's OpenSCAD, Download as 3MF now asks OpenSCAD for its own 3MF (with lazy union), so each top-level object stays its own object and each `color()` is kept. The status line says what's in it ("The 3MF has 3 parts in 3 colours") and the 3D view shows those colours. Collections export their models' 3MF the same way. With the WebAssembly engine the 3MF is written by the app as before, one object without colours.
- CI: a model with three coloured objects downloads as a 3MF with three objects and its colours, named in the status line.
- Fixed: a component pinned while the library was being read could be missing from Parametric Models (and its Edit form…) until the next change. The app now drops a list of models built while something changed. Found by CI for 0.4.19; the test also waits for the pin to be listed.

## 0.4.18 — 2026-10-14 · Review 5: newer Printables downloads as versions, duplicates, Keep apart, a one-line Download

Review 5's items.

- **Another download of a Printables model is a new version of its project** (F-23), not a second project: when a ZIP's PDF names a page a project already has, the files go in as a version of it, dated by the page's "updated" date, and its page offers **Use the new version** as an update from GitHub does. The first download's version is dated by the page too.
- **Read again reads the page's PDF again** (F-24), so a Printables ZIP added before 0.4.16 gets its details; your edits still win.
- **Duplicates stay listed** (UI-34): a project's page notes each of its files that's in another project too, byte for byte, with **Open the other** and **Hide this copy**.
- **Keep apart** (UI-35) beside Make one part: files that look like one part but aren't stay separate and aren't offered again (kept in the project's `metadata.json` as `apart`).
- **Download and its format picker** (UI-33): one line, as high as Generate; the button says "Download" ("Download (1 KB)" for a part) and the picker names the format.
- **Dropping files** (UI-36): each choice is an outlined button, and files that share no name are named "Dropped files" and the date unless you type a name.
- The Printables PDF is listed as **Printables page (PDF)** (UI-37), and the library's README describes `origin.site` and `detected` (F-25).
- CI: a newer download of a Printables ZIP becomes a version of the first and is used; Read again brings back details; Keep apart; duplicates on the project's page.

## 0.4.17 — 2026-10-14 · Milestone 5, fourth part: collections, with quantities, printed ticks and one ZIP export

Milestone 5, fourth part (0.4.17): collections.

- **Collections** in the left menu: lists of parts and models kept for one use (an RC car, a workshop wall), with **New collection** (the + beside the heading, or the command palette). They're lists, not copies: one part can be in several, and the library keeps them in `collections/<id>/collection.json`.
- **Add to collection…** in the inspector (for one or several parts and models), on a part's page and on a model's page. A model keeps the settings it has then (and a saved setting's code edits, when one is chosen and unchanged). The same thing with the same settings is counted again instead of listed twice.
- **A collection's page** lists its items with a thumbnail, the format to use (a part's own formats; STL or 3MF for a model), the quantity, a **Printed** tick and notes, with "1 of 5 printed" and a bar. Rename and Delete are under ⋯; deleting or removing an item can be undone from the message.
- **Export as ZIP…**: each item in its format, a file per copy or one file with the count in its name (chosen on the page), models made at their settings first, plus `parts.csv` (item, quantity, printed, format, files, project, authors, license, notes) and `README.txt` with each item's credits and license. Cancel stops it.
- Merging libraries brings their collections too; the library's README describes `collections/`.
- CI: the acceptance test makes a collection of a part, a made part and a model with changed settings, sets a quantity and a printed tick, and exports it.
- Review 5 (of 0.4.16) is in docs/DESKTOP_PLAN.md with its items planned for 0.4.18; multi-colour 3MF moves to 0.4.19.
- Fix (CI's first run of 0.4.17, and the unexplained call in 0.4.16's): a model whose project was being read again could be asked for while it was briefly missing, which failed a request. The page now asks for a model's details with `model_get`, which answers "not there" without failing.

## 0.4.16 — 2026-10-14 · Milestone 5, third part: a Printables download's details read from the page's PDF

Milestone 5, third part (0.4.16): Printables downloads.

- **A Printables download fills in its project's details.** Add a Printables ZIP (Add project › ZIP, or drop it on the window) and the PDF of the model's page inside it is read: the project gets the model's name, its creator (linked to their profile), license, tags, summary and the page's address, with the published and updated dates and Printables' category as fields, and any print settings as fields too. Edit details says each came "from the Printables PDF"; the project's About tab shows who made it, when, and the page's description; the header reads "ZIP file from Printables". A PDF that can't be read changes nothing: the project is added as any ZIP is.
- The PDF reader is the app's own (`desktop/core/src/pdf.rs`, no new library): it finds objects by scanning the file, unpacks object streams and Flate streams, maps text through each font's ToUnicode map, and rebuilds lines from where the text sits. Sections are found by Printables' headings, tags are the white text on its chips, and the license comes from its creativecommons.org link, else its wording.
- CI: unit tests read openGrid's Printables page PDF (`libraries/opengrid-official/`) and a hand-made PDF (a standard font, nothing compressed, coordinates from the bottom); the acceptance test adds a Printables-style ZIP and one whose PDF is damaged.
- CI's first run of 0.4.16 passed every check but saw one call to the app's backend fail (400) without saying which. The acceptance test now prints each failed call with what it asked for and the check before it.

## 0.4.15 — 2026-10-14 · Milestone 5, second part: one download with a format picker, a model's render as 3MF, near misses made one part

- Milestone 5 planned in docs/DESKTOP_PLAN.md, "Ready-made models and collections design": previews from 3MF and OBJ and dropping files on the window (0.4.14), one download with a format picker (0.4.15), Printables ZIPs (0.4.16, then Review 5), collections with ZIP export (0.4.17), multi-colour and multi-part 3MF (0.4.18, then Review 6), reusing Model Library's mesh reader, duplicate check and drop handling.

Milestone 5, second part (0.4.15): one download with a format picker.

- **A part with several formats has one Download** with the format beside it (STL · 3MF · STEP …). It offers the format you last chose for that part, else Settings › Downloads › **Preferred format** ("STL where there is one", "3MF where there is one, otherwise STL" or "STEP where there is one, otherwise STL"). Parts with labelled variants keep a button per file.
- **A model's render downloads as STL or 3MF**, picked beside Download and kept per model; the 3MF is written by the app from the render (one object, named after the model).
- **Near misses:** files in one folder whose names differ only by a version or copy mark (`latch_v2.stl` beside `latch.3mf`) and share no format are listed on the project's page with **Make one part**. A part made one says so on its page, with **Split into separate parts**. Kept in the project's `metadata.json` (`merged`), so it holds when the project is read again or updated.
- CI: checks for the part and model pickers (the 3MF read back), and a near miss made one and split.
- Fix (CI's first run of 0.4.15): the acceptance test expected Settings without its new Downloads section, and an `import zipfile` left inside the test hid the module for the new 3MF check.

## 0.4.14 — 2026-10-14 · Milestone 5, first part: 3MF and OBJ previews, dropping files on the window, duplicates named

Ready-made parts look and come in like generated ones. 72 of 72 acceptance checks, on the third run (two fixes below).

- **3MF and OBJ parts show in 3D.** A part's 3D view and thumbnail now come from its 3MF, then STL, then OBJ (before, only an STL gave a 3D view). The app reads the file with Model Library's mesh reader and hands the viewer an STL. A 3MF with colours of its own shows them when you tick **Show the file's colours** on the part's page (kept for next time); otherwise it's drawn in the filament colour. A 3MF's own picture (the one its slicer saved) is its thumbnail.
- **Drop files, folders and ZIPs on the window** to add them. A dialog offers **Add as a project** (copied into the library, like Add project; loose files become one project, named as you like, and each folder or ZIP its own), **Add to <project>** when one of your own projects is open, or **Add as a project of your own** (a new folder in `local/`). A hint shows while dragging.
- **Files already in the library are named** when a project is added ("gear copy.stl is in Made parts already"), found by size and then SHA-256. They're added anyway.
- CI: the acceptance test drops a folder's worth of files (STL, a two-colour 3MF, OBJ, STEP, a 3MF with a picture, a copy of a made part) and one file onto a project of your own, with screenshots.
- Fix (CI's first run of 0.4.14): after **Add to <project>** finished copying, the app opened that project's page even when you'd moved to another page meanwhile, which is what pulled the acceptance test off Library settings before Restore. It now opens the project only if you're still where you dropped the files, and the test waits for the copy's message.
- Fix (CI's second run of 0.4.14, 72 of 72 checks but two page errors): a 3MF without a picture of its own made the thumbnail maker's request fail, which the browser logs. The app now answers with nothing, and the page uses a picture only when there is one.

## 0.4.13 — 2026-10-14 · Review 4: the Made parts shelf, example parts, your own licenses, changes counted by the files a model uses

Review 4 (https://claude.ai/artifact/RAhYcNBQ9WnFJowHZVfunC, UI-28 to UI-32 and F-19 to F-22), all nine items. 69 of 69 acceptance checks.

- **Made parts shelf** (UI-28): no source line, Read again or Versions; it says each part keeps the license of the project it was made from, and each card names the model and project it was made from ("Basic Bin · Gridfinity Rebuilt").
- **Example parts** (UI-29): the link reads "Open the example as a model", plus the main file's model when it has settings; part lists sort by number (Brick 1x2 before 1x17); the project page's examples note is a plain information box, not a warning.
- **Tabs** (UI-30): two open tabs with the same name both show their project, even when the tab bar is crowded.
- **Your own projects' licenses** (UI-31): projects in `local/` or linked with no license stated read "Yours" in Library settings › Projects, with no Check, and no longer count as needing a license check; Made parts reads "Each part's own". A license set for the whole library still applies.
- **Status bar** (UI-32): the space before a job's timer is back ("Making thumbnails · 1 of 4 · 3 s").
- **Made parts folders** (F-19): when another model in the library has the same name, a model's parts go in "<model> (<project>)" ("Basic Bin (Gridfinity Rebuilt)"). Existing folders stay; Make again keeps using the part's own folder.
- **What counts as changed** (F-20): a recipe records the fingerprint of the files its model uses, and a made part says its model changed only when those files did, not when anything else in the project changed. Examples made into parts are kept by the same fingerprint (`sources/<id>/examples/<fingerprint>/`), so editing one file remakes only the examples that use it. Example parts made by 0.4.12 are made once more on first start.
- **Examples asked about first** (F-21): until a folder of examples is answered on its project's page, its files aren't listed under Parametric Models or checked at their defaults, and Add project's message asks you to choose.
- **README** (F-22): describes `local/Made parts/<model>/` and the `<name>.recipe.json` beside each part.
- CI: the acceptance test checks all of the above and takes screenshots of Save as part, a made part's page, Make several, the Made parts shelf and an example part.

## 0.4.12 — 2026-10-14 · Folders of examples made into parts; a model no longer dropped when its settings fail to read once

Milestone 4, last part (0.4.12): Review 3's F-15, a folder of examples.

- **A folder of examples is asked about once** on its project's page: more than 20 files in one folder that each only call the project's main file (LEGO.scad's `examples/`, a piece per file) can be **made into parts**, **listed as models** (as before) or **left out**. The answer is kept in the project's `metadata.json` (`examples: { folder, as }`), so it holds across updates, and can be changed from the same place.
- **Made into parts**, each example is made once at its own values in the background ("Making parts from examples · 3 of 180") into `sources/<id>/examples/<version>/`, listed in the Parts Library under the project with its picture, and its page still opens the model for other sizes. One that doesn't render is listed among the project's problems and isn't tried again until its defaults are checked.
- The library's README names the `examples/` folder.
- CI: the examples check waits for the example's own model page before reading its status (it read the previous page's message).
- **Reading a project no longer drops a model whose settings OpenSCAD failed to read once** while other work was running: it tries that file again on its own first. In CI, Gridfinity Extended's move to GitHub (F-1) lost a different model each time (baseplate, connector-clips) and was put back as it was. When a move still loses a model, its message now says what reading said about it. Each settings read also gets its own working folder.
- **A model's header is no longer drawn over by the one you just left** when the library is read again as you open another (pinning a component did it): the ⋯ menu then had the other model's actions, and closed under you.

## 0.4.11 — 2026-10-14 · Save as part and Make several: made parts kept in the Parts Library with their recipes

Milestone 4, first and second parts: Save as part and Make several, released together (0.4.11's own CI run was cancelled when Make several was pushed before it finished; see For agents › Working in a cloud session).

Make several:

- **Make several…** (was Batch… in a model's ⋯ menu) can **Save to the Parts Library** as well as download a ZIP: each part goes to Made parts with its recipe, named from what varies ("Basic Bin 2×1×6", "Bevel gear 22"), so each can be opened with its settings or made again.
- **Your saved settings can be what's made**: Make several offers them as a second way to choose (tick the ones to make). Ones saved with code edits aren't offered, since every part is made with the form's code.

Save as part:

- **Save as part…** under Generate and Download STL keeps what you made in the Parts Library: a name ("Basic Bin 2×2×6" to start with) and a category (the model's), made first if the preview isn't up to date. It goes in `local/Made parts/<model>/` as `<name>.stl` with `<name>.recipe.json` beside it: the model, its project and version, the settings you changed and any code edits. A message says where it went, with **Show**.
- **A made part's page says what it was made from** ("Made from Basic Bin (Gridfinity Rebuilt), 1 setting changed.") with **Open with these settings** and **Make again** (the same settings, replacing its files and keeping its name), and says when the model's project has been updated since. The inspector says the same with Open with these settings. Made parts carry their project's license.
- **Parts in your own projects** (`local/` and linked folders) show their previews, thumbnails and downloads; they were looked for in a copy only added projects have.
- **The ⋯ menu stays open when the library is read again** while you're on the same model (a pin, a thumbnail); it closed under you, which also failed the acceptance test's Edit form step.
- CI: the acceptance test gives the bevel gear's page up to 5 minutes to open while LEGO.scad's examples are checked in the background, counts thumbnails made meanwhile as kept, and logs the message after converting starter projects.
- CI: the Save as part test makes its bevel gear project again first (the test deletes it earlier, in Library settings' trash check), and the starter conversion check logs what the conversion said when Gridfinity Extended stays as it was.

## 0.4.10 — 2026-10-14 · Review 3: the 3D view beside the side viewer, Copy settings, messages in the status bar, one credit pattern, older projects catch up

Review 3's items (https://claude.ai/artifact/NU5qAAphTyG7ioqDRnZT4S), all but F-15, which goes with milestone 4.

- **The 3D view frames the model in the part you can see** (UI-20): it fits the narrower of the view's height and width, so a narrow window no longer cuts the sides off, and with the side viewer open the model is centred beside it. It fits again when the window or the side viewer changes, unless you've moved the camera.
- **The side viewer's Code and README tabs read downwards in the Linux app** (UI-21), where they showed "C" and "R". The real-app test checks each tab is taller than it is wide.
- **Copy settings replaces Share in the app** (UI-22): it copies the form's settings as an OpenSCAD Customizer set; pasting one on a model's page (not into a box) fills in the form. Share's links pointed at the app's own internal address, which nothing could open. Save's tooltip says "in the library".
- **Suggested projects say what you get** (UI-23): each has its number of models (LEGO.scad: 182, 180 of them examples), its license with "personal use only" for non-commercial ones, and the name it will have; Add project shows them under the chips.
- **Messages are part of the status bar** (UI-24), in its font and colours in every skin, with the counts hidden while one shows instead of cut off beside it. Jobs read "Making thumbnails · 2 of 18 · 12 s", without a doubled "……".
- **One credit pattern** (UI-25): "License <name>" or "License not stated", always a link, plus **Check** when the license isn't cleared for sharing prints. A component's module and which copy of its library (yours or the app's) sit under its name, and its library's authors are credited the same for either copy. Licenses & credits lists the component libraries. Part pages get their pack's README button and side tabs.
- **Notices above the form stack** (UI-26): broken, failed at its defaults, its pinned library changed, and (new on the page) defaults outside the form's limits all show, red only for the first two. The pin notice links to Library settings › Libraries, and `#/library-settings/<section>` opens at that section.
- **Plainer words** (UI-27): "From an earlier app" for starter projects, "(version in use)", "updates: changes on its main branch", the update banner quotes the commit's message, Edit the form uses the form's names and says "the Defaults list". Layer Line's top bar and status line show a plain zero; an update check's error is a short sentence with the details on hover.
- **Older projects catch up** (F-13): at start-up, projects read before 0.4.7 (starter projects included) get their `workshop.json` and their defaults checked against the form's limits, with old plain-number steps fitted as reading does now (so 1.5 isn't "off its step of 1"). Library settings › Projects offers to check every model that hasn't been rendered at its defaults (projects added before 0.4.6, starter projects), keeping their thumbnails; it's offered rather than started because it can take a while.
- **The app's description is an overlay** (F-14): `description.json` lists the files it leaves out on purpose (helpers, demos); any other file that draws something, such as one upstream adds later, becomes a model and shows in an update's summary. It records `description_version` and `app_version`, and one written by 0.4.9 is replaced. `workshop.json` now carries every key a model was described with (Gridfinity Rebuilt's `engine_override` was dropped).
- **Converted starter projects' old copies** (F-16) are removed when their files are the same as upstream's, or say what they are ("The app's copy of 910e22d, before it came from GitHub") in Versions.
- **Starter projects that are on GitHub record their repository and commit** (F-17) in `origin`, and Check for updates says when GitHub has something newer, read only. HSW, GridFlock, Rugged Box and openGrid say why they can't be updated in place.
- **The library's README keeps its indentation** (F-18), names the seven kinds of origin, and says linked projects aren't in the library or its backups and what `description.json` decides.
- The acceptance test waits for LEGO.scad's main model to be checked at its defaults, not all 182 of its examples (the slowest step: one 0.4.9 build failed without publishing its report, probably there).
- CI fix: the acceptance test expects the component page to name your BOSL2 (the test adds its own), reads the Drafting Sheet's capitalised messages without minding case, and waits for a library switch's own message before counting projects to read again (it lists them when it fails). The library's README keeps "Remove versions not in use" on one line.

## 0.4.9 — 2026-10-14 · Starter projects from GitHub, suggested projects, one head for model pages, pins that know their library copy, projects that follow releases

Milestone 3, last part (0.4.9): starter projects as GitHub projects, suggested projects, and Review 2's UI-16, F-9 and F-11.

- **Starter projects that are on GitHub can come from there** (F-1): Library settings › Projects offers it once, for libraries made before 0.4.4. Gridfinity Rebuilt, Gridfinity Extended, Gridfinity Basket, Cullenect Label, Label Generator for Gridfinity and Underware are downloaded at the commit the app's copy came from, in their authors' own layout, and follow the default branch for updates. Each keeps its id, models, saved settings (code edits move to the new file paths) and thumbnails; one that doesn't read, or loses a model, is put back as it was. The other 13 stay as they are: HSW's first model is a file from an older commit, GridFlock and Rugged Box need git submodules GitHub's download leaves out, openGrid uses the app's adapter, and the rest aren't on GitHub.
- **The app's description of projects it knows** (`sources/<id>/description.json`, workshop.json's shape): which files are models, their names and forms, from the family manifests. It's used for the converted starter projects and when Gridfinity Rebuilt or Extended (and the others) are added from GitHub, unless the project has its own workshop.json; if none of the files it lists are there any more, the project is read as usual. A project's own description now gives its name, summary and license over what was guessed from the repository.
- **Suggested projects:** a new library's Home lists projects to start with (Gridfinity Rebuilt, Gridfinity Extended, gridfinity_openscad, Gears, LEGO.scad; `catalog/suggested.json`), each added from GitHub with one click, and Add project offers the ones not yet in the library.
- **One head for model, component and part pages** (UI-16): one credit line (author, source, license, documents, and where a pin came from); the notes, Edit form, Copy code, Pin as a model, Batch, Help under each setting and Reset all settings are in the ⋯ menu.
- **Pins record the library copy they were made from** (F-9), and component thumbnails are kept per copy (`thumbs/components/<library>/<copy>/`). When a pinned component's library changes (your copy instead of the app's, or a new version), the pin says so, lists settings that no longer exist, and **Keep it with this copy** accepts the change.
- **A project added from a tag, release or commit asks what to follow** (F-11): newer releases (the default for a version number) or every change on the branch. `origin.track` is `releases` or `branch:<name>`; `origin.added_from` keeps what the link named. The project's page says what it follows.
- Two models of a project's description can use the same file (Gridfinity Extended's baseplate and connector clips).
- The acceptance test turns the starter projects into GitHub projects (real GitHub) and adds LEGO.scad from the suggestions, checking every model at its defaults.
- CI's report branches leave out files over 95 MB (listed in `LEFT_OUT.txt`; the test library's ZIP grew past GitHub's 100 MB limit with the downloaded projects), so the reports are published again.

## 0.4.8 — 2026-10-14 · Review 2's small items: messages, dialogs, the side viewer's room, Settings, update checks

Milestone 3, third part (0.4.8): Review 2's small items (`docs/DESKTOP_PLAN.md`, "Review 2").

- **Messages sit in the status bar's place** at the bottom left, so they no longer cover the colour swatches or the Generate button (UI-12).
- **Drafting Sheet's body text is in Overpass on Windows too:** the faces that only fix "·" had no weight range, so Windows took them for regular text (UI-13). The acceptance test now loads each skin font at 400, 600 and 700 for letters.
- **Dialogs:** the first field has the focus when one opens, Escape closes it, and a click outside no longer throws away what you typed (UI-14).
- **With the side viewer open, the 3D view's size bar and colours move clear of it;** in a narrow view the size takes one line and the view buttons fold under ⋯ (UI-15).
- **Settings is laid out like Library settings:** jump links, one style of intro, a rule between sections (UI-17).
- **Words:** messages say models, not generators ("Added BOSL2: a library, its modules are in Components."); the form editor no longer mentions family manifests; update checks name the project, not its id; dates read one way everywhere; Drafting's capitals status bar keeps "12 s" lower case (UI-18). A tab for a model no longer in the library shows its file name, not its id.
- **Skins:** Layer Line's category counts have a spool-coloured dot instead of white text on the colour, and its numbers use a face with a plain zero (Atkinson Hyperlegible's slashed zero read as ⌀); every colour swatch has an edge that shows on dark, so Black stays visible; Parts Bin's status bar and card lines are larger (UI-19).
- **Update checks keep what they knew** (F-7): a failed check (offline, GitHub's hourly limit) keeps a skipped or waiting update and adds the error beside it; `source.json` is written only when what it says changes, so a synced library or one in git no longer changes with every daily check.
- **Accepting an update renders the new version's models at their defaults again and remakes their thumbnails** (F-8); so does a change to a local project's files. `checks.json` records the version and engine the thumbnails were made with.
- **Saved settings record the project version they were made with, and a code edit is based on the project's own file** (its hash in `base`), not the app's copy of it (`copy`, when the app made one) (F-10). Projects with such copies are read again once (derived format 5) so their models know the original's hash.
- **The library's README explains the folders that differ:** your own projects keep their files in `local/`, `sources/pinned/pins/` must be kept, `previews/` holds models made for parts, and `files/` can hold several versions (F-12). A README from 0.4.4 to 0.4.7 is replaced; one you changed is left alone.
- Small things from Review 2: the listing head wraps instead of cutting the page's name; no stray scrollbar under the side viewer's tabs; Library settings and Licenses have their own menu icons; a yes/no setting's box sits in the value column like every other setting.
- **The acceptance test adds a ZIP project and a project linked to a folder** and checks they're kept like the others (origin, version files, `workshop.json`, checks, thumbnails), for Review 3.
- The acceptance test opens Library settings before backing up the library (the new checks before it end on another page), and lets thumbnails finish before it removes a model they would load.
- A model that's no longer in the library (its project was read again without it) isn't asked for when an open tab or a page refresh still names it; its page says it doesn't exist.

## 0.4.7 — 2026-10-14 · Projects describe themselves with workshop.json; origin and versions recorded the same way

Milestone 3, second part (0.4.7): one project description, whatever the source (Review 1's F-2, F-3 and F-5).

- **`workshop.json`, a project's own description** (schema in `schema/workshop.schema.json`, a family manifest's shape): when a project has one at its top, the app takes its models (which files, their ids and names), notes, forms (`ui`, `hidden`, `fixed`), presets, name, summary, category, tags, license, authors and links from it instead of guessing; settings still come from the files. A file it lists that isn't there is noted as a problem.
- **The library says what each project is without the app:** every project read gets `sources/<id>/workshop.json` beside its files (rebuildable; listed in the library's README), and derived data says whether the project described itself (`described_by`).
- **Save as project file** (⋯ on one of your own projects, in `local/` or linked) writes its description, with your edits (names, forms; deleted models left out), into its folder, so it goes with the files.
- **Every project records where it came from and its versions the same way:** `origin.type` (`github`, `zip`, `folder`, `linked`, `local`, `app`, `pinned`) in every `source.json`, and `date`, `added`, `files` (count) and `content` (fingerprint) on every version; libraries from earlier versions are filled in when opened.
- **Adding something that's already in the library says so** and offers to open that one: the same GitHub repository (as before), the same ZIP file (by its SHA-256) or the same folder.
- The project that holds pinned components records its origin (`pinned`) as soon as it's made, not only when the library is next opened.

## 0.4.6 — 2026-10-14 · Every model checked at its defaults, defaults that fit their limits, readable setting names

Milestone 3, first part (0.4.6, "Consistent projects design" in `docs/DESKTOP_PLAN.md`): defaults that work, and Review 1's UI-4 and UI-5.

- **Every model is rendered once at its defaults** when its thumbnail is made, and the result is kept with the project (`sources/<id>/checks.json`, rebuildable). A model that doesn't render is marked in Library settings › Projects ("Doesn't render at its defaults", with OpenSCAD's message on hover) and says so above its settings, and isn't tried again until the project or the engine changes. **Check defaults** in a project's ⋯ menu renders every model again and says how many failed (D-1).
- **Defaults fit their own limits** (D-3): a number with no Customizer comment steps by its default's last decimal, as in OpenSCAD's Customizer, so Gridfinity Anylid's 1.5 and 2.5 units no longer show as invalid; a default off its slider's step, outside its range or not among its choices is marked in Library settings ("Defaults outside their limits"), and the form accepts it rather than calling it invalid.
- **NopSCADlib parts start on the first of the library's own list of types** (`fans = [fan17x8, …]`, also from `batteries.scad`-style files), so more of them render straight away: 326 of 796 components now open with settings to fill in, from 342 (D-2).
- **Component settings are named like model settings** ("Mate teeth", not `mate_teeth`), with the name in the code on hover; model settings show it on hover too (UI-4).
- **A model's page is titled with its own name**, as on its card and tab, with its project (a link to the project's page) or library on the line under it (UI-5).
- Making thumbnails while they're being made (a project added meanwhile) now goes round again afterwards, so the new project's models aren't left until the next start.

## 0.4.5 — 2026-10-14 · Skins, one head for listing pages, readable tabs

Milestone 2 of the owner's direction of 2026-10-14: Model Library's skins, and Review 1's interface items UI-2, UI-3, UI-7, UI-8, UI-10 and UI-11.

- **Skins** (`docs/DESKTOP_PLAN.md`, "Skins design"): Settings › Appearance has a Skin choice above Theme: **Drafting Sheet** (the new default: square corners, fine rules, drafting blue, Overpass and Overpass Mono, a faint grid behind pictures and the 3D view, mono section headings with a rule, the status bar as a title block), **Parts Bin** (machine-tool green, Sofia Sans, label-tape headings, pegboard, drawer-front cards), **Layer Line** (silk magenta, Atkinson Hyperlegible and M PLUS Rounded 1c, pill shapes, layer lines, a spool colour per category) and **Classic** (the look up to 0.4.4). Each has light and dark colours; Night keeps its own. The command palette lists them too. Colours, fonts, corners and icon lines that were written in place are now names the skins set (`--chrome-ink`, `--accent-hover`, `--accent-edge`, `--star`, `--font-head`, `--font-mono`, `--head-stretch`, `--radius-xs`, `--icon-stroke`…), so the top bar works light or dark. Home, star, box, settings and search icons have a drawing per skin (Model Library's). The skins' fonts (SIL OFL) are bundled by `build_desktop.py --fetch-fonts`; CI installs fontTools to cut Layer Line's heading font to Latin letters.
- **The size of the model and the view buttons share one bar** above the 3D view, size on the left, buttons on the right, wrapping under each other in a narrow window, so the buttons no longer cover the size (UI-2).
- **Every page that lists things has the same head** (UI-3): its name and count with the view buttons and the inspector button on the right; below, Sort, Group, Condense and the thumbnail size; then the filters. On a project's page that row comes under the project's tabs, and the project has one main button (Check for updates, or Read again) with Edit details…, Hide, Open its folder, Treat as… and Delete project… under ⋯.
- **Tabs keep a readable width** (UI-7): at least 170 px; when the row is crowded they leave out the project's name first, and tabs that don't fit go in a "+N ▾" list at the end of the row (where they can be opened or closed). The open model's tab always stays in the row.
- **Messages appear above the status bar at the right** (UI-8), clear of the settings column and its Generate and Download buttons.
- **A drop-down whose choices are long takes the whole row** under its label (UI-10), so "7mm increments - Excluding…" isn't cut off.
- **Model and component pages have the same head** (UI-11): name, a one-line summary (a component's is the first sentence of its notes), credits, then one row of actions: Code, README, Edit form, with Copy code and Pin as a model… under ⋯ on a component.
- **Fixed:** Settings showed "null" under Engine on Linux and macOS (an empty Windows-only line).
- **Tests:** the acceptance test's Scan for changes check clicks again when its click only waited for a scan already running (one started when the window got focus), which stopped the first 0.4.5 build.

## 0.4.4 — 2026-10-14 · No starter projects, website retired, license status in Library settings

Milestone 1 of the owner's direction of 2026-10-14 (the rename clean-up), with the owner's decisions of the same evening and items UI-1, UI-6 and F-6 of Review 1.

- **No starter projects in the installer** (owner: "we can stop including SCAD generators and models with the build, those were included as initial tests"). A new library starts with Components only; Home says how to add a project when there are no models. A library that already has the starter projects keeps them as ordinary projects. Library settings shows "Bring back starter projects" only when the app has a starter library (tests); `app_info` gains `starter`. CI checks the .deb has no `starter/` and still has `libs/`.
- **The website is retired** (owner's choice on the decision card): `site.yml` is gone, so nothing deploys to GitHub Pages any more; its vendored-checksum check moved into `desktop.yml`'s site job, which now also runs when `vendor/`, `adapters/`, `libraries/` or `sources/` change. The page at drflgd.github.io stays up until Pages is switched off in the repository's settings.
- **No Needs attention** (owner, on Review 1's UI-1: "that Needs Attention bar can be removed and just show the license status… in the settings pages"). The menu row, Home's card and the list page are gone. Library settings › Projects has a License column (the license, with "Check license" when it isn't cleared for sharing prints) and marks anything wrong with a project beside its name (Folder missing, Not read, Flagged as broken (n), …; the details on hover). Messages that pointed at Needs attention point at Library settings. The catalog's `attention` list is unchanged (the marks come from it).
- **The menu follows the page** (Review 1, UI-6): it noticed the page only when it redrew, so Library settings opened from Settings' link or the command palette left Settings lit. It now follows the address. Licenses & credits lights up too. Library settings' jump links include Backups.
- **The library's README.txt** lists every folder and what's inside a project, and says which folders are rebuildable (Review 1, F-6). A README.txt the app wrote before is replaced; one you changed is left alone.
- **Fixed: a test of Scan for changes depended on timing** (it edited a file within the same second it had touched it, at the same size, which the quick look can't tell apart); it stopped the first build of this release.
- **Rename clean-up:** README, this file and the plan say SCAD-Workshop; the app's releases link points at the new address. The README now describes the desktop app, with the website-era projects as test projects.

- **Roadmap: the owner's direction of 2026-10-14** in `docs/DESKTOP_PLAN.md` ("Owner direction of 2026-10-14"): consistency of imported projects is the main theme (one project description for every source, defaults that render, `workshop.json`), a review of the interface and of the library's file structure after each milestone, skins and other work reused from Model Library, and five milestones in order (rename clean-up, skins, consistent projects, making parts from a generator, the rest of Phase 4). Opening a `.scad` file without adding it is on hold. Documentation only.

## 0.4.3 — 2026-10-12 · Category picker, Condense with a selection, Scan for changes, backups

0.4.3: the small and mid-sized requests of 2026-10-12, as the owner asked (items 3, 4, 5 and 10 of "Owner requests of 2026-10-12" in `docs/DESKTOP_PLAN.md`), before reviewing priorities.

- **Category picker** (owner's request: drop-downs for categories should show the other categories). Edit details' Category was a text box with suggestions, which only suggested categories matching what was already in it. It's now a drop-down listing every category, built-in and your own, with how many models and parts each holds, a box to filter it, and New category… at the bottom (or type a name that isn't listed and press Enter). The same control is used in Edit details (one item, several, a folder, a project), Pin as model, and Library settings › Categories (where a removed category's items go). Keyboard: arrows, Enter, Escape. (The acceptance test caught Enter pressed straight after typing a new name closing the list without choosing anything; the highlight now follows what you type at once.)
- **Fixed: grouping (Condense) with a selection** (owner's report: it behaved strangely after selecting something and turning Condense off). Turning Condense on with no grouping switched to grouping by project, and turning it off left it there; it now goes back to the grouping you had. Arrow keys, Shift-click ranges and Ctrl+A worked on every result in sorted order, so inside an opened group they reached items of other groups, and with grouping on the arrow keys jumped about; they now follow the items on screen, in the order shown. A selection could outlive its items (on the condensed overview the inspector still showed it); the selection and the Shift-click anchor are now kept to what's shown. What you selected inside an open group stays selected when you turn Condense off.
- **Scan for changes** (Library settings › Projects, and by itself when the app starts and when its window comes back into focus, at most once a minute). Besides `local/` projects and linked folders, it now looks at the stored copies of projects added from GitHub, ZIPs and folders: names, sizes and modification times first (cheap), contents only when those differ, so a file touched but not changed isn't a change. Changed projects are read again; new `local/` folders are added; a project whose files are gone (a `local/` folder moved or deleted, a stored copy removed) is listed under Needs attention. The message says what it did ("Added Scanned. Read Gears again. Old box is missing (see Needs attention)").
- **Back up library… and Restore from backup…** (Library settings › Backups). A backup is the whole library folder in one ZIP (projects, saved settings, your edits, the trash), written through a save dialog, with progress and Cancel in the status bar; "Leave out thumbnails" makes it smaller (projects', trashed projects' and components' thumbnails; the app makes them again). It's written to a `.part` file and renamed when done, so a stopped backup never looks finished; it can't be saved inside the library. Restoring unpacks a backup into an empty folder you choose and opens it as the library; the open library isn't touched. Linked projects live outside the library and aren't in a backup (the message says how many). Previews stay in by default: some parts' previews come from the app's own copy and wouldn't come back.
- **Fixed (from 0.4.2): a project added as a library was read again at every start**, because the check for projects read before format 4 compared its (deliberately empty) models with its files.
- **Commands:** `library_backup` and `library_restore` (jobs that can stop), `job_cancel`; `source_rescan` also checks stored copies and returns `added` and `missing`. Reading a project records `files_stat` and `files_fp` in its derived data (projects read before get them at the first scan). The app's `pick_save_path` dialog (and the test server's stand-in).
- **Tests:** Rust tests for scanning (touched, edited, read before fingerprints, a library-role project, gone) and for backups (stopped, inside the library, without thumbnails, restore into a non-empty folder or from a ZIP that isn't a library). The acceptance test checks Condense with a selection (Shift-click range; on, an open group, arrow keys and Ctrl+A, off), the category picker (every category listed with one chosen; a new one made), Scan for changes (a file added to a stored copy, a new `local/` project, a missing one), and a backup and restore.

## 0.4.2 — 2026-10-12 · Front ends over a shared engine (Birdhouse), roadmap for 0.5

The Birdhouse fix (owner's report of 2026-10-12), and the roadmap for 0.5.

- **Fixed: projects made of front ends over a shared engine showed the wrong models** (owner's report: Birdhouse Workshop, added from a ZIP, showed only Castle and Engine). Its square, corner and polygon files are only settings and an `include` of `birdhouse_engine_v2.6.3.scad`, which draws the model at its top level with their settings; the app judged each file by its own top level, so they looked like library code, and the engine, which can't render alone (it never sets `design_preset`), was listed as a model. Reading a project now follows `include`s inside it (not `use`, which runs no top-level code): a file of settings only (no geometry, no modules of its own) that includes a file that draws, and sets something that file reads but never sets itself, is a model; the file it draws through, and any file on the way, is library code. Birdhouse now has Square, Corner, Polygon and Castle. Checked against all 19 starter projects, Gears, LEGO.scad, BOSL, BOSL2, NopSCADlib, MCAD, gridfinity_openscad and threads-scad: only Birdhouse's models change (GridFlock's docs animations, which only override settings gridflock.scad has, and BOSL2's `_BOSL2_STD` guard flag, checked with `is_undef`, don't count).
- **Fixed: category keywords matched inside words**, so "design" counted as "sign" and filed Birdhouse under Labels (also "showcase" as "case", "donut" as "nut"). Keywords now match whole words, with the usual endings (gears, boxes, threaded). Birdhouse is now Other; no starter project's category changes.
- **Project data format 4.** At start-up (and with Look for changes), each project read before 0.4.2 is looked at again without OpenSCAD; it's read again only if that finds other models than it has, otherwise it's just marked as current. A project already in a library keeps the category it was given; Read again picks up the new guess.
- **Triage: Birdhouse Workshop shows 2 of its 4 designs** ("0.5 notes" in `docs/DESKTOP_PLAN.md`): its square, corner and polygon front ends are only settings plus an `include` of a shared engine that draws the model, and the project reader decides "model or library" without following includes; the engine itself is listed but can't render alone. Also: "sign" in "design" files it under Labels. Fixed in 0.4.2 (above).
- **Roadmap: the owner's requests of 2026-10-12** in `docs/DESKTOP_PLAN.md` ("Owner requests of 2026-10-12 (next: 0.5)"): ten features, each with what the app does today and what to build, and the Birdhouse bug. Multiple libraries moves there from the future options. Documentation only.
- **Tests:** Rust tests for front ends over an engine (directly, through another settings file, an engine switched off with its own drawing, `use` not counting, a library with modules of its own, a guard flag), a drawing file that others only override (stays a model), and whole-word categories.

## 0.4.1 — 2026-10-06 · Borrowed settings, code editor, links in documents

Owner's reports and requests of 2026-10-06 (0.4.1). The owner put the settings work ahead of editing the code in the viewer; both are in this release, with a fix for links in documents asked for before it went out.

- **Fixed: no settings for models whose values are typed into a module call** (owner's report, 2026-10-06: every model of [chrisspen/gears](https://github.com/chrisspen/gears) rendered but had no settings). Its examples are files like `include <../gears.scad>` + `bevel_gear(modul=1, tooth_number=30, ...);`, with no top-level variables, so OpenSCAD's Customizer lists nothing. For any project, when a model's file is only includes and calls (no settings, definitions or blocks of its own), reading it now makes a copy with each literal argument lifted into a setting after the includes (named after the argument; positional values named from the module's signature; a second call of the same module numbered `_2`; a name the library already uses as a global gets the module's name in front; `$` arguments stay fixed), described by the library's comment for that argument where it has one (`name = description`). The copy is kept in the library (`sources/<id>/generated/<version>/`, blobs `<id>@<version>~gen:<path>`), names its original at the top, and renders in its place; the original is untouched. All 17 Gears examples now have settings (`demo.scad` already had its own). Project data format 2: projects read by 0.3 are read again when the app starts (they also get their documents then). Its first build failed the Linux acceptance test on an unrelated race (below), so it ships in 0.4.1. Not yet: files with blocks (`difference() { }`, `for`, `if`) or their own definitions.
- **Settings borrow the library's descriptions and drop-downs** (owner's request, after [cfinke/LEGO.scad](https://github.com/cfinke/LEGO.scad)'s examples came up as bare numbers). When a project file calls the module with one of its own Customizer variables (`block(width=block_width, type=block_type, …)` under `// Width of the block, in studs` and `block_type = "brick"; // [brick:Brick, tile:Tile, …]`), the lifted setting gets that variable's comment as its description and its `// [...]` note as its drop-down or slider. The library's call is looked for in the files the model includes, then the project's other model files. An expression of one variable (`reinforcement=(use_reinforcement=="yes")`) lends its description only; a note is used only for a value of the same type, and a slider only when the value is inside it. A module's own `name = description` comment (Gears) still comes first. LEGO.scad's 179 examples now have described settings with Height, Type, Wing type… as drop-downs.
- **Fractions in a call are settings too** (owner's choice: the library's drop-down rather than a number): `height=1/3` becomes the library's own 1/3 choice (`.33333333333`, labelled "1/3"); a value that isn't one of the choices (`2/3`) is added to the list, labelled as written; with no list it's a number. 111 LEGO examples had their height fixed before.
- **Setting names:** a lifted setting is renamed after its module (`block_wing_type`) only when the file `include`s a library with a variable of that name; `use` doesn't bring variables in, so LEGO's examples show "Wing type".
- **Project data format 3.** Only projects with settings lifted from calls are read again (at start-up, or with Read again); others are left as they are.
- **Fixed: a linked project's README pictures** (and the .scad buttons in its Files tab) pointed at nothing: its folder is outside the library. Library paths `~linked/<id>/<path>` now read from the linked folder (the app's `library://` protocol and `workshop-cli serve`).
- **Fixed: an update accepted while a check was still asking GitHub came back as "ready to review"** (and the project's version went back): the check saved the project as it had read it before asking. It now reads it again first. This was the Linux acceptance test failure of the first 0.4.1 build; the test also waits for its own check to finish.
- **The Code tab is an editor** (owner's request: edit the raw OpenSCAD, Generate from the edits, save them like a preset without touching the original or adding a copy of the model, so library updates aren't affected). On a model's own page, the model's own files (not a library's, not a component's or a pinned component's, not in the inspector) can be edited in place: highlighted as you type, Tab indents, Ctrl+Enter generates, Ctrl+S saves, Revert goes back to the project's version of the file. Generate renders the edited text in place of the file (native OpenSCAD reads it by its hash from the app's data folder, `blob_put`; the WebAssembly engine gets the bytes); while the code is edited only the settings changed from their defaults are passed, so a default changed in the code applies (a note above the form says so), and the status says when the preview is older than the code. **Save** (the settings bar, or Save… in the Code tab) keeps the edits in the saved-settings record next to the values, `edits: { "<path>": { text, base } }` (`recipes/<id>.json` in the library, the browser's storage on the website); the record shows as "· code edited" in the same list. Picking it brings back its values and code; Defaults goes back to the original code (asking first if the edits aren't saved); a preset changes only values. `base` is the hash of the file the edit started from: if an update changed the file since, picking the variant says so and still uses the edited copy. The project's files are never written, and no model is added.
- **Fixed: a link in a README or a PDF could take over the app's window** (owner's report: the website opened in the app with no way back short of restarting). The window is now made by the app (`"create": false` in tauri.conf.json) with a navigation guard: the app's own addresses load, anything else opens in the default browser, and no other windows open (a PDF's links included). If a link still opens a page inside the README or PDF panel, a bar offers "Back to README".
- **Roadmap: a "Future and experimental options" section** (owner's request) in `docs/DESKTOP_PLAN.md`, with direct slicing in the software, multiple libraries and agent integration to edit SCAD files, and a plugin architecture (slicers, inference engines, model editors), each with how it might work and what's still open. Documentation only.
- **Reading projects:** a model's included files are read for lifting only when the model is just includes and calls.
- **Commands:** `blob_put` (an edited file's text, stored by its hash for native renders; the `blob` command reads it back).
- **Tests:** Rust tests for borrowing (straight, expression, type mismatch, out of range, from another file), fractions, `use` vs `include` naming; the acceptance test adds a local project in LEGO.scad's style and checks the descriptions, the Type drop-down, Height showing 1/3, and a render after choosing 1; then edits its code (Generate uses it, saved as a variant with `edits` in the library's record, Defaults is the original, the file and the model list unchanged); the website test edits, saves and switches back too.

## 0.4.0 — 2026-10-06 · Side viewer and reference documents (Phase 4 starts)

Released 2026-10-06 (v0.4.0). Owner's request of 2026-10-05: view a model's raw SCAD script (read-only for now) in a viewer on the right; model documentation works the same way, in a collapsible view with tabs on its edge. The owner's answers to the reference-document questions: documents are project-level (for NopSCADlib, scroll the README to the part); stored as HTML (GitHub READMEs), the raw PDF (Printables) or text (readme.txt); a scratch pad of notes per part comes later.

- **Side viewer:** a collapsible panel on the right with tabs along its edge: **Code** (the model's OpenSCAD files, read-only: line numbers, highlighting, a file picker, Find, Wrap, Copy) and the project's documents (**README**, PDFs, readme.txt). Code and README buttons on model pages, in the inspector and on a project's page; .scad files in a project's Files tab and README links to them open in Code. A component shows the file made for its settings, then the library file that defines it, opened at the module. It follows what's on screen, overlays the right of the window when open, folds away (Esc, ×, or its tab), and remembers its tab and width (drag the edge). On the website too (Code from the model's files, the README from the build).
- **Reference documents** (`desktop/core/src/docs.rs`): found when a project is read: a Markdown README converted to HTML with pulldown-cmark (GitHub tables, task lists, footnotes, heading anchors) and kept at `sources/<id>/docs/<version>/readme.html`; raw HTML in it is kept minus scripts, frames, forms, styles and event handlers, and it's shown in a frame that can't run scripts. Pictures and links stay relative to the project's folder (offline); in-page links scroll. PDFs at the top level or one folder down are kept raw (shown where the webview has a PDF viewer, i.e. Windows; "Open in your PDF viewer" otherwise); readme.txt shows as text. Projects read by 0.3 get theirs when first opened. App libraries' READMEs are converted at build time (`libs/docs/`, pictures and links to GitHub at the pinned commit); built-in families' READMEs by the website build (`data/docs/<family>.html`), copied into the starter projects (existing libraries get them on the next start).
- **Fixed before release** (the first 0.4.0 build failed the Windows app test): the side viewer dropped what it showed whenever the library reloaded (a thumbnail saved, a project read), so Code went blank for a moment and the README frame reloaded (losing its place). It now keeps the model and documents on screen while they're fetched again; only another model or project replaces them. The app test waits for the code itself.
- **New dependency:** `pulldown-cmark` (vendored for offline builds on the `crates-vendor` branch).
- **Commands:** `docs_list` and `doc_get` (a PDF as bytes); `source_get` also returns the version folder's library-relative path. MIME types for PDF, GIF and text in the app's `library://` protocol and `workshop-cli serve`.
- **Tests:** the acceptance test opens a GitHub project model's code and README (HTML kept in the library, a picture from the project's files), a component's generated code and module file, the BOSL2 and Gridfinity Extended READMEs, and a project file from the Files tab; the website test opens Code and README; Rust tests for the conversion, cleaning, anchors and finding documents.

## 0.3.1 — 2026-10-05 · Left menu panel, drop-downs from the docs

Released 2026-10-05 (v0.3.1). Owner's requests of the evening ("a few basic UI changes"):

- **Left menu:** Parametric Models, Components and the Parts Library are rows of their own (no more "All models" / "All components" / "All parts"). The section you're browsing shows its categories, each opening to its projects, in a panel attached to the menu that scrolls on its own and folds to a strip (« / », remembered). In the small-window drawer the categories open under each section instead.
- **Component settings whose docs list their values are drop-downs** (BOSL2 `spheroid()` style: orig, aligned, stagger, octa, icosa; 85 settings in 62 BOSL2 components, including ones that point to another function's list, like `vnf_vertex_array()` style). Lists the docs leave open ("…, or a 3D point") stay text fields. The library index format is now 2.
- **Library settings:** an Edit… button on each project's row, before Hide, opens its details.
- **Fixed:** long text spilling out of its box. A long folder or ZIP path in Add a project pushed the field, the Choose… button and the dialog's contents past the dialog's edge (and the "Link instead of copying" checkbox was stretched); paths now wrap. The details editor's one-line fields wrap and grow to show all their text.
- **Tests:** the acceptance test checks the menu's panel (sections, categories, projects, folding), the spheroid drop-down rendering a choice, Edit… in Library settings, and that a long path and long details stay inside the dialog (34 checks); the website test checks the panel too.
- **This changelog**, combining the git history, release notes, the plan's notes, the early backlog and the owner's requests. `CLAUDE.md` points agents here.
- **Plan:** reference documents for projects added to Phase 4 (requested 2026-10-05; to be discussed before building).

## 0.3.0 — 2026-10-05 · Components (Phase 3)

Released 2026-10-05 (v0.3.0: AppImage, deb, Windows setup). Owner's brief: make the modules inside OpenSCAD libraries usable as forms, in a section of their own ("Components", the owner's choice of name over "Primitives"), with the user's own copy of a library winning over the bundled one and a switch to go back.

**Added**
- **Bundled libraries:** BOSL2, MCAD, NopSCADlib, Round-Anything and threads-scad ship inside the installer (`libs/`, about 9 MB), pinned in `libraries.json` by commit and a SHA-256 over the file list (`tools/fetch_libraries.py`; only OpenSCAD and data files, no docs or tests, no data file over 2 MB). Credits in THIRD_PARTY.md.
- **Components section** in the left menu, between Parametric Models and the Parts Library: 796 modules grouped by topic (Gears & pulleys, Threads, Screws/nuts/fasteners, Bearings, Hinges & joints, Motors, Electronics, Boxes & enclosures, 3D and 2D shapes, Rounding & masks, Paths & sweeps, Textures, Text, Hardware), collapsed, with the libraries inside each topic; also on Home, in the palette and the status bar.
- **Reading a library** (`components.rs`): BOSL2-style doc blocks (Synopsis, Topics, Arguments with defaults, Named Anchors, Examples), NopSCADlib's `//!` comments (and its type constants as choices), or plain module signatures with the comment above. Only modules that make geometry on their own count.
- **Component pages:** the form starts from the first doc example that is a plain call (every such example is a "Start from" preset), else start values from `catalog/components.json`, else guesses from argument names, else empty required fields that must be set first. Empty settings aren't passed (the module's default applies); expression fields take any OpenSCAD value (checked: one expression, no statements, no file access); `anchor`/`orient` are choices; 2D shapes are extruded; Detail (`$fa`, `$fs`, `$fn`). **Copy code** gives the OpenSCAD file. The first render becomes the thumbnail; Library settings can make them all.
- **Rendering a component:** the app writes the file for each render (the library's includes, then one call with the values set) beside the library's laid-out files.
- **Search:** typing a module's name puts it first ("bevel gear" → BOSL2 `bevel_gear()`).
- **Pin as a model:** a component becomes a Parametric Model ("Pinned components" project, `sources/pinned/pins/<id>.json`) with your values as defaults and only the chosen settings shown.
- **Edit form** on any model in the library: labels, help, sections, defaults, limits, box names, choices, show-when conditions, printer-profile links, presets, hidden settings. Stored as `form` in the item's metadata in the same shape as a family manifest (Copy as JSON) and applied with the website build's own code (`ingest::apply_form`, which now also takes `step`, `group` and `placeholder`).
- **Your copy or the app's:** a library you add as a project (role "library") wins over the bundled copy with the same name, for Components and for includes; Library settings → OpenSCAD libraries switches back (`prefer_bundled` in library.json). Projects that include the library are read again. Projects including `<BOSL2/...>` and the others work without adding them.
- **Category icon from an image file** (Library settings → Categories → Image…, kept in the library's `icons/`).
- `workshop-cli libs-index`, `components`, `components-check`.

**Verified**
- BOSL2: all 146 modules whose first doc example is a plain call render it (CI fails otherwise); 21 more start from a later example, 2 from defaults, 32 wait for values. Across all five libraries, every component either renders at its start values or waits for values (429 render, 367 wait).
- The bevel-gear flow (search, open, change teeth and module, preview, download) takes about 5 s in the acceptance test; the plan's target was a minute.
- A Rust test shows a form edit made in the app gives exactly what the same `ui` block in a manifest gives.
- Acceptance test 32 of 32 on Linux CI; the app test opens BOSL2 and threads-scad components in the real app on Linux and Windows.

**Fixed along the way**
- The model benchmark started rendering all 796 components (they joined the model table) and hung CI on ones with unset arguments: `bench` now renders the library's models only.
- Test flakiness: the update check now waits for background work first; the test server reports cancelled renders as cancelled, not as server errors; the browser's harmless "ResizeObserver loop" notice isn't counted as a page error.

**Details:** "Components" in section 2 and "0.3 notes" in `docs/DESKTOP_PLAN.md`.

## 0.2.0 — 2026-10-05 · Portable library (Phase 2) and follow-ups

Released 2026-10-05 (v0.2.0, the first numbered release; the unnumbered `desktop-latest` pre-release was retired).

### Phase 2: the portable library, adding projects, update checks, metadata

Owner's brief: ingest projects from GitHub (with periodic update checks), ZIPs and folders, stored locally; a searchable index ("a bevel gear for a model car, found and generated quickly"); a model library of your own and imported parts. The library folder must be selectable, portable and importable into a new build, with its metadata inside it; metadata editable per project and per file ("relicense a whole project"); printer profiles stay with the app, not the library; the built-in generators and parts become library projects installed alongside the app.

- **One project reader, in Rust**, shared by the app and the website build (`workshop-cli site-prepare` / `site-finish`; output identical to the old Python build).
- **Library folder** (format 1): `library.json`, `sources/<id>/` (source.json, metadata.json, files/<version>/, derived/, thumbs/), `local/` (your own projects, read when they change), `collections/`, `recipes/` (saved settings); relative paths, small sorted JSON, no database. A Phase 1 workspace upgrades itself; a newer format opens read-only. **Merge** combines libraries, with conflicts to pick.
- **Starter library:** the website's 58 generators and the openGrid parts become 19 library projects shipped with the installer and copied in on first start; changed ones arrive as updates.
- **Adding projects:** GitHub link (a pinned commit's tarball, no git; branch, tag, commit and subfolder links), ZIP, folder (copied or linked); "library" projects such as BOSL2 make `<BOSL2/...>` resolve.
- **Update checks:** daily and on request; the new version is read beside the current one and summarised (commits, models added or removed, settings changed); accept or skip; your edits survive.
- **Layered metadata:** item, folder, project, detected, library defaults, with where each value comes from, undo, bulk edit, open fields that become filters and columns.
- **Interface:** project pages (README, files, versions, problems, updates), condensed groups with chosen icons (also on the website).
- **Decided differently from the plan:** search stays in the page (the in-memory index is fast enough and identical to the website's) instead of SQLite.
- **Tests:** `tests/desktop_page.py` (page + real backend; real GitHub in CI, `tests/github_mock.py` locally); CI opens the Linux-made library on Windows and compares.
- **Fixed:** relative paths broke native settings export in CI; local projects re-read after copying (fingerprint by content); a toast showing "null"; the Windows WebDriver test typed the GitHub link and clicked Add before the dialog re-rendered (the dialog now reads the field when submitted); projects added from GitHub got readable names.

### 0.2 follow-ups (from the owner using Phase 2)

- **Numbered releases** (owner request): `0.<phase>.<n>`, version in Settings → About, release only when every check passed.
- **Names** (owner request): "Generators" → **Parametric Models**, "Ready-made parts" → **Parts Library**.
- **Left menu** (owner request): both sections by category, collapsed by default, projects inside; the separate Projects list removed; **Add a project** at the bottom of the menu and on Home. (The owner clarified that "grouping" meant condensing groups in these views, not a project list.)
- **Condense** button (owner request): with no grouping it groups by project; a project's tile opens its page.
- **Editing** (owner request): fields start with the value in use, editable in place; an item can be listed under another project.
- **Hide, delete, flag as broken** (owner request), for items and projects; **trash** for deleted projects with Empty the trash (owner refinement).
- **Library settings** page (owner request): folder, projects, categories (add, rename, remove with items moving, bring back), hidden and deleted items, trash, defaults, GitHub token.
- Update checks run one at a time and respect an update accepted mid-check; web links open in the default browser.
- **Fixed:** thumbnail requests failing for deleted or moved items (they now use the item's own project).

## Before numbered releases

Builds from `main` were published as the rolling `desktop-latest` pre-release.

### Phase 1.5 — 2026-10-04 · Library interface; renamed SCAD Workshop

Owner's brief: the main page had to change drastically to cope with many more models and libraries: a browsable library with several views, search, metadata editing. Also: rename to **SCAD Workshop**, add a night theme, plan for more metadata filters and for Printables ZIPs with their PDFs.

- A clickable mockup first, then the new interface in `web/ui/` (Preact + htm, vendored, no build step): sidebar, browser with grid, list, table and grouped views, inspector, workbench tabs that keep their state, status bar with background jobs, command palette (Ctrl+K), quick look (Space), favourites and recent, light/dark/night themes.
- The `index` seam (`web/ui/index-local.js`): weighted search with prefix, typo and synonym matching, typed filters, facets, sorting; under 15 ms.
- "Updated" filter, sort and column from upstream commit dates (`tools/source_dates.py`).
- Thumbnails for every generator and part (`tools/thumbnails.py`).
- 3D view renders on demand (it had kept the GPU busy even when hidden); the palette no longer loses what you type while it re-renders.
- Rename: the app's identifier stayed the same so settings carried over; the default folder became `~/SCAD Workshop`, with an existing `~/Claude Grid Workshop` still used.
- Tests: `tests/interface.py` (33 checks then; 38 by 0.2).
- 2026-10-05: tabs shrink to fit, the Library tab stays in view, the wheel scrolls the strip (a scrollbar appeared on Windows with four tabs open).

### Phase 1 — 2026-10-03/04 · Desktop shell with native OpenSCAD

Owner's choices (2026-10-03): a desktop version to get past WebAssembly's speed; Tauri over a native UI or Electron (a web wrapper reuses the front end); Windows and Linux; same repository; keep the website as the light version; write the plan into the repo first (`docs/DESKTOP_PLAN.md`, reviewed before Phase 0).

- `desktop/src-tauri` over `desktop/core` (native renders, queue, cancel, on-disk cache, workspace folder, saved settings as files); `workshop-cli` renders every model natively in CI.
- Engine bundling: Windows ships the snapshot folder; Linux ships the AppImage and unpacks it on first start (no FUSE; the `.deb` declares the OpenGL libraries).
- Desktop-only models: the three Underware channels that crash the WebAssembly engine (58 models in the app, 55 on the website).
- Printer profile (bed size, nozzle) bound to the generators that ask for them; magnet and tolerance settings left unlinked on purpose (projects define them differently).
- **Windows engine race:** native OpenSCAD on Windows spends seconds on `use`/`include` for projects split into many files (Gridfinity Extended: about 12 s native vs about 1–4 s in WebAssembly). CI experiments ruled out Defender, the library path and fonts (the same code as one file parses in 0.2 s). So the Windows app races both engines on a model's first render and remembers the winner. Worth reporting upstream.
- Windows fontconfig setup with a persistent cache; the UI font (Archivo) bundled so the app makes no web requests.
- Results: all 58 models render natively on Linux and Windows; on Linux the 55 website models take 28 s natively against 188 s in WebAssembly.

### Phase 0 — 2026-10-03 · Seams, saved settings, share links

- `web/platform.js` chooses engine and storage (browser now, desktop later); prefs in localStorage, saved settings in IndexedDB.
- A settings picker on every model: defaults, author presets, saved sets (save, update, save as new, rename, delete).
- **Share** links with the changed settings compressed into the URL; export and import of OpenSCAD Customizer parameter files.

### The website — 2026-10-03 · Claude Grid Workshop

The first product: a website to generate Gridfinity, openGrid and Honeycomb Storage Wall parts from open-source OpenSCAD projects.

- **First a server** (Starlette: catalog API, render queue with time limits and cancel, STL cache, Docker), then within hours **replaced by a static site** with OpenSCAD running in the browser (pinned 2026.10.02 WebAssembly build, one web worker per render) on GitHub Pages. The server version is kept on `archive/server-v1` (backlog item G1, on hold).
- `tools/build_site.py`: per-model dependency discovery, content-addressed files, bundled fonts, settings from OpenSCAD's own Customizer export, upstream `editor.toml` metadata (conditions, presets, help, section switches).
- Generate, preview (three.js on a 42 mm grid, with dimensions) and download (backlog A1, A2); batch generation to a ZIP (E3); a compact settings form with help on demand; the home page grouped by type, then project; Licenses & credits page.
- Generators added through the day: all of Gridfinity Extended (13 more), four label generators (Laurens Guijt, Maurice Kevenaar, Nadia Santalla with bundled BOSL v1), Just Fit Base Grid and Modular Minimalist Kitchen (owner-supplied), Pred-label bin, Gridfinity Kitchen (owner ZIP; its 12 size files became "Start from" presets, each verified identical), Anylid (owner-supplied, via a bug-fixed copy), openGrid Shelf; Underware enabled for private use with 7 more variants.
- openGrid parts library (44 official parts, CC-BY-4.0 by David D), with what the generator reproduces left out, verified by `tools/parity.py`.
- Fixes worth knowing: SCAD files with CRLF line endings break Customizer dropdowns (normalised); include paths resolved case-insensitively (Extended's marble run includes `dotscad/`, the folder is `dotSCAD`); values a file computes with an expression are passed with `-D` (Kitchen's cutout width).
- Owner's last website request: show a silverware holder's utensil settings only up to the chosen number of utensils, and name the boxes of multi-value settings (family-wide `ui` overrides).

### Foundation — 2026-10-03

- The sources were first collected in a separate repository, **Grid-Storage-Workshop** (two commits, made with Codex): 271 SCAD files vendored with licenses and an audit of the 14 project families on the reference site gridfinity.perplexinglabs.com.
- This repository started from that collection (commit `69e44ab`): 9 families carried forward and re-verified byte for byte against upstream, GRIPS and GridPlates left out (superseded, owner's decision), per-family manifests, schemas, an openGrid Snap adapter, and CI rendering every model. Details: docs/SOURCE_AUDIT.md.
- The website-era feature backlog with IDs (A1 generate, A2 preview, E3 batch, F4 in-browser OpenSCAD, G1 server fallback…) is `docs/FEATURES.md`; the desktop plan superseded it as the roadmap.

---

## Owner requests, in order (desktop era)

What the owner asked for, so the reasons behind the design stay visible. From the working sessions of 2026-10-03 to 2026-10-05.

1. After the website's feature set: "what do you recommend next?"; then fixing the Gridfinity Extended silverware form (utensil settings only up to the chosen count).
2. "Make this a more versatile toolbox for OpenSCAD generation software… something that increases productivity and usability for generating a number of different 3D models" (the owner knows native OpenSCAD).
3. A desktop version to avoid WebAssembly's slowness; asked why a web wrapper rather than native; chose **Tauri**.
4. The vision: a Swiss Army knife of 3D design; common libraries built in; ingest from GitHub links (with periodic update checks) or files, stored locally; an easy-to-search index organised by feature ("a bevel gear for a model car"); a model library for your own and premade models (rugged storage cases, parts for an RC car). Choices: Windows + Linux, same repo, website kept as the light version, plan written into the repo first.
5. Phases 0, 1 approved and built. After testing Phase 1: the interface must change drastically for many more models: browsable library with views, search, metadata editing → Phase 1.5.
6. Later filtering by more metadata; ingesting Printables ZIPs with their PDFs (creator, license, dates); rename to **SCAD Workshop**; a night theme.
7. Roadmap additions: one item per file name with a download format picker; multi-colour and multi-part 3MF; condensed groups with customisable icons.
8. Library storage: a selectable, portable folder importable into a new build; metadata stored with it, editable at group and file level (relicense a project). Printer profiles stay with the software. → Phase 2.
9. The built-in generators and parts become a library installed alongside the software (still in the installer).
10. Clarification: condensing groups in the generator and parts views, not a Projects list; remove the Projects heading. Numbered releases.
11. UI changes before the next version: rename to Parametric Models / Parts Library; editable project text; add an item to an existing project; categories under both sections, collapsible, collapsed by default, with projects inside; a Library settings page for categories; delete, hide, flag as broken. Refinement: deleted projects go to a local trash folder with an option to empty it. → 0.2.
12. Phase 3 as its own section: "Components" (over "Primitives"); for a library present twice, the user's copy wins with a switch back to the bundled one. → 0.3.
13. A reference document per project in the interface (a GitHub project's README, a Printables PDF's information); talk through the implementation when the time comes. → roadmap, Phase 4.
14. One changelog combining all the history, for future agents → this file.
15. Basic UI changes, then a release: the left menu's sub-categories in a collapsible menu attached to it that scrolls ("All Models" replaced by "Parametric Models" and so on); settings like BOSL2 spheroid's "style" as drop-downs if the SCAD files define their values; an Edit button on each project's row in Library settings, before Hide; text running past the edge of its box (the details editor, the folder path). → 0.3.1.
16. A viewer on the right for a model's raw SCAD script (read-only for now); model documentation the same way, a collapsible view with tabs on its edge; part of Phase 4, released. Answers for reference documents: project-level (scroll NopSCADlib's README to the part); HTML for GitHub READMEs, the raw PDF for Printables, readme.txt as text; a scratch pad of notes per part later. → 0.4.0.
17. Bug: the Gears project (chrisspen/gears) renders but none of its models has settings → settings lifted from the values in their module calls, for any project written that way (0.4.1).
18. LEGO.scad (cfinke): fractions like `height=1/3` should be the library's drop-down; lifted settings should borrow the library's own descriptions and choice lists. Then: make the Code viewer editable (Generate uses the edits; Save stores them like saved settings, a variant picked from the same list, never a copy of the model or a change to the original, so updates aren't affected). The owner put the first two ahead of the editor. → 0.4.1 (all three).
19. Links in a README or PDF opened the website in the app's window with no way back → the window keeps the app; the web opens in the browser (0.4.1).
20. A section of the roadmap for future and experimental options: direct slicing in the software, multiple libraries, agent integration to edit SCAD files → "Future and experimental options" in the plan; unscheduled.
21. A plugin architecture added to the same list, for different slicers, agentic inference engines, model editors and so on.
22. After using 0.4.1: choose an icon or picture for an object; add content (video, documents…) to a model, component or part; category drop-downs that show the other categories; grouping behaves strangely after selecting something and turning Condense off; scan the library for changes; should there be a standard folder format?; open a .scad file with or without importing it and generate models; make single or batch parts from a generator into the ready-made parts; several libraries open at once, showing which library an item is from; library backups as a ZIP through a save dialog. Bug: Birdhouse Workshop shows only Castle and Engine → "Owner requests of 2026-10-12" in the plan.
23. Make the Birdhouse fix (the owner re-imports Birdhouse to test it), then work through the 0.5 features → 0.4.2.
24. Start with the small and mid-sized requests (category picker, grouping, scan for changes, backups), release, then review priorities → 0.4.3.
25. After a review of the project at 0.4.3: put opening a `.scad` file without adding it on hold; periodic reviews of the interface and the library's file structure; the major work is consistency (imported SCAD files that work more consistently, valid default values, one file structure for files and generators whatever their source, keeping update checks from the source); reuse lessons and features from Model Library, the skins first; then the remaining milestones → "Owner direction of 2026-10-14" in the plan.
