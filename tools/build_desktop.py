#!/usr/bin/env python3
"""Split a built site into what the desktop app bundles.

    python3 tools/build_desktop.py --site _site --out build/desktop [--fetch-fonts] [--cli workshop-cli]

  ui/       the front end loaded in the app window (HTML, JS, CSS, three.js; of the
            site's WebAssembly engine only its licence, engine/COPYING and NOTICE.txt)
  site/     the app's own files: data/catalog.json (engine version, common files)
            and fs/ (the fonts every render gets)
  starter/  the starter library: every generator and parts pack of the site,
            packaged as library projects (workshop-cli bundle). The app copies
            them into the user's library on first start.
  libs/     the OpenSCAD libraries the app ships with (fetched beforehand by
            tools/fetch_libraries.py --out build/desktop/libs); this adds the index
            of their modules (Components), libs/index/<Name>.json
The native engine goes in build/desktop/engine (tools/fetch_native_engine.py).
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from build_site import find_cli  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument("--site", type=Path, default=Path("_site"))
ap.add_argument("--out", type=Path, default=Path("build/desktop"))
ap.add_argument("--cli", type=Path, help="workshop-cli (default: desktop/target/release, or $WORKSHOP_CLI)")
ap.add_argument("--fetch-fonts", action="store_true",
                help="bundle the skins' fonts (SIL OFL, from google/fonts) instead of loading them from Google Fonts")
a = ap.parse_args()

DATA = {"data", "fs", "parts", "thumbs"}  # library content: goes into the starter library, not the window
if not (a.site / "data/catalog.json").exists():
    raise SystemExit(f"{a.site} is not a built site; run tools/build_site.py first")
for sub in ("ui", "site", "starter"):
    if (a.out / sub).exists():
        shutil.rmtree(a.out / sub)
(a.out / "ui").mkdir(parents=True)
for item in sorted(a.site.iterdir()):
    if item.name in DATA:
        continue
    dest = a.out / "ui" / item.name
    if item.name == "engine":
        # the app renders with native OpenSCAD only (Review 7, item 1): the licence the Licenses page links to
        dest.mkdir()
        for name in ("COPYING", "NOTICE.txt"):
            if (item / name).exists():
                shutil.copy2(item / name, dest / name)
        continue
    (shutil.copytree if item.is_dir() else shutil.copy2)(item, dest)

# the app's own files: engine version and the fonts every model gets
catalog = json.loads((a.site / "data/catalog.json").read_text())
(a.out / "site/data").mkdir(parents=True)
(a.out / "site/fs").mkdir()
(a.out / "site/data/catalog.json").write_text(json.dumps({"engine": catalog["engine"], "common_files": catalog["common_files"]}))
for sha in catalog["common_files"].values():
    shutil.copy2(a.site / "fs" / sha, a.out / "site/fs" / sha)

# everything else becomes the starter library
subprocess.run([str(find_cli(a.cli)), "bundle", "--site", str(a.site), "--out", str(a.out / "starter")], check=True)

# the bundled libraries' modules (Components)
if (a.out / "libs/libraries.json").exists():
    shutil.copy2(ROOT / "catalog/components.json", a.out / "libs/components.json")  # start values for modules without examples
    subprocess.run([str(find_cli(a.cli)), "libs-index", "--libs", str(a.out / "libs")], check=True)
else:
    print("no bundled libraries (run tools/fetch_libraries.py --out build/desktop/libs first): the app will have no Components", file=sys.stderr)

# The app works offline: no Google Fonts request. Bundle the skins' fonts if
# asked (CI does), otherwise fall back to system fonts. A font file is only read
# when a skin in use needs it.
FONT_URL = "https://raw.githubusercontent.com/google/fonts/main/ofl/"
# The skins' fonts (docs/DESKTOP_PLAN.md, "Skins design"): (family, folder in google/fonts,
# [(file, weights)], widths). A family whose folder has no OFL.txt gets the
# licence's text with the copyright line from its METADATA.pb.
FONTS = [
    ("Archivo", "archivo", [("Archivo%5Bwdth%2Cwght%5D.ttf", "100 900")], "62% 125%"),
    ("Overpass", "overpass", [("Overpass%5Bwght%5D.ttf", "100 900")], None),
    ("Overpass Mono", "overpassmono", [("OverpassMono%5Bwght%5D.ttf", "300 700")], None),
    ("Sofia Sans", "sofiasans", [("SofiaSans%5Bwght%5D.ttf", "1 1000")], None),
    ("Sofia Sans Condensed", "sofiasanscondensed", [("SofiaSansCondensed%5Bwght%5D.ttf", "1 1000")], None),
    ("Sofia Sans Extra Condensed", "sofiasansextracondensed", [("SofiaSansExtraCondensed%5Bwght%5D.ttf", "1 1000")], None),
    ("Atkinson Hyperlegible", "atkinsonhyperlegible", [("AtkinsonHyperlegible-Regular.ttf", "400 600"), ("AtkinsonHyperlegible-Bold.ttf", "700 900")], None),
    # Layer Line's headings: one heavy weight for all of them, cut to Latin
    # letters and the app's symbols when fontTools is there (3.6 MB to ~100 KB).
    ("M PLUS Rounded 1c", "mplusrounded1c", [("MPLUSRounded1c-ExtraBold.ttf", "100 1000")], None),
]
SUBSET = "U+0000-024F,U+02B0-02FF,U+2000-206F,U+2070-209F,U+20A0-20CF,U+2100-214F,U+2190-21FF,U+2200-22FF,U+2300-23FF,U+25A0-25FF,U+2600-27BF"
index = a.out / "ui/index.html"
html = index.read_text(encoding="utf-8")
lines = [l for l in html.splitlines() if "fonts.googleapis.com" not in l and "fonts.gstatic.com" not in l]
font_css = ""
if a.fetch_fonts:
    import io, re, urllib.request
    get = lambda path: urllib.request.urlopen(FONT_URL + path, timeout=60).read()
    fonts = a.out / "ui/vendor/fonts"
    fonts.mkdir(parents=True, exist_ok=True)
    faces = []
    for family, folder, files, widths in FONTS:
        name = family.replace(" ", "")
        for remote, weights in files:
            data = get(folder + "/" + remote)
            if family == "M PLUS Rounded 1c":
                try:
                    from fontTools import subset
                    from fontTools.ttLib import TTFont
                    font = TTFont(io.BytesIO(data))
                    sub = subset.Subsetter(subset.Options(layout_features=["*"], name_IDs=["*"], notdef_outline=True))
                    sub.populate(unicodes=subset.parse_unicodes(SUBSET))
                    sub.subset(font)
                    out = io.BytesIO()
                    font.save(out)
                    data = out.getvalue()
                except ImportError:
                    print(f"fontTools isn't installed: {family} is bundled whole")
            local = name + "-Variable.ttf" if "%5B" in remote else remote
            (fonts / local).write_bytes(data)
            faces.append(f"@font-face {{ font-family: '{family}'; src: url('{local}') format('truetype');\n"
                         f"  font-weight: {weights};{f' font-stretch: {widths};' if widths else ''} font-display: swap; }}\n")
        try:
            licence = get(folder + "/OFL.txt")
        except Exception:
            meta = get(folder + "/METADATA.pb").decode("utf-8")
            owner = re.search(r'copyright: "([^"]+)"', meta).group(1)
            body = get("atkinsonhyperlegible/OFL.txt").decode("utf-8")
            licence = (owner + "\n\n" + body[body.index("This Font Software is licensed"):]).encode("utf-8")
        (fonts / (name + "-OFL.txt")).write_bytes(licence)
    (fonts / "fonts.css").write_text("".join(faces))
    font_css = '  <link rel="stylesheet" href="vendor/fonts/fonts.css">'
out_lines = []
for l in lines:
    if font_css and 'href="styles.css"' in l:
        out_lines.append(font_css)
    out_lines.append(l)
index.write_text("\n".join(out_lines) + "\n")

size = lambda p: sum(f.stat().st_size for f in p.rglob("*") if f.is_file())
print(f"ui {size(a.out / 'ui') / 1e6:.1f} MB, site {size(a.out / 'site') / 1e6:.1f} MB, "
      f"starter library {size(a.out / 'starter') / 1e6:.1f} MB, "
      f"libraries {size(a.out / 'libs') / 1e6 if (a.out / 'libs').exists() else 0:.1f} MB -> {a.out}")
