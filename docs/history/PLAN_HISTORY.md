# SCAD Workshop: plan history

What was planned and built, release by release, from Phase 0 (2026-10-05) to 0.4.30 (2026-11-02): the phases, the owner's requests and directions, the designs of each milestone, the reviews and their items, and the notes written after each release. It was the second half of [DESKTOP_PLAN.md](../DESKTOP_PLAN.md) until Review 7 (2026-11-13), which moved it here so the plan holds only what's current. Code comments that name a plan section ("Milestone 6 design", "0.4.22 notes", "Review 3, F-13") point here. For the same story in release order, see [CHANGELOG.md](../../CHANGELOG.md).

## Status as of 2026-10-17



- **Phase 0 done:** engine and storage seams, saved settings, share links, OpenSCAD parameter-file import/export.
- **Phase 1 done:** see "Phase 1 notes" at the end.
- **Phase 1.5 done:** the library interface (section 5); see "Phase 1.5 notes" at the end.
- **Phase 2 done:** the portable library, adding projects, update checks, metadata editing; see "Phase 2 notes" at the end.
- **0.2.x (Phase 2 follow-ups):** numbered releases, the left menu by category (Parametric Models and Parts Library), Library settings, hide / delete / flag as broken, the trash; see "0.2 notes" at the end.
- **Phase 3 done (0.3):** bundled libraries and their modules as **Components**, pinning, the form editor; see "Components" in section 2 and "0.3 notes" at the end.
- **Next (owner, 2026-10-14):** five milestones and a review after each; see "Owner direction of 2026-10-14". Milestones 1 to 5 are done (0.4.4 to 0.4.19, Review 6's items in 0.4.20, the logo in 0.4.21, the library format in 0.4.22, the future features of 2026-10-16 in 0.4.23); next is reading settings reliably (owner, 2026-10-16), then milestone 6 without several libraries (backlog, owner 2026-10-17), then a UI review for overengineering.
- **Phase 4 (0.4) started:** the side viewer (a model's code, a project's reference documents) in 0.4.0; see "Reference documents and the side viewer" in section 5. Next: importing ready-made models and Printables ZIPs, one item per file name, 3MF export, collections.

## Phases

Each phase ends with something usable and with checks in CI.

| Phase | Delivers | Done when |
| --- | --- | --- |
| **0. Seams** | Engine and store interfaces in `web/`; saved settings and share links on the website (browser storage) | Website unchanged for users, plus saved settings; all 55 models still pass in Chromium |
| **1. Desktop shell** | Tauri app (Windows + Linux) with native engine, render queue, cache, workspace folder, built-in catalog offline, saved settings as files, workspace profile (bed size, tolerances, font) | Installers built in CI; all 55 models render natively (including the 3 Underware channels the browser can't); native vs WebAssembly benchmark published |
| **1.5 Interface** | Mockup first. Then: front end split into modules (Preact + htm); app layout with sidebar, browser, inspector, workbench tabs, status bar; grid, list, table and grouped views; search with filters and command palette; `index` seam (in-memory over `catalog.json` for now); thumbnails for the 58 models; favourites, recent; light/dark/night themes; "Updated" filter and sort from source dates | All 58 models and the parts library browsable in all four views; search and filters answer in under 100 ms; the website keeps working with the new browser views; WebDriver tests updated |
| **2. Ingest + index** | Add from GitHub / file / ZIP; update checks with change summary; Rust ingest CLI shared with the website build; the index rebuilt from the library behind the `index` seam (SQLite deferred, see Phase 2 notes), with open metadata fields that become filters, sorts and columns; source pages; "Needs attention"; portable library folder (choose, open, move, merge; versioned format; workspace migrated); the built-in projects packaged as a starter library installed with the app and copied into the library; layered metadata editing at library, project, folder and item level (inspector, overlays, bulk edit, undo, provenance); condensed groups in the browser, with editable group icons | Add 3 public repos by URL; search finds their generators; a simulated upstream change is detected and summarised; edits survive a source update; a project-level license change shows on all its items except those with their own; the library folder, moved to another path and opened on the other operating system's CI runner, shows the same items and metadata; a condensed project shows the icon chosen for it |
| **3. Libraries + modules** | Bundled libraries; BOSL2 doc parser; signature parser; module → form; pinned generators; form editor (labels, box names, conditions, presets as overlays); category editor | The bevel-gear acceptance test passes; every BOSL2 module with geometry gets a form that renders its first doc example; a form edit made in the app matches what a family manifest `ui` block produces |
| **4. Model library + file formats** | Collections; premade import (STL/3MF/OBJ/STEP); Printables ZIPs with their PDF metadata (creator, license, dates); reference documents for projects and a model's code in a side viewer (README as HTML, Printables PDF, readme.txt; section 5; done in 0.4.0); notes per part (a scratch pad, later); one item per file name with a download format picker (built-in parts, imports and generator output); multi-colour and multi-part 3MF export; recipes; quantities, notes, status; ZIP export; collection page; drag and drop in and out; saved-setting thumbnails and variant compare; size search | Import a pack, save 3 recipes, re-render them after a source update, export the collection; a pack with STL, 3MF and STEP copies of each part imports as one item per name; a two-colour label and the Rugged Box download as 3MF files that open in Bambu Studio and OrcaSlicer with their colours and separate parts |
| **5. Productivity** | Batch from CSV, multi-setting sweeps, multi-part 3MF arranged on the bed for the printer profile, send to slicer (open the file in Bambu Studio / OrcaSlicer / PrusaSlicer), live reload when a watched .scad file is saved | As listed |

## Owner requests of 2026-10-12 (next: 0.5)

Ten features and one bug from the owner using 0.4.1. Each item says what it is, what the app does today, and what to build. They are not yet ordered; the owner picks the order. Items that add to the library folder (2: attachments, 8: made parts) bump its format version and keep older libraries readable, as Phase 2 set out.

### 1. Choose the picture shown for anything

Today a project's icon can only be one of its own items' thumbnails (Edit details › Icon); a model or part always shows its generated thumbnail. Many projects ship better pictures (Birdhouse's `renders/*.jpg`, a Printables pack's photos).

- **Build:** an **Icon** field at every level (project, folder, model, part, saved settings, collection), offering: the generated render (default), any picture in the project's files, a render from a saved setting's current values, or a picture you add (copied into the library, so the folder stays portable). The choice is ordinary metadata (`icon` in `metadata.json`, an own field per level like `name`), so it survives updates and travels with the library.
- **Where:** `meta.rs` (allow `icon` below project level), `catalog.rs` (`icon_url` for files and added pictures), `metaedit.js` (a picker with thumbnails instead of the drop-down), the browser cards and the inspector.

### 2. Add your own content to a model, component or part

Documents today come only from the project (its README, a Printables PDF, readme.txt; 0.4.0). You can't attach your own build video, assembly PDF, photos or notes.

- **Build:** **Add content…** on a project, model, part or component page: pick files (video, PDF, images, text, links) that are copied into the library under the item, plus a web link as an entry of its own. They show as extra tabs in the side viewer's Docs (PDF and images as now; video with the webview's own player; anything else opens in its own app), and in the project's Files. Removing one moves it to the library's trash.
- **Where:** a new `attachments/` folder per source (`sources/<id>/attachments/<item>/…`) listed in `metadata.json`, so the project's own files stay untouched and updates don't affect them; `docs.rs` lists them beside the project's documents; the scratch pad of notes per part (Phase 4) becomes one kind of content.

### 3. Category pickers that show every category

Edit details' Category box is a text box with suggestions (`<input list>`). Once it holds a value, the browser only suggests categories that match what's typed, so the other categories you've made can't be seen or picked without clearing the box first.

- **Build:** a real drop-down listing every category (built-in and your own, nested ones indented under their parent, with counts), a filter box at the top, and **New category…** at the bottom. Use the same control wherever a category is chosen: Edit details (one item or several), Pin as model, Library settings › Categories (moving a category under another, or where its items go when it's removed).
- **Where:** a shared `CategoryPicker` in `web/ui/` used by `metaedit.js`, `forms.js` and `libsettings.js`.

- **Done (0.4.3).** `web/ui/catpick.js` (`CategoryPicker`): a button showing the category, opening a list of every category with model and part counts, a filter box, "New category…" (or a typed name that isn't listed, then Enter), keyboard (arrows, Enter, Escape). The list is placed with `position: fixed` so dialogs don't clip it, and clicks inside it are kept from reaching the `<label>` around it (a click in a label goes to its first control: the picker's button, which would reopen it). Used in Edit details, Pin as model and Library settings › Categories. Categories are one flat list, so nothing is indented yet.

### 4. Grouping (Condense) needs fixing and tests

The owner reports odd behaviour after selecting something in the main area and then turning Condense off. Two likely causes in `browser.js`: turning Condense on from "no grouping" switches grouping to "project", and turning it off leaves that grouping in place instead of going back; and the keyboard anchor (`anchor`) is kept while the selection is cleared, so arrow keys and Shift-click start from an item that's no longer on screen.

- **Build:** Condense off restores the grouping you had before; the selection and anchor are kept when the items are still shown and cleared only when they aren't; the inspector follows. Reproduce the owner's steps first.
- **Tests:** the acceptance test selects an item (and a range) in each view, condenses, opens a group, goes back and turns Condense off, checking the grouping, the selection, the inspector and arrow-key movement at each step.

- **Done (0.4.3).** Besides the two causes above: arrow keys, Shift-click ranges and Ctrl+A used every result in sorted order, so inside an opened group they reached items of other groups, and with grouping on the arrow keys jumped (the grid shows items by group). `browser.js` now keeps the items on screen, in the order shown (`shownRef`: none on the condensed overview, the open group's inside one, by group otherwise, as sorted in the table), and selecting, keys and the pruning of the selection and its anchor all use it. `groupBefore` (a saved preference) remembers the grouping Condense replaced. The acceptance test covers the owner's steps.

### 5. Scan the library for changes

The app notices changes by itself only in `local/` projects and linked folders, and only when it starts. Files changed inside a project's stored copy (`sources/<id>/files/<version>/`), or a folder moved, renamed or deleted under `local/` while the app is open, go unnoticed until Read again is pressed project by project. The Birdhouse report below is an example: its files were moved out of `src/` after it was added, and the app still records `src/`.

- **Build:** **Scan for changes** (the Library menu and Library settings), and the same scan when the window regains focus (cheap: compare file lists and sizes first, then contents only for what differs). It reports new, changed, moved and missing projects, reads the changed ones again, and lists anything it couldn't resolve under "Needs attention". Optionally, a folder watcher for `local/` and linked projects (this is also Phase 5's "live reload of a watched .scad file").
- **Where:** `sources::sync_local` grows into a `scan` command covering stored copies too (a stored copy that changed is recorded as an edited version, like a linked folder's `live-` version, so a GitHub update still compares against what upstream sent).

- **Done (0.4.3).** `project::scan_changes` (called by `source_rescan` after `sync_local`): a stored copy is compared with `files_stat` (paths, sizes, modification times to the nanosecond) and, only when that differs, `files_fp` (contents), both recorded in the derived data when a project is read; changed ones are read again (not recorded as an edited version yet: an update from GitHub still downloads upstream's files beside them). Projects whose files are gone are returned as `missing` and listed under Needs attention (any kind, not just linked). The page scans at start-up, when the window comes back into focus (at most once a minute, not while a dialog is open) and with Scan for changes in Library settings. Not done: a folder watcher.

### 6. A standard project folder layout (the owner's question: should there be one?)

Now part of milestone 3, "Consistent projects", in "Owner direction of 2026-10-14" below.


**Recommendation: yes, as a convention the app reads when it's present, never a requirement.** Projects from GitHub, Printables and ZIPs arrive in whatever shape their authors chose, so the app must keep guessing well; but a project you make yourself, or tidy up, should be able to say exactly what it is.

- **The layout:** a `workshop.json` at the project's top level naming its models (file, name, picture, category), the files that are library code only, its documents and pictures, and its license and authors. Suggested folders: `models/` (the `.scad` files to open), `lib/` (included code), `parts/` (ready-made STL/3MF/STEP), `docs/`, `images/`. Without `workshop.json` the app guesses as now; with it, it doesn't guess. The app can write one for you from what it found plus your edits (**Save as project file**), which makes a project portable to another library or a fresh install with no work.
- **Relation to today:** `editor.toml` (settings forms) and the family manifests in `catalog/families/` (the website's models) already do part of this; `workshop.json` should reuse the family manifest's shape so one reader serves both. It also settles the Birdhouse kind of case: the project says which file is the engine.

### 7. Open a .scad file without adding it, and generate from it

**On hold** (owner, 2026-10-14).


Today a `.scad` file only gets a page after it's added to the library (as a ZIP, a folder, or dropped into `local/`).

- **Build:** **Open file…** (and dropping a `.scad` file on the window) opens it on a temporary model page: settings from OpenSCAD's export, the Code tab, Generate and download, as for any model, with its folder as the include path. A bar offers **Add to library** (copy or link its folder, as Add project does) and otherwise nothing is written to the library. Recently opened files are listed on Home. On the desktop the file can also be watched, so saving it in another editor re-renders (Phase 5's live reload).
- **Where:** a `scratch` source kind held in memory (or the app's data folder) rather than in `sources/`; the render path is the same.

### 8. Make parts from a generator and keep them in the Parts Library

Generating today ends in a download. The owner wants the output kept as ready-made parts: one, or a batch.

- **Build:** **Save as part** next to Download (the current settings: one part), and **Make several…** (a table of rows, each a set of values and a name, or the saved settings you tick; Phase 5's batch from CSV and sweeps are the same table filled from a file or a range). Each result becomes a ready-made part in the Parts Library, in a category you choose, with its STL/3MF, its picture, and a link back to the model and the exact values and code edits that made it, so it can be made again after an update (this is Phase 4's "recipes" with their output file).
- **Where:** a `made/` project in the library (`sources/made/…`, or `local/made/`) holding the files and a manifest of where each came from; renders go through the existing queue.

### 9. Several libraries open at once, and which one an item is from

This moves "Multiple libraries" out of the future options below and makes it a requested feature.

- **Build:** Settings › Libraries lists the open libraries; each can be added, closed, set as the one new things go into, or opened read-only. The browser shows all of them together, with a filter by library, and every card, row, model page and inspector names its library (a short label and a colour chosen per library). Each library keeps its own saved settings, trash, categories and metadata; copying or moving an item to another library is an action of its own.
- **Decisions:** which copy wins when the same project is in two libraries (show both, labelled, is the simplest); whether favourites and recent belong to the app or to each library.
- **Where:** `config.rs` (a list instead of one path), `App` holds several `Library` values and every command names the library it acts on, the catalog is built per library and merged with the library's id in each item's key.

### 10. Back up a library to a ZIP

- **Build:** **Back up library…** opens a save dialog (default name `<library name> <date>.zip`) and writes the whole folder to one ZIP, with progress and Stop, leaving out what can be rebuilt (thumbnails, previews, render cache) unless you tick "Include previews". **Restore from backup…** unpacks one as a new library folder and opens it (never over the open one). Later: a reminder after a number of days, and keeping the last N backups in a chosen folder.
- **Where:** the `zip` crate is already used to read ZIPs; it needs its write side. The backup is the same thing as Phase 4's "ZIP export", for the whole library instead of a collection.

- **Done (0.4.3)**, in Library settings › Backups (`backup.rs`, commands `library_backup` and `library_restore`, the app's `pick_save_path` dialog). Changed from the plan: the backup keeps previews (some parts' previews come from the app's own copy of the starter library and wouldn't come back) and offers "Leave out thumbnails" instead; restoring takes an empty folder chosen in a folder dialog. Jobs can now be stopped (`job_cancel`; the status bar's Cancel). Not done: reminders, keeping the last N.

### Bug: Birdhouse Workshop shows only Castle and Engine

Reported with the project's folder from the library (`zip-birdhouse-workshop-main`). Triage: see "0.5 notes" at the end.

## Owner direction of 2026-10-14 (next: 0.4.4 onwards)

After reviewing the project at 0.4.3, the owner set the direction for the next releases. In the owner's words, "the major work that needs to be done is increase the consistency of the workflow so that imported scad files work more consistently, default values are valid, the file structure is consistent across files and generators regardless of source (while maintaining the ability to conduct update checks from source)."

### What changes in the plan

- **Item 7 (open a .scad file without adding it) is on hold** until the owner brings it back.
- **Consistency is the main theme** (below). Item 6 (a standard project layout, `workshop.json`) becomes part of it rather than a feature of its own.
- **Periodic reviews** of the interface and of the library's file structure (below), as Model Library does after each group.
- **Reuse from Model Library** ([DrFlGd/Model-Library](https://github.com/DrFlGd/Model-Library), the owner's other app, which began as a trimmed copy of this one): its **skins** first, then whatever else fits (below).

### Milestones, in order

Each milestone is a release (0.4.x while Phase 4 runs) and is followed by a review. The order can change after any review.

| # | Milestone | Done when |
| --- | --- | --- |
| 1 | **Rename clean-up** (0.4.4). Links and text that still say Claude-Grid-Workshop point at SCAD-Workshop; 0.4.3's notes move out of Unreleased. Grown the same evening by the owner's decisions below: no starter projects in the installer, the website retired, no Needs attention; plus Review 1's UI-6 and F-6. | No old addresses left except where history is described; the installer has no `starter/` but has `libs/`; the app's releases link opens |
| 2 | **Skins from Model Library** (0.4.5, "Skins design" below), then Review 1's interface items UI-2, UI-3, UI-7, UI-8, UI-10 and UI-11. Drafting Sheet, Parts Bin, Layer Line and Classic (today's look), chosen in Settings. | Every page looks right in each skin, light, dark and night, in the app test's screenshots |
| 3 | **Consistent projects** (0.4.6 to 0.4.9, "Consistent projects design" below): one project description for every source, defaults that render, a check for every model on import, and the `workshop.json` layout. | The consistency checks below pass for the 19 starter projects and the test projects; an update from GitHub still compares against upstream's own files |
| 4 | **Make parts from a generator** (item 8, "Make parts from a generator design" below; 0.4.11, with Review 3's F-15 in 0.4.12): Save as part and Make several…, with recipes. | As in item 8; made parts can be made again after an update |
| 5 | **The rest of Phase 4:** importing STL/3MF/OBJ/STEP and Printables ZIPs (with Model Library's readers where they fit), one item per file name with a format picker, multi-colour and multi-part 3MF, collections, ZIP export. | Phase 4's "done when" in the Phases table |

Items 1, 2 and 9 of the requests of 2026-10-12 (pictures, your own content, several libraries) come after these, unless a review moves them up.

**Added 2026-10-16 (owner), before milestone 6: the library format.** The folder layout is already the same for every project whatever its source (milestone 3), but it's written down only in the code and the library's README. Several libraries open side by side (milestone 6) need it fixed first. Planned in detail before it's built:

- **A written spec**, `docs/LIBRARY_FORMAT.md`: every folder and file in a library, what writes it, what's rebuildable, and what must be kept; the README in each library becomes a short summary of it.
- **A format version in each JSON file the app writes** (`library.json`, `source.json`, `metadata.json`, `collection.json`, recipes, derived data), so a newer app can upgrade a file it reads and an older one can tell it's looking at something newer. Files without one are read as today's.
- **One recipe shape:** saved settings (`recipes/<id>.json`) and a made part's `<name>.recipe.json` share the same fields (model, project and its version, files fingerprint, values, code edits, format), so either can be made again the same way.
- **Check library** in Library settings: reads the whole folder against the spec and lists anything off it (a missing file, an unknown version, a recipe whose model is gone, a collection entry that refers to nothing), with Repair for what can be repaired safely. Nothing in `files/<version>/` is ever changed: projects stay exactly as they came, which is what update checks and hash checks rely on.
- **Not changed:** your own and linked projects keep being read where they are, and the project files themselves aren't reorganised.

**Added 2026-10-16 (owner), after the library format: reading settings reliably.** Each author writes the Customizer comments differently, and many BOSL2 components have no defaults or defaults that don't render. Instead of guessing from the comment style, each setting is checked when a project is added, and the owner's failed edge cases become CI tests:

- **OpenSCAD's own parameter list** where the bundled build offers it, with the app's comment reader filling in what it lacks.
- **Types from evaluation:** the file is run once with every top-level setting echoed, so a default written as an expression (`w = 2*pitch`) gets its real value and type.
- **Defaults that fail are repaired** from the project itself: its `.json` parameter sets, its `examples/` files, and for BOSL2 the `// Example:` blocks in its documentation, which are working calls.
- **Choices instead of free text:** a text setting whose values form a known set (from the code's comparisons, such as `if (style == "round")`, or from the examples) becomes a drop-down; you can also turn any text field into a drop-down, or back, on the model's page, kept in the project's `metadata.json`.
- **Uncertain settings are marked** for you to check; what you correct is kept in the project's description so the next update doesn't guess again.

**Added 2026-10-16 (owner), future features:**

- **Show in folder** on a model, part or project: opens the original files in Explorer (or the system's file manager), so they're quick to find.
- **"Make several" becomes "Batch generate"** wherever it appears.
- **2D models:** a model whose output is flat (`projection()`, `square()`, `polygon()` and so on at the top level) is shown as a 2D drawing and downloads as SVG or DXF. This explains at least some of the models that show nothing at their defaults today; the check on import treats a 2D result as a success.
- **No suggested projects in Add a project:** the Suggested row goes.
- **A single .scad file as a project:** Add a project accepts one file as well as a folder or ZIP; it becomes a project of its own (copied in, as folders are).
- **Notes** on a project and on each model: a Notes tab on the project's page and on the model's page for free-form comments, kept in the project's `metadata.json` (so they stay with the library, survive updates, and are found by search).
- **One Code button:** on a model's page the Code button sits in the header beside README; the tab strip along the right edge of the 3D view doesn't repeat it. The code panel still opens on the right when asked for.
- **Edit form shows the real values:** each field (Label, Help, Section, Default, Min, Max, Step, Choices, Show when) is filled in with what was read from the file, editable as text, rather than a greyed hint that vanishes when you start typing. Only a field you change is saved as an override; Reset puts back what was read.

**Owner, 2026-10-17:** "go ahead with all of this and then do a UI review to make sure we haven't overengineered the UI … It's worth having a fallback if this new type of parsing yields errors." The order: the future features above (0.4.23), reading settings reliably (with today's reader kept as the fallback whenever the new reading errors), milestone 6, then a UI review looking for what's overengineered. The same day: "multiple libraries can be put in the back log for now", so milestone 6 is pictures and your own content; several libraries (section 9) waits in the backlog.

### Decisions later the same evening

- **No starter projects in the build.** "We can stop including SCAD generators and models with the build, those were included as initial tests. The component libraries should still be included and I am open to other foundational generators or components you find to be relevant." The installer stops shipping the starter library; a new library has Components only. The website-era projects stay in the repository as test projects (CI builds a starter library for the tests) until smaller test projects replace them. Suggesting foundational projects to add (a list on Home, added from GitHub so they get update checks) belongs with milestone 3, once a project's models are checked at their defaults when it's added.
- **The website is retired** (decision card: "Retire website"). Its workflow is removed; Pages is switched off by the owner in the repository's settings. The desktop build still uses the site build for its WebAssembly engine pieces and the test projects.
- **No Needs attention** (on Review 1's UI-1): "that Needs Attention bar can be removed and just show the license status when looking at the libraries included in the settings pages". Library settings › Projects shows each project's license and marks what's wrong with it; the OpenSCAD libraries table already shows theirs.

### Consistency: what it means and how it's checked

Today a project's shape depends on where it came from: the built-in generators are described by hand-written family manifests (`catalog/families/*.json`, with `ui` overlays) and arrive as starter projects; projects added from GitHub, ZIPs and folders are described by what reading guessed (`derived/<version>.json`); `local/` and linked projects are read when they change; components and pinned components, lifted copies (`generated/`) and saved settings each have their own records. The aim is that every project, whatever its source, ends up described the same way, with the same guarantees, while its upstream files stay untouched so updates can still be checked against the source.

- **One project description.** Every project has the same description in the library, in the shape of a family manifest (models with entrypoints, library folders, settings overlays, presets, license, authors, links, documents, pictures), whether it was written by hand (`workshop.json` in the project, a family manifest for the built-ins) or found by reading. What reading found and what you edited stay separate (as `derived/` and `metadata.json` are now), and one reader serves the website build, the starter library and added projects.
- **Upstream files stay as they came** in `files/<version>/`, so an update still downloads the new version beside the old one and compares them. Anything the app makes (lifted copies, the description, thumbnails) lives beside them, never inside.
- **`workshop.json`** (item 6): a project can carry its own description at its top level; when present the app uses it instead of guessing. **Save as project file** writes one from what reading found plus your edits.
- **Defaults that work.** When a project is read, every model is rendered once at its defaults (natively, in the background, like the bench). A model that fails is listed under Needs attention with OpenSCAD's message, and the reason is kept with the project. Settings whose default is outside its own range or choices, or empty when required, are reported the same way. Components get the same check (today about 370 of 796 open with required settings empty).
- **The same checks in CI** for the starter projects and a set of test projects of different shapes (Gears, LEGO.scad, Birdhouse, a library-only project, a ZIP with its files in a subfolder).

The detailed design (formats, commands, what changes in `derived/`) is written here before it's built, as usual.

### Periodic reviews

After each milestone, a review in two parts, published as a page for the owner with numbered items, ranked, each with a recommended fix (Model Library's R17 is the model for this):

- **Interface:** every page in the website and the app (screenshots from CI at two window sizes, light and dark, and each skin once skins land), looking for inconsistent wording, layout, controls and behaviour between pages.
- **Library file structure:** a library built from the starter projects and the test projects, looking at how each project is stored (`source.json`, `metadata.json`, `derived/`, `generated/`, recipes, thumbnails), for differences between sources, files that can't be explained without the app, and anything that would break an update check or a move to another computer.

The owner picks which items to take; they ship in the next release, and the review is noted under "Review N" in the notes at the end.

### Reusing Model Library's work

Model Library began as a trimmed copy of this repository (its Tauri shell, command table, library folder code and Preact interface), so its code fits here with little change. In order of use:

- **Skins** (its 0.5.19 and 0.5.20; its plan's "Skins design"). The skin is `data-skin` on the page's root beside `data-theme`; each skin is a block of colour names in `web/styles.css` for light and dark, plus names for what used to be written in place (`--font-head`, `--font-mono`, `--radius-s`, `--radius-m`, `--accent-hover`, `--accent-edge`, `--chrome-ink`, the icon line `--icon-stroke`, `--icon-cap`, `--icon-join`). Icons keep their names and follow the skin's line; anything a skin adds beyond colours (grid paper, tape labels, layer lines) is a few rules under its `data-skin`, never a change to the pages' markup. Fonts (all SIL OFL) are bundled into the app by `tools/build_desktop.py --fetch-fonts` and come from Google Fonts on the website. The skins: **Drafting Sheet** (technical drawing: square corners, fine rules, drafting blue, Overpass and Overpass Mono), **Parts Bin** (workshop drawers: machine-tool green, Sofia Sans, label-tape headings, pegboard), **Layer Line** (print bed and filament: silk magenta, M PLUS Rounded 1c and Atkinson Hyperlegible, pill shapes, layer lines) and **Classic** (today's look). The choice is per computer, in Settings beside Theme.
- **Later, where they fit Phase 4:** `mesh.rs` (reading STL, OBJ and 3MF, with 3MF colours and the picture a slicer saved), `archive.rs` (7z and RAR beside ZIP), `dupes.rs` (duplicates by fingerprint and SHA-256), `watch.rs` (a folder watcher, for live reload and Scan for changes), the journal with Undo for changes that move files, and the About page with the changelog.
- **Lessons:** a review after each group of work keeps the interface consistent; the library folder must make sense without the app (keep unknown keys, sorted JSON, never a database); one action list per kind of thing, used by every menu and bar.

### Skins design (milestone 2, 0.4.5)

Model Library's skins (its plan's "Skins design"), brought over with its colours, fonts and icon drawings unchanged so the owner's two apps look alike. What differs is only where SCAD Workshop's pages differ.

- **Where it's chosen.** Settings › Appearance gets a **Skin** box above Theme: Drafting Sheet (the default, as in Model Library), Parts Bin, Layer Line and **Classic** (the look up to 0.4.4). It's this computer's choice, kept with the other layout settings (`skin` in `gw-ui`). The command palette lists the skins as it lists the themes. Theme stays as it is: every skin has light and dark colours, and Night keeps its dim warm colours with the skin's fonts, shapes and icons.
- **How it works.** `data-skin` on the page's root beside `data-theme`. The colours that were written in place become names in `web/styles.css`: `--chrome-ink` (the top bar's text; its search box and buttons are mixed from it, so a light top bar works), `--accent-hover`, `--accent-edge`, `--star`, `--font-head` with `--head-stretch` (headings), `--font-mono` (code, sizes, counts), `--radius-xs`, and the icon line `--icon-stroke`, `--icon-cap`, `--icon-join`. Each skin sets them, with its colours for light and dark, plus `--stage-bg` and `--hud` for the 3D view.
- **What each skin adds**, as rules under its `data-skin` (no change to the pages' markup), mapped onto this app's pages:
  - *Drafting Sheet:* a faint grid behind pictures and the 3D view; an ink rule under the top bar; section headings (Home, Library settings) in small mono capitals with a rule after them; the current menu row outlined in blue and counts as [12]; corner ticks on cards and tiles; sizes, counts and code in Overpass Mono; the status bar as the drawing's title block.
  - *Parts Bin:* pegboard behind Home and the text pages; label-tape headings and category-panel title; drawer-front cards (heavier bottom edge); the current menu row in green with counts in small drawers; the status bar as tape.
  - *Layer Line:* layer lines on pictures, the menu and the 3D view; pill-shaped buttons, chips and menu rows; each category in the category panel gets a spool colour, in order of eight.
- **Icons:** every icon's line follows the skin; home, star, box, cog and search have a drawing for each skin (Model Library's), and the brand mark keeps its four squares.
- **Fonts** (all SIL OFL, credited in THIRD_PARTY.md) are bundled by `tools/build_desktop.py --fetch-fonts` as Archivo is; Layer Line's heading font is cut to Latin letters with fontTools. A font file is only read when the skin in use needs it.
- **Tests:** the app test opens Settings, checks the four skins are offered and that Drafting Sheet is the default, switches to each one and takes a screenshot in light and dark, so the review after this milestone can look at all of them.

### Consistent projects design (milestone 3, 0.4.6 to 0.4.9)

Milestone 3 ships in four releases: **defaults that work** (0.4.6: D-1 to D-3, UI-4, UI-5), **one project description** (0.4.7: F-2 to F-5), **Review 2's small items** (0.4.8) and **starter projects as GitHub projects** with suggested projects on Home, plus Review 2's UI-16, F-9 and F-11 (0.4.9: F-1). A review follows the milestone. The order puts what the owner sees first (models that open and render) before what only the library folder shows.

#### Defaults that work (0.4.6)

- **Settings that fit their own limits (D-3).** OpenSCAD's settings export gives a plain number with no Customizer comment a step of 1, so `Width_Units = 1.5;` arrives as a number box that calls its own default invalid (Gridfinity Anylid). Reading a project now gives such a setting the step of its default's last decimal (0.1 for 1.5), as OpenSCAD's own Customizer does. Then every setting is checked against its declared limits: a number outside its slider's range or off its step, a choice list its default isn't in (today the default is quietly added as an extra choice; it stays, and is reported), an empty text the model needs. Each is a problem of kind `limits` on the model, with the setting's name and what's wrong ("Width units: 1.5 isn't a step of 1 from 0"). The form never refuses a default: a number box whose step doesn't fit its default takes any value.
- **Every model rendered once at its defaults (D-1).** Thumbnails already render each model at its defaults; that render becomes the check. The result is kept per project in `sources/<id>/checks.json`: `{ "version", "engine", "models": { "<model>": { "ok", "message", "seconds", "date" } } }`, rewritten whenever a model is checked. It's rebuildable (listed as safe to delete in the library's README). A model that fails keeps OpenSCAD's first error as its message and isn't tried again until the project's version or the engine changes, or you ask (**Check defaults** in the project's ⋯ menu, which checks every model again, thumbnails or not). Command: `defaults_put { source, model, ok, message, seconds }`.
- **Where it shows.** Library settings › Projects marks a project with failing models (“2 models don't render at their defaults”) beside its license, listing each with its message; the model's own page says so above the form, with the message, so the first thing you see on opening it isn't a silent failure. Problems of kind `limits` show the same way. Nothing else nags (Needs attention is gone).
- **Library lists as choices (D-2).** A NopSCADlib part whose first setting is its type (`ball_bearing(type)`) already offers the lists in the matching plural file (`ball_bearings.scad`) as choices, but starts on "Choose…". It now starts on the first in the list, so it renders straight away; the same for any setting whose choices come from a library's own list. CI's component report counts again how many components open with nothing to render (about 370 of 796 before).
- **Names (UI-4, UI-5).** Component settings get the same names as model settings (words split and capitalised: `mate_teeth` → "Mate teeth"), with the code name on hover and in Code. A model's own name is the main text everywhere it appears (card, tab, page title), with its project as the second line.
- **Tests.** Reading a project with a fractional unannotated default gives a step that fits it, and a setting off its limits makes a `limits` problem (core tests). The acceptance test opens Anylid and finds no invalid number box, adds Gears and checks that `checks.json` records its models after thumbnails are made, and opens a model whose default render fails (a small test project made to fail) to find the message on its page and in Library settings.

#### One project description (0.4.7)

- **The same version fields (F-2).** Every entry in `versions` records `id`, `date`, `added`, `files` (count) and `content` (the files' fingerprint, as `files_fp` in derived), plus `commit` and `message` when it came from GitHub and `sha256` for a ZIP. Today's version ids stay (renaming folders would break saved settings and thumbnails); missing fields are filled in when a project is next read.
- **Origin recorded the same way (F-3).** Every `source.json` has an `origin` with `type` (`github`, `zip`, `folder`, `linked`, `local`, `app`) and its address: `url`, `owner`, `repo`, `subdir` for GitHub; `file` for a ZIP; `path` for folders. Adding a project whose origin matches one already there says so and offers to open that one (GitHub already refuses; ZIPs are matched by `sha256`, folders by path).
- **What reading found has the same fields (F-4).** One reader for every source: starter projects in old libraries are read like any other project the next time the app reads them (they get `files_stat`, `files_fp`, `readme`, `libraries_used`; their models get `folder`), so Scan for changes covers them too.
- **`workshop.json` (F-5, item 6).** A project's description in the family manifest's shape (`name`, `summary`, `category`, `tags`, `license`, `authors`, `links`, `models` with `entrypoint`, `name`, `summary` and a `ui` overlay, `presets`, `documents`, `pictures`), at the top of the project's own files. When a project ships one, reading uses it instead of guessing which files are models and what they're called; what reading finds still fills in the settings. The app writes the description it uses for every project to `sources/<id>/workshop.json` (beside `files/`, never inside upstream's files), so the library folder says what each project is without the app. **Save as project file** (project ⋯ menu) writes it into a `local/` or linked project's own folder, from what reading found plus your edits, for sharing. The schema is `schema/workshop.schema.json`, checked in CI.

#### Starter projects as GitHub projects (0.4.9)

- **F-1.** Libraries made before 0.4.4 keep the starter projects as `bundled`, stored in this repository's layout and never updated. A one-time offer in Library settings (not automatic: it downloads) turns each into a GitHub project pinned to the commit its family manifest recorded, with upstream's own layout under `files/<commit>/`, keeping its id, saved settings, edits and thumbnails. Projects that aren't on GitHub (Anylid, the openGrid adapters, the parts packs) become `local`-style projects marked as from the app, and stay as they are. (Built in 0.4.9 for the 6 that are on GitHub as the app had them; see "0.4.9 notes".)
- **Suggested projects on Home.** A short list of foundational projects, each added from GitHub (so it gets update checks) and checked at its defaults when added: Gridfinity Rebuilt, gridfinity_openscad, Gears (chrisspen), LEGO.scad, and the openGrid and Underware projects once their licenses are settled. The list is a file in the app (`catalog/suggested.json`), shown on Home in a new library and under Add project; adding one is the ordinary Add from GitHub.
- **CI** adds Gears, LEGO.scad, Birdhouse and gridfinity_openscad to the default-render checks the starter projects have.

## Phase 1 notes

What was built, and where it differs from the plan above.

- **App:** `desktop/src-tauri` (window and commands) over `desktop/core` (Rust with no GUI: native renders, queue, cache, workspace, saved settings as files). The website's front end runs unchanged except for `web/platform-desktop.js`. Data and model files go through the app's commands rather than URLs.
- **Engine bundling:** Windows ships the snapshot folder. Linux ships OpenSCAD's AppImage as one file and unpacks it on first start into the app's data folder, which avoids FUSE and symlinks in the package. The Linux AppImage expects `libOpenGL`, `libEGL` and `libGLX` from the system; the `.deb` declares them.
- **Desktop-only models:** `build_site.py --native-engine` reads the settings of the three Underware channels that crash the WebAssembly engine. The desktop build includes them (58 models); the website build leaves them out (55).
- **Printer profile:** limited to print bed size and nozzle. Magnet and tolerance settings are left unlinked: projects define them differently (hole size with or without clearance), so one value can't safely feed them all. The profile works on the website too.
- **Windows engine race:** native OpenSCAD on Windows spends 4–5 s handling `use`/`include` for Gridfinity Extended. CI experiments ruled out Defender, the library path and font setup; the same code parsed as one file takes 0.2 s. So the Windows app races native against the WebAssembly engine on each model's first render and remembers the faster one. It might be worth reporting upstream to OpenSCAD.
- **Fonts:** on Windows the app writes a fontconfig file (system fonts, OpenSCAD's bundled fonts, a cache in the workspace), because the snapshot ships none. The UI font (Archivo) is bundled, so the app makes no web requests.
- **Testing:** this development environment can't reach crates.io, npm's Tauri packages or Ubuntu's package mirrors. All Rust builds and app tests therefore run in CI (`desktop.yml`):
  - unit tests;
  - `workshop-cli bench` (all 58 models, first and repeated renders) on Linux and Windows;
  - a WebDriver test of the built app on both systems: catalog, native and desktop-only renders, saved settings in the workspace, Settings, a part preview;
  - installers on the `desktop-latest` pre-release (numbered releases replaced it in 0.2.0).
- **Results (CI, 2026-10-04):**
  - All 58 models render natively on Linux and Windows.
  - In the built app on Windows, the race picked WebAssembly for the Gridfinity Extended bin: 3.7 s, against about 12 s native on the same runner.
  - On Linux, native won (1.1 s).
- **Not yet:**
  - code signing;
  - auto-update (the release is rebuilt from `main`; download it again to update).

## Phase 1.5 notes

What was built, and where it differs from section 5.

- **Mockup first:** a clickable mockup of the layout and the four views settled the look before code.
- **Front end:** `web/ui/` holds the new interface as Preact + htm islands (sidebar, browser, inspector, tabs, status bar, command palette, quick look) sharing one small store (`web/lib/store.js`); about 1,300 lines. `web/app.js` keeps routing, the model workbench (form, preview, batch, saved settings) and the settings and license pages. Preact and htm are vendored as ES modules (`tools/vendor_preact.py`), so the website still deploys as static files with no build step.
- **`index` seam:** `web/ui/index-local.js`, in memory over `catalog.json` and the parts libraries: weighted fields (name, project, tags, category, description, setting names), prefix matching, one-typo tolerance, synonyms, typed filters (`kind:`, `tag:`, `project:`, `cat:`, `license:`, `updated:`), facet counts and sorting. Searches take under 15 ms in CI's Chromium. SQLite replaces it behind the same `query()` in Phase 2.
- **Updated date:** `tools/source_dates.py` records when each pinned upstream commit was made (`source.commit_date` in the family manifests); owner-supplied files use their supply date. The browser can filter (last month / 3 months / year / older), sort and show a column by it. This is the first of the "more metadata" filters (section 5, Search); creator, license family and fields from model-site downloads follow with the Phase 2 index.
- **Thumbnails:** `tools/thumbnails.py` draws every generator's default render and every part preview with the site's own viewer in headless Chromium (480 × 360 WebP, transparent, about 0.8 MB for all). CI makes them from the benchmark's STLs on the website and with native OpenSCAD for the app. Parts that come only as CAD files show their initials.
- **Workbench tabs:** each opened generator keeps its form, unsaved changes and last render while you switch tabs; the open tabs are remembered (browser storage, or the workspace in the app).
- **Themes:** light, dark and night (near-black and warm, with dimmed thumbnails, for a dark workshop), following the system by default. The top-bar button cycles them; Settings has all four choices.
- **Render on demand:** the 3D view now draws only when something changes, so a preview in a hidden tab or a closed quick look uses no GPU time.
- **Rename:** SCAD Workshop. The app's identifier stays the same, so settings carry over; the workspace default is `~/SCAD Workshop`, with an existing `~/Claude Grid Workshop` folder kept in use.
- **Not yet (later phases):** metadata editing (the inspector's "Edit details" is a placeholder), saved searches as smart lists, drag-select, and resizable table columns.
- **Testing:** `tests/interface.py` (33 checks in Chromium: views, parts, filters, search speed and relevance, inspector, quick look, favourites, palette, tabs, themes, phone width) runs in the Site workflow; `tests/desktop_ui.py` gained the library, thumbnails, search, tabs and night-theme checks for both app builds.

## Phase 2 notes

What was built, and where it differs from the plan above.

- **One project reader, in Rust** (`desktop/core`: `ingest.rs`, `sitebuild.rs`, `scan.rs`, `project.rs`). The website build runs its catalog steps through `workshop-cli site-prepare` / `site-finish`; their output matches the earlier Python build exactly (every data file and fs/). The app reads added projects with the same code: models are `.scad` files that make geometry at top level, settings come from OpenSCAD's own export run natively, and README, license (SPDX), preset files, editor.toml and ready-made model files (one item per name, every format) are picked up.
- **The library** (`library.rs`, format 1) is laid out as in "Library folder" above, with `derived/`, `thumbs/` and `previews/` per project. A Phase 1 workspace upgrades itself when opened (saved settings to `recipes/`; preferences and the printer profile to the app). A library from a newer app opens read-only.
- **Starter library:** the website build's 58 generators and the openGrid parts pack are packaged by `workshop-cli bundle` into 19 library projects that ship with the installer; the app copies them into the library on first start and offers changed ones as updates later. CI renders all 58 models from the starter library on Linux and Windows.
- **Adding projects:** from a GitHub link (a pinned commit's tarball; branch, tag, commit and subfolder links work), a ZIP, a folder (copied or linked) or the library's `local/` folder (read when it changes). Projects can be marked as libraries: `<BOSL2/std.scad>` then resolves to them, so a project that includes BOSL2 works once BOSL2 is in the library.
- **Updates:** GitHub projects are checked at most daily (and on request). A newer commit is downloaded and read beside the current version, and summarised: commits, models added or removed, settings added, removed or with new defaults. Accept or skip; edits live separately and survive either way.
- **Metadata:** edited at item, folder, project or library level, several items at once, with where each value comes from, undo, and an offer to clear items' own values after a project-wide change. Open fields ("material: PETG") become filters, sort keys, table columns and search terms. Categories can be added and given icons; a project's icon is one of its items' thumbnails.
- **Condensed groups:** any grouping can show one tile per group (an icon, or a collage of its items), on the website too.
- **Search stays in the page.** The plan had SQLite behind the `index` seam for the app. The in-memory index answers in a few milliseconds for the current library and is the same code as the website's, so the app and the site search identically; it gained open fields and is rebuilt from the library on each change. SQLite can come back behind the same seam if libraries grow past tens of thousands of items.
- **The app's backend** is one command table (`api.rs`) behind the Tauri app and `workshop-cli serve`, a local stand-in that serves the page and the same commands over HTTP. Library files reach the page through a `library://` protocol.
- **Not yet:** reading Printables' PDFs (Phase 4, with ZIP packs of ready-made parts); re-rendering saved settings to compare before accepting an update (with recipes in Phase 4); GitLab and other hosts.
- **Testing:** Rust unit tests for the reader, library, metadata and sources; `tests/desktop_page.py`, the Phase 2 acceptance test, drives the page against the real backend in CI: the starter library, three public GitHub projects added by URL (one pinned to an older commit, one a library), a local project that includes BOSL2, search, a render from library files, the upstream change detected, summarised and accepted with edits surviving, a project-wide license with one item keeping its own, a condensed group's icon, and the library opening the same after moving it. CI then opens that Linux-made library on Windows and compares (`library-summary`). Locally the same test runs against `tests/github_mock.py`.

## 0.2 notes

Follow-ups to Phase 2, from using it, before Phase 3.

- **Numbered releases.** The repository keeps `<major>.<minor>.0` (tauri.conf.json and the Rust workspace); the minor number is the roadmap phase. CI gives each build the next free number in that series from the release tags (`tools/set_version.py --next`), stamps it into the app and the installers, and publishes release `v<version>` (with the commits since the previous release as notes) only when the Linux and Windows apps, the acceptance test and the portability check all passed. The unnumbered `desktop-latest` pre-release is gone. Settings → About shows the version; `workshop-cli --version` too.
- **Names:** generators are **Parametric Models** and ready-made parts the **Parts Library**, in the menu, on Home, in filters and in the inspector.
- **Left menu:** both sections list categories, collapsed until opened, with their projects inside; parts now have categories too (a parts pack's category, from the project like a model's; the pack's own groupings become a "Type" filter). The Projects list is gone; Add a project sits at the bottom of the menu and on Home.
- **Condense** is a labelled button, always shown; with no grouping it groups by project. A project's tile opens the project's page (details, README, files, versions, updates) above its items.
- **Editing:** fields start with the value in use, editable in place, with a note on where it comes from. An unchanged value stays inherited; Reset drops one set at this level. An item can be listed under another project (`project` on the item): it groups, filters and lists there, and is hidden with that project, while its files, license and credits stay with the project it came from.
- **Hide, delete, flag:** `hidden` (inherited: a hidden project hides its items), `deleted` (the item is left out of the catalog, also after updates; Library settings brings it back) and `broken` ({ note, date }: a badge, a "Status" filter, Needs attention; a failed render offers it) are metadata like any other, so they move with the library and undo works. A deleted project goes to `trash/<time>-<id>/` (with your own project's `local/` folder) until it's brought back or the trash is emptied.
- **Library settings** (`#/library-settings`): the folder, projects (hide, delete), categories (add, rename, remove: a removed category points to the one its items moved to, so projects added later land there too; it can be brought back), hidden and deleted items, the trash, defaults, GitHub.
- **Testing:** the acceptance test adds hide, flag, delete (with Ctrl+Z and restore), moving an item to another project, categories, a hidden project and the trash; the website test checks the left menu and Condense; the app test checks the version and Library settings on Linux and Windows.

## 0.3 notes

Phase 3 as built.

- **Libraries:** `libraries.json` pins five libraries by commit and by a SHA-256 over their file list; `tools/fetch_libraries.py` downloads GitHub's tarball at that commit (or reads a local clone), keeps the OpenSCAD and data files outside docs and tests (and no data file over 2 MB: threads-scad's demo STL), and checks the hash. `tools/build_desktop.py` indexes them (`workshop-cli libs-index` → `libs/index/<Name>.json`); the installer carries `libs/` (about 9 MB).
- **Reading a library** (`desktop/core/src/components.rs`), three ways: BOSL2-style docs (`// Module:` / `// Function&Module:` blocks; Synopsis, Topics, SynTags, Usage, Arguments with "Default:", Named Anchors, Examples), NopSCADlib's `//!` comments, or plain signatures with the comment above. A module counts when it makes geometry on its own: BOSL2's "SynTags: Geom" without `CHILDREN`, `{ ... }` or `PARENT()` in its usage, and not drawn only in preview (`ruler()`); other libraries' modules that don't call `children()`, aren't examples, demos or tests, and don't say they work on their children. Topics map to the menu's groups (Gears & pulleys, Threads, Screws/nuts/fasteners, Bearings, Hinges & joints, Motors, Electronics, Boxes & enclosures, 3D and 2D shapes, Rounding & masks, Paths & sweeps, Textures, Text, Hardware); without topics, words in the module's name and summary, then its file's name.
- **Start values:** BOSL2's first doc example that is one plain call of the module (simple `t1 = 16;` assignments filled in; each plain call in an example is a preset), else `catalog/components.json` (written start values for threads-scad, MCAD and Round-Anything modules), else guesses from argument names (radius 10, height 10, length 20…) when they cover every argument without a default, except modules listed in `$no_guess` there, which wait for values. Components that would only fail start with those settings empty and marked, and don't render until they're set.
- **Settings:** a literal default or example value gives a number, checkbox, text or vector field; a missing (undef) default gives an optional field left empty; anything else an expression field, passed as written (checked: one expression, no statements, no `import`/`include`). `anchor` and `orient` (BOSL2) are choices; NopSCADlib `type` arguments are choices of the type constants in the library's matching list file (`vitamins/screws.scad` for `screw()`). 2D shapes are extruded (an "Extrude to" setting; a component that turns out to be 2D is extruded 1 mm, and a 3D one the docs drew flat isn't). Detail: `$fa` 6 and `$fs` 0.5 by default, `$fn` optional.
- **Rendering:** the page sends the component's call (includes, module, which settings are expressions) with the values; the app writes the file for that render and runs it beside the library's laid-out files (one tree per include set, cached). The result cache key includes the call.
- **Results** (`workshop-cli components-check`, in CI on Linux): BOSL2 201 components: all 146 whose first doc example is a plain call render it; 21 more start from a later example and render; 2 render at their defaults; 32 wait for values (no plain-call example). MCAD 146 (75 render at their start values, 71 wait), NopSCADlib 429 (167 render, 262 wait: many need a part type the library doesn't list), Round-Anything 8 (7, 1), threads-scad 12 (11, 1). About 800 in all.
- **Search:** components join the index (kind "component", the library as their project, topics as tags). Typing a module's whole name ("bevel gear") puts that module first (exact name +4, exact model name +3), so BOSL2 `bevel_gear()` leads even with a project model called "Bevel gear". The bevel-gear flow (search, open, change teeth and module, preview, download) takes about 5 s in the acceptance test.
- **Your copy or the app's:** `components::effective` picks, per library name, your library project unless `prefer_bundled` in library.json names it. Includes in your projects resolve the same way (your library projects first, then the app's copies; a preferred bundled copy first). Each project's derived data records which copy it used (`libraries_used`); when that changes (the switch, or a new app version with a different bundled commit) the project is listed under Needs attention and "Look for changes" (and the switch itself) reads it again.
- **Pins:** `sources/pinned/` (kind "pinned", "Pinned components"); its models are made when the catalog is, from the component in use, so a pin follows a library update or switch. Pinning keeps the values as `defaults`, the unshown settings as `hidden` with their values in `fixed`.
- **Form edits:** `form` on an item in metadata.json, applied by `ingest::apply_form` (the website build uses the same function for manifests; it now also takes `step`, `group` and `placeholder`). A Rust test checks that an edit gives the same settings and sections as the same block in a manifest; the acceptance test checks the stored block after editing in the app.
- **Thumbnails:** a component's first render becomes its thumbnail (`thumbs/components/<library>/<module>.webp` in the library); Library settings → OpenSCAD libraries makes the rest.
- **Category icons** can also be an image file (Library settings → Categories → Image…), copied into the library's `icons/` folder.
- **Testing:** the acceptance test adds the Components menu, the bevel-gear flow under a minute, Copy code, pinning, a form edit, switching between your BOSL2 and the app's, and a category image; the app test opens a BOSL2 and a threads-scad component on Linux and Windows; CI fails when a BOSL2 first-example form doesn't render, and reports the other libraries.

**0.3.1** (owner's requests after using 0.3.0):

- **Left menu:** the sections are rows of their own (Parametric Models, Components, Parts Library, replacing the "All …" rows and the section headings). The section being browsed shows its categories, each opening to its projects, in a panel attached to the right of the menu that scrolls on its own; « folds it to a strip with the section's name, » brings it back (remembered with the other layout preferences). The panel shows only while browsing a section (not on Home, Recent, a model page or the settings pages). In the small-window drawer there's no room beside it, so each section opens to its categories in place.
- **Choices from the docs:** a text setting whose doc lists its values becomes a drop-down (`components::doc_choices`): a run of two or more quoted values joined by commas, "or", "and" (or each followed by what it means, `"ltr" for left to right.`) that the sentence introduces ("One of", "Can be", "Select", "either", "between", or the start of a sentence). Not examples (`e.g.`, `such as`), not defaults (`Default is "2B" for UTS…`), and not when the list is left open (`…, or a 3D point`; "Alternatively", "You can also"). A setting that points to another module's or function's argument (`style = vnf_vertex_array() style to use`) takes that one's list. The signature's default and the examples' values join the list; with no default the first choice is "Default (…)". Only when the signature and examples are text or absent. BOSL2: 85 settings in 62 components (atype, style, texture, shape, method, direction, halign/valign…). Stored in the index as `listed` (index format 2; a library project's stored index from 0.3.0 is made again in memory until it's read again).
- **Library settings:** Edit… on each project's row opens its details (as the project page's Edit details).
- **Long text stays in its box:** dialogs and their fields are one column no wider than the dialog (a long folder path pushed the Add a project dialog's contents past its edge); a chosen file or folder path wraps; the details editor's one-line fields (name, tags, authors, origin, a detail's value) wrap and grow to show all their text (Enter still saves). Category and License keep their suggestion lists as single-line fields.


## 0.4 notes

Phase 4, first part (0.4.0): the side viewer and reference documents, as described in "Reference documents and the side viewer" (section 5). Where it went beyond or differs:

- **Conversion** is pulldown-cmark (a new dependency) plus a small cleaner in `desktop/core/src/docs.rs` (regular expressions over tags and attributes; one pattern per removed element). Pictures get `loading="lazy"` (NopSCADlib's README has about 500). The page makes relative pictures and links absolute before showing the HTML: the frame's own `<base>` came too late for Chromium's preloader, which fetched pictures from the page's address first.
- **The frame** is `sandbox="allow-same-origin"` without `allow-scripts`: nothing in the README runs, and the page (same origin) styles it in the app's colours, follows the theme, and handles its links: `#anchors` scroll (wired as soon as the document exists, since "load" waits for every picture), the project's `.scad` files open in Code, other project files open on GitHub at the version in use (or in the system), web links in the browser.
- **Starter projects** keep their version id (it hashes the models, not the documents), so a library made by 0.3 gets the documents copied in on the next start rather than as an update to accept.
- **Code tab:** a model's files come from its content hashes (`blob` in the app, `fs/<sha>` on the website), so every file a model uses is listed (entry first). Find jumps between matching lines (Enter, Shift+Enter).
- **PDFs:** WebView2 (Windows) shows them; WebKitGTK (Linux) has no PDF viewer (`navigator.pdfViewerEnabled` is false), so the panel offers "Open in your PDF viewer". A PDF.js viewer could close that gap later.
- **Settings from module calls** (a bug report while 0.4.0 was being released): a model file that is only includes and calls with the values typed in (chrisspen/gears' examples) gave OpenSCAD's Customizer nothing to list. Reading a project now makes a copy with the literal arguments lifted into settings after the includes (`components::lift_call_values`), kept in `sources/<id>/generated/<version>/` and rendered in place of the original (same path, so its includes resolve the same); the original stays untouched and the copy says where it came from. Descriptions come from the library's comment for each argument (`name = description`, the Gears style). Files with settings of their own, definitions or blocks are left alone (blocks could come later). Project data format 2: older projects are read again at start-up.
- **Borrowed settings** (0.4.1, asked for after LEGO.scad's examples came up with bare numbers and `height=1/3` fixed): when a project file (one the model includes first, then the project's other model files) calls the module with `arg=variable`, and that variable is a top-level Customizer setting with a comment or a `// [...]` note, the lifted setting gets the comment as its description and the note as its drop-down or slider (`components::borrowed_args`). An expression of one such variable (`reinforcement=(use_reinforcement=="yes")`) lends its description only, since the types differ; so does a variable whose value is another type, and a slider the value is outside of. Arithmetic of numbers (`1/3`, `2+(2/3)`) is lifted as a number: the library's choice when it's one (`.33333333333`, labelled "1/3"), else one more choice labelled as written (`2/3`), or a plain number when there's no list. A setting is renamed after its module only when the file `include`s a library with a variable of that name (`use` doesn't bring variables in), so LEGO's examples show "Wing type", not "Block wing type". Project data format 3; only projects read at format 2 that have such models are read again. The module's own comment (the Gears style) still comes first.
- **Editing the code** (0.4.1), as designed above. The editor is a transparent textarea over the highlighted lines (they scroll and wrap with it; bold and italic tokens are plain while editing so the widths match), not a code-editor library. Edits live on the page per open model (closing the tab drops unsaved ones; a form change from the form editor keeps them). Choices made along the way: a saved record without edits means the original code (it was saved against it); Reset all resets values only (the Code tab has Revert); a shared link opens the original code; choosing Defaults or other saved settings asks before dropping unsaved edits; the status and a note above the form say when the code is edited. Batch renders use the edits too.
- **Links in documents** (0.4.1, owner's report): a link in a PDF (or one clicked in a README before its links were handled) could navigate the app's window away, with no way back. The window is now created in `setup` (`"create": false`) with `on_navigation` (the app's own schemes and `*.localhost` load; the rest opens in the browser) and `on_new_window` (always denied; web links go to the browser). A frame that still ends up on a web page shows "Back to <document>".
- **Linked projects' pictures** (fixed in 0.4.1): a linked project's folder is outside the library, so its README pictures and its Files tab had nothing to point at; library paths `~linked/<id>/<path>` now read from the linked folder.
- **Testing:** the acceptance test covers a GitHub project's code and README (with a picture from its files), a component's code at its module, the app's BOSL2 README, a starter project's README and a project file; the website test covers Code and README; the app test opens the viewer in the real window on Linux and Windows.

## 0.5 notes

### Triage: Birdhouse Workshop shows only Castle and Engine (owner's report, 2026-10-12)

Reproduced with the project folder the owner sent (`sources/zip-birdhouse-workshop-main`) and the app's own reader (`scan::survey`): 5 `.scad` files, 2 models (castle, engine), 3 counted as library code (square, corner, polygon).

- **Cause.** The project is one shared engine and four front ends. Square, corner and polygon are only Customizer settings (205 each) and one `include <birdhouse_engine_v2.6.3.scad>` at the end; the engine draws the model with its own top-level `if (engine_autorun_effective) { … }`, which OpenSCAD runs inside the including file. The reader decides "model or library" from each file's own top level (`scan::shape(...).geometry`, `survey()` in `scan.rs`), and it doesn't follow includes, so the three front ends look like library code and get no page. Castle shows because it sets `engine_autorun = false` and draws with its own calls.
- **Second half of the same cause.** The engine is listed as a model because its `if` block counts as geometry. It can't render on its own: `design_preset` and `body_style` are set only by the front ends, so its own `assert(design_preset == …, "Unknown design_preset")` stops it. So one design that works is missing and one that doesn't work is shown in its place.
- **Fix (made in 0.4.2).** `survey()` now reads every `.scad` file first (`ScadFile`: its shape, the project files it `include`s, the names it assigns and the names it mentions) and decides in `models_among()`. A file that draws at top level is a model. A file of settings only (no geometry, no modules of its own) is a model when it includes, directly or through other includes, a project file that draws **and** sets a name that file mentions but never assigns (itself or through its own includes); names that appear only inside `is_undef(...)` don't count. Every file such a front end includes is library code. The extra condition came from checking the first version against real projects: GridFlock's `docs/*.scad` animations have Birdhouse's shape (settings, then `include <../gridflock.scad>`) but only override settings `gridflock.scad` already has, so it must stay a model; Gridfinity Kitchen's `module_gridfinity_Extendable.scad` reaches `ub.scad` but sets nothing it reads; BOSL2's `std.scad` sets the guard flag `_BOSL2_STD`, which `vnf.scad` checks with `is_undef`. With it, across the 19 starter projects, Gears, LEGO.scad, BOSL, BOSL2, NopSCADlib, MCAD, gridfinity_openscad and threads-scad, only Birdhouse's models change. Project data format 4: at start-up `source_rescan` surveys each project read before it (`project::models_differ`, no OpenSCAD) and reads again only those whose models change; the rest are stamped as format 4.
- **Also found: the category.** Birdhouse was filed under Labels. `guess_category` matched keywords anywhere in the text, and "sign" (a Labels keyword) is inside "design" in its README. Fixed in 0.4.2: keywords match whole words with the usual endings (s, es, ed, ing); no starter project's category changes.
- **Also in the folder sent: moved files.** The app read the ZIP with the `.scad` files in `src/` (as its README says, and as the project's derived data records), but in the folder the owner sent they are at the top level and `src/` is empty; `src/` was changed about seven minutes after the project was added, so they look moved by hand. A project's stored copy isn't checked for changes (request 5), so Castle and Engine still point at `src/` and may not render, and the README's `src/` links lead nowhere. Moving them back into `src/`, or pressing Read again, fixes that part; the fix above is still needed for the three missing designs.
- **Tests (added in 0.4.2).** A Rust test with a small engine-and-front-ends project (an engine with `if (autorun) cube(size);` and two front ends that set `size` and include it, plus one that sets `autorun = false` and calls a module) expecting the three front ends as models and the engine as library code; and a `guess_category` test for "design" and "showcase".

## Review 1 (of 0.4.3, 2026-10-14)

The first periodic review: https://claude.ai/artifact/RzgZVDPVXHLBpnAXgaWYAp (11 interface items, 6 on the library's file structure, 3 on default values), from CI's screenshots of 0.4.3, the acceptance test's library (`p2/library.zip` on `desktop-ci-linux`) and its render reports. Unless the owner leaves items out, all are taken: UI-6 and F-6 in milestone 1 (0.4.4), the interface items in milestone 2 with the skins, the file structure and default items in milestone 3. The owner replaced UI-1's recommendation: Needs attention is removed and license status shown in Library settings (0.4.4). F-1 (starter projects can't check their source for updates) mostly falls away with the starter library; libraries that have the starter projects keep them as `bundled`, and milestone 3 decides whether to turn them into GitHub projects.

Milestone 2 (0.4.5) took the six interface items with the skins: UI-2 (one bar over the 3D view: size left, view buttons right, wrapping), UI-3 (one head for listing pages: name, count and view buttons, then Sort, Group, Condense and size, then filters; a project's page has one main button and ⋯), UI-7 (tabs at least 170 px, project name dropped first when crowded, the rest in "+N ▾"), UI-8 (messages above the status bar at the right), UI-10 (long drop-downs take the whole row) and UI-11 (the same head on model and component pages, the rarer actions under ⋯). Found while checking the skins, for milestone 3's default checks: some starter models open with number boxes marked invalid at their defaults (Gridfinity Anylid's width and length units, 1.5 and 2.5), so the browser's own range or step check disagrees with the model's.

## Milestone 3 notes

**0.4.6, defaults that work.** Built as designed, with what reading showed:

- OpenSCAD's settings export already reconciles two of D-3's cases itself: a default above its slider's maximum widens the maximum, and a default that isn't one of its choices is added as the first choice. So from a project's own files only the step can disagree (`height = 2.5; // [0:1:10]`); the other checks matter for settings overlays (a family manifest's `ui`, an edited form). The step of a number with no Customizer comment comes from its default (`ingest::decimal_step`), and the form also falls back to any step or a wider range rather than call a default invalid (`fitStep` in `web/app.js`).
- `limits` problems carry the settings' messages in `settings` beside `message`. Projects read before 0.4.6 get them when next read; nothing forces a read.
- The render check rides on thumbnails (`makeThumbnails` in `web/ui/library.js`), so it uses whichever engine the app uses for that model, and records through `defaults_put`. A request to make thumbnails while they're being made now goes round again afterwards, so a project added meanwhile isn't left until the next start. The catalog only shows checks for the project's current version (`checks.json`'s `version`) and marks ones from another engine `stale`, which are tried again.
- D-2: NopSCADlib's types files end with their own list (`fans = [fan17x8, …];`), which is now used for the choices, and plural files ending in -es and -ies are found (`batteries.scad`). Components that open with settings to fill in: 342 → 326 of 796. Most of the rest are NopSCADlib's printed parts (boxes, shrouds, hinges) whose type is a property list built by a function, and MCAD modules with no defaults; their own examples (`tests/*.scad` in NopSCADlib) would be the next source of start values.
- UI-4 and UI-5: component settings no longer carry their code names as labels (the form names them as it names model settings; labels equal to the code name, from projects read earlier, are ignored). The model page's title is the model's own name, with its project (a link to the project's page) or library on the line under it.

**0.4.7, one project description.** Built as designed, with these choices:

- The project's own `workshop.json` is read from the top of its files only (not subfolders), and its `models` list, when given, is the whole list: a project that describes itself isn't also guessed at. Entrypoints are relative to the project's top folder (a family manifest's are relative to this repository). Model keys other than `id`, `entrypoint` and `status` are passed through to the model as a family manifest's are, so `ui`, `hidden`, `fixed`, `defaults`, `extra_params`, `presets`, `notes`, `part_parameter` and `browser` all work. A `license` can be a plain SPDX id.
- `sources/<id>/workshop.json` is written from what reading found (`project::describe`); edits stay in `metadata.json` beside it. Save as project file (`workshop_save`) is offered only for `local/` and linked projects, whose folders are yours: for a GitHub, ZIP or copied project it would change upstream's files, which must stay as they came for update checks. It merges the catalog's resolved project details and each model's name and form edits (a form's `ui` merged per setting over the model's own).
- F-3's "offer to open that one" is a toast action on the error ("Open it"), parsed from the message's `(id)`.
- F-4 (one reader writing the same fields) needs nothing more for projects added since 0.4.4; old libraries' starter projects get it with F-1 (0.4.9), when they become GitHub projects read like any other.

## Review 2 (of 0.4.5, 2026-10-14)

The second periodic review: https://claude.ai/artifact/5T6K5P1TFV7eUMguCv6CGD (8 interface items, UI-12 to UI-19; 6 on the library's file structure, F-7 to F-12), from CI's screenshots of 0.4.5 (every skin in light and dark, the Windows and Linux apps), the acceptance test's library and the code. Unless the owner leaves items out, the small ones ship in 0.4.8 and UI-16 (one head for model, component and part pages), F-9 (pins record their library's copy and version) and F-11 (a project added at a tag or commit follows releases, not main) in 0.4.9 with F-1. The worst finding was mine: the extra font rules for "·" have no weight range, so on Windows and in Chromium regular text in Drafting Sheet falls back to the system font (UI-13). The test library has no ZIP or linked project; the acceptance test adds one of each so Review 3 can compare them (done in 0.4.8, section 13d).


### 0.4.8 notes (Review 2's small items)

Built as listed in the review, with these choices:

- **UI-12:** the message takes the status bar's place (bottom left, its height) instead of floating over the 3D view.
- **UI-15:** the side viewer sets `--side-over` (its panel's width) on the page; the size bar and the colours are inset by it. The size bar is a container (`container: hud`): under 360 px the size drops its triangle count and the view buttons fold under a ⋯ button. The 3D view itself still centres on the whole stage; framing the model in the uncovered part is left for later.
- **UI-17:** Settings is still built in `app.js`, but from the same `.ls-section` blocks and `.ls-toc` jump links as Library settings (`section()`, `settingsToc()`).
- **F-7:** an error keeps the update record (`error` and `error_date` added beside it); a check that finds nothing new writes nothing. When each computer last checked stays in its own preferences (`gw-update-check`), as before.
- **F-8:** the catalogue marks a model `check.outdated` when `checks.json` is for another version of its project; `makeThumbnails` renders those again (their thumbnails and checks). An engine change alone doesn't: a model that failed is tried again, the others keep their pictures.
- **F-10:** a model whose entry file was lifted into a copy has `file_bases` (the project file's hash) in its details; a recipe's edit stores that as `base`, the copy's hash as `copy`, and the recipe stores `version`. An edit saved before (based on the copy) isn't reported as changed. Derived format 5 reads such projects again, once, at the start-up scan (`scan_changes` now reads any project `outdated` says needs it).
- **F-12:** the README gained the lines the review asked for; the texts written by 0.4.4–0.4.7 are kept in `library.rs` so an unchanged one is replaced.

### 0.4.9 notes (starter projects as GitHub projects; UI-16, F-9, F-11)

- **F-1, which projects:** only 6 of the 19 starter projects are upstream's files exactly as GitHub's download gives them: Gridfinity Rebuilt, Gridfinity Extended, Gridfinity Basket, Cullenect Label, Label Generator for Gridfinity and Underware. HSW's "Grid" model is the original v2 recovered from an older commit (not in the tree at the pinned commit); GridFlock and Rugged Box need git submodules (and GridFlock a generated `paths/puzzle.scad`) that the tarball leaves out; openGrid's snap is the app's adapter; the rest were supplied by the owner. Those 13 stay `bundled` (origin type `app`). The list is `STARTER_ON_GITHUB` in `sources.rs`, which embeds their family manifests from `catalog/families/`.
- **F-1, how:** `starter_convert { ids? }` downloads each at the manifest's commit into `files/<commit12>`, writes the app's description (`description.json`: the manifest with entry points rebased past `vendor_path`, model ids, names, forms and presets kept) and makes `source.json` a GitHub project following the default branch (`origin.converted` records the old version). It reads the project and compares model ids with the old version's; on a read error or a missing model everything is put back. Saved settings' code edits are re-keyed (`/vendor/<x>/a.scad` becomes `/a.scad`). The old `bundle-…` version stays in `versions` and on disk until Remove versions not in use.
- **The app's description:** ingest reads the project's own `workshop.json`, else `sources/<id>/description.json`; when none of the files that description lists exist (upstream reorganised), it's ignored and the project is read as if there were none. Adding a repository the app knows from GitHub writes the same description (`known_on_github`). When a project is described either way, the catalogue takes its name, summary, license, authors, tags and category from the description rather than from what was guessed when it was added (the owner's edits still win).
- **Suggested projects:** `catalog/suggested.json`, embedded in the core and served as `catalog.suggested` with `added` (the project already added from that repository, or a starter project that is). Home shows them while the library has no parametric models; Add project shows those not added as chips that fill in the link. openGrid and Underware wait for their licenses. Birdhouse, named in the plan, isn't in the list: no repository was settled for it.
- **UI-16:** the model, component and part pages share one credit line (`renderModelHeader`), and the page's own actions moved to a ⋯ menu (`#model-more`) beside the search; the notes open from About in that menu.
- **F-9:** a component library's copy is `<who>@<commit12>` (`app`, the project's id, or `project`); pins store `from: { library, copy }`, and a changed copy gives the pin a problem of kind `pin_library` with the settings that are gone; `pin_accept` takes the new copy. Older component thumbnails are moved into the copy's folder the first time.
- **F-11:** `source_add` takes `follow: "releases" | "branch"` when the link names a tag, release or commit. Release tracking takes GitHub's latest release, or the highest version-numbered tag when there are no releases, and never offers an older release than the version in use.

## Review 3 (of 0.4.9, 2026-10-14)

The third periodic review, at the end of milestone 3: https://claude.ai/artifact/NU5qAAphTyG7ioqDRnZT4S (8 interface items, UI-20 to UI-27; 6 on the library's file structure, F-13 to F-18), from CI's screenshots of 0.4.9 (the Linux acceptance test; the Linux and Windows apps), a test library holding every kind of project, and the code. Projects added since 0.4.7 are stored alike; the gap is older projects, which never get milestone 3's checks (F-13). Unless the owner leaves items out, everything but F-15 ships in 0.4.10, before milestone 4; F-15 (a folder of example files becoming models) goes with milestone 4, since deciding which files are models is close to making parts from a generator.

### 0.4.10 plan (Review 3's items)

- **UI-20 (framing):** the camera fits the model's bounding sphere to the narrower of the vertical and horizontal fields of view, and offsets the view (three.js `setViewOffset`) by the side viewer's width so the model is centred in the part of the stage you can see. It fits again when the stage resizes or the side viewer opens or closes, unless the camera was moved by hand since the last fit.
- **UI-21 (side tabs):** the vertical writing moves to a `<span>` inside each rail button; `desktop_ui.py` checks each tab is taller than it is wide.
- **UI-22 (Share):** in the app, Share becomes **Copy settings**: the changed settings as an OpenSCAD Customizer parameter set (JSON), which "Import OpenSCAD file…" reads back. Save's tooltip says "in the library".
- **UI-23 (suggestions):** `catalog/suggested.json` carries each suggestion's models count, license (with "personal use only" for NC licenses) and the name it will have; Home and the Add project chips show them.
- **UI-24 (status messages):** the message is drawn in the status bar's left part in the bar's own font and colours, with the counts hidden while it shows; jobs read "label · n of m · 12 s", and a trailing "…" is stripped from job labels.
- **UI-25 (credits):** "License <name>" or "License not stated", always a link, plus "· Check" when the status isn't OK; a component's module and copy move to the line under its name; BOSL2's authors come from `libraries.json` whichever copy; part pages get the README button and ⋯; the presets ⋯ is labelled "Saved settings".
- **UI-26 (notices):** every notice that applies shows, stacked; red only for broken or failed, the neutral note style for a pin's library change and for defaults outside their limits (shown on the model page too); the pin notice names copies and dates, with a link to Library settings › Libraries.
- **UI-27 (words):** "From an earlier app", "(version in use)", "Updates: changes on its <branch> branch" / "Updates: new releases", the update banner quotes the commit message, Edit the form uses the form's labels and says "the Defaults list", `KIND_TEXT` gains pinned components.
- **F-13 (checks for older projects):** the start-up scan reads again every project with derived data older than the current format or with no `workshop.json`, bundled ones included. `makeThumbnails`'s check pass also takes models with no check for their version (keeping their existing thumbnails), bundled included, with progress in the status bar. Conversion marks the converted projects for it.
- **F-14 (description as an overlay):** `description.json` names, forms and presets the files it lists, and reading also lists other model files it finds; those appear in the update summary as new models. `description.json` records `app_version` and its manifest's version, and is rewritten when the app's is newer. `workshop.json` carries every model key the description has.
- **F-16 (old copies):** conversion records the old version's commit and message ("The app's copy of <commit>, before it came from GitHub"), and removes the old copy when its file fingerprints match the new version's.
- **F-17 (bundled projects on GitHub):** the four that can't be converted record `origin.repository` and `origin.commit`; the update check looks at them read-only and says a newer version is on GitHub, with "add it from GitHub to follow it".
- **F-18 (README):** indentation kept (no `\` continuations), the seven origin kinds named, linked projects and `description.json` explained; the 0.4.8–0.4.9 text is kept so an unchanged README is replaced.
- **Also seen:** Layer Line's slashed zero is turned off in the top bar and status line; update-check errors show a short message with the detail on hover.

Built as planned, except:

- **F-13:** checking the unchecked models is an offer in Library settings › Projects (**Check them**), not started on its own: in a library from before 0.4.6 it's every model, which can take many minutes and would hold up everything else in the background. The start-up part (workshop.json, limits) needs no OpenSCAD and runs on its own. Older derived data had plain numbers at step 1 (before D-3's rule), so those steps are fitted first; otherwise a default of 1.5 was reported as off its step.
- **F-14:** files neither listed nor left out become models straight away (as reading any project does), rather than being offered with "Add as a model"; the update summary lists them as new models. What the 6 descriptions leave out is in `LEFT_OUT` in `sources.rs`, checked against `vendor/` by a core test.
- **UI-25:** the presets row's ⋯ keeps its mark, with "Saved settings: rename, delete, export, import" as its name and tooltip (a text label didn't fit beside Copy settings in a narrow form). Part pages have no ⋯: nothing to put in it yet.
- **F-17:** the 6 starter projects that can come from GitHub record their repository and commit too, until they're converted.

## Make parts from a generator design (milestone 4, 0.4.11 onwards)

Roadmap item 8, plus Review 3's F-15. Generating ends in a download today; the owner wants what's made kept as ready-made parts in the Parts Library, one at a time or many, each able to be made again.

**Where made parts live: a local project.** `local/Made parts/` is an ordinary local project (milestone 3 already reads local folders, makes a part of every model file, keeps versions and checks), so made parts move, sync and back up with the library, and need no new kind of project. Inside it, one folder per model (`local/Made parts/<model name>/`) holds each part's files (`<part name>.stl`, and `.3mf` once Phase 4's 3MF lands) and beside each one `<part name>.recipe.json`:

```json
{ "made_by": "SCAD Workshop 0.4.11", "model": "gridfinity-rebuilt/bin", "project": "gridfinity-rebuilt",
  "project_version": "71525de98144", "model_name": "Basic Bin",
  "values": { "gridx": 3, "gridy": 2 }, "edits": null, "engine": "2026.10.02", "date": "2026-10-15T09:12:00Z" }
```

Only the changed values are stored (as saved settings do), with the code edits when there are any. Reading a local project picks up `*.recipe.json` beside a part: the part's item carries `made_from` (model, name, values, version), and its category defaults to the model's.

- **Save as part** beside Download STL on a model page: asks for a name (default: the model's name and its changed settings, "Basic Bin 3×2") and a category (default: the model's), renders if the preview isn't current, writes the files, and opens nothing: a message says "Saved Basic Bin 3×2 to the Parts Library · Show". The part's picture is made like any part's.
- **Make several…** is the Batch dialog with a second way out: **Save to the Parts Library** next to **Download ZIP**, naming each part from the settings that vary ("Basic Bin 2×1", "Basic Bin 3×1", …). It also offers your saved settings as rows (tick the ones to make). Phase 5's CSV and sweeps fill the same table.
- **A made part's page** says "Made from Basic Bin (Gridfinity Rebuilt), 2 settings changed", with **Open with these settings** (the model page, like a saved setting) and **Make again** (renders with the same values and replaces the files, keeping the name). When the model's project has a newer version than the recipe's, the page says so: "Made with the version of Oct 2; Gridfinity Rebuilt has been updated since. Make again to use it."
- **Tests:** Save as part writes the files and recipe and the part appears under Parts Library in its category; Make several saves three; Make again after a project update replaces the file and the recipe's version.

**F-15: a folder of example files.** When reading a project finds one folder with more than 20 files that draw something, each of which only includes or uses the same main file and calls one of its modules (LEGO.scad's `examples/`), the project page asks once: **List them as models** (as today), **Make them parts** (each is rendered once at its own values into the project's parts, through the same queue, so they're browsable without 180 forms), or **Leave them out**. The answer is kept in the project's `metadata.json` (`examples: { "folder": "examples", "as": "parts" }`) so it holds across updates, and `catalog/suggested.json` can carry it for LEGO.scad. Gears' `examples/` (18 files with settings of their own) isn't asked about: the rule needs files that are only calls.

**Order:** Save as part and made parts' pages (recipe, Open with these settings, Make again); Make several (Batch to the Parts Library, saved settings as rows); F-15. Review 4 follows. Released as 0.4.11 (Save as part and Make several: the first's CI run was cancelled when the second was pushed before it finished) and 0.4.12 (F-15).

### 0.4.11 notes: Save as part

Save as part and made parts' pages, built as planned, except:

- **Make again** opens the model's page with the recipe's settings and code, makes it there (so its progress and any error show where they always do) and then replaces the part's files and recipe, rather than making it on the part's page. **Open with these settings** is the same without the saving. Both are links (`#/m/<model>?part=<pack>/<part>`, plus `&again`), so the inspector can offer the first too.
- The dialog's category picker lists the categories there are; a new one is made in Edit details, as for any part. A component's part has no category of its own (its topic group isn't a library category), so it goes under the Made parts project's.
- **A made part carries its project's license** (the edited one when you've changed it), since what's made from a project is under its license.
- **Your own projects' part files** (in `local/` or linked) were looked for in a stored copy that only added projects have, so their previews, thumbnails and downloads failed. Their files are now read where the project is (`library/local/<name>/…`, `library/~linked/<id>/…`), and the backend's `read` serves `library/` paths the same way as the `library://` protocol.
- The recipe keeps the code edits as saved settings do (`{ path: { text, base } }`), so an edited part opens with the same code.

### 0.4.11 notes: Make several

Make several, built as planned, except:

- **F-15 goes after it** (0.4.12), so Make several doesn't wait on it: example folders made into parts need their own place for the made files in an added project (whose stored copy never changes) and a queue that survives closing the app, which is more than a dialog.
- The dialog chooses **either** combinations of settings **or** saved settings ("Vary settings" or "Your saved settings", shown when the model has saved settings), rather than mixing them as rows. Saved settings with code edits aren't offered: the parts are made with the form's code.
- Parts are named by the model, its sizes and whatever else varies (a choice by its label, a yes/no by its name), or by the saved setting's name ("Basic Bin Drawer wide"). Two with the same name get " 2" from the backend, as Save as part does.
- Made parts are filed under the model's category, with no dialog to ask: Edit details changes it afterwards.

### 0.4.12 notes

F-15, a folder of examples, built as planned, except:

- **What counts:** one folder with more than 20 models, each including or using exactly one file outside the folder (the same one for all), with no modules, functions or settings of its own (`$` variables allowed). `survey` records it as `examples: { folder, main, count }`; reading adds `as` from `metadata.json` ("" until asked).
- **Made into parts:** the examples stay models in the catalog's model table (so their pages, renders and checks work as before) but carry `as_part` and aren't listed under Parametric Models. Each becomes a part of the project's pack once its STL is in `sources/<id>/examples/<version>/<model>.stl` (rebuildable; Remove versions removes it with its version, and a local project keeps only the current one). Until then it's in the catalog's `examples_todo`, which the background queue (the thumbnails' queue) works through before thumbnails.
- `catalog/suggested.json` doesn't choose for LEGO.scad: the project's page asks, like any other. Choosing parts there would make 180 parts in the background straight after adding it, which should be the user's call.
- **Found by CI on the way:** reading a project left out any model whose settings OpenSCAD failed to read, and under load (renders from the page alongside) that happened now and then: Gridfinity Extended's move to GitHub (F-1) lost a model in 3 of 6 runs, a different one at times. A failed settings read is now tried once more on its own before the model is left out, each read has its own working folder, and a move that still loses a model says what reading said about it. Separately, a library reload drew the previous model's header over the one just opened (pinning a component did it), closing the ⋯ menu.

## Review 4 (of 0.4.12, 2026-10-14)

The fourth periodic review, at the end of milestone 4: https://claude.ai/artifact/RAhYcNBQ9WnFJowHZVfunC (5 interface items, UI-28 to UI-32; 4 on the library's file structure, F-19 to F-22), from CI's screenshots of 0.4.12 (the Linux and Windows apps; 64 of 64 acceptance checks), local screenshots of milestone 4's screens (CI doesn't take any yet) and the code. Making parts works end to end; the rough edges are how made parts are filed and shown, and when things in your own projects count as changed. Unless the owner leaves items out, all of them ship in 0.4.13, before milestone 5.

### 0.4.13 plan (Review 4's items)

- **UI-28 (Made parts shelf):** the Made parts pack shows no source line, Read again or Versions; each card's subtitle is its model and project from the recipe; the pack's credit says each part keeps the license of the project it was made from.
- **UI-29 (example parts):** the link reads "Open the example as a model" (plus the main file's model when it has settings); part lists on part pages sort by number, as the grid does; the project page's examples note uses the information style.
- **UI-30 (tabs):** a tab whose name another open tab shares shows its project on every tab, not only the active one.
- **UI-31 (your own licenses):** `local/` and linked projects with no license read "Yours" in Library settings › Projects, with no Check; Made parts follows each part's project.
- **UI-32 (status bar):** the space before a job's timer is kept; the real-app test reads the status line.
- **F-19 (made parts folders):** when another model in the library has the same name, the folder is "<model> (<project>)"; existing folders stay as they are; Make again keeps using the part's own folder.
- **F-20 (what counts as changed):** recipes and example parts record the fingerprint of the files their model uses (the render cache's file map), and "updated since" and remaking compare that rather than the project's version; example parts are kept by that fingerprint so only those whose files changed are made again.
- **F-21 (examples asked when added):** Add project's result asks about a folder of examples when reading finds one; until it's answered the examples aren't listed under Parametric Models or checked at their defaults.
- **F-22 (README):** `local/Made parts/<model>/` and the `<name>.recipe.json` beside each part are described (not rebuildable; Make again and Open with these settings read it).
- **CI:** the acceptance test takes screenshots of Save as part, a made part's page, Make several and an example part, for Review 5.

### 0.4.13 notes

Review 4's nine items, built as planned, except:

- **UI-31:** a license set for the whole library (Library settings) still applies to your own projects; "Yours" is only for no license at all. Your own projects no longer count among the projects that need a license check. The project page's own line still says "License not stated", which is true.
- **F-20:** the fingerprint is of the model's file map (each file's path and SHA-256), which is what the render cache keys on; components don't have one, so recipes made from components (and recipes from before 0.4.13) still compare the project's version. Each example made into a part has a folder of its own under `examples/` (named by its fingerprint) because each example's own file is in its map. Remove versions no longer touches `examples/`: an example's earlier part is removed when it's made again.
- **F-21:** Add project's message names the folder and asks you to choose on the project's page (it opens there), rather than a second dialog. The examples' model pages still open.
- **CI:** a fifth screenshot, the Made parts shelf.

## Ready-made models and collections design (milestone 5, 0.4.14 onwards)

The rest of Phase 4 ("4. Model library" and "Files and formats" above), in five releases, each followed by its checks in the app test. Review 5 follows 0.4.16 (everything about bringing ready-made files in); Review 6 follows 0.4.18. Model Library's code is reused where it fits (its `desktop/core/src/mesh.rs`, `thumb.rs`, `dupes.rs` and `web/ui/drop.js`, read from the owner's copy); its 7z/RAR reading and folder watcher need crates this build doesn't have offline, so they wait.

**0.4.14: ready-made parts look and come in like generated ones.**
- **Previews from any format.** Model Library's `mesh.rs` (STL, OBJ and 3MF, with a 3MF's colours from its colour groups, materials and the slicer's extruder settings) is ported as is; it needs only crates the app already has. A part's 3D view and thumbnail use 3MF, then STL, then OBJ: the core reads the file and hands the viewer an STL (with per-triangle colours when the 3MF has them, drawn only when you turn on "Show the file's colours"). Today a part that comes only as 3MF or OBJ has no 3D view. A 3MF's own picture (the one the slicer saved) is its thumbnail when it has one. STEP keeps "No 3D preview" until OpenCascade's license is checked.
- **Dropping files on the window**, as Model Library's `drop.js` does (Tauri's drag-drop events): `.scad` files, model files, folders and ZIPs. A drop offers **Add as a project** (copied into the library, like Add project › Folder or ZIP) and, for files of your own, **Add to a project of yours** (copied into a `local/` folder you pick). A hint shows while dragging.
- **Already in the library:** each model file is checked by size and SHA-256 (Model Library's `dupes.rs` quick fingerprint, then the full hash) against the library's parts; the add's message lists what was already there ("3 of 12 files are already in Gridfinity Rebuilt") and adds them anyway, so nothing is silently dropped.
- **Tests:** a folder with `latch.stl`, `latch.3mf` (two colours), `knob.obj` and `case.step` added as a project gives 3 parts; the 3MF part's 3D view shows, and its colours when asked; the OBJ part's view shows; the drop is driven by a shim event (Model Library's `__shimEmit`); a duplicate is named in the message.

**0.4.15: one download with a format picker.**
- A part with several formats gets one **Download** button with the format beside it (STL · 3MF · STEP); the default is Settings › Downloads › **Preferred format** ("3MF where there is one, otherwise STL", "STL", "3MF", "STEP where there is one") and the last choice for that part is remembered. Generators get the same picker for their output (STL or 3MF; OpenSCAD writes both), and recipes and saved settings keep the format.
- **Near misses** (`latch_v2.stl` beside `latch.3mf`) are listed on the project's page after it's read, with **Make one part**; any part can be split again (kept in `metadata.json` as `parts.<id>.files`, so it holds across updates).
- **Tests:** the picker's default follows the setting; the choice is remembered; a merge holds after the project is read again.

**0.4.16: Printables ZIPs.**
- A Printables download (a ZIP with the model files and PDFs of the page) is added like any ZIP; its PDF's text is read for **creator, license, published and updated dates, description and tags**, which become the project's details with "from the Printables PDF" as where each came from. The text comes from the PDF's content streams (Flate, which the app already reads); no new library. Print settings in the PDF go in the project's notes.
- What can't be read leaves the field empty, as for any project, and the PDF stays in the side viewer.
- **Tests:** a Printables-style ZIP made by the test (a PDF written with the page's usual sections) fills each field; a PDF that can't be read leaves them empty and the project is still added. The owner is asked for one or two real Printables downloads to check against before release, since the session can't download them.
- **Review 5** follows.

**0.4.17: collections.**
- `collections/<id>/collection.json` (as in "Library folder"): name, summary, icon, and items by id (parts, made parts, models with saved settings) with **quantity, notes, status** (to print / printed) and the format to use. Items are never copied.
- **Collections** in the left menu, a collection's page (its items as cards with quantity and status, a total count), **Add to collection…** on any part, made part or model (one action list, used by the inspector, the card menu and the part page), and remove.
- **Export as ZIP** (written like a library backup, with progress and Cancel): each item's file in the chosen format, a copy per quantity or one file with the count in its name (asked once), a parts list (`parts.csv` and `README.txt` with each item's license and source), and models made at their saved settings first.
- **Tests:** make a collection of a part, a made part and a model with saved settings; set quantities and status; export it; the ZIP has the files, the counts and the parts list with licenses.

**0.4.18: multi-colour and multi-part 3MF from generators.**
- First a check against the app's OpenSCAD: that 3MF export with lazy union keeps top-level objects apart and writes `color()` as materials, natively and in the WebAssembly engine. If either doesn't, the app writes the 3MF itself from the separate objects (Model Library's 3MF writer pattern) and says so here.
- When a render has more than one object or colour, the format picker offers "3MF, 2 colours" or "3MF, 3 parts"; the 3D view colours each object, with a parts list to show or hide them.
- A family manifest can name a part-picker setting ("part to print"); the download then offers **All parts in one 3MF**, made from each choice and laid out side by side. Make several gains "one 3MF with every variant".
- **Tests:** a two-colour label and a three-part model download as 3MF files whose objects and materials are checked by reading the file back (`mesh.rs`); opening them in Bambu Studio and OrcaSlicer is a manual check for the owner.
- **Review 6** follows; then milestone 6 (pictures, your own content, several libraries, and Model Library's folder watcher and 7z/RAR if their crates can be vendored).

### 0.4.14 notes

Built as planned, except:

- **Duplicates** are found by comparing sizes, then SHA-256, against the files of every other project's parts in the catalog; Model Library's `dupes.rs` (a quick fingerprint of the first and last 64 KB, kept in a sidecar) wasn't needed at this size. Linked projects' files aren't compared.
- **Drops** use a dialog rather than Model Library's menu at the pointer (this app has no pop-up menu yet). Loose files dropped together are one project (kind `folder`, `origin.files` lists them); each folder or ZIP dropped is its own project, through the same command as Add project. A ZIP can't be added to a project of your own (it'd need unpacking there).
- **Show the file's colours** is a box on the part's page, remembered on this computer, not a Settings item.
- **Previews:** a part's 3MF is preferred over its STL, as planned; the 3MF's picture is used only for the thumbnail, never instead of the 3D view.

### 0.4.15 notes

Built as planned, except:

- **The default stays STL** ("STL where there is one"), so downloads don't change for anyone until they choose otherwise; the plan's "3MF where there is one, otherwise STL" is the second choice.
- **Formats are kept per part and per model** (this computer's preferences), not in saved settings or recipes: a saved setting reopens the model, whose picker already remembers its format, and a made part is always kept as STL (its page then offers whatever formats it has).
- **A model's 3MF** is written by the app from the render (`mesh::to_3mf`, shared corners written once), not by OpenSCAD's own 3MF export, so it works the same with every engine; 0.4.18's colours and parts will need OpenSCAD's.
- **Near misses** strip `v2`/`v1.3`, `final`, `copy`, `new` and `(1)` from the end of a name (after a space, `_`, `-` or `.`), so `bolt10` and `bolt12` are never offered. Merged parts are kept as `merged: { <kept part>: [<parts it took>] }` in `metadata.json`.


### 0.4.16 notes

- **Checked against the one real Printables PDF there is** (openGrid's page in `libraries/opengrid-official/`, made by WeasyPrint 66): 34 pages, PDF 1.7 with object streams and a cross-reference stream, text as two-byte glyph ids through ToUnicode maps, the page flipped by its first `cm`. Page 1's head: the title (the biggest text, right column), the creator, VIEW IN BROWSER, "updated d. m. yyyy | published d. m. yyyy". Then **Summary** (the summary line, the category with " > ", "Tags:" and the tags as white text on chips, then the description, running on over pages), **Model files** (folders and file names), **Other files**, **License** (its wording, a creativecommons.org link, and ✔/✖ lines). Links give the model's page, the creator's profile and the license. The document title is "Printables PDF" and the bookmarks' first title is the model's.
- **Not yet checked against a real page:** Print settings (this model is "3D MODEL ONLY"); it's read by its heading as "Label: value" lines. Nor the ZIP as Printables makes it: the PDF is looked for at the top of the project or one folder down. Owner: one or two real downloads, one with print settings, would confirm both.
- **Where values come from:** detected values can now say which file they came from (`detected.from.<field>` in source.json), shown as "From the Printables PDF"; other detected values still say "the project's files". The description is kept as `detected.description` and shown on About as HTML; it isn't searched yet.
- **Not done:** checking the page for a newer upload (printables.com is out of reach from the build sessions), and other sites (Thingiverse ZIPs carry README and LICENSE text, which are read already).


### 0.4.17 notes

- **What an entry is:** `{ id, ref, name, settings, values, quantity, status, notes, format }`. `ref` is the item's id in the page's index (`part:<pack>/<part>`, `gen:<model key>`, `comp:<key>`), so made parts are parts like any other. A model's `values` are its changes from the defaults when it was added; `settings` names the saved setting when one was chosen and unchanged, for its code edits. `format` null follows Settings › Downloads.
- **Kept out of the catalog:** collections are read with `collections_list` and written one at a time (`collection_put`, `collection_remove`), so ticking Printed doesn't rebuild the catalog.
- **Export is done by the page**, like Make several's ZIP: part files are read through the library protocol, models are made by the page's engine (and turned into 3MF by the core when chosen), and `web/zip.js` writes the ZIP. Nothing new in the core but storage.
- **Not done:** drag from the browser onto a collection, a collection's icon and summary editing (the fields are there), and entries whose project was renamed by a merge (their `ref` goes stale and shows "No longer in the library"). An entry for a project that's removed keeps its name and is left out of the export, with a line in README.txt.

## Review 5 (of 0.4.16, 2026-10-14)

The fifth periodic review, partway through milestone 5: https://claude.ai/artifact/1VgTyDLDce1rtek4ADpt7c (5 interface items, UI-33 to UI-37; 3 on the library's file structure, F-23 to F-25), from CI for 0.4.14 to 0.4.16, screenshots of each new screen in a local test library, the library folder after adding a Printables ZIP, and the code. Collections (0.4.17) was already built and tested, so it ships first; Review 5's items are 0.4.18 and multi-colour 3MF moves to 0.4.19, followed by Review 6.

### 0.4.18 plan (Review 5's items)

- **UI-33 (Download and its picker):** Generate and Download stay one line high, the format picker a narrow part of the Download button; a part's button reads "Download (1 KB)" and the picker names the format.
- **UI-34 (duplicates):** files found in another project when a project is added are listed in a note on its Items, like near misses, each with the project it's also in, Open the other and Hide this copy; kept in `derived/` so the note survives a restart.
- **UI-35 (near misses):** Keep apart, remembered in the project's `metadata.json`; that group isn't offered again.
- **UI-36 (drop dialog):** the second choice has the outlined button style; files that share no name suggest "Dropped files" and the date.
- **UI-37 (the Printables PDF):** listed as "Printables page (PDF)" with the site details on About.
- **F-23 (a newer Printables download):** a ZIP whose PDF names a page a project already has (`origin.site.page`) is added as a new version of that project, dated by the page's updated date, and offered as Use this version, as an update is; a version's date comes from the page for the first download too.
- **F-24 (read again):** Read again reads the site PDF again, as it does the README and license; your edits in `metadata.json` still win.
- **F-25 (README):** `origin.site` and `detected` are described under `source.json`.
- **CI:** a newer download of a Printables ZIP becomes a version of the first; Read again fills in details for a ZIP added without them; a near miss kept apart stays dismissed; duplicates listed on the project.

### 0.4.18 notes

Review 5's eight items, built as planned, except:

- **F-23:** a ZIP is unpacked once beside the projects (`sources/.adding-<pid>/`) and moved into place, so its PDF can be read before deciding which project it belongs to. When several projects have the same page, the first by id gets the version. The update's `from` is `download`; nothing checks printables.com.
- **F-24:** Read again never re-read the README or license (the review said it did); it now re-reads the page PDF only. What the PDF no longer says stays in `detected` until edited.
- **UI-34:** duplicates are found when the page opens (`source_duplicates`: sizes first, then SHA-256 of same-sized files), not kept in `derived/`; a hidden copy drops out of the note.
- **UI-33:** a stale render's button reads "Last render" beside the picker ("Download last render" without one); the picker's wrapper had the actions' padding, which made the row taller.

### 0.4.19 notes

The first and second points of 0.4.18's multi-colour plan (now 0.4.19), checked against the app's OpenSCAD 2026.10.02:

- **Native OpenSCAD writes the 3MF.** `--enable=lazy-union --export-format=3mf --backend=Manifold` keeps every top-level object apart (one object and one build item each) and writes each `color()` as a material. A render asks for it with `format: "3mf"` (`RenderRequest.format`); it's cached apart from the STL (`-3mf` on the key). The page asks for it only for a 3MF download and a collection's export; previews stay STL. If it fails, the app's own 3MF (`mesh::to_3mf`) is used, as before.
- **What's in it** is read back by `mesh_info` (`mesh::summary`: objects, distinct colours, triangles) and said in the status line. Objects without a `color()` get OpenSCAD's default yellow (#F9D72C), so it counts as a colour. When there's more than one colour the 3D view loads the 3MF's colours (`mesh_coloured`) until the next render.
- **Not done:** the format picker still says "3MF" (what's in it is only known after the render, so the status line says it instead); the parts list to show or hide objects; "All parts in one 3MF" from a family manifest's part picker (no project the app ships has a part picker since the starter projects were dropped); Make several's "one 3MF with every variant"; and the WebAssembly engine, which keeps the app-written 3MF without colours. Opening the files in Bambu Studio and OrcaSlicer is still the owner's check.

## Review 6 (of 0.4.19, 2026-10-14)

The sixth periodic review, at the end of milestone 5: https://claude.ai/artifact/SjWTMLEC3JSBU4PfvsgXPQ (5 interface items, UI-38 to UI-42; 4 on the library's file structure, F-26 to F-29), from CI for 0.4.17 to 0.4.19, screenshots from a local test library, the library folder on disk and the code. The owner took all nine (2026-10-16): 0.4.20, then milestone 6.

### 0.4.20 plan (Review 6's items)

- **UI-38 (filters):** a project's site fields stop making an "Updated" filter of their own (the page's updated date is the version's date, so the library's Updated covers it); Published is grouped like Updated (Today, This week, This year, older years).
- **UI-39 (colours):** reading a project notes which models call `color()` in their own file (`colours: true` in the model's details; comments don't count). On the desktop app with OpenSCAD, after such a model's preview the page makes OpenSCAD's 3MF too, in the background and only once per set of values, and reads what's in it. With more than one colour or object the status line says so: with STL chosen "It has 3 colours: choose 3MF to keep them", with 3MF "The 3MF has 3 parts in 3 colours". The 3D view shows the colours when **Show the file's colours** is ticked, now beside the colour swatches on models as on parts, and on unless you untick it (one setting for both, remembered on this computer). While a file's colours are shown the swatches are hidden. The 3MF download uses the 3MF already made.
- **UI-40 (printed copies):** an entry with a quantity above 1 shows printed as a count, "2 of 3", with − and +; the bar counts copies. A finished row gets a tick and a quieter name, not a strike-through. Notes shows its hint only when it's focused. Stored as `printed` (a number) beside `status` in collection.json; an entry written before keeps working (`status: "printed"` means all copies).
- **UI-41 (part downloads):** every part's download is one full-width "Download (3.6 MB)" with its format beside it: a picker when there are several formats, a label when there's one. Parts with labelled files (several files per part) keep their list.
- **UI-42 (sidebar):** Add a project moves up, under Parts Library; Home lists collections under Continue when there are any.
- **F-26 (Printables versions):** a ZIP whose files (by the content fingerprint versions already carry) match one of the project's versions isn't added again: "already in the library, as the version of …". Read again dates a version from the page when the version's date was the day it was added. A newer download dated later than the version in use is offered as an update; one dated earlier is only listed under Versions, with a toast saying so. The update's summary lists parts added, removed and changed, as it lists models.
- **F-27 (collections):** a model entry keeps `files_id` (the fingerprint of the files the model used, as a made part's recipe does) and `project_version`. When the model's files change, the row says "Made from an earlier version" with **Use the current version**; Export's message says how many entries changed. Entries added before 0.4.20 take the current fingerprint the first time the collection is opened.
- **F-28 (made parts' format):** Save as part and Make several write the format chosen beside Download: `<name>.3mf` from OpenSCAD's 3MF (colours and objects kept) when 3MF is chosen and the app's OpenSCAD is there, else `<name>.stl` as before. The recipe records `format`; Make again makes the same format and replaces the file.
- **F-29 (README):** `metadata.json` is described with what it holds now (names, categories, licenses, forms, parts made into one and groups kept apart, merges, examples), and collections as referring to items by id.
- **CI:** a coloured model's preview shows its colours and names them with STL chosen; a re-download of the same Printables ZIP isn't added again; a collection entry counts printed copies and shows when its model changed; a part saved with 3MF chosen is a .3mf with its colours.

### 0.4.20 notes

Review 6's nine items, built as planned, except:

- **UI-39:** whether a model calls `color()` is found by the page from the model's own file when it's opened (comments stripped), not stored in the model's details, so projects read before needn't be read again. OpenSCAD's 3MF is made once per render and shared by the 3D view, the 3MF download and Save as part (`colour3mf` in app.js). Uncoloured objects count as OpenSCAD's yellow, as in 0.4.19.
- **F-26:** a download dated the same day as the version in use is offered as an update (only one dated earlier is just listed), so a re-upload on the same day isn't missed. Matching is by the content fingerprint versions already carry; a project with several of the same page gets the version by its first id, as in 0.4.18.
- **F-28:** Make several makes each row as OpenSCAD's 3MF when 3MF is chosen, and the last row once more as an STL for the preview. With the WebAssembly engine parts stay STL.
- **F-27:** entries added before 0.4.20 take the current fingerprint the first time their collection's page is opened (parts and components have none).


## Library format design (0.4.22, before milestone 6)

The owner's direction of 2026-10-16 (above, "Added 2026-10-16 … the library format"). A survey of every file the app writes in a library (2026-10-16) found: the folder layout is already the same for every project, but only `library.json` (format 1), derived data (format 5), `workshop.json` and `description.json` carry a version; an older app opening files written by a newer one gets no warning unless `library.json`'s number is higher; and there are two recipe shapes (saved settings `recipes/<id>.json`; a made part's `<name>.recipe.json`, whose `format` is the file type, "stl" or "3mf"). The design:

- **The spec**, `docs/LIBRARY_FORMAT.md`: every folder and file, which ones hold your work (keep) and which the app can make again (rebuildable), each JSON file's fields and format number, the ids other files use (`gen:`, `part:`, `comp:`), and the rules: files in `files/<version>/` are never changed; unknown keys are kept; a file with a higher format than the app knows is read but never written. The library's README.txt points to the same rules in short.
- **One table in the code** (`desktop/core/src/format.rs`) names each kind of JSON file by its path and its current format. `write_json` stamps the format on any file the table names, so no writer can forget it, and Check library reads the same table.
- **Library format 2.** A library opened by this version is upgraded once: the JSON files it changes are first copied to `backups/format-1-<date>.zip` in the library; every JSON file in the table gets its format; recipes take the one shape below; `library.json` says format 2. An older app then opens the library read-only and says it was made by a newer version (as now for a higher number). A read-only library isn't upgraded. Merge library and Restore upgrade what they bring in the same way.
- **A file newer than the app:** a recipe, collection, project or its metadata with a higher format than this version knows is shown but can't be changed ("made by a newer version of SCAD Workshop"), and Check library lists it.
- **One recipe shape (format 1)** for saved settings and made parts: `format`, `kind` ("settings" or "part"), `name`, `model`, `model_name`, `project`, `project_version`, `files_id`, `values`, `edits`, `output` (the file type a made part was saved as, or that saved settings download as: "stl" or "3mf"), `engine`, `made_by`, `created`, `updated`. Saved settings also keep their `id` (the file name); a made part is still known by its file. The upgrade renames saved settings' `version` to `project_version` and a made part's `format` ("stl"/"3mf") to `output` and its `date` to `created`; files_id is added where the project's current version is the one the recipe was made with. Readers accept the old names too, so a recipe copied in by hand still works.
- **Check library** (Library settings › Check library, and `workshop-cli library-call … library_check`): reads the whole folder against the table and lists what's off, grouped as "Can be repaired" and "To look at":
  - repaired safely: a JSON file without its format; temporary folders left by an interrupted add (`.adding-*`, `*.tmp-*`); derived data, docs and generated copies for a version that's gone; a missing README or top folder.
  - listed only: a file that isn't valid JSON; a file newer than the app; a project folder without `source.json`; a version whose files are missing, or a files folder no version lists; a recipe whose model is gone; a made part's recipe without its part, or a part without its recipe; a collection entry or favourite that refers to nothing; metadata for items that are gone.
  - Nothing under `files/<version>/` and nothing you made is changed or removed by Repair.
- **CI:** a format-1 library (written by the test from the old shapes) opens as format 2 with its backup, its recipes in the one shape, saved settings and Make again still working; Check library on the library the tests made finds nothing to repair or look at, then finds and repairs a planted temporary folder and lists a planted collection entry that refers to nothing. Rust tests cover the upgrade and the table.
- **Not changed:** your own and linked projects are read where they are; project files aren't reorganised; folder names stay as they are.

### 0.4.22 notes

Built as planned, except:

- Check library is in Library settings under Backups, and from the command line through `workshop-cli library-call --library <dir> --app-site <dir> library_check` (or `library_repair`) rather than a command of its own.
- Example parts aren't checked: they're kept by the fingerprint of the files each example uses, not by version, and a later read makes them again.
- A project not read yet (your own projects are read just after the library opens) has its items left out of "refers to nothing", so Check library run at once doesn't list them.
- A made part's recipe from before 0.4.20 had no file type: the upgrade takes it from the part beside it.
- Saved settings now record what made parts' recipes do (the model's name, its project, the files' fingerprint).

### 0.4.23 notes (the future features of 2026-10-16)

Built as listed under "Added 2026-10-16 (owner), future features", with these choices:

- **Show in folder** is in a model's ⋯ menu (its .scad file), beside a part's Download (its file) and in a project's ⋯ menu (the project's folder; this replaces "Open its folder"). The core's `reveal_path` ({ source, rel, examples }) gives the file's place on disk, or the project's folder when the file isn't there.
- **2D models:** a model whose render fails with OpenSCAD's "not a 3D object" is made again as SVG (`render.rs`: the request's `format` takes "svg" and "dxf", cached by its own key, and a 2D model's drawing is found in the cache next time). The page shows the drawing on a white sheet in place of the 3D view, with its size read from the outlines; the picker beside Download offers SVG and DXF (DXF made by OpenSCAD at the same settings); Batch generate downloads drawings in the chosen format. Thumbnails are the drawing; Check defaults counts a drawing as a pass. Not yet: keeping a drawing in the Parts Library (Save as part is off for drawings), since the Parts Library doesn't read SVG or DXF files as parts (every SVG in a project would become one).
- **Notes:** "Your notes" on a model's page (from its ⋯ menu, shown open once there are some) and a Notes tab on a project's page, saved when you click away, kept in `metadata.json` as `my_notes` (the item's or the project's; "notes" was already the author's notes, shown as About this model, and is labelled so in Edit details now). Search finds a model by its notes and its project's.
- **One Code button:** on a model's page the edge tabs show only while the side panel is open (to switch between Code and README); folded, the header's buttons are the way in. Other pages keep the edge tabs.
- **Edit form:** each field is filled with what was read (the label falls back to the setting's name made readable), and Put back restores a field you changed.
- **A single .scad file** chosen under Add a project › File becomes a project of its own, named after the file. The Suggested row is gone.
- The library's README says Batch generate and lists `my_notes`.

## Reading settings reliably: design (0.4.24)

The owner's direction of 2026-10-16 ("Added 2026-10-16 (owner), after the library format: reading settings reliably") and of 2026-10-17 ("It's worth having a fallback if this new type of parsing yields errors"). A survey of how settings are read today (2026-10-17):

- **Models** already take their settings from OpenSCAD's own parameter list (`--export-format=param`, `render.rs` `export_params`, then `ingest::convert_params`): names, types from the values, ranges and drop-downs from the Customizer comments. A text setting is a drop-down only when its comment lists the choices (`// [round, square]`). Values typed into calls become settings in a generated copy (`components::lift_call_values`).
- **Components** (library modules) are read from the text alone: the module's signature, its documentation (BOSL2's `Arguments:` and `Example:` blocks) and the first example that is a plain call as start values. A default written as arithmetic (`1/3`, `[10, 10, 10] / 2`) is shown as typed-in code, not a number.
- Nothing tries other values when the defaults don't render: the page shows the error, Check defaults marks the model.

The design, each step falling back to today's reading if it fails:

1. **Choices read from the code.** A model's text setting with no choices of its own becomes a drop-down when its file (and the project's files it includes) compares it with two or more different words: `style == "round"`, `"square" != style`. Its default is kept as a choice if the code doesn't name it. The empty word doesn't count (`label == ""` is a test for "no label", not a choice), and nothing changes for a setting the comment already gave choices, or one compared with fewer than two words. Such a setting is marked as read from the code (Edit form says so). The library's own files aren't read for this: only the project's.
2. **Typed in, or back.** In Edit form, clearing a setting's Choices makes it a typed-in field even when the file or the code gave it choices (saved as `options: []` in the model's form); typing choices into any field makes it a drop-down, as today.
3. **Arithmetic is a number.** A component's default or example value written as arithmetic of numbers (`1/3`, `-0.5*4`, `[1/2, 0, 3]`) is read as its value, so the field is a number or a vector instead of typed-in code. Defaults that refer to the library (`CENTER`, another argument) stay typed-in code; evaluating those needs OpenSCAD run on the library, which is left for the owner's edge cases to justify.
4. **Defaults that don't render are repaired from the project's own presets.** On a model's page, when a render at the defaults fails and the model has presets (a project's `.json` parameter sets, a component's documented examples), each preset is tried in turn (at most three); the first that renders is chosen in the Defaults list, with a note: "Its defaults don't render; showing the preset <name> instead." Check defaults does the same and counts a model that renders with a preset as working (its thumbnail is that render). Nothing is written over the model's defaults: choosing Defaults again shows the failure.
5. **The fallback.** Each new step runs on its own: an error in one (a file it can't read, a pattern it doesn't expect) leaves that model's settings as today's reader made them, and the project's problems list says which step was skipped and why. Steps 2 and 4 are choices you make or retries of today's renders, so they have nothing to fall back from.
6. **Edge cases as tests.** `tests/settings/` holds small `.scad` files with the reading each should give (`<name>.expect.json`), read by a Rust test; the owner's failed edge cases go there as they arrive.

Not now: OpenSCAD runs that echo each setting's evaluated value (OpenSCAD's own list already has every setting a Customizer would show; computed values aren't settings), and marking uncertain settings on the model's form itself (Edit form shows where a setting's choices came from instead, which keeps the form plain).

### 0.4.24 notes

Built as designed above, with these details:

- Choices from the code: `ingest::choices_from_code`, run on a model's settings after OpenSCAD's list is read (`project.rs`), over the model's own .scad files (not the libraries'). The read is caught whole: if it errors or stops, the model keeps OpenSCAD's list and the project's problems get a "reading" entry. Choices keep the code's order, the default first when the code doesn't name it.
- Typed in: a form's `ui.<setting>.options: []` (Edit form with Choices cleared) makes the setting a typed-in field and stops the code's choices coming back on the next read (`typed_in`).
- Arithmetic: `components::folded` (literals, numbers written as arithmetic, vectors of them) replaces `parse_literal` where a component's settings are typed.
- Presets for failing defaults: on a model's page, once per model while the app is open (so choosing Defaults afterwards shows the error); Check defaults records "Renders with its preset …" as a pass.
- `tests/settings/`: three cases to start (compared words, a ternary inside a module, comparisons in comments), read by `settings_edge_cases`.

## Milestone 6 design: pictures and your own content (0.4.25)

Items 1 and 2 of the owner's requests of 2026-10-12; item 9 (several libraries) is in the backlog (owner, 2026-10-17). Kept small on purpose, since a UI review for overengineering follows: one new action on each page, one dialog each, no new settings.

**Pictures (item 1).** Today a project's picture can be one of its items' thumbnails (Edit details › Icon); a model or part always shows its render.

- **Choose picture…** in the ⋯ menu of a model's page, beside a part's Show in folder, and in a project's ⋯ menu (Edit details' Icon list goes; this replaces it). One dialog shows the choices as pictures: **The render** (the default), every picture in the project's files (PNG, JPEG, WebP, GIF; at most 200, the project's folders named under each), for a project its items' thumbnails, and **Add a picture…** (a file you choose, copied into the library so the folder stays portable).
- **Stored** as the level's `icon` in `metadata.json`, as today: `item:<project>/<item>` (a thumbnail, as now), `file:<path>` (a picture in the project's files, found in whichever version is in use; a picture an update removes falls back to the render), or `picture:<name>` (one you added, in `sources/<id>/pictures/`, a "keep" folder in the format).
- **Shown** wherever the item's thumbnail is (cards, the inspector, tabs, collections); the render is still made, for the 3D view and as the fallback.

**Your own content (item 2).** Documents today are the project's own (its README, PDFs, readme.txt).

- **Add content…** on a model's page (⋯), a part's page and a project's page (⋯): files you choose (PDF, pictures, video, text, anything) are copied into `sources/<id>/attachments/` (a "keep" folder), and a web link can be added as an entry of its own. Each is listed in `metadata.json` under the level it belongs to as `attachments: [{ name, file | url, added }]`.
- **Shown** in the side viewer next to the project's documents, as their own tabs, named by you: PDFs, pictures and text as documents are now, a video with the system's player in the panel, a link opening in the browser, anything else opening in its own app. A model's content shows when that model is open, a project's whenever the project is.
- **Removed** from the same tab (Remove, with an in-page confirmation); the file is moved to `backups/removed/` rather than deleted, so a mistake can be undone by hand.
- Your notes (0.4.23) stay a Notes tab of their own: they're text you write, not a file.

**Format.** New keys in `metadata.json` (`attachments`, new `icon` forms) and two new folders: library format stays 2, since an older app keeps keys it doesn't know (rule 4) and ignores folders it doesn't know. `docs/LIBRARY_FORMAT.md` lists them; Check library lists an attachment or picture whose file is missing.

**Tests.** Choosing a project's picture and an added picture changes the card's picture and survives reading the project again; adding a PDF and a link to a model lists them in the side viewer, opens the PDF, and Remove moves the file to `backups/removed/`.

### 0.4.25 notes

Built as designed above, with these details:

- `desktop/core/src/content.rs` holds it: the pictures a project can show (`pictures_list`), copying a file in under a name of its own ("Guide 2.pdf" beside "Guide.pdf"), what kind of document an attachment is, and setting a removed one's file aside in `backups/removed/<date>-<project>-<name>`. Commands: `pictures_list`, `picture_add`, `attachment_add`, `attachment_remove`.
- Choose picture… replaces Edit details' Icon list. A `file:` picture or `picture:` one whose file is gone shows the render again, and Check library lists it. Names in `icon` and `attachments` are read only inside their folder.
- Content you add is a document in the side viewer like the project's own; a model's shows only on that model's page, a part's on that part's, a project's everywhere in it. Its name is the file's without the extension unless you give one. Remove asks first, as the app's other deletions do.
- A part's page gets a ⋯ menu (Choose picture…, Add content…, Show in folder) beside its buttons.

## UI review after milestone 6 (0.4.26)

The owner asked (2026-10-17) for a review checking the UI isn't overengineered: https://claude.ai/artifact/Mrc79MwmEvqBLmmhnSqK9X. What the owner asked for stays; the changes remove scaffolding around it. Ten items (three more were proposed and dropped on a closer look, below):

**Hide what can't matter.**
1. A project of 12 items or fewer shows no view, sort, grouping, Condense, size or filter controls (unless it has hidden items, or a filter or search is on). Any list hides a filter whose one value every item shares.
2. The details panel (inspector) shows only while something is selected; its button still turns it off.
3. No "desktop" badge in the desktop app.
4. A collection's "Copies in the ZIP" shows only when a row's quantity is above 1.

**One way to do each thing.**
5. Choose picture… is how a project gets a picture; the inspector's "Use as the group's icon" goes (categories keep Image… in Library settings).
6. The Grouped view goes; the Group select and Condense group the list. A saved Grouped choice shows as Grid.
7. Reset all settings goes from a model's ⋯ menu: Defaults in the settings drop-down does it.
8. About this model is a header button beside Code and README, shown when the author wrote notes. (Your notes already show above the form once written; ⋯ › Your notes is for writing the first.)

**Cut leftovers.**
9. Home's "Projects to start with" goes (suggestions left Add a project on 2026-10-16).
10. The Add a project tile in a condensed list goes.

**Dropped on a closer look.** Read again shows once already (the review misread it). Settings › Downloads stays: it's how STEP is preferred where a part has one. "Save as project file" and "Treat as a library" stay in a project's ⋯: moving them would add a menu to Library settings' rows.

### 0.4.26 notes

Built as listed above. The Settings cases project in the acceptance test has 3 items, so it checks that a small project shows no browse controls; the Printables filters check reads the whole library's filters when its project is that small.

## Fusion 360 files: tried and removed (0.4.27 to 0.4.30)

From 0.4.27 to 0.4.30 the app read Fusion 360 files (`.f3d`) as Parametric Models and generated them with MitCad, at the end with a build of MitCad from the owner's fork (DrFlGd/Mitcad, branch `f3d-sketch-projections`). The design and what was learnt are in this file and in CHANGELOG.md as of the `v0.4.30` tag. The owner removed the feature on 2026-11-12 to focus on cleaning up and reviewing the existing app, so the code is back to 0.4.26's. `.f3d` files are still listed as Parts Library files, as they were before 0.4.27.
