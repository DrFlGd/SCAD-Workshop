# SCAD Workshop

<img src="desktop/src-tauri/icons/src/logo.svg" alt="" width="112" align="right">

A desktop app for Windows and Linux that keeps a library of OpenSCAD projects and makes 3D-printable parts from them. Add projects from GitHub links, ZIPs and folders, browse and search everything they make, fill in a model's settings and render it with native OpenSCAD on your computer. The modules of five common OpenSCAD libraries (BOSL2, MCAD, NopSCADlib, Round-Anything, threads-scad) come with the app as ready-to-use **Components**.

**Download:** installers on the [latest release](https://github.com/DrFlGd/SCAD-Workshop/releases/latest). Windows: the `-setup.exe` (unsigned, so SmartScreen asks once: More info › Run anyway). Linux: the `.deb`, or the `.AppImage` (needs `libopengl0 libegl1 libglx0`, present on most desktops). There's no auto-update: install a new release over the old one.

**History:** [CHANGELOG.md](CHANGELOG.md) (what was built, when and why; start there if you're picking up the work). **Plan:** [docs/DESKTOP_PLAN.md](docs/DESKTOP_PLAN.md).

## What it does

- **Your library** is one folder (`~/SCAD Workshop` by default) of plain files: everything you add, save or edit. Move it, sync it or keep it in git, then open it here or on another computer; Library settings also merges, backs up and restores libraries. Its layout is in [docs/LIBRARY_FORMAT.md](docs/LIBRARY_FORMAT.md).
- **Add a project** from a GitHub link (pinned to a commit; branch, tag, commit and subfolder links work), a ZIP (Printables downloads included) or a folder. The app finds the models, reads their settings with OpenSCAD, and picks up the README, license, presets, ready-made files and pictures. GitHub projects are checked for updates, which you review before accepting; your edits are kept.
- **Parametric Models, Components and the Parts Library** in the left menu, each by category and project; grid, list and table views, filters, grouping and a search that answers as you type (Ctrl K).
- **A model's page:** its settings as a form, Generate, a 3D preview with dimensions, Download (STL or 3MF), saved settings, Save as part, Batch generate (vary one or two settings, keep the results as parts or download a ZIP), the code and the project's documents in the side viewer, and your own edits to the code kept as a variant.
- **Components:** about 800 modules as forms, by topic (gears, threads, fasteners, bearings, hinges, shapes…), starting from their documentation's examples or suggested values. Pin one as a Parametric Model with your values.
- **Collections:** lists of parts and models for one use, with quantities, downloaded as one ZIP.
- **Make it yours:** edit any item's details (name, category, tags, license, notes, your own fields) for one item, a project or the whole library; edit a model's form; choose the picture shown; add your own files and links; hide, delete or flag items.
- **Skins:** Drafting Sheet, Parts Bin, Layer Line and Classic, each in light, dark and night.

## For developers

```
web/ (front end, no build step) --platform-desktop.js--Tauri IPC--> desktop/src-tauri (window, dialogs, library://)
                                                                     └─> desktop/core (Rust, no GUI): api.rs (the app's
                                                                         commands), native OpenSCAD render queue and cache,
                                                                         library folder, project reading, updates, merging
```

The map of the code, the conventions and how to build and test in a cloud session are in [CHANGELOG.md](CHANGELOG.md) under **For agents**. CI (`.github/workflows/desktop.yml`) builds both apps, renders every test model natively on Linux and Windows, checks the components, runs the acceptance test (`tests/desktop_page.py`) and the WebDriver tests, and publishes a numbered release from `main` when everything passes.

Build it yourself (needs Rust, Node, Python and on Linux the WebKitGTK development packages; see the Linux job in `.github/workflows/desktop.yml` for the exact list):

```sh
python3 tools/fetch_engine.py --out build/engine
python3 tools/fetch_native_engine.py --out build/desktop/engine --extract build/native
(cd desktop && cargo build --release -p workshop-core)
python3 tools/build_site.py --engine build/engine --out _site --native-engine build/native/squashfs-root/AppRun
python3 tools/thumbnails.py --site _site --native-engine build/native/squashfs-root/AppRun
python3 tools/fetch_libraries.py --out build/desktop/libs   # the bundled OpenSCAD libraries (or --from-git <folder of clones>)
python3 tools/build_desktop.py --site _site --out build/desktop --fetch-fonts   # ui/, site/, starter/ (tests only), libs/index/
python3 tools/component_thumbs.py --cli desktop/target/release/workshop-cli --engine build/native/squashfs-root   # libs/thumbs/ (needs Playwright)
cd desktop && npx @tauri-apps/cli@2 build          # installers in desktop/target/release/bundle/
```

`workshop-cli` runs the app's own code from the command line:

```sh
cli=desktop/target/release/workshop-cli
$cli bench --app-site build/desktop/site --starter build/desktop/starter --engine build/native/squashfs-root   # every model, from the starter library
$cli render --site _site --engine build/native/squashfs-root --model gridfinity-rebuilt/bin --set gridx=3 --out bin.stl
$cli serve --ui build/desktop/ui --app-site build/desktop/site --starter build/desktop/starter \
    --engine build/native/squashfs-root --home /tmp/workshop-home     # the app's page and backend in a browser
$cli components --libs build/desktop/libs --library BOSL2                       # what each module becomes (topic, start values)
$cli components-check --libs build/desktop/libs --app-site build/desktop/site --engine build/native/squashfs-root --library BOSL2
$cli library-call --library ~/"SCAD Workshop" --app-site build/desktop/site --engine build/native/squashfs-root \
    source_add '{"kind": "github", "url": "https://github.com/chrisspen/gears"}'
```

`serve` is for testing: open http://127.0.0.1:8790/ with `tests/tauri_shim.js` loaded (as `tests/desktop_page.py` does) to use the app's page in an ordinary browser.

## Test projects

`vendor/`, `adapters/`, `catalog/`, `libraries/`, `sources/` and `schema/` hold the generators and parts packs from the website era, vendored unmodified and hash-checked. They're test projects now: CI builds them into a starter library that the tests use, and the app doesn't ship them. Their sources and licences: [docs/SOURCE_AUDIT.md](docs/SOURCE_AUDIT.md).

## Licensing

The app's **Licenses & credits** page (in the menu) lists every project's authors, license, status and source, plus the engine, fonts, three.js, Preact and htm. Model pages only credit the author and link there.


No repository-wide license overrides third-party terms. Each `vendor/` project keeps its own notices; see [THIRD_PARTY.md](THIRD_PARTY.md). The OpenSCAD engine is GPL-2.0-or-later (source: https://github.com/openscad/openscad; text in `assets/engine/COPYING`). Original code in `web/`, `tools/`, `adapters/`, `catalog/` and `schema/` is the repository owner's.

Reference: <https://gridfinity.perplexinglabs.com/> (inventory only; no UI code or adapter scripts copied).
