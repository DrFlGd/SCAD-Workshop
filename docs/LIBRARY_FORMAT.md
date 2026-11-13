# The library format

A SCAD Workshop library is one folder of plain files: the projects you added, your own projects, saved settings, made parts, collections and all your edits. This is what's in it, for library format **2** (SCAD Workshop 0.4.22 on). The app's code for it: `desktop/core/src/library.rs` (the folder), `format.rs` (the JSON files and their numbers), `check.rs` (Check library).

## Rules

1. **A project's files are never changed.** `sources/<id>/files/<version>/` holds a project exactly as it came (GitHub's download at a commit, a ZIP's contents, a copied folder). Update checks, hash checks and "what changed" all rely on it. What the app makes from them lives beside them.
2. **Keep or rebuildable.** Each file below is marked. *Keep* files hold your work or can't be made again; *rebuildable* ones the app makes again when they're missing, so they can be deleted (and a backup can leave thumbnails out).
3. **Every JSON file the app writes says its format** (`"format": <n>`), apart from those that had their own number before (derived data, `workshop.json`, `description.json`). A file with a higher number than the app knows was written by a newer version: the app reads it but never writes over it. A file without one is read as format 1.
4. **Unknown keys are kept.** A key the app doesn't know is left in the file when the app changes it, so a newer version's additions survive an older one.
5. **The library's own number** is `library.json`'s `format`. An app opening a library with a higher number opens it read-only and says why. A library with a lower number is upgraded when it's opened (not when read-only): the JSON files it changes are first copied as they were to `backups/format-<n>-<date>.zip`.
6. JSON files are written pretty, with sorted keys and a final newline, via a temporary file and a rename, so a library kept in git or a sync tool changes only where something changed and a crash never leaves half a file.
7. Ids (`sources/<id>`, collection ids, saved settings' ids) are letters, digits, `.`, `_` and `-`, at most 96 characters, not starting with a dot. Folders starting with a dot are the app's temporary ones.

## The folder

| Path | What | |
| --- | --- | --- |
| `library.json` | name, id, favourites, categories, library-wide defaults | keep |
| `README.txt` | a short version of this page (replaced when the app's text changes, unless you edited it) | rebuildable |
| `sources/<id>/` | one folder per project (below) | |
| `local/<name>/` | your own projects, read where they are | keep |
| `local/Made parts/<model>/` | parts made from models, each with its recipe | keep |
| `recipes/<id>.json` | saved settings | keep |
| `collections/<id>/collection.json` | collections | keep |
| `icons/category-<slug>.<ext>` | pictures chosen for categories | keep |
| `thumbs/components/<library>/<copy>/<module>.webp` | components' thumbnails | rebuildable |
| `trash/<date>-<id>/` | a deleted project (`source/`, `local/` when it was your own, `trash.json`), until the trash is emptied | keep |
| `backups/*.zip` | JSON files as they were before an upgrade, Merge or Repair changed them | yours to delete |
| `backups/removed/` | files of content you removed (`<date>-<project>-<name>`), set aside rather than deleted (0.4.25) | yours to delete |
| `sources/.adding-<n>/`, `files/.<version>.tmp-<n>` | an add or download in progress (left only if the app stopped part way; Check library removes them) | temporary |

Not in the library: the render cache, code-edit blobs, preferences, printer profiles and the list of recent libraries live with the app on each computer.

## A project: `sources/<id>/`

| Path | What | |
| --- | --- | --- |
| `source.json` | where it came from, its versions and which is in use | keep |
| `metadata.json` | your edits (removed when empty) | keep |
| `pictures/` | pictures you added for the project or its items (0.4.25) | keep |
| `attachments/` | files you added to the project, a model or a part: guides, videos, photos (0.4.25) | keep |
| `files/<version>/` | the files, exactly as they came; several versions until **Remove versions not in use** | keep |
| `derived/<version>.json` | what reading the files found (models, settings, parts, documents, problems); its own `format`, 5 | rebuildable |
| `workshop.json` | what the app read the project as (a report; a project's **own** `workshop.json`, at the top of its files, is what decides) | rebuildable |
| `description.json` | the app's description of a project it knows from GitHub (starter projects of earlier apps) | rebuildable (from the app) |
| `checks.json` | whether each model renders at its defaults | rebuildable |
| `thumbs/`, `docs/<version>/`, `generated/<version>/`, `examples/<fingerprint>/`, `previews/` | thumbnails, documents as HTML, copies made so typed-in values become settings, parts made from a folder of examples, previews | rebuildable |
| `pins/<id>.json` (in `sources/pinned/` only) | components pinned as models | keep |

Your own projects (`kind: "local"`) are read from `local/<name>/` and have no `files/`; linked projects (`kind: "linked"`) are read from their folder outside the library, don't move with it and aren't in its backups.

### `source.json` (format 1)

`id`; `kind`: `github`, `zip`, `folder`, `linked`, `local`, `bundled` or `pinned`; `role`: `project` or `library`; `added`; `origin` (`type`; for GitHub `url`, `owner`, `repo`, `track` (`release` or `branch:<name>`), `subdir`, `added_from`; for ZIPs `file`, `files` and `site` (`name`, `page`, `pdf`) for a page's download; `path` for folders); `version` (the one in use), `version_date`, `previous_version`; `versions` (`id`, `date`, `added`, `files`, `content` (the files' fingerprint), and `commit`, `message`, `tag`, `sha256` where they apply); `detected` (what was read from the files: `name`, `summary`, `license`, `authors`, `tags`, `category`, `origin`); `update` (`state`, `latest`, `checked`, `error`).

### `metadata.json` (format 1)

`project` (the project's own fields), `folders` (by path), `items` (models, by id), `parts` (by id): each a set of fields (`name`, `category`, `summary`, `tags`, `license`, `authors`, `form`, `hidden`, `notes` (the author's, as About this model), `my_notes` (your own, 0.4.23), `icon` (a picture you chose: `item:<project>/<item>`, `file:<path>` in the project's files, or `picture:<name>` in `pictures/`; 0.4.25), `attachments` (content you added, 0.4.25: a list of `{ name, file | url, added }`, `file` being a name in `attachments/`), `fields`…); `merged.keep` (projects merged into it); `apart` (groups of files kept as separate parts); `examples` (`folder`, `as`). Your values here win over what was detected.

## Recipes: saved settings and made parts (format 1)

Saved settings (`recipes/<id>.json`) and a made part's recipe (`local/Made parts/<model>/<name>.recipe.json`, beside `<name>.stl` or `<name>.3mf`) have one shape:

| Key | |
| --- | --- |
| `format` | 1 |
| `kind` | `settings` or `part` |
| `id` | saved settings only: the file's name |
| `name` | the name you gave (a made part's is its file's) |
| `model` | `<project>/<model>`, or `@<library>/<module>` for a component |
| `model_name`, `project` | the model's name and project id when it was saved |
| `project_version` | the project's version it was made with |
| `files_id` | the fingerprint of the files the model used, to tell when they've changed |
| `values` | the settings you changed (the rest are the model's defaults) |
| `edits` | your code edits, by file: `{ text, base, copy? }`, or null |
| `output` | the file type: `stl` or `3mf` |
| `category` | a made part's category |
| `engine`, `made_by` | the OpenSCAD and app that made it |
| `created`, `updated` | when |

Format 1 libraries wrote saved settings' project version as `version`, and a made part's file type as `format` ("stl"/"3mf") and its date as `date`; the upgrade renames them, and the app reads the old names too.

## Collections: `collections/<id>/collection.json` (format 1)

`id`, `name`, `summary`, `created`, `updated`, `items`: each entry has its own `id`, `ref` (the item's id, below), `name`, `quantity`, `printed` (how many copies are printed; `status` `"printed"` from before 0.4.20 means all), `notes`, `format` (the download format), and for a model `settings` (a saved settings' id or null), `values`, `files_id` and `project_version`.

### Item ids

Collections and favourites refer to items by id, not by copying them:

- `gen:<project>/<model>`: a model
- `comp:@<library>/<module>`: a component
- `part:<project>/<part>`: a part; a made part's project is `local-made-parts` and its id is made from its folder and name

## Other JSON files

- `checks.json` (format 1): `version` and `engine` it was checked with, `models` (`ok`, `message`, `seconds`, `date` per model).
- `pins/<id>.json` (format 1): `id`, `name`, `component`, `category`, `summary`, `defaults`, `hidden`, `fixed`, `presets`, `from` (`library`, `copy`).
- `trash/<entry>/trash.json` (format 1): `entry`, `kind`, `id`, `name`, `deleted`, `local`, `source_kind`.
- `library.json` (format 2): `format`, `id`, `name`, `created`, `favourites`, `categories` (`label`, `icon`, `moved_to`), `metadata` (library-wide defaults), `prefer_bundled`; `removed_starter` (the starter projects you deleted) is left from earlier versions and no longer used.

## Check library

Library settings › Check library (and `workshop-cli library-call --library <dir> --app-site <dir> library_check`, or `library_repair`) reads the whole folder against this page.

- **Can be repaired** (Repair does it): a JSON file not yet in this version's format; an unfinished copy left by an add or a download; derived data, documents and generated copies of a version whose files are gone; a missing README or top folder.
- **To look at** (listed only): a file that isn't valid JSON; a file made by a newer version; a folder in `sources/` without `source.json`; the version in use with its files missing; a version's files the project doesn't list; a linked project whose folder is gone; saved settings for a model that isn't in the library; a made part's recipe without its part; a collection entry or favourite that refers to nothing; content you added whose file is gone from `attachments/`, and a chosen `picture:` that's gone from `pictures/`.

Repair never changes anything under `files/<version>/` or anything you made, and backs up the JSON files it changes.

## History

- **Format 2** (0.4.22): every JSON file says its format; one recipe shape; `backups/`.
- **Format 1** (0.4.0 to 0.4.21): `library.json` had the number; other files had none.
