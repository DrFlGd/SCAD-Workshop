# Test projects

Small projects for the app's tests, written for them and public domain (CC0 1.0).
The bench (`workshop-cli bench --projects tests/projects`), the acceptance test
(`tests/desktop_page.py`) and the app test (`tests/desktop_ui.py`) copy each folder
into a library's `local/` folder, where the app reads it as one of your own projects.

| Folder | What it is | What it tests |
|---|---|---|
| `bins/` | A bin, a tray and a lid on a 42 mm grid | `constants.scad` is included by every file in `modules/` and the models include all of them, so it is included several times per render (include once); the bin's width, depth and height are pairs (units, mm) for Batch generate; the lid's sizes have fractions as defaults and no Customizer comment |
| `labels/` | A text label and a hanging tag | text with the app's fonts; a drop-down of fonts; the Labels category |
| `parts/` | A hook (STL and 3MF, in two colours), a bracket (STL) and a spacer (OBJ) | ready-made parts: one part with two formats, a 3MF's colours |

Each has a `workshop.json` that names its models, category and license. The parts were
made with OpenSCAD from a few lines of code each.
