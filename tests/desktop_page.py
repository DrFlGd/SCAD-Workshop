#!/usr/bin/env python3
"""Phase 2 and 3 acceptance test: the desktop app's page against its real backend
(`workshop-cli serve`, the same command table as the Tauri app), in Chromium.

    python3 tests/desktop_page.py --cli desktop/target/release/workshop-cli --ui build/desktop/ui \\
        --app-site build/desktop/site --starter build/desktop/starter --engine build/native/squashfs-root \\
        --home /tmp/home --out shots [--github-api http://127.0.0.1:8795]

Checks: the starter library; adding three public GitHub projects by URL (one
pinned to an old commit, one a library); a local project that includes the
library; search; a render from library files; an upstream change detected and
summarised, accepted, with edits surviving; a project-wide license with one item
keeping its own; a condensed group showing the icon chosen for it; hiding,
flagging as broken, deleting (undo, restore) and moving items to another
project; categories added, removed and brought back; a hidden project; a deleted
project through the trash; Components (Phase 3): the bevel-gear flow (search,
the doc example's values, change teeth and module, preview, download, in under a
minute), pinning a component as a model, the OpenSCAD code, a form edit stored in
the shape of a manifest's ui block, and switching between your BOSL2 and the
app's; 0.3.1: the left menu's category panel, a doc-listed setting as a drop-down,
Edit… in Library settings, long text kept inside its box; 0.4: the side viewer (a
model's code, a project's README, a component's file at its module), settings from
the values in a model's module call (the Gears examples; LEGO.scad's style, borrowing the library's
descriptions and drop-downs); 0.4.1: the Code tab as an editor (Generate uses the
edits, saved as a variant with the settings, the project's file untouched); 0.4.3: Condense with a
selection, the category picker, Scan for changes, a library backup and restore; and the library opening
the same after being moved (library-summary). Writes
<out>/library-summary.json and <out>/library.zip for the cross-platform check.
"""
import argparse
import asyncio
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import time
import urllib.request
import zipfile
import zlib

import model_files
from pathlib import Path

from playwright.async_api import async_playwright

ap = argparse.ArgumentParser()
ap.add_argument("--cli", required=True)
ap.add_argument("--ui", required=True)
ap.add_argument("--app-site", required=True)
ap.add_argument("--starter", required=True)
ap.add_argument("--engine", required=True)
ap.add_argument("--home", required=True)
ap.add_argument("--out", default="shots")
ap.add_argument("--port", type=int, default=8790)
ap.add_argument("--github-api", help="a stand-in for GitHub's API (tests/github_mock.py)")
a = ap.parse_args()
out = Path(a.out)
out.mkdir(parents=True, exist_ok=True)
home = Path(a.home).resolve()
shutil.rmtree(home, ignore_errors=True)
library = home / "library"
B = f"http://127.0.0.1:{a.port}/"
SHIM = Path(__file__).with_name("tauri_shim.js")
OLD = "af95c658fdd11e56bbc826b68ac13022c8bc79e4"  # vector76/gridfinity_openscad, Oct 2022 (newer commits exist)
REPOS = [
    ("vector76-gridfinity", f"https://github.com/vector76/gridfinity_openscad/commit/{OLD}", False),
    ("gears", "https://github.com/chrisspen/gears", False),
    ("bosl2", "https://github.com/BelfrySCAD/BOSL2", True),
]
# anything in the open dialog that reaches past its edges
OVERFLOW = """() => { const d = document.querySelector('.dialog'); if (!d) return ['no dialog']; const r = d.getBoundingClientRect();
  return [...d.querySelectorAll('*')].filter((e) => { const b = e.getBoundingClientRect(); return b.width && (b.right > r.right + 1 || b.left < r.left - 1); })
    .map((e) => e.tagName + '.' + e.className).slice(0, 5); }"""
DONE = "()=>{const s=document.querySelector('#status');return s&&(s.classList.contains('ok')||s.classList.contains('error'))}"
results, errors = [], []
calls = []  # failed calls to the backend, with what they asked for and the check before them


def check(name, ok, detail=""):
    results.append((name, bool(ok)))
    print(f"{'PASS' if ok else 'FAIL'} {name}{' :: ' + str(detail) if detail != '' else ''}", flush=True)


def start_server():
    env = dict(os.environ)
    if a.github_api:
        env["WORKSHOP_GITHUB_API"] = a.github_api
        for k in ("HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy", "ALL_PROXY", "all_proxy"):
            env.pop(k, None)
    log = open(out / "serve.log", "w")
    p = subprocess.Popen([a.cli, "serve", "--ui", a.ui, "--app-site", a.app_site, "--starter", a.starter, "--engine", a.engine,
                          "--home", str(home), "--library", str(library), "--port", str(a.port)], stdout=log, stderr=log, env=env)
    for _ in range(100):
        try:
            urllib.request.urlopen(B, timeout=1)
            return p
        except Exception:
            time.sleep(0.2)
    raise SystemExit("the server didn't start; see serve.log")


async def gen(pg):
    """Generate and wait for it; the dimensions shown."""
    await pg.click("#generate")
    await pg.wait_for_function("()=>!document.querySelector('#generate').disabled", timeout=120000)
    await pg.wait_for_function(DONE, timeout=120000)
    return (await pg.inner_text("#dims")).split("\n")[0]


async def index_items(pg, project):
    """Items of one project as the page's index has them (name, license)."""
    return await pg.evaluate("""(p) => window.__workshop.index.items.filter((i) => i.projectId === p)
        .map((i) => ({ id: i.id, name: i.name, license: i.license?.spdx, kind: i.kind }))""", project)


async def main():
    server = start_server()
    try:
        async with async_playwright() as p:
            b = await p.chromium.launch(args=["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"])
            ctx = await b.new_context(viewport={"width": 1400, "height": 900})
            await ctx.add_init_script(path=str(SHIM))
            pg = await ctx.new_page()
            pg.on("pageerror", lambda e: errors.append(str(e)))
            # (a render that fails answers 500, which the browser logs: section 13b's model that doesn't
            # render at its defaults does so on purpose, and the checks report any other failed render)
            pg.on("console", lambda m: errors.append(f"{m.text} ({m.location.get('url', '')})") if m.type == "error" and "net::ERR_" not in m.text
                  and not ("status of 500" in m.text and m.location.get("url", "").endswith("/invoke/render")) else None)
            pg.on("dialog", lambda d: asyncio.ensure_future(d.accept()))
            pg.on("response", lambda r: calls.append(f"{r.status} {r.url.rsplit('/', 1)[-1]} {(r.request.post_data or '')[:300]} (after: {results[-1][0][:70] if results else 'the start'})")
                  if r.status >= 400 and "/invoke/" in r.url and not (r.status == 500 and r.url.endswith("/invoke/render")) else None)
            await pg.goto(B)
            await pg.wait_for_selector(".home .tile", timeout=120000)
            await pg.wait_for_function("() => !!window.__workshop", timeout=10000)

            # 1. the starter library
            n = await pg.evaluate("() => window.__workshop.index.items.filter((i) => i.kind === 'generator').length")
            parts = await pg.evaluate("() => window.__workshop.index.items.filter((i) => i.kind === 'part').length")
            projects = await pg.evaluate("() => window.__workshop.catalog.sources.length")
            check("starter library installed", n == 58 and parts == 44 and projects == 19, f"{n} generators, {parts} parts, {projects} projects")
            await pg.screenshot(path=str(out / "p2-00-home.png"))
            # the left menu: the three sections; the one being browsed shows its categories in a panel beside it
            sections = await pg.eval_on_selector_all(".sidebar [data-section-row] .nav-label", "els => els.map(e => e.textContent.trim())")
            panel_home = await pg.locator(".catpanel").count()
            await pg.click('.sidebar [data-scope="components"]')
            await pg.wait_for_selector('.catpanel [data-scope^="ccat:"]')
            ccats = await pg.locator('.catpanel [data-scope^="ccat:"]').count()
            scrolls = await pg.eval_on_selector(".catpanel-list", "e => getComputedStyle(e).overflowY")
            await pg.click('.sidebar [data-scope="parts"]')
            await pg.wait_for_selector('.catpanel [data-scope^="pcat:"]')
            pcats = await pg.locator('.catpanel [data-scope^="pcat:"]').count()
            await pg.click(".catpanel .nav-toggle >> nth=0")
            await pg.click('.catpanel [data-scope^="lib:"] >> nth=0')
            await pg.wait_for_function("() => location.hash.includes('/browse/lib/')")
            within = await pg.wait_for_selector('.sidebar [data-scope="parts"].within', timeout=5000) and 1
            await pg.click("#catpanel-toggle")
            await pg.wait_for_selector(".catpanel.folded")
            await pg.click("#catpanel-toggle")
            await pg.wait_for_selector('.catpanel:not(.folded) [data-scope^="pcat:"]')
            check("left menu: Parametric Models, Components, Parts Library; the categories in a panel beside it (projects inside, folds away, scrolls)",
                  sections == ["Parametric Models", "Components", "Parts Library"] and panel_home == 0 and ccats >= 8 and pcats >= 1 and within == 1 and scrolls == "auto",
                  (sections, panel_home, ccats, pcats, within, scrolls))

            # 1b. (0.4.5) skins: Drafting Sheet unless chosen; Settings offers all four; each one in light and dark
            await pg.goto(B + "#/settings")
            await pg.wait_for_selector("#skin-pick input")
            default_skin = await pg.evaluate("document.documentElement.dataset.skin")
            offered = await pg.eval_on_selector_all("#skin-pick input", "els => els.map((e) => e.value)")
            first_gen = await pg.evaluate("() => window.__workshop.index.items.find((i) => i.kind === 'generator' && !i.hidden).id.replace(/^gen:/, '')")
            applied = []
            for skin in offered:
                for theme in ("light", "dark"):
                    await pg.goto(B + "#/settings")
                    await pg.wait_for_selector("#skin-pick input")
                    await pg.check(f'#skin-pick input[value="{skin}"]')
                    await pg.check(f'#theme-pick input[value="{theme}"]')
                    applied.append(await pg.evaluate("[document.documentElement.dataset.skin, document.documentElement.dataset.theme].join('/')"))
                    for page, target in (("home", "#/"), ("browse", "#/browse/all"), ("model", f"#/m/{first_gen}")):
                        await pg.goto(B + target)
                        await pg.wait_for_selector({"home": ".home .tile", "browse": ".results [data-item]", "model": "#generate"}[page])
                        await asyncio.sleep(1.5)
                        await pg.screenshot(path=str(out / f"p5-skin-{skin}-{theme}-{page}.png"))
            await pg.goto(B + "#/settings")
            await pg.wait_for_selector("#skin-pick input")
            await pg.check('#skin-pick input[value="partsbin"]')
            await pg.check('#theme-pick input[value="system"]')
            await asyncio.sleep(1)   # preferences are written a moment after a change
            await pg.reload()
            await pg.wait_for_function("() => !!window.__workshop")
            kept = await pg.evaluate("document.documentElement.dataset.skin")
            await pg.wait_for_selector("#skin-pick input")
            await pg.check('#skin-pick input[value="drafting"]')
            fonts_ok = True
            if (Path(a.ui) / "vendor/fonts/fonts.css").is_file():   # bundled by build_desktop.py --fetch-fonts
                # (at every weight in use, for letters, not just "·": Review 2's UI-13, where a "·"-only face
                # with no weight range took over regular text)
                fonts_ok = await pg.evaluate("""async () => (await Promise.all(['Overpass', 'Overpass Mono', 'Sofia Sans', 'Atkinson Hyperlegible', 'M PLUS Rounded 1c']
                    .flatMap((f) => [400, 600, 700].map((w) => document.fonts.load(`${w} 14px "${f}"`, 'Gridfinity 28')
                      .then((l) => l.some((x) => !/00B7/i.test(x.unicodeRange))))))).every(Boolean)""")
            check("skins: Drafting Sheet by default; Settings offers four, each applies in light and dark and is kept; their fonts are bundled",
                  default_skin == "drafting" and offered == ["drafting", "partsbin", "layerline", "classic"] and applied == [f"{s}/{t}" for s in offered for t in ("light", "dark")]
                  and kept == "partsbin" and fonts_ok, (default_skin, offered, applied, kept, fonts_ok))
            # (0.4.21) the logo: the sheet with the cube, in the skin's colour; the favicon is its small form
            logo = await pg.evaluate("""async () => { const m = document.querySelector('.brand .brand-mark');
                return { cube: !!m?.querySelector('.cube'), face: getComputedStyle(m.querySelector('.face')).fill,
                         favicon: /M126 77/.test(await (await fetch('favicon.svg')).text()) }; }""")
            check("the logo is in the sidebar, its shaded face in the skin's colour, and is the favicon",
                  logo["cube"] and logo["face"] not in ("none", "rgb(0, 0, 0)") and logo["favicon"], logo)
            await pg.goto(B + "#/")

            # 2. three public projects by URL
            ids = {}
            add_toasts = []
            for short, url, lib in REPOS:
                await pg.click("#add-project")
                await pg.fill("#add-url", url)
                if "/commit/" in url:   # (0.4.9) a link to a commit asks what updates follow; the branch by default
                    follow_pick = await pg.eval_on_selector_all("#add-follow input", "els => els.map((e) => [e.value, e.checked])")
                if lib:
                    await pg.check("#add-library")
                await pg.click("#add-go")
                await pg.wait_for_function("() => location.hash.includes('/source/')", timeout=600000)
                await pg.wait_for_selector(".source-panel")
                src = (await pg.evaluate("location.hash")).split("/source/")[1]
                ids[short] = src
                add_toasts.append(await pg.locator("#toast").text_content() or "")
                items = await index_items(pg, src)
                ok = (len(items) == 0) if lib else (len([i for i in items if i["kind"] == "generator"]) > 0)
                check(f"added {url.split('github.com/')[1]}", ok, f"{src}: {len(items)} items")
                await pg.goto(B + "#/")
            await pg.screenshot(path=str(out / "p2-01-added.png"))
            v76_origin = json.loads((library / f"sources/{ids['vector76-gridfinity']}/source.json").read_text())["origin"]
            check("a link to a commit asks whether to follow new releases or the branch; the project records what it follows and the commit it was added from",
                  follow_pick == [["releases", False], ["branch", True]] and v76_origin.get("track", "").startswith("branch:")
                  and v76_origin.get("added_from") == OLD and "requested" not in v76_origin, (follow_pick, v76_origin))
            check("the messages say models, and that a library's modules are in Components (not generators)",
                  not any("generator" in t for t in add_toasts) and any("a library, its modules are in Components" in t for t in add_toasts)
                  and any(" model" in t for t in add_toasts), add_toasts)

            # 3. a project of your own that includes the library (<BOSL2/...>)
            (library / "local/bevel").mkdir(parents=True, exist_ok=True)
            bevel_src = (
                "include <BOSL2/std.scad>\ninclude <BOSL2/gears.scad>\n\n// Number of teeth\nteeth = 20; // [8:60]\n"
                "// Tooth size (module)\nmod = 2; // [0.5:0.5:5]\n\nbevel_gear(mod=mod, teeth=teeth, mate_teeth=teeth, face_width=10);\n")
            (library / "local/bevel/bevel_gear.scad").write_text(bevel_src)
            # and one in LEGO.scad's style: Customizer variables passed to the module by the library's own
            # call, and an example whose values (a fraction among them) are typed into its call
            (library / "local/bricks/examples").mkdir(parents=True, exist_ok=True)
            (library / "local/bricks/brick.scad").write_text(
                "/* [General] */\n\n// Width of the block, in studs\nblock_width = 2;\n\n// Length of the block, in studs\nblock_length = 4;\n\n"
                "// Height of the block: 1/3 is a plate, 1 a brick\nblock_height_ratio = 1; // [.33333333333:1/3, .5:1/2, 1:1, 2:2]\n\n"
                "// What type of block should this be?\nblock_type = \"brick\"; // [brick:Brick, tile:Tile]\n\n"
                "block(width=block_width, length=block_length, height=block_height_ratio, type=block_type);\n\n"
                "module block(width=1, length=2, height=1, type=\"brick\") {\n    cube([width * 8 - 0.2, length * 8 - 0.2, height * 9.6]);\n"
                "    if (type == \"brick\")\n        for (x = [0:width - 1], y = [0:length - 1])\n"
                "            translate([x * 8 + 4, y * 8 + 4, height * 9.6]) cylinder(d=4.8, h=1.8, $fn=24);\n}\n")
            (library / "local/bricks/examples/plate.scad").write_text(
                "use <../brick.scad>;\n\nblock(\n    width=2,\n    length=4,\n    height=1/3,\n    type=\"tile\"\n);\n")
            await pg.reload()
            await pg.wait_for_function("() => window.__workshop?.index.get('gen:local-bevel/bevel-gear') && window.__workshop?.index.get('gen:local-bricks/examples-plate')", timeout=180000)
            check("local project read (includes BOSL2 from the library)", True)

            # 4. search finds the new generators
            await pg.fill("#global-search", "bevel gear")
            await pg.wait_for_function("()=>/ms$/.test(document.querySelector('.browse-count')?.textContent || '')")
            top = await pg.eval_on_selector_all('.results [data-item^="gen:"]', "els=>els.slice(0,5).map(e=>e.dataset.item)")
            check("search 'bevel gear' finds the new generators", any("local-bevel" in t for t in top) and any(ids["gears"] in t for t in top), top)
            await pg.fill("#global-search", "silverware")
            await pg.wait_for_function("()=>/ms$/.test(document.querySelector('.browse-count')?.textContent || '')")
            top = await pg.eval_on_selector_all(".results [data-item]", "els=>els.map(e=>e.dataset.item)")
            check("search 'silverware' finds the added project's too", any(ids["vector76-gridfinity"] in t for t in top), top)
            await pg.screenshot(path=str(out / "p2-02-search.png"))

            # 5. render from library files
            await pg.goto(B + "#/m/local-bevel/bevel-gear")
            await pg.wait_for_function("()=>document.body.dataset.model==='local-bevel/bevel-gear'")
            await pg.wait_for_function(DONE, timeout=300000)
            status = await pg.inner_text("#status")
            check("a model that includes BOSL2 renders", "matches" in status, status)
            await pg.screenshot(path=str(out / "p2-03-bevel.png"))

            # 5b. Components: the libraries' modules as forms (your BOSL2, added above, wins over the app's)
            comps = await pg.evaluate("() => window.__workshop.index.items.filter((i) => i.kind === 'component').length")
            libs = await pg.evaluate("() => window.__workshop.catalog.component_libraries.map((l) => [l.name, l.provider, l.active])")
            check("Components: modules of the bundled libraries, your BOSL2 in use", comps > 700 and ["BOSL2", "project", True] in libs
                  and ["BOSL2", "bundled", False] in libs and ["NopSCADlib", "bundled", True] in libs, (comps, libs))
            # the bevel-gear flow: search, open the first result (a doc example's values), change teeth and module, preview, download
            t0 = time.time()
            await pg.goto(B + "#/")
            await pg.fill("#global-search", "bevel gear")
            await pg.wait_for_function("()=>/ms$/.test(document.querySelector('.browse-count')?.textContent || '')")
            first = await pg.eval_on_selector(".results [data-item]", "e => e.dataset.item")
            check("search 'bevel gear': BOSL2 bevel_gear() is the first result", first == "comp:@bosl2/bevel_gear", first)
            await pg.dblclick(f'.results [data-item="{first}"]')
            await pg.wait_for_function("()=>document.body.dataset.model==='@bosl2/bevel_gear'")
            await pg.wait_for_function(DONE, timeout=120000)
            start = {k: await pg.input_value(f"#p-{k}") for k in ("teeth", "mate_teeth", "circ_pitch")}
            # (0.4.6) the page is titled with the component's own name, its library under it; settings are
            # named as on model forms, with the code name on hover (UI-4, UI-5)
            head = [await pg.inner_text("#model-title"), await pg.inner_text("#model-project")]
            mate = [await pg.inner_text("label[for='p-mate_teeth']"), await pg.get_attribute("label[for='p-mate_teeth']", "title") or ""]
            check("component page: its own name with its library under it; settings named like a model's, the code name on hover",
                  head[0] == "Bevel gear" and head[1] == "BOSL2 · bevel_gear() · your copy"
                  and mate[0] == "Mate teeth" and "In the code: mate_teeth" in mate[1], (head, mate))
            await pg.fill("#p-teeth", "24")
            await pg.fill("#p-circ_pitch", "")
            await pg.fill("#p-mod", "2")
            await pg.click("#generate")
            await pg.wait_for_function("()=>!document.querySelector('#generate').disabled", timeout=120000)
            await pg.wait_for_function(DONE, timeout=120000)
            status = await pg.inner_text("#status")
            await pg.click("#download")
            saved_stl = home / "saved-files"
            for _ in range(50):
                if saved_stl.is_dir() and any(saved_stl.glob("*.stl")):
                    break
                await pg.wait_for_timeout(200)
            took = time.time() - t0
            stl = next(iter(saved_stl.glob("*.stl")), None) if saved_stl.is_dir() else None
            check("bevel gear: form from the docs' example, teeth and module changed, preview, download, under a minute",
                  start == {"teeth": "36", "mate_teeth": "36", "circ_pitch": "5"} and "matches" in status and stl and stl.stat().st_size > 10000 and took < 60,
                  (start, status, stl.name if stl else None, round(took, 1)))
            await pg.screenshot(path=str(out / "p3-01-bevel-component.png"))
            code = await pg.evaluate("() => window.__workshop.platform.api('component_code', { key: '@bosl2/bevel_gear', values: { teeth: 24, mod: 2, mate_teeth: 36 } })")
            check("the component's OpenSCAD code", "include <BOSL2/gears.scad>" in code and "bevel_gear(teeth=24, mate_teeth=36" in code and "mod=2" in code, code.replace("\n", " | "))
            # pin it as a model (the settings it has now become its defaults)
            await pg.click("#model-more summary")
            await pg.click("#pin-component")
            await pg.wait_for_selector(".pin-dialog #pin-name")
            await pg.fill("#pin-name", "Bevel gear 24T")
            await pg.click("#pin-go")
            await pg.wait_for_function("() => location.hash === '#/m/pinned/bevel-gear-24t'", timeout=60000)
            # (the page's index is reloaded after the pin; the status can still be the component's own until then)
            await pg.wait_for_function("() => !!window.__workshop.index.get('gen:pinned/bevel-gear-24t')", timeout=60000)
            await pg.wait_for_function("()=>document.body.dataset.model==='pinned/bevel-gear-24t'", timeout=60000)
            await pg.wait_for_function(DONE, timeout=120000)
            pin = await pg.evaluate("() => { const i = window.__workshop.index.get('gen:pinned/bevel-gear-24t'); return i && [i.kind, i.category, i.project]; }")
            pinned_teeth = await pg.input_value("#p-teeth")
            check("pinned as a model under Parametric Models, with its settings", pin == ["generator", "mechanical", "Pinned components"] and pinned_teeth == "24"
                  and (library / "sources/pinned/pins/bevel-gear-24t.json").is_file(), (pin, pinned_teeth))
            # the form editor: stored in the shape of a family manifest's model entry (under ⋯ since 0.4.9)
            await pg.click("#model-more summary")
            await pg.click("#edit-form")
            await pg.wait_for_selector(".form-editor")
            await pg.click("[data-fe=teeth] .fe-name")
            # (0.4.23) each field starts with what the file has, as text you can change
            prefilled = await pg.evaluate("() => ['label', 'default', 'group'].map((k) => document.querySelector(`[data-form='teeth.${k}']`)?.value || '')")
            await pg.fill("[data-form='teeth.label']", "Tooth count")
            await pg.fill("[data-form='teeth.min']", "8")
            await pg.click("[data-fe=mate_teeth] .fe-name")
            await pg.fill("[data-form='mate_teeth.when']", "teeth > 10")
            await pg.click("[data-form='spiral.show']")
            await pg.screenshot(path=str(out / "p3-02-form-editor.png"))
            await pg.click("#fe-save")
            await pg.wait_for_selector(".form-editor", state="detached")
            await pg.wait_for_function("() => document.querySelector('[data-name=teeth] label')?.textContent === 'Tooth count'", timeout=60000)
            stored = json.loads((library / "sources/pinned/metadata.json").read_text())["items"]["bevel-gear-24t"]["form"]
            want = {"ui": {"teeth": {"label": "Tooth count", "min": 8}, "mate_teeth": {"display-condition": {"js": "teeth > 10"}}}, "hidden": ["spiral"]}
            spiral_gone = await pg.evaluate("() => !document.querySelector('[data-name=spiral]')")
            check("form edit stored as a manifest ui block, and applied", stored == want and spiral_gone, stored)
            check("the form editor's fields start with the values read from the file, and only changed ones are kept",
                  all(prefilled) and prefilled[1] != "" and stored == want, prefilled)
            # your BOSL2 or the app's: switch, projects that include it are read again, and back
            await pg.goto(B + "#/library-settings")
            await pg.click('#ls-libraries [data-library="BOSL2"][data-provider="bundled"] [data-act=prefer]')
            await pg.wait_for_selector('#ls-libraries [data-library="BOSL2"][data-provider="bundled"][data-active]', timeout=300000)
            # (the row can show the switch before its projects are read again, when something else reloads the
            # catalog first: wait for the switch's own message)
            try:
                await pg.wait_for_function("() => (document.querySelector('#toast')?.textContent || '').includes('comes with the app is used now')", timeout=120000)
            except Exception:
                pass
            switched = await pg.evaluate("""() => [window.__workshop.catalog.components.find((c) => c.key === '@bosl2/bevel_gear')?.provider,
                window.__workshop.catalog.attention.filter((a) => a.kind === 'libraries').map((a) => a.message)]""")
            switched[1] = switched[1] or 0   # (the messages, when there are any)
            # (0.4.9) the pin made with your BOSL2 records that copy, and is marked while the app's is used
            pin_from = json.loads((library / "sources/pinned/pins/bevel-gear-24t.json").read_text()).get("from", {})
            pin_marked = await pg.evaluate("() => (window.__workshop.catalog.sources.find((s) => s.id === 'pinned')?.problems || []).some((p) => p.kind === 'pin_library')")
            await pg.click('#ls-libraries [data-library="BOSL2"][data-provider="project"] [data-act=prefer]')
            await pg.wait_for_selector('#ls-libraries [data-library="BOSL2"][data-provider="project"][data-active]', timeout=300000)
            try:
                await pg.wait_for_function("() => (document.querySelector('#toast')?.textContent || '').includes('your copy is used now')", timeout=120000)
            except Exception:
                pass
            back = await pg.evaluate("() => window.__workshop.catalog.components.find((c) => c.key === '@bosl2/bevel_gear')?.provider")
            prefer = json.loads((library / "library.json").read_text()).get("prefer_bundled")
            pin_back = await pg.evaluate("() => (window.__workshop.catalog.sources.find((s) => s.id === 'pinned')?.problems || []).some((p) => p.kind === 'pin_library')")
            check("switch between your BOSL2 and the app's (projects read again); a pin records the copy it was made with and is marked while another is used",
                  switched == ["bundled", 0] and back == "project" and not prefer
                  and pin_from.get("copy", "").startswith("github-belfryscad-bosl2@") and pin_marked and not pin_back, (switched, back, prefer, pin_from, pin_marked, pin_back))

            # 5c. a setting whose doc lists its values is a drop-down (BOSL2 spheroid's style), and renders with the choice
            await pg.goto(B + "#/m/@bosl2/spheroid")
            await pg.wait_for_function("()=>document.body.dataset.model==='@bosl2/spheroid'")
            await pg.wait_for_function(DONE, timeout=120000)
            tag = await pg.eval_on_selector("#p-style", "e => e.tagName")
            opts = await pg.eval_on_selector_all("#p-style option", "els => els.map(e => e.textContent)")
            await pg.select_option("#p-style", label="icosa")
            await pg.click("#generate")
            await pg.wait_for_function("()=>!document.querySelector('#generate').disabled", timeout=120000)
            await pg.wait_for_function(DONE, timeout=120000)
            status = await pg.inner_text("#status")
            check("a setting whose doc lists its values is a drop-down (spheroid style), and renders with the choice",
                  tag == "SELECT" and opts == ["orig", "aligned", "stagger", "octa", "icosa"] and "matches" in status, (tag, opts, status))

            # 5d. the side viewer (Phase 4): a model's code and its project's README (HTML kept in the
            # library, pictures from the project's own files), a component's file at its module, the
            # app's README for a built-in model, a project file from its Files tab, folding away
            gears = ids["gears"]
            await pg.goto(B + f"#/m/{gears}/examples-bevel-gear")
            await pg.wait_for_function(f"()=>document.body.dataset.model==='{gears}/examples-bevel-gear'")
            # (its values are typed into a module call, bevel_gear(modul=1, tooth_number=30, ...): they become settings)
            await pg.wait_for_function(DONE, timeout=120000)
            lifted = await pg.eval_on_selector_all("#params .field[data-name]", "els => els.map(e => e.dataset.name)")
            dims0 = await pg.inner_text("#dims")
            await pg.fill("#p-tooth_number", "18")
            await pg.click("#generate")
            await pg.wait_for_function("()=>!document.querySelector('#generate').disabled", timeout=120000)
            await pg.wait_for_function(DONE, timeout=120000)
            status = await pg.inner_text("#status")
            dims1 = await pg.inner_text("#dims")
            check("a model whose values are typed into its module call has them as settings, and renders with a change",
                  {"modul", "tooth_number", "partial_cone_angle", "bore"} <= set(lifted) and "matches" in status and dims0 != dims1,
                  (lifted, status, dims0.split("\n")[0], dims1.split("\n")[0]))
            # (in LEGO.scad's style, the settings borrow the library's descriptions and drop-downs, and 1/3 is
            # the library's own 1/3)
            await pg.goto(B + "#/m/local-bricks/examples-plate")
            await pg.wait_for_function("()=>document.body.dataset.model==='local-bricks/examples-plate'")
            await pg.wait_for_function(DONE, timeout=120000)
            fields = await pg.eval_on_selector_all("#params .field[data-name]", """els => Object.fromEntries(els.map((e) => {
                const s = e.querySelector('select'); const l = e.querySelector('label');
                return [e.dataset.name, [s ? s.options[s.selectedIndex].text : e.querySelector('input')?.type, l?.title || '']]; }))""")
            dims0 = await pg.inner_text("#dims")
            await pg.select_option("#p-height", label="1")
            await pg.click("#generate")
            await pg.wait_for_function("()=>!document.querySelector('#generate').disabled", timeout=120000)
            await pg.wait_for_function(DONE, timeout=120000)
            dims1 = await pg.inner_text("#dims")
            check("settings from a module call borrow the library's descriptions and choices (a fraction is the library's 1/3)",
                  fields.get("width", [None])[0] == "number" and fields["width"][1].startswith("Width of the block, in studs") and fields.get("height", [None])[0] == "1/3"
                  and fields.get("type", [None])[0] == "Tile" and "15.8 × 31.8 × 3.2" in dims0 and "× 9.6" in dims1.split("\n")[0],
                  (fields, dims0.split("\n")[0], dims1.split("\n")[0]))
            # 5e. the Code tab edits the model (0.4.1): Generate uses the edits, Save keeps them with the
            # settings as a variant (not a copy of the model), Defaults goes back to the original code, and
            # the project's file is never written
            plate = (library / "local/bricks/examples/plate.scad").read_text()
            await pg.click("#model-extra [data-side-open=code]")
            await pg.wait_for_selector("#side-editor")
            await pg.wait_for_function("() => document.querySelector('#side-editor').value.includes('width = 2;')")
            original = await pg.input_value("#side-editor")
            await pg.fill("#side-editor", original.replace("width = 2;", "width = 3;"))
            stale = await pg.inner_text("#status")
            dims_edit = await gen(pg)
            await pg.click("#side-save")
            await pg.wait_for_selector("#save-dialog[open]")
            await pg.fill("#save-name", "Three wide")
            await pg.click("#save-primary")
            await pg.wait_for_selector("#save-dialog", state="hidden")
            recs = [json.loads(f.read_text()) for f in (library / "recipes").glob("*.json")]
            rec = next((r for r in recs if r.get("model") == "local-bricks/examples-plate" and r.get("name") == "Three wide"), {})
            stored = rec.get("edits") or {}
            entry_edit = stored.get("/examples/plate.scad") or {}
            await pg.select_option("#settings-pick", "default")
            await pg.wait_for_function("() => !document.querySelector('#side-editor').value.includes('width = 3;')")
            await pg.wait_for_function(DONE, timeout=120000)
            dims_default = (await pg.inner_text("#dims")).split("\n")[0]
            await pg.select_option("#settings-pick", f"saved:{rec.get('id')}")
            await pg.wait_for_function("() => document.querySelector('#side-editor').value.includes('width = 3;')")
            await pg.wait_for_function(DONE, timeout=120000)
            dims_variant = (await pg.inner_text("#dims")).split("\n")[0]
            listed = await pg.eval_on_selector_all("#settings-pick option", "els => els.map((e) => e.textContent)")
            items_after = [i for i in await index_items(pg, "local-bricks") if "plate" in i["id"]]
            bricks_version = await pg.evaluate("() => window.__workshop.catalog.sources.find((s) => s.id === 'local-bricks').version")
            check("the Code tab edits a model: Generate uses the edits; saved as a variant with the settings; Defaults is the original code; the file is untouched",
                  "Code changed" in stale and "23.8 × 31.8 × 9.6" in dims_edit and "width = 3;" in entry_edit.get("text", "")
                  and len(entry_edit.get("base") or "") == 64 and rec.get("values") == {"height": 1}
                  # (0.4.8) the project version it was saved with (project_version since 0.4.22); its edit based on the project's file, not the app's copy
                  and rec.get("project_version") == bricks_version and entry_edit.get("base") == hashlib.sha256(plate.encode()).hexdigest() and len(entry_edit.get("copy") or "") == 64
                  and "15.8 × 31.8 × 3.2" in dims_default and "23.8 × 31.8 × 9.6" in dims_variant
                  and "Three wide · code edited" in listed and len(items_after) == 1
                  and (library / "local/bricks/examples/plate.scad").read_text() == plate,
                  (stale, dims_edit, dims_default, dims_variant, rec.get("values"), list(stored), listed, len(items_after)))
            await pg.screenshot(path=str(out / "p4-03-code-edited.png"))
            await pg.select_option("#settings-pick", "default")
            await pg.wait_for_function(DONE, timeout=120000)
            await pg.goto(B + f"#/m/{gears}/examples-bevel-gear")
            await pg.wait_for_function(f"()=>document.body.dataset.model==='{gears}/examples-bevel-gear'")
            await pg.wait_for_function(DONE, timeout=120000)
            await pg.click("#model-extra [data-side-open=code]")
            await pg.wait_for_selector(".side-panel[data-side-panel=code] .code .cl")
            code = await pg.inner_text(".side-panel .code")
            await pg.click("[data-side-tab=readme]")
            await pg.wait_for_selector(".side-panel[data-side-panel=readme] .doc-frame")
            await pg.wait_for_function("() => document.querySelector('.doc-frame')?.contentDocument?.querySelector('h1')")
            h1 = await pg.frame_locator(".doc-frame").locator("h1").first.inner_text()
            await pg.wait_for_function("""() => [...(document.querySelector('.doc-frame').contentDocument.images || [])]
                .some((i) => i.src.includes('/library/') && i.complete && i.naturalWidth > 0)""", timeout=15000)
            stored = any((library / f"sources/{gears}/docs").rglob("readme.html"))
            await pg.screenshot(path=str(out / "p4-01-readme.png"))
            # (0.4.8) the 3D view's size and view buttons stay clear of the open side viewer
            hud = await pg.evaluate("""() => { const p = document.querySelector('.side-panel').getBoundingClientRect().left;
                return ['.hud-dims', '.hud-tools'].map((s) => Math.round(document.querySelector(s).getBoundingClientRect().right - p)); }""")
            check("side viewer: a model's code, its project's README as HTML kept in the library, pictures from its files; the 3D view's size bar stays clear of it",
                  "include" in code and "tooth_number = 30" in code and "Gear" in h1 and stored and all(x <= 1 for x in hud), (code[:60], h1, stored, hud))
            await pg.goto(B + "#/m/@bosl2/bevel_gear")
            await pg.wait_for_function("()=>document.body.dataset.model==='@bosl2/bevel_gear'")
            await pg.wait_for_selector(".side-panel[data-side-panel=readme] .doc-frame")
            await pg.wait_for_function("() => /BOSL2/.test(document.querySelector('.doc-frame')?.contentDocument?.querySelector('h1')?.textContent || '')")
            await pg.click("[data-side-tab=code]")
            await pg.wait_for_selector(".side-panel[data-side-panel=code] .code .cl")
            generated = await pg.inner_text(".side-panel .code")
            await pg.select_option("#side-file", index=1)
            await pg.wait_for_selector(".side-panel .code .cl.mark")
            marked = await pg.inner_text(".side-panel .code .cl.mark")
            await pg.screenshot(path=str(out / "p4-02-component-code.png"))
            await pg.goto(B + "#/m/gridfinity-extended/bin")
            await pg.wait_for_function("()=>document.body.dataset.model==='gridfinity-extended/bin'")
            await pg.click("[data-side-tab=readme]")
            await pg.wait_for_function("() => /Gridfinity Extended/.test(document.querySelector('.doc-frame')?.contentDocument?.querySelector('h1')?.textContent || '')")
            await pg.click("[data-side-tab=readme]")
            await pg.wait_for_selector(".side-panel", state="detached")
            await pg.goto(B + f"#/browse/source/{gears}")
            await pg.wait_for_selector(".source-panel")
            await pg.click("[data-source-tab=files]")
            await pg.click("[data-code-file] >> nth=0")
            await pg.wait_for_selector(".side-panel[data-side-panel=code] .code .cl")
            from_files = await pg.inner_text(".side-head")
            await pg.click(".side-close")
            await pg.wait_for_selector(".side-panel", state="detached")
            check("side viewer: a component's generated code and its module's file, the BOSL2 and Gridfinity Extended READMEs, a project file",
                  "bevel_gear(" in generated and "module bevel_gear(" in marked and ".scad" in from_files, (generated[:80], marked[:60], from_files))

            # 6. upstream change: edit first, then detect, summarise, accept; edits survive
            v76 = ids["vector76-gridfinity"]
            await pg.goto(B + f"#/browse/source/{v76}")
            await pg.wait_for_selector(".source-panel")
            await pg.click("#source-more summary")
            await pg.click(".source-actions [data-act=edit]")
            await pg.fill("#meta-license", "CC-BY-4.0")
            await pg.click("#meta-save")
            await pg.wait_for_selector(".dialog", state="detached")
            await pg.wait_for_timeout(500)
            cup = f"gen:{v76}/gridfinity-basic-cup"
            await pg.click(f'.results [data-item="{cup}"]')
            await pg.click(".inspector [data-act=edit]")
            await pg.fill("#meta-name", "My cup")
            await pg.click("#meta-save")
            await pg.wait_for_selector(".dialog", state="detached")
            # (no other background work: the daily check may still be reading the new version)
            await pg.wait_for_selector(".status-idle", timeout=600000)
            await pg.click("[data-act=check]")
            await pg.wait_for_selector(".update-banner", timeout=600000)
            # (the banner can be there already, from the daily check: wait for this check to finish)
            await pg.wait_for_function("() => document.querySelector('[data-act=check]')?.textContent === 'Check for updates'", timeout=600000)
            await pg.wait_for_selector(".status-idle", timeout=600000)
            banner = await pg.inner_text(".update-banner")
            check("upstream change detected and summarised", "Update ready" in banner, banner.replace("\n", " | ")[:300])
            await pg.screenshot(path=str(out / "p2-04-update.png"))
            before = await pg.evaluate(f"() => window.__workshop.catalog.sources.find((s) => s.id === '{v76}').version")
            t_apply = time.time()
            await pg.click("[data-update=apply]")
            try:
                await pg.wait_for_selector(".update-banner", state="detached", timeout=60000)
            except Exception:
                toasts = await pg.eval_on_selector_all(".toast", "els => els.map(e => e.textContent)")
                raise SystemExit(f"the update wasn't applied; the page said: {toasts}")
            after = await pg.evaluate(f"() => window.__workshop.catalog.sources.find((s) => s.id === '{v76}').version")
            items = {i["id"]: i for i in await index_items(pg, v76)}
            check("update accepted", before != after, f"{before} -> {after}")
            check("edits survive the update", items.get(cup, {}).get("name") == "My cup" and items[cup]["license"] == "CC-BY-4.0",
                  items.get(cup))
            # (0.4.8) the new version's models are rendered at their defaults again, with new thumbnails
            n_models = len([i for i in items.values() if i["kind"] == "generator"])
            checks_file, rechecked = library / f"sources/{v76}/checks.json", {}
            for _ in range(600):
                rechecked = json.loads(checks_file.read_text()) if checks_file.is_file() else {}
                # (a thumbnail can be written just after its model's check)
                fresh = [m for m, r in rechecked.get("models", {}).items() if r.get("ok")
                         and (library / f"sources/{v76}/thumbs/{m}.webp").is_file() and (library / f"sources/{v76}/thumbs/{m}.webp").stat().st_mtime >= t_apply]
                if rechecked.get("version") == after and len(rechecked.get("models", {})) >= n_models \
                        and len(fresh) == len([r for r in rechecked["models"].values() if r.get("ok")]):
                    break
                await pg.wait_for_timeout(1000)
            check("after an update its models are rendered at their defaults again, and their thumbnails remade",
                  rechecked.get("version") == after and len(rechecked.get("models", {})) >= n_models
                  and len(fresh) == len([r for r in rechecked.get("models", {}).values() if r.get("ok")]),
                  (rechecked.get("version"), after, len(rechecked.get("models", {})), n_models, len(fresh)))

            # 7. project-wide license; one item keeps its own
            other = f"gen:{v76}/gridfinity-baseplate"
            await pg.click(f'.results [data-item="{other}"]')
            await pg.click(".inspector [data-act=edit]")
            await pg.fill("#meta-license", "MIT")
            await pg.click("#meta-save")
            await pg.wait_for_selector(".dialog", state="detached")
            await pg.click("#source-more summary")
            await pg.click(".source-actions [data-act=edit]")
            await pg.fill("#meta-license", "CC-BY-SA-4.0")
            await pg.click("#meta-save")
            await pg.wait_for_selector(".dialog", state="detached")
            await pg.wait_for_function(f"() => window.__workshop.index.get('{cup}')?.license?.spdx === 'CC-BY-SA-4.0'", timeout=20000)
            items = await index_items(pg, v76)
            gens = [i for i in items if i["kind"] == "generator"]
            ok = all(i["license"] == ("MIT" if i["id"] == other else "CC-BY-SA-4.0") for i in gens)
            check("project license applies to all items but the one with its own", ok, [(i["id"].split("/")[-1], i["license"]) for i in gens])

            # 8. a condensed group shows the picture chosen for its project (Choose picture… since 0.4.26)
            await pg.goto(B + "#/browse/all")
            await pg.wait_for_selector(".results [data-item]")
            await pg.select_option("select[aria-label=Group]", "project")
            await pg.click("[data-condense]")
            name = await pg.evaluate(f"() => window.__workshop.catalog.sources.find((s) => s.id === '{v76}').name")
            await pg.locator(".group-tile", has_text=name).first.click()
            await pg.click("#source-more summary")
            await pg.click("[data-act=picture]")
            await pg.click('.pic-tile[data-pic^="item:"]')
            await pg.wait_for_function(f"() => !!window.__workshop.catalog.sources.find((s) => s.id === '{v76}').icon", timeout=20000)
            await pg.click(".group-crumb .link-btn")
            tile = pg.locator(".group-tile", has_text=name).first
            await tile.wait_for()
            try:
                await tile.locator(".thumb > img").first.wait_for(timeout=10000)
            except Exception:
                pass
            check("condensed group shows its chosen icon", await tile.locator(".thumb > img").count() == 1)
            await pg.screenshot(path=str(out / "p2-05-condensed.png"))
            await pg.click("[data-condense]")
            await pg.select_option("select[aria-label=Group]", "none")

            # 8b. Condense with a selection (owner's report, 0.4.3): turning it on clears the selection and
            # groups by project; inside a group, arrow keys and Ctrl+A stay in it; turning it off goes back
            # to no grouping and keeps what was selected
            await pg.wait_for_selector(".results [data-item]")
            sel_q = "() => [...document.querySelectorAll('.results [data-item][aria-selected=true]')].map((e) => e.dataset.item)"
            first_ids = await pg.eval_on_selector_all(".results [data-item]", "els => els.slice(0, 3).map((e) => e.dataset.item)")
            await pg.click(f'.results [data-item="{first_ids[0]}"]')
            await pg.click(f'.results [data-item="{first_ids[2]}"]', modifiers=["Shift"])
            ranged = await pg.evaluate(sel_q)
            await pg.click("[data-condense]")
            await pg.wait_for_selector(".group-tile")
            on_group = await pg.eval_on_selector("select[aria-label=Group]", "e => e.value")
            on_empty = await pg.locator(".inspector").count() == 0  # (shown only with a selection, 0.4.26)
            # the biggest group, so arrow keys have room to run past it
            await pg.evaluate("""() => { const t = [...document.querySelectorAll('.group-tile[data-group]')].sort((a, b) => parseInt(b.querySelector('.card-sub').textContent) - parseInt(a.querySelector('.card-sub').textContent))[0]; t.click(); }""")
            await pg.wait_for_selector(".group-crumb")
            in_group = await pg.eval_on_selector_all(".results [data-item]", "els => els.map((e) => e.dataset.item)")
            await pg.click(f'.results [data-item="{in_group[0]}"]')
            for _ in range(len(in_group) + 5):
                await pg.keyboard.press("ArrowRight")
            walked = await pg.evaluate(sel_q)
            await pg.keyboard.press("Control+a")
            all_sel = await pg.evaluate(sel_q)
            keep = in_group[min(1, len(in_group) - 1)]
            await pg.click(f'.results [data-item="{keep}"]')
            await pg.click("[data-condense]")
            await pg.wait_for_selector(".results [data-item]")
            off_group = await pg.eval_on_selector("select[aria-label=Group]", "e => e.value")
            kept = await pg.evaluate(sel_q)
            insp = await pg.locator(".inspector .insp-title").count() == 1
            check("Condense: on clears the selection and groups by project; arrow keys and Ctrl+A stay in an open group; off goes back to no grouping, keeping the selection",
                  len(ranged) == 3 and on_group == "project" and on_empty and walked == [in_group[-1]] and sorted(all_sel) == sorted(in_group)
                  and off_group == "none" and kept == [keep] and insp,
                  (len(ranged), on_group, on_empty, walked, in_group[-1], len(all_sel), len(in_group), off_group, kept, insp))
            await pg.screenshot(path=str(out / "p4-04-condense-off.png"))

            # 9. hide, flag as broken, delete and list under another project (with undo and restore)
            gens = sorted(i["id"] for i in await index_items(pg, v76) if i["kind"] == "generator" and i["id"] not in (cup, other))
            hide_id, flag_id, del_id, move_id = gens[:4]
            await pg.goto(B + f"#/browse/source/{v76}")
            await pg.wait_for_selector(f'.results [data-item="{hide_id}"]')
            await pg.click(f'.results [data-item="{hide_id}"]')
            await pg.click(".inspector [data-act=hide]")
            await pg.wait_for_selector(f'.results [data-item="{hide_id}"]', state="detached")
            await pg.click("[data-show-hidden]")
            await pg.wait_for_selector(f'.results [data-item="{hide_id}"][data-hidden]')
            check("hidden item leaves the list; Show hidden brings it back", True)
            await pg.click("[data-show-hidden]")
            await pg.click(f'.results [data-item="{flag_id}"]')
            await pg.click(".inspector [data-act=flag]")
            await pg.fill("#flag-note", "Lid too tight")
            await pg.click("#flag-save")
            await pg.wait_for_selector(".inspector .insp-flag.broken")
            key = flag_id.split(":", 1)[1]
            attn = await pg.evaluate(f"() => window.__workshop.catalog.attention.some((a) => a.kind === 'broken' && a.model === '{key}')")
            note = await pg.evaluate(f"() => window.__workshop.index.get('{flag_id}').broken?.note")
            check("flagged as broken, with its note (counted in Library settings)", attn and note == "Lid too tight", note)
            await pg.click(f'.results [data-item="{del_id}"]')
            await pg.click(".inspector [data-act=delete]")
            await pg.wait_for_selector(f'.results [data-item="{del_id}"]', state="detached")
            await pg.keyboard.press("Control+z")
            await pg.wait_for_selector(f'.results [data-item="{del_id}"]')
            await pg.click(f'.results [data-item="{del_id}"]')
            await pg.click(".inspector [data-act=delete]")
            await pg.wait_for_selector(f'.results [data-item="{del_id}"]', state="detached")
            removed = await pg.evaluate("() => window.__workshop.catalog.removed_items.map((r) => r.target)")
            check("deleted item leaves the library (Ctrl+Z undoes)", del_id.split("/")[-1] in removed, removed)
            await pg.click(f'.results [data-item="{move_id}"]')
            await pg.click(".inspector [data-act=edit]")
            await pg.wait_for_selector("#meta-save:not([disabled])")
            prefilled = await pg.input_value("#meta-license")
            await pg.select_option("#meta-project", ids["gears"])
            await pg.click("#meta-save")
            await pg.wait_for_selector(".dialog", state="detached")
            await pg.wait_for_function(f"() => window.__workshop.index.get('{move_id}')?.projectId === '{ids['gears']}'", timeout=20000)
            mv = await pg.evaluate(f"() => window.__workshop.index.get('{move_id}')")
            check("edit dialog starts with the values in use", prefilled == "CC-BY-SA-4.0", prefilled)
            check("item listed under another project; credits stay", mv["projectId"] == ids["gears"] and mv["sourceId"] == v76 and mv["license"]["spdx"] == "CC-BY-SA-4.0",
                  (mv["projectId"], mv["sourceId"], mv["license"]))

            # 10. library settings: categories, deleted items, project hide, trash
            await pg.goto(B + "#/library-settings")
            await pg.wait_for_selector("#ls-category-table")
            await pg.fill("#category-new", "Kitchen")
            await pg.click("#category-add")
            await pg.wait_for_selector('[data-category="kitchen"]')
            await pg.click('[data-category="labels"] [data-act=remove-category]')
            await pg.click('[data-category="labels"] #category-move-to')
            await pg.click('.catpick-opt[data-cat="gridfinity"]')
            await pg.click('[data-category="labels"] [data-act=remove-confirm]')
            await pg.wait_for_selector('[data-category="labels"]', state="detached")
            left = await pg.evaluate("() => window.__workshop.index.items.filter((i) => i.category === 'labels').length")
            await pg.click(".ls-removed summary")
            await pg.click("[data-act=restore-category]")
            await pg.wait_for_selector('[data-category="labels"]')
            back = await pg.evaluate("() => window.__workshop.index.items.filter((i) => i.category === 'labels').length")
            check("categories: add, remove (items move), bring back", left == 0 and back >= 5, (left, back))
            # a picture of your own as a category's icon (the file dialog answered through /test/pick)
            icon_src = home / "kitchen.svg"
            icon_src.write_text('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="#f2b705"/></svg>')
            urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(icon_src)}).encode(), method="POST"))
            await pg.click('[data-category="kitchen"] [data-act=category-image]')
            await pg.wait_for_selector('[data-category="kitchen"] img.ls-cat-icon', timeout=30000)
            icon_ok = await pg.evaluate("""() => { const i = document.querySelector('[data-category="kitchen"] img.ls-cat-icon'); return !!i && i.complete && i.naturalWidth > 0; }""")
            check("a category icon from an image file, kept in the library", icon_ok and (library / "icons/category-kitchen.svg").is_file(), icon_ok)
            await pg.click("#ls-deleted [data-act=restore-item]")
            await pg.wait_for_function(f"() => !!window.__workshop.index.get('{del_id}')")
            check("a deleted item comes back from Library settings", True)
            await pg.click(f'[data-source="{ids["gears"]}"] [data-act=hide-project]')
            await pg.wait_for_function(f"() => window.__workshop.index.items.filter((i) => i.projectId === '{ids['gears']}').every((i) => i.hidden)")
            hidden_n = await pg.evaluate(f"() => window.__workshop.index.query({{ scope: {{ project: '{ids['gears']}' }} }}).total")
            await pg.click(f'[data-source="{ids["gears"]}"] [data-act=hide-project]')
            await pg.wait_for_function(f"() => window.__workshop.index.query({{ scope: {{ project: '{ids['gears']}' }} }}).total > 0")
            check("a hidden project hides everything in it", hidden_n == 0)
            # Edit… on a project's row: its details; long text stays inside the boxes (they grow), and a long folder path too
            long_tags = "a-very-long-tag-name-that-keeps-going, another-long-tag-for-wrapping, yet-another-tag-so-it-needs-three-lines, edited-from-library-settings"
            await pg.click(f'[data-source="{ids["gears"]}"] [data-act=edit-project]')
            await pg.wait_for_selector(".meta-editor #meta-save:not([disabled])")
            one_line = await pg.eval_on_selector("#meta-tags", "e => e.offsetHeight")
            await pg.fill("#meta-tags", long_tags)
            grown = await pg.eval_on_selector("#meta-tags", "e => e.offsetHeight") >= one_line + 15
            spill = await pg.evaluate(OVERFLOW)
            await pg.click("#meta-save")
            await pg.wait_for_selector(".dialog", state="detached")
            await pg.wait_for_timeout(500)
            stored_tags = json.loads((library / f"sources/{ids['gears']}/metadata.json").read_text()).get("project", {}).get("tags")
            long_path = str(home / "a folder with a long name that keeps on going" / "and-another-folder-inside-it-with-no-spaces-at-all" / "project")
            urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": long_path}).encode(), method="POST"))
            await pg.click("#settings-add-project")
            await pg.click("[data-add-tab=folder]")
            await pg.click("#add-pick")
            await pg.wait_for_function("() => document.querySelector('#add-path')?.textContent.includes('project')")
            spill_path = await pg.evaluate(OVERFLOW)
            await pg.screenshot(path=str(out / "p3-04-long-path.png"))
            await pg.keyboard.press("Escape")
            await pg.wait_for_selector(".dialog", state="detached")
            check("Edit… in Library settings edits the project; long text stays inside its box (folder path, details)",
                  grown and not spill and not spill_path and "edited-from-library-settings" in (stored_tags or []), (grown, spill, spill_path, stored_tags))
            await pg.click('[data-source="local-bevel"] [data-act=delete-project]')
            await pg.wait_for_selector("[data-entry]")
            in_trash = not (library / "local/bevel").exists() and any((library / "trash").iterdir())
            await pg.click("[data-entry] [data-act=trash-restore]")
            await pg.wait_for_selector('[data-source="local-bevel"]')
            restored = (library / "local/bevel/bevel_gear.scad").is_file()
            await pg.click('[data-source="local-bevel"] [data-act=delete-project]')
            await pg.wait_for_selector("[data-entry]")
            await pg.screenshot(path=str(out / "p2-06-library-settings.png"), full_page=True)
            await pg.click("#trash-empty")
            await pg.wait_for_selector("#trash-none")
            emptied = not any((library / "trash").iterdir())
            check("deleted project goes to the trash, comes back, trash empties", in_trash and restored and emptied, (in_trash, restored, emptied))

            # 12. (0.4.3) the category picker lists every category even with one chosen, and makes new ones
            await pg.goto(B + f"#/browse/source/{ids['gears']}")
            await pg.wait_for_selector(".results [data-item]")
            await pg.locator(".results [data-item]").first.click()
            await pg.click(".inspector [data-act=edit]")
            await pg.wait_for_selector("#meta-save:not([disabled])")
            await pg.click("#meta-category")
            await pg.wait_for_selector(".catpick-pop")
            n_choices = await pg.evaluate("() => window.__workshop.catalog.category_choices.length")
            listed = await pg.eval_on_selector_all(".catpick-opt[data-cat]", "els => els.map((e) => e.dataset.cat)")
            await pg.screenshot(path=str(out / "p4-05-category-picker.png"))
            await pg.fill(".catpick-filter", "Garden tools")
            await pg.keyboard.press("Enter")
            shown_new = await pg.inner_text("#meta-category")
            picked_item = await pg.evaluate("() => document.querySelector('.results [data-item]').dataset.item")
            await pg.click("#meta-save")
            await pg.wait_for_selector(".dialog", state="detached")
            await pg.wait_for_function(f"() => window.__workshop.index.get('{picked_item}')?.category === 'garden-tools'", timeout=20000)
            made = await pg.evaluate("() => window.__workshop.catalog.category_choices.some((c) => c.id === 'garden-tools' && c.label === 'Garden tools')")
            cats_listed = [c for c in listed if c not in ("empty", "newprompt", "new")]
            check("category picker: every category listed with one chosen; a new one typed in is made",
                  len(cats_listed) == n_choices and "newprompt" in listed and "Garden tools" in shown_new and made, (len(cats_listed), n_choices, listed[-2:], shown_new, made))

            # 13. (0.4.3) Scan for changes: a file added to a project's stored copy, a new local/ project, a missing one
            await pg.goto(B + "#/library-settings")
            await pg.wait_for_selector("#library-rescan")
            gsrc = await pg.evaluate(f"() => window.__workshop.catalog.sources.find((s) => s.id === '{ids['gears']}')")
            (library / f"sources/{ids['gears']}/files/{gsrc['version']}/scan_extra.scad").write_text("size = 3; // [1:10]\ncube(size);\n")
            (library / "local/scanned").mkdir(parents=True, exist_ok=True)
            (library / "local/scanned/plate.scad").write_text("w = 20; // [5:50]\ncube([w, w, 2]);\n")
            await pg.click("#library-rescan")
            try:
                await pg.wait_for_function(f"() => window.__workshop.index.get('gen:{ids['gears']}/scan-extra') && window.__workshop.index.get('gen:local-scanned/plate')", timeout=180000)
            except Exception:
                pass
            seen = await pg.evaluate(f"() => [!!window.__workshop.index.get('gen:{ids['gears']}/scan-extra'), !!window.__workshop.index.get('gen:local-scanned/plate')]")
            toast1 = await pg.eval_on_selector_all(".toast", "els => els.map((e) => e.textContent).join(' | ')")
            shutil.rmtree(library / "local/scanned")
            # (a click while another scan runs, one started when the window got focus, only waits for that
            # one, whose message doesn't mention the folder: click again until a scan of our own reports it)
            for _ in range(3):
                await pg.click("#library-rescan")
                try:
                    await pg.wait_for_function("() => window.__workshop.catalog.attention.some((a) => a.kind === 'missing-folder' && a.source === 'local-scanned')"
                                               " && document.querySelector('#toast')?.textContent.includes('missing')", timeout=30000)
                    break
                except Exception:
                    pass
            missing = await pg.evaluate("() => window.__workshop.catalog.attention.some((a) => a.kind === 'missing-folder' && a.source === 'local-scanned')")
            toast2 = await pg.eval_on_selector_all(".toast", "els => els.map((e) => e.textContent).join(' | ')")
            # (0.4.4) no Needs attention: a project's problems are marked in Library settings › Projects, beside its license
            await pg.wait_for_selector('[data-source="local-scanned"] [data-problem="missing-folder"]', timeout=30000)
            marked = await pg.locator('[data-source="local-scanned"] [data-problem="missing-folder"]').count()
            check("Scan for changes: a file added to a stored copy and a new local/ project are read; a folder gone is marked in Library settings",
                  all(seen) and missing and marked == 1 and "missing" in toast2, (seen, missing, marked, toast1, toast2))
            lic = await pg.eval_on_selector_all("#ls-project-table td.ls-license", "els => els.map((e) => e.textContent.trim())")
            nav_attn = await pg.locator(".sidebar .nav-attention").count()
            check("Library settings lists each project's license (one not cleared is marked), and the menu has no Needs attention",
                  len(lic) >= 3 and any("Check license" in t or "License unclear" in t for t in lic) and nav_attn == 0, (lic[:6], nav_attn))
            await pg.click('[data-source="local-scanned"] [data-act=delete-project]')

            # 13b. (0.4.6) defaults that work: a number without a Customizer comment whose default is a
            # fraction is accepted at its default (Gridfinity Anylid's width units, 1.5); a default off its
            # slider's step is reported; every model is rendered once at its defaults (with its thumbnail)
            # and one that fails is marked in Library settings and on its page, with OpenSCAD's message
            await pg.goto(B + "#/m/anylid/lid")
            await pg.wait_for_selector("#p-Width_Units")
            invalid = await pg.evaluate("() => [...document.querySelectorAll('.field input')].filter((i) => !i.checkValidity()).map((i) => i.id)")
            check("Gridfinity Anylid opens with no setting marked invalid at its defaults (1.5 and 2.5 units)", not invalid, invalid)
            (library / "local/defaults").mkdir(parents=True, exist_ok=True)
            (library / "local/defaults/tray.scad").write_text("// width in grid units\nunits = 1.5;\n// height\nheight = 2.5; // [0:1:10]\ncube([units * 42, 42, height]);\n")
            (library / "local/defaults/broken.scad").write_text("size = 10;\nassert(size < 5, \"size must be under 5\");\ncube(size);\n")
            await pg.goto(B + "#/library-settings")
            await pg.wait_for_selector("#library-rescan")
            await pg.click("#library-rescan")
            checks_file = library / "sources/local-defaults/checks.json"
            for _ in range(240):
                if checks_file.is_file() and len(json.loads(checks_file.read_text()).get("models", {})) >= 2:
                    break
                await pg.wait_for_timeout(500)
            checks = json.loads(checks_file.read_text()) if checks_file.is_file() else {}
            got = {k: (v.get("ok"), v.get("message") or "") for k, v in checks.get("models", {}).items()}
            try:
                await pg.wait_for_selector('[data-source="local-defaults"] [data-problem="render"]', timeout=30000)
            except Exception:
                pass
            marks = await pg.eval_on_selector_all('[data-source="local-defaults"] [data-problem]', "els => els.map((e) => [e.dataset.problem, e.title])")
            check("each model is rendered at its defaults when it's added; results kept in checks.json; failures and settings off their limits marked in Library settings",
                  got.get("tray", (None,))[0] is True and got.get("broken", (None, ""))[0] is False and "size must be under 5" in got["broken"][1]
                  and any(k == "render" and "size must be under 5" in t for k, t in marks) and any(k == "limits" and "Height" in t for k, t in marks),
                  (got, marks))
            await pg.goto(B + "#/m/local-defaults/broken")
            await pg.wait_for_selector("#model-title")
            alert = await pg.locator("#default-check").inner_text() if await pg.locator("#default-check").count() else ""
            title = [await pg.inner_text("#model-title"), await pg.inner_text("#model-project")]
            await pg.screenshot(path=str(out / "p6-01-default-check.png"))
            check("a model that didn't render at its defaults says so on its page, with OpenSCAD's message; the page has the model's own name, its project under it",
                  "size must be under 5" in alert and title == ["Broken", "Defaults"], (alert, title))
            await pg.goto(B + "#/browse/source/local-defaults")
            await pg.click("#source-more summary")
            await pg.click("[data-act=check-defaults]")
            try:
                await pg.wait_for_function("() => /didn't render/.test(document.querySelector('#toast')?.textContent || '')", timeout=60000)
            except Exception:
                pass
            toast = await pg.inner_text("#toast")
            check("Check defaults (project ⋯ menu) renders every model again and says how many failed", "1 model of 2" in toast.lower(), toast)  # (the Drafting Sheet's status bar is in capitals)
            # 13c. (0.4.7) one project description: workshop.json beside every project's files; Save as project
            # file writes one into your own project's folder; a project's own workshop.json says what its models are
            await pg.click("#source-more summary")
            await pg.click("[data-act=save-project-file]")
            own = library / "local/defaults/workshop.json"
            for _ in range(40):
                if own.is_file():
                    break
                await pg.wait_for_timeout(250)
            saved = json.loads(own.read_text()) if own.is_file() else {}
            # (saving changed the project's files: its models' thumbnails are made again in the background;
            # let that finish so it doesn't load a model the edit below removes)
            await pg.wait_for_selector(".status-idle", timeout=180000)
            beside = (library / "sources/local-defaults/workshop.json").is_file()
            saved["category"] = "labels"
            saved["models"] = [dict(m, name="Grid tray") for m in saved.get("models", []) if m.get("id") == "tray"]
            own.write_text(json.dumps(saved, indent=2))
            await pg.goto(B + "#/library-settings")
            await pg.click("#library-rescan")
            try:
                await pg.wait_for_function("() => window.__workshop.index.get('gen:local-defaults/tray')?.name === 'Grid tray' && !window.__workshop.index.get('gen:local-defaults/broken')", timeout=120000)
            except Exception:
                pass
            tray = await pg.evaluate("() => { const t = window.__workshop.index.get('gen:local-defaults/tray'); return t && [t.name, t.category]; }")
            gone = await pg.evaluate("() => !window.__workshop.index.get('gen:local-defaults/broken')")
            origins = {json.loads(p.read_text()).get("origin", {}).get("type") for p in (library / "sources").glob("*/source.json")}
            versions_ok = all(v.get("files") and v.get("content") for p in (library / "sources").glob("*/source.json")
                              for s in [json.loads(p.read_text())] if s["kind"] in ("github", "local") for v in s["versions"] if v["id"] == s["version"])
            check("workshop.json: written beside every project's files; Save as project file writes your project's; its models, names and category are used when it's read; every project records where it came from and its version's files the same way",
                  [m.get("id") for m in json.loads(own.read_text()).get("models", [])] == ["tray"] and beside and tray == ["Grid tray", "labels"] and gone
                  and None not in origins and versions_ok, (beside, tray, gone, origins, versions_ok))
            await pg.goto(B + "#/library-settings")
            await pg.click('[data-source="local-defaults"] [data-act=delete-project]')

            # 13d. (0.4.8) a project from a ZIP and one linked to a folder outside the library are kept the
            # same way as the others (Review 3 compares them); the Settings page is laid out like Library
            # settings; a yes/no setting's box is in the value column; the library's README explains the
            # folders that differ
            outside = home / "outside"
            (outside / "zipped").mkdir(parents=True, exist_ok=True)
            (outside / "zipped/peg.scad").write_text("// Peg length\nlength = 20; // [5:1:60]\ncylinder(d=6, h=length, $fn=24);\n")
            (outside / "zipped/hook.scad").write_text("// Hook depth\ndepth = 12; // [4:1:30]\ncube([6, depth, 6]);\n")
            zip_path = Path(shutil.make_archive(str(outside / "pegs"), "zip", outside / "zipped"))
            (outside / "linked shelf").mkdir(parents=True, exist_ok=True)
            (outside / "linked shelf/shelf.scad").write_text("// Shelf width\nwidth = 80; // [40:5:200]\ncube([width, 30, 4]);\n")
            added = {}
            for tab, pick in (("zip", zip_path), ("folder", outside / "linked shelf")):
                await pg.goto(B + "#/")
                await pg.click("#add-project")
                await pg.click(f"[data-add-tab={tab}]")
                urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(pick)}).encode(), method="POST"))
                await pg.click("#add-pick")
                await pg.wait_for_function("(p) => document.querySelector('#add-path')?.textContent === p", arg=str(pick))
                if tab == "folder" and not await pg.is_checked("#add-link"):
                    await pg.check("#add-link")
                await pg.click("#add-go")
                await pg.wait_for_function("() => location.hash.includes('/source/')", timeout=180000)
                await pg.wait_for_selector(".source-panel")
                added[tab] = [(await pg.evaluate("location.hash")).split("/source/")[1], await pg.locator("#toast").text_content() or ""]
            zid, lid = added["zip"][0], added["folder"][0]
            for _ in range(240):   # (each model's check is written just before its thumbnail)
                if len(list((library / f"sources/{zid}/thumbs").glob("*.webp"))) >= 2 and len(list((library / f"sources/{lid}/thumbs").glob("*.webp"))) >= 1:
                    break
                await pg.wait_for_timeout(500)
            shapes = {}
            for t, i in (("zip", zid), ("linked", lid)):
                sj = json.loads((library / f"sources/{i}/source.json").read_text())
                cur = next((v for v in sj.get("versions", []) if v["id"] == sj.get("version")), {})
                d = library / f"sources/{i}"
                shapes[t] = [sj.get("origin", {}).get("type"), bool(cur.get("files") and cur.get("content") and cur.get("added")), (d / "workshop.json").is_file(),
                             (d / "checks.json").is_file(), len(list((d / "thumbs").glob("*.webp"))) if (d / "thumbs").is_dir() else 0]
            check("a ZIP project and a project linked to a folder are kept like the others: where it came from, its version's files, workshop.json, checks and thumbnails",
                  shapes == {"zip": ["zip", True, True, True, 2], "linked": ["linked", True, True, True, 1]}
                  and "2 models" in added["zip"][1] and "1 model" in added["folder"][1], (shapes, added))
            # (0.4.23) one .scad file, from Add a project › File, is a project of its own
            one_scad = {}
            try:
                (outside / "Single Knob.scad").write_text("// Knob size\nsize = 18; // [10:1:40]\ncylinder(d=size, h=8, $fn=32);\n")
                await pg.goto(B + "#/")
                await pg.click("#add-project")
                await pg.click("[data-add-tab=zip]")
                urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(outside / "Single Knob.scad")}).encode(), method="POST"))
                await pg.click("#add-pick")
                await pg.wait_for_function("(p) => document.querySelector('#add-path')?.textContent === p", arg=str(outside / "Single Knob.scad"))
                await pg.click("#add-go")
                await pg.wait_for_function("() => location.hash.includes('/source/')", timeout=180000)
                sid = (await pg.evaluate("location.hash")).split("/source/")[1]
                sj = json.loads((library / f"sources/{sid}/source.json").read_text())
                one_scad = {"kind": sj.get("kind"), "files": sorted(p.name for p in (library / f"sources/{sid}/files/{sj['version']}").iterdir()),
                            "name": await pg.evaluate("(i) => window.__workshop.catalog.sources.find((s) => s.id === i)?.name", sid)}
            except Exception as e:
                print("one .scad:", e)
            check("one .scad file added from Add a project is a project of its own", one_scad.get("kind") == "folder" and one_scad.get("files") == ["Single Knob.scad"]
                  and one_scad.get("name") == "Single Knob", one_scad)
            # (0.4.23) a model that makes a 2D shape shows its drawing and downloads it as SVG or DXF; your notes on a
            # model and a project are kept in its metadata.json and found by search; Show in folder; one Code button
            flat = {}
            try:
                (outside / "Sign Plate.scad").write_text("width = 80; // [20:200]\nheight = 30; // [10:100]\n"
                    "difference() { square([width, height]); for (x = [6, width - 6]) translate([x, height / 2]) circle(d = 4, $fn = 32); }\n")
                await pg.goto(B + "#/")
                await pg.click("#add-project")
                await pg.click("[data-add-tab=zip]")
                urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(outside / "Sign Plate.scad")}).encode(), method="POST"))
                await pg.click("#add-pick")
                await pg.wait_for_function("(p) => document.querySelector('#add-path')?.textContent === p", arg=str(outside / "Sign Plate.scad"))
                await pg.click("#add-go")
                await pg.wait_for_function("() => location.hash.includes('/source/')", timeout=180000)
                sid = (await pg.evaluate("location.hash")).split("/source/")[1]
                await pg.goto(B + f"#/m/{sid}/sign-plate")
                await pg.wait_for_function("() => /Preview matches/.test(document.querySelector('#status').textContent)", timeout=120000)
                flat["drawing"] = await pg.is_visible("#stage-drawing")
                flat["dims"] = await pg.inner_text("#dims .mm")
                flat["formats"] = await pg.eval_on_selector_all("#download-format option", "o => o.map((x) => x.value)")
                flat["edge_tabs"] = await pg.locator(".side-tab").count()
                await pg.select_option("#download-format", "dxf")
                await pg.wait_for_function("() => /\\.dxf$/.test(document.querySelector('#download').download) && !document.querySelector('#download').classList.contains('is-disabled')", timeout=60000)
                flat["dxf"] = "SECTION" in await pg.evaluate("() => fetch(document.querySelector('#download').href).then((r) => r.text())")
                await pg.select_option("#download-format", "svg")
                await pg.click("#model-more summary")
                flat["menu"] = await pg.eval_on_selector_all("#model-more-own button", "b => b.map((x) => x.id)")
                await pg.click("#model-notes-open")
                await pg.fill("#model-own-notes-text", "Cut from 3 mm birch ply")
                await pg.click("#params")
                await pg.wait_for_function("() => /notes saved/i.test(document.querySelector('#toast')?.textContent || '')", timeout=10000)
                await pg.goto(B + f"#/browse/source/{sid}")
                await pg.wait_for_selector(".source-panel")
                await pg.click("[data-source-tab=notes]")
                await pg.fill("#project-notes-text", "Fits the shed door")
                await pg.click("[data-source-tab=items]")
                # the model's "Notes saved." toast may still be showing, so wait for the file itself
                for _ in range(40):
                    flat["meta"] = json.loads((library / f"sources/{sid}/metadata.json").read_text())
                    if flat["meta"].get("project", {}).get("my_notes"):
                        break
                    await asyncio.sleep(0.25)
                await pg.fill("#global-search", "birch")
                await pg.wait_for_function("()=>/ms$/.test(document.querySelector('.browse-count')?.textContent || '')")
                try:  # (the library may still be reading the notes just saved)
                    await pg.wait_for_selector('.results [data-item*="sign-plate"]', timeout=15000)
                except Exception:
                    pass
                flat["found"] = await pg.eval_on_selector_all(".results [data-item]", "els=>els.map(e=>e.dataset.item)")
                await pg.fill("#global-search", "")
                flat["reveal"] = await pg.evaluate("(s) => window.__TAURI__.core.invoke('api', { cmd: 'reveal_path', args: { source: s, rel: 'Sign Plate.scad' } })", sid)
            except Exception as e:
                print("2D model:", e)
            meta = flat.get("meta", {})
            check("a 2D model shows its drawing and downloads SVG or DXF; your notes on a model and its project are kept and searched; Show in folder; Code only in the header",
                  flat.get("drawing") and flat.get("dims") == "80 × 30 mm" and flat.get("formats") == ["svg", "dxf"] and flat.get("dxf")
                  and flat.get("edge_tabs") == 0 and {"model-notes-open", "model-reveal"} <= set(flat.get("menu", []))
                  and meta.get("items", {}).get("sign-plate", {}).get("my_notes") == "Cut from 3 mm birch ply"
                  and meta.get("project", {}).get("my_notes") == "Fits the shed door"
                  and any("sign-plate" in i for i in flat.get("found", [])) and str(flat.get("reveal", "")).endswith("Sign Plate.scad"), flat)
            # (0.4.24) reading settings: a text setting the code compares with words is a drop-down, and Edit form can
            # make it typed in again; a model whose defaults don't render shows its first preset that does
            reading = {}
            try:
                cases = outside / "Settings cases"
                cases.mkdir(exist_ok=True)
                (cases / "Box.scad").write_text('style = "round";\nlabel = "Box";\nif (style == "square") cube(10);\n'
                    'else if (style == "hex") cylinder(d = 10, h = 10, $fn = 6);\nelse cylinder(d = 10, h = 10);\n')
                (cases / "Bad.scad").write_text('size = 0; // [0:50]\nassert(size > 0, "size must be over 0");\ncube(size);\n')
                (cases / "Bad.json").write_text('{ "fileFormatVersion": "1", "parameterSets": { "Small": { "size": "10" } } }\n')
                await pg.goto(B + "#/")
                await pg.click("#add-project")
                await pg.click("[data-add-tab=folder]")
                urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(cases)}).encode(), method="POST"))
                await pg.click("#add-pick")
                await pg.wait_for_function("(p) => document.querySelector('#add-path')?.textContent === p", arg=str(cases))
                await pg.click("#add-go")
                await pg.wait_for_function("() => location.hash.includes('/source/')", timeout=180000)
                sid = (await pg.evaluate("location.hash")).split("/source/")[1]
                await pg.goto(B + f"#/m/{sid}/box")
                await pg.wait_for_selector("#p-style")
                reading["choices"] = await pg.eval_on_selector_all("select#p-style option", "o => o.map((x) => x.value)")
                reading["label"] = await pg.evaluate("document.querySelector('#p-label').tagName")
                await pg.click("#model-more summary")
                await pg.click("#edit-form")
                await pg.click("[data-fe=style] .fe-name")
                await pg.fill("[data-form='style.choices']", "")
                await pg.click(".form-editor button.primary")
                await pg.wait_for_function("() => document.querySelector('#p-style')?.tagName === 'INPUT'", timeout=10000)
                reading["typed"] = json.loads((library / f"sources/{sid}/metadata.json").read_text())["items"]["box"]["form"]
                await pg.goto(B + f"#/m/{sid}/bad")
                await pg.wait_for_function(f"() => document.body.dataset.model === '{sid}/bad' && /Preview matches/.test(document.querySelector('#status').textContent)", timeout=120000)
                reading["preset"] = [await pg.input_value("#settings-pick"), await pg.input_value("#p-size")]
            except Exception as e:
                print("reading settings:", e)
            check("a text setting the code compares with words is a drop-down; cleared in Edit form it's typed in; defaults that fail show a preset that works",
                  reading.get("choices") == ["round", "square", "hex"] and reading.get("label") == "INPUT"
                  and reading.get("typed") == {"ui": {"style": {"options": []}}} and reading.get("preset") == ["preset:0", "10"], reading)
            # (0.4.25, milestone 6) a project's picture is one of its own files or one you add; content you add to a
            # model (a file copied into the library, or a link) is shown beside its documents, and Remove sets it aside
            m6 = {}
            try:
                def tiny_png(rgb):
                    raw = b"".join(b"\0" + bytes(rgb) * 4 for _ in range(4))
                    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
                    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 4, 4, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
                pc = outside / "Pictures and content"
                (pc / "pics").mkdir(parents=True, exist_ok=True)
                (pc / "Box.scad").write_text("w = 20; // [10:50]\ncube(w);\n")
                (pc / "pics" / "front.png").write_bytes(tiny_png((200, 40, 40)))
                (outside / "my photo.png").write_bytes(tiny_png((40, 120, 200)))
                (outside / "Guide.pdf").write_bytes(b"%PDF-1.4\n%%EOF\n")
                await pg.goto(B + "#/")
                await pg.click("#add-project")
                await pg.click("[data-add-tab=folder]")
                urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(pc)}).encode(), method="POST"))
                await pg.click("#add-pick")
                await pg.wait_for_function("(p) => document.querySelector('#add-path')?.textContent === p", arg=str(pc))
                await pg.click("#add-go")
                await pg.wait_for_function("() => location.hash.includes('/source/')", timeout=180000)
                sid = (await pg.evaluate("location.hash")).split("/source/")[1]
                meta_of = lambda: json.loads((library / f"sources/{sid}/metadata.json").read_text())
                await pg.click("#source-more summary")
                await pg.click("[data-act=picture]")
                await pg.wait_for_selector('[data-pic="file:pics/front.png"]')
                m6["tiles"] = await pg.eval_on_selector_all(".pic-tile", "ts => ts.map((t) => t.dataset.pic)")
                await pg.click('[data-pic="file:pics/front.png"]')
                await pg.wait_for_function("() => !document.querySelector('.picture-dialog')")
                m6["file"] = meta_of()["project"].get("icon")
                await pg.click("#source-more summary")
                await pg.click("[data-act=picture]")
                await pg.wait_for_selector("#pic-add")
                urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(outside / "my photo.png")}).encode(), method="POST"))
                await pg.click("#pic-add")
                await pg.wait_for_function("() => !document.querySelector('.picture-dialog')")
                m6["added"] = [meta_of()["project"].get("icon"), (library / f"sources/{sid}/pictures/my photo.png").is_file()]
                await pg.goto(B + f"#/m/{sid}/box")
                await pg.wait_for_selector("#model-more summary")
                await pg.click("#model-more summary")
                await pg.click("#model-content")
                urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(outside / "Guide.pdf")}).encode(), method="POST"))
                await pg.click("#content-pick")
                await pg.wait_for_function("() => /Guide\\.pdf$/.test(document.querySelector('#content-path')?.textContent || '')")
                await pg.click("#content-add")
                await pg.wait_for_function("() => !document.querySelector('.content-dialog')")
                await pg.click("#model-more summary")
                await pg.click("#model-content")
                await pg.fill("#content-url", "https://example.com/build-video")
                await pg.fill("#content-name", "Build video")
                await pg.click("#content-add")
                await pg.wait_for_function("() => !document.querySelector('.content-dialog')")
                m6["listed"] = [{k: a.get(k) for k in ("name", "file", "url")} for a in meta_of()["items"]["box"]["attachments"]]
                await pg.get_by_role("button", name="Guide", exact=True).click()
                await pg.wait_for_selector("#attachment-remove")
                await pg.click("#attachment-remove")
                for _ in range(40):
                    if len(meta_of()["items"]["box"].get("attachments", [])) == 1:
                        break
                    await asyncio.sleep(0.25)
                m6["left"] = [a.get("name") for a in meta_of()["items"]["box"].get("attachments", [])]
                m6["set_aside"] = [f.name.endswith(f"-{sid}-Guide.pdf") for f in (library / "backups" / "removed").glob("*")]
            except Exception as e:
                print("pictures and content:", e)
            check("a project's picture is one of its files or one you add; a file or a link added to a model is listed and shown beside its documents; Remove sets the file aside",
                  "file:pics/front.png" in m6.get("tiles", []) and m6.get("file") == "file:pics/front.png" and m6.get("added") == ["picture:my photo.png", True]
                  and m6.get("listed") == [{"name": "Guide", "file": "Guide.pdf", "url": None}, {"name": "Build video", "file": None, "url": "https://example.com/build-video"}]
                  and m6.get("left") == ["Build video"] and True in m6.get("set_aside", []), m6)
            # (0.4.26, the UI review) a small project has no browse controls; no details panel without a selection; no
            # Grouped view; About this model is a header button; Reset all settings is gone
            lean = {}
            try:
                await pg.goto(B + "#/browse/all")
                await pg.wait_for_selector(".results [data-item]")
                lean["inspector"] = await pg.locator(".inspector").count()
                lean["layouts"] = await pg.eval_on_selector_all("button[data-layout]", "bs => bs.map((b) => b.dataset.layout)")
                small = await pg.evaluate("() => window.__workshop.catalog.sources.find((s) => s.name === 'Settings cases')?.id")
                await pg.goto(B + f"#/browse/source/{small}")
                await pg.wait_for_selector(".results [data-item]")
                lean["small"] = await pg.locator(".browse-tools, .filters, button[data-layout]").count()
                await pg.goto(B + "#/m/@bosl2/bevel_gear")
                await pg.wait_for_selector("#model-about:not([hidden])", timeout=60000)
                lean["reset"] = await pg.locator("#reset-all").count()
            except Exception as e:
                print("UI review:", e)
            check("a small project has no browse controls; no details panel until something is selected; no Grouped view; About this model in the header; no Reset all",
                  lean.get("inspector") == 0 and lean.get("layouts") == ["grid", "list", "table"] and lean.get("small") == 0 and lean.get("reset") == 0, lean)
            await pg.goto(B + "#/settings")
            await pg.wait_for_selector("#ls-appearance")
            toc = await pg.eval_on_selector_all("#about-body .ls-toc a", "els => els.map((e) => e.textContent)")
            await pg.goto(B + "#/m/anylid/lid")
            await pg.wait_for_selector("#p-Mirror_X")
            cols = await pg.evaluate("() => ['#p-Mirror_X', '#p-Width_Units'].map((s) => Math.round(document.querySelector(s).getBoundingClientRect().left))")
            readme = (library / "README.txt").read_text()
            check("Settings is laid out like Library settings; a yes/no setting's box is in the value column; the library's README explains pins, your own projects and old versions",
                  toc[:4] == ["Appearance", "Printer profile", "Downloads", "Engine"] and abs(cols[0] - cols[1]) <= 2
                  and "sources/pinned/pins/" in readme and "Remove versions not in use" in readme, (toc, cols))

            # (Review 7, items 3 and 8) a model that isn't in the library gets a page saying so, with nothing
            # to make; everything in the Parts Library is a part, so no "part" badges there
            await pg.goto(B + "#/m/anylid/no-such-model")
            await pg.wait_for_selector("#model-missing")
            missing = await pg.evaluate("() => [document.querySelector('#view-model').hidden, document.querySelector('#about-title').textContent]")
            await pg.goto(B + "#/browse/parts")
            await pg.wait_for_selector("[data-item]")
            part_badges = await pg.evaluate("() => [document.querySelectorAll('[data-item]').length, [...document.querySelectorAll('.badge-mini')].filter((b) => b.textContent === 'part').length]")
            check("a model that isn't in the library gets a page saying so, with nothing to make; no part badges in the Parts Library",
                  missing == [True, "This model isn't in the library"] and part_badges[0] > 0 and part_badges[1] == 0, (missing, part_badges))

            # 13e. (0.4.9, F-1) the starter projects that are on GitHub as the app had them become GitHub projects
            # at the same commit, in upstream's own layout: same model ids, saved settings' code edits moved to
            # the new paths, thumbnails kept, update checks possible; the others stay as they are
            on_github = ["cullenect-label", "gridfinity-basket", "gridfinity-extended", "gridfinity-rebuilt", "label-generator-gridfinity", "underware"]
            def model_ids(i):
                sj = json.loads((library / f"sources/{i}/source.json").read_text())
                d = library / f"sources/{i}/derived/{sj['version']}.json"
                return sorted(m["id"] for m in json.loads(d.read_text())["models"]) if d.is_file() else []
            ids_before = {i: model_ids(i) for i in on_github}
            thumbs_before = {i: len(list((library / f"sources/{i}/thumbs").glob("*.webp"))) for i in on_github}
            (library / "recipes/f1-edit.json").write_text(json.dumps({"id": "f1-edit", "model": "gridfinity-basket/basket", "name": "Edited basket", "values": {},
                "edits": {"/vendor/gridfinity-basket/gridfinityBasket.scad": {"text": "// edited\n", "base": "0" * 64}}}))
            await pg.goto(B + "#/library-settings")
            await pg.wait_for_selector("#starter-convert")
            offer = await pg.inner_text("#starter-convert")
            # (0.4.10, F-13) models of projects from before the app checked new ones are offered a check at their defaults
            unchecked = await pg.inner_text("#unchecked-offer") if await pg.locator("#unchecked-offer").count() else ""
            await pg.click("#starter-convert button")
            try:
                await pg.wait_for_function("() => !document.querySelector('#starter-convert')", timeout=900000)
            except Exception:
                pass
            toast = await pg.locator("#toast").text_content() or ""
            conv, old_copy = {}, {}
            for i in on_github:
                sj = json.loads((library / f"sources/{i}/source.json").read_text())
                d = library / f"sources/{i}/derived/{sj['version']}.json"
                # (0.4.10, F-16) the app's old copy is removed when upstream's files are the same, else it says what it was
                was = sj.get("origin", {}).get("converted", {}).get("version", "")
                old_v = [v for v in sj.get("versions", []) if v.get("id") == was]
                old_copy[i] = "removed" if not old_v and not (library / f"sources/{i}/files/{was}").exists() else (old_v[0].get("message", "") if old_v else "?")
                conv[i] = [sj["kind"], sj.get("origin", {}).get("track", "")[:7], json.loads(d.read_text()).get("described_by") if d.is_file() else None,
                           model_ids(i) == ids_before[i], len(list((library / f"sources/{i}/thumbs").glob("*.webp"))) >= thumbs_before[i]]   # (more: made meanwhile)
            print("the app's old copies:", old_copy)
            print("after converting:", toast)
            if conv["gridfinity-extended"][0] != "github":   # (diagnosis: what the conversion said, and what's in the project)
                jobs = await pg.evaluate("() => window.__workshop.platform.api('jobs', {})")
                print("conversion job:", [j.get("result") or j.get("error") for j in jobs if j.get("label") == "Getting projects from GitHub"])
                ge = library / "sources/gridfinity-extended"
                print("gridfinity-extended:", sorted(p.name for p in ge.iterdir()), sorted(p.name for p in (ge / "files").iterdir()) if (ge / "files").is_dir() else None,
                      sorted(p.name for p in (ge / "derived").iterdir()) if (ge / "derived").is_dir() else None)
            edit_keys = list(json.loads((library / "recipes/f1-edit.json").read_text())["edits"])
            others = {json.loads((library / f"sources/{i}/source.json").read_text())["kind"] for i in ("hsw", "gridflock", "anylid")}
            await pg.goto(B + "#/m/gridfinity-basket/basket")
            try:
                await pg.wait_for_function("() => /\\d/.test(document.querySelector('.hud-dims')?.textContent || '')", timeout=180000)
            except Exception:
                pass
            dims = await pg.inner_text(".hud-dims")
            await pg.screenshot(path=str(out / "p6-03-from-github.png"))
            check("starter projects on GitHub become GitHub projects at the same commit (same models, edits moved to upstream's paths, thumbnails kept); the others stay as they are",
                  "Gridfinity Rebuilt" in offer and "checked at their default settings" in unchecked
                  and all(v == "removed" or v.startswith("The app's copy of") for v in old_copy.values())
                  and all(v == ["github", "branch:", "description.json", True, True] for v in conv.values())
                  and edit_keys == ["/gridfinityBasket.scad"] and others == {"bundled"} and "mm" in dims, (conv, old_copy, edit_keys, others, dims, toast, unchecked[:120]))

            # 13f. (0.4.9) adding a project from GitHub reads it and checks its models at their defaults.
            # (0.4.23) Add a project has no Suggested row (the owner's choice); Home keeps its list for an empty library
            await pg.goto(B + "#/")
            await pg.click("#add-project")
            await pg.wait_for_selector("#add-url")
            chips = await pg.evaluate("() => document.querySelectorAll('#add-suggested, .add-project [data-suggested]').length")
            await pg.fill("#add-url", "https://github.com/cfinke/LEGO.scad")
            await pg.click("#add-go")
            await pg.wait_for_function("() => location.hash.includes('/source/')", timeout=180000)
            lego = (await pg.evaluate("location.hash")).split("/source/")[1]
            lego_models, lego_checks = [], {}
            for _ in range(600):
                sj = json.loads((library / f"sources/{lego}/source.json").read_text())
                d = library / f"sources/{lego}/derived/{sj['version']}.json"
                lego_models = [m["id"] for m in json.loads(d.read_text())["models"]] if d.is_file() else []
                c = library / f"sources/{lego}/checks.json"
                lego_checks = json.loads(c.read_text()).get("models", {}) if c.is_file() else {}
                if "lego" in lego_checks:   # (its 180 examples are checked too, in the background)
                    break
                await pg.wait_for_timeout(500)
            failing = {m: v.get("message", "")[:120] for m, v in lego_checks.items() if not v.get("ok")}
            print("LEGO.scad at its defaults:", len(lego_models), "models,", len(failing), "failing", failing)
            check("Add a project has no suggestions; adding from GitHub reads the project and checks every model at its defaults",
                  chips == 0 and "lego" in lego_models and lego_checks.get("lego", {}).get("ok"), (chips, len(lego_models), failing))

            # 13g. (0.4.11, milestone 4) Save as part keeps what's made in the Parts Library (local/Made parts/<model>/)
            # with a recipe; its page opens the model with those settings or makes it again, and says when
            # the model's project has changed since
            # (the bevel gear's project was deleted in 11: made again; LEGO.scad's 180 examples are still
            # being checked in the background, so the page can be slow)
            (library / "local/bevel").mkdir(parents=True, exist_ok=True)
            (library / "local/bevel/bevel_gear.scad").write_text(bevel_src)
            await pg.evaluate("import('/ui/library.js').then(async (m) => { await m.rescanLocal(false); await m.rescanLocal(false); })")
            await pg.wait_for_function("() => window.__workshop?.index.get('gen:local-bevel/bevel-gear')", timeout=180000)
            await pg.goto(B + "#/m/local-bevel/bevel-gear")
            try:
                await pg.wait_for_function("()=>document.body.dataset.model==='local-bevel/bevel-gear'", timeout=300000)
            except Exception:
                print("bevel gear page didn't open:", await pg.evaluate("() => [location.hash, document.body.dataset.model, document.querySelector('#status')?.textContent]"))
                raise
            await pg.wait_for_function(DONE, timeout=300000)
            await pg.click("#save-part")
            await pg.fill("#save-part-name", "Test gear")
            await pg.screenshot(path=str(out / "m4-01-save-part.png"))
            await pg.click("#save-part-save")
            toast_js = "(t)=>{const e=document.querySelector('#toast');return e&&!e.hidden&&e.textContent.includes(t)}"
            await pg.wait_for_function(toast_js, arg="Parts Library", timeout=120000)
            made = list((library / "local" / "Made parts").glob("*/Test gear.stl"))
            recipe_path = made[0].with_name("Test gear.recipe.json") if made else None
            recipe = json.loads(recipe_path.read_text()) if recipe_path and recipe_path.is_file() else {}
            await pg.click("#toast .ghost")
            await pg.wait_for_selector("#made-from", timeout=20000)
            made_text = await pg.inner_text("#made-from")
            await pg.screenshot(path=str(out / "m4-02-made-part.png"))
            cats = await pg.evaluate("""() => { const i = window.__workshop.index.items;
                const part = i.find((x) => x.kind === 'part' && x.madeFrom?.model === 'local-bevel/bevel-gear');
                return [part?.category, i.find((x) => x.id === 'gen:local-bevel/bevel-gear')?.category] }""")
            check("Save as part writes the part and its recipe; the part is in the Parts Library in its model's category, saying what it was made from",
                  len(made) == 1 and recipe.get("model") == "local-bevel/bevel-gear" and "Made from" in made_text
                  and cats[0] and cats[0] == cats[1], (made, recipe.get("model"), made_text, cats))
            part_hash = await pg.evaluate("location.hash")
            # (0.4.13, F-20) a change elsewhere in the project isn't a change to the part's model
            version_before = await pg.evaluate("() => window.__workshop.catalog.sources.find((s) => s.id === 'local-bevel')?.version")
            (library / "local/bevel/notes.txt").write_text("Not used by the gear.\n")
            await pg.evaluate("import('/ui/library.js').then(async (m) => { await m.rescanLocal(false); await m.rescanLocal(false); })")  # (the second: after any scan already running)
            await pg.wait_for_function("(v) => window.__workshop.catalog.sources.find((s) => s.id === 'local-bevel')?.version !== v", arg=version_before, timeout=60000)
            await pg.goto(B + part_hash)
            await pg.wait_for_selector("#made-from", timeout=20000)
            unchanged_text = await pg.inner_text("#made-from")
            check("A change elsewhere in the model's project doesn't count as a change to the part (the recipe keeps the fingerprint of the files its model uses)",
                  bool(recipe.get("files_id")) and "since" not in unchanged_text, (recipe.get("files_id"), unchanged_text))
            # the model's files change after the part was made: its page says so, and Make again brings it up to date
            recipe["files_id"] = "000000000000"
            recipe_path.write_text(json.dumps(recipe))
            before = made[0].stat().st_mtime_ns
            await pg.evaluate("import('/ui/library.js').then(async (m) => { await m.rescanLocal(false); await m.rescanLocal(false); })")
            await pg.goto(B + part_hash)
            await pg.wait_for_function("(h)=>location.hash===h && /changed since/.test(document.querySelector('#made-from')?.textContent || '')", arg=part_hash, timeout=30000)
            await pg.click("#made-again")
            await pg.wait_for_function(toast_js, arg="Made Test gear again", timeout=300000)
            again = json.loads(recipe_path.read_text())
            check("Make again replaces the part's file and brings its recipe up to the model's files",
                  again.get("files_id") not in (None, "000000000000") and made[0].stat().st_mtime_ns != before
                  and len(list(made[0].parent.glob("*.stl"))) == 1, (again.get("files_id"), list(made[0].parent.iterdir())))
            await pg.goto(B + part_hash)
            await pg.wait_for_selector("#made-open", timeout=20000)
            await pg.click("#made-open")
            await pg.wait_for_function("()=>document.body.dataset.model==='local-bevel/bevel-gear' && location.hash==='#/m/local-bevel/bevel-gear'")
            await pg.wait_for_function(DONE, timeout=300000)
            check("Open with these settings opens the model and makes it", "matches" in await pg.inner_text("#status"))
            # (0.4.11) Batch generate… saves each part to the Parts Library, named from the setting that varies
            await pg.click("#model-more summary")
            await pg.click("#batch-open")
            await pg.wait_for_selector("#batch[open] .batch-range input")
            ins = pg.locator(".batch-range input")
            for i, v in enumerate(("20", "24", "2")):
                await ins.nth(i).fill(v)
            await pg.screenshot(path=str(out / "m4-03-make-several.png"))
            await pg.click("#batch-keep")
            await pg.wait_for_function(toast_js, arg="Saved 3 parts", timeout=300000)
            several = sorted(f.name for f in made[0].parent.glob("*.stl"))
            several_recipes = [json.loads(f.read_text()).get("model") for f in made[0].parent.glob("*.recipe.json")]
            await pg.click("#batch-close")
            check("Batch generate saves each part to the Parts Library with its recipe, named from what varies",
                  len(several) == 4 and several_recipes.count("local-bevel/bevel-gear") == 4, several)
            # (0.4.13, UI-28) the Made parts shelf: no source line, Read again or Versions; cards say what each was made from
            await pg.goto(B + "#/browse/source/local-made-parts")
            await pg.wait_for_selector(".source-panel[data-source=local-made-parts]", timeout=20000)
            shelf = await pg.evaluate("""() => [!!document.querySelector('.source-panel [data-act=reread]'), !!document.querySelector('[data-source-tab=versions]'),
                document.querySelector('.source-panel .source-sub')?.textContent || '',
                [...document.querySelectorAll('.card .card-sub')].map((e) => e.textContent)]""")
            await pg.screenshot(path=str(out / "m4-05-made-parts.png"))
            check("the Made parts shelf has no Read again or Versions, says each part keeps its project's license, and each card names its model and project",
                  not shelf[0] and not shelf[1] and "license" in shelf[2] and shelf[3] and all(" · " in t for t in shelf[3]), shelf)

            # 13h. (0.4.12, F-15) a folder of examples (more than 20 files that only call the main file) is asked
            # about once on the project's page; made into parts, each is made once and listed as a part, kept in
            # metadata.json, and its model page still opens
            blocks = library / "local" / "blocks"
            (blocks / "examples").mkdir(parents=True, exist_ok=True)
            (blocks / "blocks.scad").write_text("module block(w = 1, l = 2) { cube([w * 8, l * 8, 9.6]); }\n")
            (blocks / "plate.scad").write_text("// Plate size\nsize = 4; // [1:10]\nuse <blocks.scad>\nblock(size, size);\n")
            for i in range(1, 23):
                (blocks / "examples" / f"brick-1x{i}.scad").write_text(f"use <../blocks.scad>;\n\nblock(w = 1, l = {i});\n")
            await pg.evaluate("import('/ui/library.js').then(async (m) => { await m.rescanLocal(false); await m.rescanLocal(false); })")
            await pg.goto(B + "#/browse/source/local-blocks")
            await pg.wait_for_selector("#examples-choice [data-examples=parts]", timeout=120000)
            asked = await pg.inner_text("#examples-choice")
            count_js = "() => { const i = window.__workshop.index.items.filter((x) => x.projectId === 'local-blocks'); return [i.filter((x) => x.kind === 'generator').length, i.filter((x) => x.kind === 'part').length] }"
            # (0.4.13, F-21) until it's answered, the examples aren't listed as models
            unasked = await pg.evaluate(count_js)
            await pg.click("#examples-choice [data-examples=parts]")
            # (0.4.13, UI-32) the status line keeps the space before a job's timer
            job_text = ""
            try:
                job_text = await (await pg.wait_for_function("() => { const t = document.querySelector('.status-job')?.innerText || ''; return /\\S · \\d+ s/.test(t) && t }", timeout=120000)).json_value()
            except Exception:
                pass
            check("the status line reads a job and its time with a space before the timer",
                  bool(re.search(r"\S · \d+ s", job_text)), job_text)
            try:
                await pg.wait_for_function(f"() => ({count_js})()[1] >= 22", timeout=300000)
            except Exception:
                pass
            counts = await pg.evaluate(count_js)
            ex_meta = json.loads((library / "sources/local-blocks/metadata.json").read_text()).get("examples")
            brick = await pg.evaluate("() => window.__workshop.index.items.find((x) => x.projectId === 'local-blocks' && x.kind === 'part' && x.name === 'Brick 1x3')?.href")
            await pg.goto(B + (brick or "#/"))
            await pg.wait_for_selector("#part-status a", timeout=20000)
            # (0.4.13, UI-29) the part list sorts by number, and the link opens the example as a model
            listed = await pg.evaluate("() => [...document.querySelectorAll('#part-list a span:first-child')].map((e) => e.textContent)")
            link_text = await pg.inner_text("#part-status a")
            await pg.screenshot(path=str(out / "m4-04-example-part.png"))
            check("example parts are listed by number (1x2 before 1x17) and open as the example's model",
                  listed.index("Brick 1x2") < listed.index("Brick 1x17") and link_text == "Open the example as a model", (listed[:4], link_text))
            await pg.click("#part-status a")
            try:
                await pg.wait_for_function("()=>document.body.dataset.model==='local-blocks/examples-brick-1x3'", timeout=60000)
                await pg.wait_for_function(DONE, timeout=120000)
            except Exception:
                pass
            ex_status = await pg.evaluate("() => [location.hash, document.body.dataset.model, document.querySelector('#status')?.textContent]")
            check("a folder of examples is asked about once; made into parts, each is made and listed as a part, the choice kept, its model page still opens",
                  "22 examples" in asked and counts == [1, 22] and ex_meta == {"folder": "examples", "as": "parts"}
                  and unasked == [1, 0]
                  and ex_status[1] == "local-blocks/examples-brick-1x3" and "matches" in (ex_status[2] or ""), (asked[:80], unasked, counts, ex_meta, brick, ex_status))
            # (0.4.13, F-20) a change to a file the examples don't use doesn't make them again
            ex_files = sorted(p.relative_to(library / "sources/local-blocks/examples").as_posix() for p in (library / "sources/local-blocks/examples").rglob("*.stl"))
            ver_js = "() => window.__workshop.catalog.sources.find((s) => s.id === 'local-blocks')?.version"
            blocks_version = await pg.evaluate(ver_js)
            (blocks / "plate.scad").write_text("// Plate size\nsize = 5; // [1:10]\nuse <blocks.scad>\nblock(size, size);\n")
            await pg.evaluate("import('/ui/library.js').then(async (m) => { await m.rescanLocal(false); await m.rescanLocal(false); })")
            await pg.wait_for_function(f"(v) => ({ver_js})() !== v", arg=blocks_version, timeout=60000)
            todo = await pg.evaluate("() => (window.__workshop.catalog.examples_todo || []).length")
            after = await pg.evaluate(count_js)
            ex_files_after = sorted(p.relative_to(library / "sources/local-blocks/examples").as_posix() for p in (library / "sources/local-blocks/examples").rglob("*.stl"))
            check("example parts are kept by the files they use: changing another file in the project doesn't make them again",
                  todo == 0 and after == [1, 22] and len(ex_files) == 22 and ex_files_after == ex_files, (todo, after, len(ex_files), ex_files[:2], ex_files_after[:2]))

            # 13i. (0.4.14, milestone 5) files dropped on the window: added as one project, one part per name;
            # 3MF and OBJ parts show in 3D (a 3MF's colours when asked), a 3MF's own picture is its thumbnail,
            # and a file another project has already is named
            drops = home / "drops"
            drops.mkdir(parents=True, exist_ok=True)
            model_files.write_stl(drops / "latch.stl")
            model_files.write_3mf(drops / "latch.3mf")
            model_files.write_obj(drops / "knob.obj")
            (drops / "case.step").write_text("ISO-10303-21;\nEND-ISO-10303-21;\n")
            model_files.write_3mf(drops / "badge.3mf", png=model_files.png())
            shutil.copy(made[0], drops / "gear copy.stl")
            await pg.goto(B + "#/")
            await pg.evaluate("() => window.__shimEmit('tauri://drag-enter', {})")
            hint = await pg.is_visible("#drop-hint")
            await pg.evaluate("(p) => window.__shimEmit('tauri://drag-drop', { paths: p, position: { x: 400, y: 300 } })",
                              [str(drops / n) for n in ("latch.stl", "latch.3mf", "knob.obj", "case.step", "badge.3mf", "gear copy.stl")])
            await pg.wait_for_selector(".drop-dialog", timeout=20000)
            await pg.fill("#drop-name", "Drop test")
            await pg.screenshot(path=str(out / "m5-01-drop.png"))
            await pg.click("#drop-project")
            await pg.wait_for_function("(t)=>{const e=document.querySelector('#toast');return e&&!e.hidden&&e.textContent.toLowerCase().includes(t)}", arg="added drop test", timeout=180000)
            drop_toast = await pg.evaluate("() => document.querySelector('#toast').textContent")
            drop_js = "() => window.__workshop.index.items.filter((i) => i.project === 'Drop test').map((i) => [i.name, (i.preview || '').split('.').pop(), i.files.map((f) => f.format).sort().join('+'), !!i.thumb])"
            try:
                await pg.wait_for_function(f"() => ({drop_js})().filter((x) => x[3]).length >= 4", timeout=120000)
            except Exception:
                pass
            dropped = sorted(await pg.evaluate(drop_js))
            check("files dropped on the window are added as one project, one part per name, each viewable (3MF, STL, OBJ) with a thumbnail, and a file already in the library is named",
                  hint and dropped == [["Badge", "3mf", "3mf", True], ["Case", "", "step", False], ["Gear copy", "stl", "stl", True], ["Knob", "obj", "obj", True], ["Latch", "3mf", "3mf+stl", True]]
                  and "gear copy.stl is in made parts already" in drop_toast.lower(), (hint, dropped, drop_toast))
            # (0.4.18, UI-34) and listed on the project's page for as long as it's there
            dup_text = ""
            try:
                dup_id = await pg.evaluate("() => window.__workshop.catalog.sources.find((s) => s.name === 'Drop test')?.id")
                await pg.goto(B + f"#/browse/source/{dup_id}")
                await pg.wait_for_selector("#duplicates [data-dup-open]", timeout=30000)
                dup_text = await pg.inner_text("#duplicates")
            except Exception as e:
                print("duplicates:", e)
            check("a file that's in another project too is listed on its project's page, with that project, Open the other and Hide this copy",
                  "gear copy.stl" in dup_text.lower() and "made parts" in dup_text.lower(), dup_text)
            latch = await pg.evaluate("() => window.__workshop.index.items.find((i) => i.project === 'Drop test' && i.name === 'Latch')?.href")
            await pg.goto(B + latch)
            # (0.4.20, UI-39) its colours are shown unless Show the file's colours is unticked; the swatches hide meanwhile
            await pg.wait_for_selector("#file-colours-hud:not([hidden])", timeout=30000)
            latch_dims = await pg.evaluate("() => !document.querySelector('#dims').hidden && document.querySelector('#dims').textContent")
            latch_colours = await pg.evaluate("() => [document.querySelector('#file-colours').checked, document.querySelector('#filament').hidden]")
            await pg.screenshot(path=str(out / "m5-02-3mf-colours.png"))
            check("a 3MF part shows in 3D at its size, with its own colours unless unticked", "10 × 10 × 16" in (latch_dims or "") and latch_colours == [True, True],
                  (latch_dims, latch_colours))
            # into one of your own projects: the one open
            model_files.write_stl(drops / "spacer.stl")
            await pg.goto(B + "#/browse/source/local-blocks")
            await pg.wait_for_selector(".source-panel[data-source=local-blocks]", timeout=20000)
            await pg.evaluate("(p) => window.__shimEmit('tauri://drag-drop', { paths: p })", [str(drops / "spacer.stl")])
            await pg.wait_for_selector("#drop-own", timeout=20000)
            await pg.click("#drop-own")
            try:
                # (the copy is done when it says so: it reads the project twice first)
                await pg.wait_for_function("() => (document.querySelector('#toast')?.textContent || '').toLowerCase().includes('copied spacer.stl')", timeout=120000)
                await pg.wait_for_function("() => window.__workshop.index.items.some((i) => i.projectId === 'local-blocks' && i.name === 'Spacer')", timeout=60000)
            except Exception:
                pass
            check("a file dropped on one of your own projects is copied into its folder and listed",
                  (library / "local/blocks/spacer.stl").is_file()
                  and await pg.evaluate("() => window.__workshop.index.items.some((i) => i.projectId === 'local-blocks' && i.name === 'Spacer')"), None)

            # 13j. (0.4.15) one Download with a format picker, remembered per part and per model; a model's
            # render as 3MF; near misses made one part and split again
            await pg.goto(B + latch)
            await pg.wait_for_selector("#part-format", timeout=30000)
            part_formats = await pg.evaluate("() => [...document.querySelectorAll('#part-format option')].map((o) => o.value)")
            first_pick = await pg.evaluate("() => document.querySelector('#part-format').value")
            await pg.select_option("#part-format", "3mf")
            await pg.goto(B + "#/")
            await pg.goto(B + latch)
            await pg.wait_for_selector("#part-format", timeout=30000)
            kept_pick = await pg.evaluate("() => [document.querySelector('#part-format').value, document.querySelector('#part-download').textContent, document.querySelector('#part-download').getAttribute('download')]")
            await pg.screenshot(path=str(out / "m5-03-format-picker.png"))
            check("a part with several formats has one Download with a format picker (which names the format), STL first by default, the choice kept for that part",
                  sorted(part_formats) == ["3mf", "stl"] and first_pick == "stl" and kept_pick[0] == "3mf" and kept_pick[1].startswith("Download (") and kept_pick[2] == "latch.3mf",
                  (part_formats, first_pick, kept_pick))
            await pg.goto(B + "#/m/local-bevel/bevel-gear")
            await pg.wait_for_function("()=>document.body.dataset.model==='local-bevel/bevel-gear'", timeout=60000)
            await pg.wait_for_function("() => !document.querySelector('#download').classList.contains('is-disabled')", timeout=300000)
            await pg.select_option("#download-format", "3mf")
            await pg.wait_for_function("() => (document.querySelector('#download').getAttribute('download') || '').endsWith('.3mf') && !document.querySelector('#download').classList.contains('is-disabled')", timeout=120000)
            before_3mf = set(saved_stl.glob("*.3mf"))
            await pg.click("#download")
            for _ in range(50):
                if set(saved_stl.glob("*.3mf")) - before_3mf:
                    break
                await pg.wait_for_timeout(200)
            new_3mf = sorted(set(saved_stl.glob("*.3mf")) - before_3mf)
            names_3mf = zipfile.ZipFile(new_3mf[0]).namelist() if new_3mf else []
            check("a model's render downloads as 3MF when chosen", "3D/3dmodel.model" in names_3mf, (new_3mf, names_3mf))
            # (0.4.19) a model with colours and separate objects: OpenSCAD's own 3MF keeps each object apart and
            # its colours, said in the status line and shown in the 3D view
            (library / "local" / "colours").mkdir(parents=True, exist_ok=True)
            (library / "local" / "colours" / "label.scad").write_text(
                "// Label width\nwidth = 20; // [10:40]\ncolor(\"red\") cube([width, 10, 2]);\n"
                "color(\"white\") translate([2, 2, 2]) cube([5, 5, 1]);\ntranslate([width + 10, 0, 0]) cylinder(h = 5, r = 4);\n")
            await pg.evaluate("import('/ui/library.js').then(async (m) => { await m.rescanLocal(false); await m.rescanLocal(false); })")
            mc_status, mc_objects, mc_colours, mc_stl, mc_part = "", 0, [], [], []
            try:
                await pg.wait_for_function("() => !!window.__workshop.index.get('gen:local-colours/label')", timeout=120000)
                await pg.goto(B + "#/m/local-colours/label")
                await pg.wait_for_function("()=>document.body.dataset.model==='local-colours/label'", timeout=60000)
                await pg.click("#generate")
                await pg.wait_for_function("() => !document.querySelector('#download').classList.contains('is-disabled')", timeout=300000)
                # (0.4.20, UI-39) with STL chosen its colours are made behind the preview: shown, and named
                await pg.select_option("#download-format", "stl")
                await pg.wait_for_function("() => /choose 3MF to keep them/.test(document.querySelector('#status')?.textContent || '')", timeout=120000)
                mc_stl = [await pg.inner_text("#status"), await pg.evaluate("() => !document.querySelector('#file-colours-hud').hidden && document.querySelector('#filament').hidden")]
                await pg.screenshot(path=str(out / "m6-01-colours-stl.png"))
                await pg.select_option("#download-format", "3mf")
                await pg.wait_for_function("() => (document.querySelector('#download').getAttribute('download') || '').endsWith('.3mf') && !document.querySelector('#download').classList.contains('is-disabled')", timeout=120000)
                await pg.wait_for_function("() => /The 3MF has/.test(document.querySelector('#status')?.textContent || '')", timeout=60000)
                mc_status = await pg.inner_text("#status")
                before_3mf = set(saved_stl.glob("*.3mf"))
                await pg.click("#download")
                for _ in range(50):
                    if set(saved_stl.glob("*.3mf")) - before_3mf:
                        break
                    await pg.wait_for_timeout(200)
                got = sorted(set(saved_stl.glob("*.3mf")) - before_3mf)
                model_xml = zipfile.ZipFile(got[0]).read("3D/3dmodel.model").decode() if got else ""
                mc_objects = len(re.findall(r"<item ", model_xml))
                mc_colours = sorted(set(c[:7].upper() for c in re.findall(r'displaycolor="([^"]+)"', model_xml)))
                await pg.screenshot(path=str(out / "m5-05-3mf-colours.png"))
                # (0.4.20, F-28) saved as a part with 3MF chosen: OpenSCAD's 3MF, its recipe saying so
                await pg.click("#save-part")
                await pg.wait_for_selector("#save-part-name", timeout=10000)
                await pg.fill("#save-part-name", "Label in colour")
                await pg.click("#save-part-save")
                for _ in range(120):
                    if list((library / "local" / "Made parts").glob("*/Label in colour.3mf")):
                        break
                    await pg.wait_for_timeout(250)
                made_3mf = list((library / "local" / "Made parts").glob("*/Label in colour.3mf"))
                made_xml = zipfile.ZipFile(made_3mf[0]).read("3D/3dmodel.model").decode() if made_3mf else ""
                made_rec = json.loads(made_3mf[0].with_name("Label in colour.recipe.json").read_text()) if made_3mf else {}
                mc_part = [len(re.findall(r"<item ", made_xml)), made_rec.get("output"), sorted(p.name for p in made_3mf[0].parent.glob("Label in colour.*")) if made_3mf else []]
                await pg.select_option("#download-format", "stl")
            except Exception as e:
                print("multi-colour 3MF:", e)
            check("a model with colours and separate objects downloads as OpenSCAD's own 3MF: each object apart, its colours kept and named in the status line",
                  mc_objects == 3 and "#FF0000" in mc_colours and "#FFFFFF" in mc_colours and "3 parts" in mc_status.lower(),
                  (mc_objects, mc_colours, mc_status))
            check("a model that calls color() shows its colours after the preview, and with STL chosen says to choose 3MF to keep them",
                  bool(mc_stl) and "3 parts in 3 colours: choose 3MF to keep them" in mc_stl[0] and mc_stl[1] is True, mc_stl)
            check("a part saved with 3MF chosen is OpenSCAD's 3MF, objects kept, its recipe naming the format",
                  mc_part == [3, "3mf", ["Label in colour.3mf", "Label in colour.recipe.json"]], mc_part)
            near = library / "local" / "near"
            near.mkdir(parents=True, exist_ok=True)
            model_files.write_3mf(near / "clip.3mf")
            model_files.write_stl(near / "clip_v2.stl")
            await pg.evaluate("import('/ui/library.js').then(async (m) => { await m.rescanLocal(false); await m.rescanLocal(false); })")
            await pg.goto(B + "#/browse/source/local-near")
            near_js = "() => window.__workshop.index.items.filter((i) => i.projectId === 'local-near').map((i) => i.files.map((f) => f.format).sort().join('+')).sort()"
            near_before = near_after = near_split = None
            try:
                await pg.wait_for_selector("#near-misses [data-merge]", timeout=60000)
                near_before = await pg.evaluate(near_js)
                await pg.click("#near-misses [data-merge]")
                await pg.wait_for_function(f"() => ({near_js})().length === 1", timeout=30000)
                near_after = await pg.evaluate(near_js)
                await pg.goto(B + await pg.evaluate("() => window.__workshop.index.items.find((i) => i.projectId === 'local-near').href"))
                await pg.wait_for_selector("#part-split", timeout=20000)
                await pg.click("#part-split")
                await pg.wait_for_function(f"() => ({near_js})().length === 2", timeout=30000)
                near_split = await pg.evaluate(near_js)
            except Exception as e:
                print("near misses:", e)
            check("files whose names differ by a version mark are offered as one part, made one, and split again",
                  near_before == ["3mf", "stl"] and near_after == ["3mf+stl"] and near_split == ["3mf", "stl"], (near_before, near_after, near_split))
            # (0.4.18, UI-35) Keep apart: files that look like one part but aren't, not offered again
            apart_gone, apart_meta = False, None
            try:
                await pg.goto(B + "#/browse/source/local-near")
                await pg.wait_for_selector("#near-misses [data-apart]", timeout=30000)
                await pg.click("#near-misses [data-apart]")
                await pg.wait_for_function("() => !document.querySelector('#near-misses')", timeout=30000)
                apart_gone = True
                apart_meta = json.loads((library / "sources/local-near/metadata.json").read_text()).get("apart")
            except Exception as e:
                print("keep apart:", e)
            check("Keep apart: files that look like one part but aren't stay separate and aren't offered again (kept in metadata.json)",
                  apart_gone and isinstance(apart_meta, list) and len(apart_meta) == 1 and len(apart_meta[0]) == 2, apart_meta)

            # 13k. (0.4.16) a Printables download: its page PDF fills in the project's details; a PDF that
            # can't be read leaves them as for any ZIP
            pz = home / "printables"
            pz.mkdir(exist_ok=True)
            page_pdf = Path(__file__).resolve().parent.parent / "libraries/opengrid-official/printables-model-page.pdf"
            model_files.write_stl(pz / "connector.stl")
            with zipfile.ZipFile(pz / "opengrid.zip", "w") as z:
                z.write(pz / "connector.stl", "files/opengrid-connector-test.stl")
                z.write(page_pdf, "opengrid-walldesk-mounting-framework-and-ecosystem.pdf")
            with zipfile.ZipFile(pz / "damaged.zip", "w") as z:
                z.write(pz / "connector.stl", "files/bracket.stl")
                z.writestr("bracket.pdf", b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n" + os.urandom(2000))
            await pg.goto(B + "#/")
            for zp in ("opengrid.zip", "damaged.zip"):
                await pg.evaluate("(p) => import('/ui/library.js').then((m) => m.addProject({ kind: 'zip', path: p }, 'a Printables download'))", str(pz / zp))
            srcs = await pg.evaluate("() => window.__workshop.catalog.sources.filter((s) => s.origin?.type === 'zip' && /opengrid|damaged/.test(s.origin.file)).map((s) => ({ id: s.id, file: s.origin.file, name: s.name, site: s.origin.site || null, meta: s.meta, from: s.from }))")
            og = next((x for x in srcs if x["file"] == "opengrid.zip"), {})
            dm = next((x for x in srcs if x["file"] == "damaged.zip"), {})
            ogm = og.get("meta") or {}
            check("a Printables download's PDF gives the project its name, creator, license, tags, page, dates and summary, each marked as from the PDF",
                  og.get("name") == "openGrid - Wall/Desk mounting framework and ecosystem"
                  and [a.get("name") for a in ogm.get("authors") or []] == ["David D"]
                  and (ogm.get("license") or {}).get("spdx") == "CC-BY-4.0"
                  and "opengrid" in (ogm.get("tags") or [])
                  and ogm.get("origin", "").startswith("https://www.printables.com/model/1214361")
                  and (ogm.get("fields") or {}).get("Published") == "2025-10-20"
                  and ogm.get("summary") == "This is openGrid, the open wall/desk mounting framework."
                  and (og.get("from") or {}).get("license") == "detected:Printables PDF",
                  {k: og.get(k) for k in ("name", "site", "from")} | {"meta": {k: ogm.get(k) for k in ("authors", "license", "tags", "origin", "fields", "summary")}})
            about = ""
            try:
                await pg.goto(B + f"#/browse/source/{og['id']}")
                await pg.wait_for_selector(".source-panel", timeout=20000)
                await pg.click("[data-source-tab=about]")
                await pg.wait_for_selector("#site-details .site-description li", timeout=20000)
                about = await pg.inner_text("#site-details")
                await pg.screenshot(path=str(out / "m5-04-printables.png"))
                await pg.click("#source-more summary")
                await pg.click("[data-act=edit]")
                await pg.wait_for_selector(".meta-editor .meta-from", timeout=20000)
                about += "\n" + " | ".join(await pg.evaluate("() => [...document.querySelectorAll('.meta-editor .meta-from')].map((e) => e.textContent)"))
                await pg.keyboard.press("Escape")
            except Exception as e:
                print("printables page:", e)
            check("the project's About tab shows who made it, when, and the page's description; Edit details says the values are from the Printables PDF",
                  "by David D" in about and "published Oct 20, 2025" in about and "Version history" in about and "From the Printables PDF" in about, about[:400])
            check("a ZIP whose PDF can't be read is still added, with its details as for any ZIP",
                  dm.get("name") == "Damaged" and dm.get("site") is None and ((dm.get("meta") or {}).get("license") or {}).get("spdx") in (None, "NOASSERTION"), dm)
            # (0.4.18, F-23) another download of the same model: a new version of its project, dated by the
            # page and offered as an update, rather than a second project
            og_json = library / "sources" / og.get("id", "none") / "source.json"
            newer = bytearray((pz / "connector.stl").read_bytes())
            newer[:16] = b"newer download  "
            with zipfile.ZipFile(pz / "opengrid (1).zip", "w") as z:
                z.writestr("files/opengrid-connector-test-v2.stl", bytes(newer))
                z.write(page_pdf, "opengrid-walldesk-mounting-framework-and-ecosystem.pdf")
            n_src = await pg.evaluate("() => window.__workshop.catalog.sources.length")
            await pg.evaluate("(p) => import('/ui/library.js').then((m) => m.addProject({ kind: 'zip', path: p }, 'a newer download'))", str(pz / "opengrid (1).zip"))
            n_src2 = await pg.evaluate("() => window.__workshop.catalog.sources.length")
            og_src = json.loads(og_json.read_text()) if og_json.is_file() else {}
            waiting = (og_src.get("update") or {}).get("latest") or {}
            applied = {}
            try:
                await pg.goto(B + f"#/browse/source/{og['id']}")
                await pg.wait_for_selector(".update-banner [data-update=apply]", timeout=30000)
                await pg.click(".update-banner [data-update=apply]")
                await pg.wait_for_function("() => !document.querySelector('.update-banner')", timeout=60000)
                applied = json.loads(og_json.read_text())
            except Exception as e:
                print("newer download:", e)
            check("another download of a Printables model becomes a new version of its project, dated by the page, used when you choose",
                  n_src2 == n_src and len(og_src.get("versions") or []) == 2 and str(waiting.get("date", "")).startswith("2025-10-20")
                  and applied.get("version") == waiting.get("id") and str(applied.get("version_date", "")).startswith("2025-10-20"),
                  (n_src, n_src2, [v.get("date") for v in og_src.get("versions") or []], waiting, applied.get("version")))
            # (0.4.20, F-26) the same files downloaded again (a ZIP made again, other bytes) aren't another version
            again_text, again_versions = "", -1
            try:
                with zipfile.ZipFile(pz / "opengrid (1).zip") as src_zip, zipfile.ZipFile(pz / "opengrid (2).zip", "w") as z:
                    for zi in src_zip.infolist():
                        z.writestr(zipfile.ZipInfo(zi.filename, date_time=(2026, 1, 2, 3, 4, 6)), src_zip.read(zi))
                await pg.evaluate("(p) => import('/ui/library.js').then((m) => m.addProject({ kind: 'zip', path: p }, 'the same download'))", str(pz / "opengrid (2).zip"))
                await pg.wait_for_function("() => /has these files already/.test(document.querySelector('#toast')?.textContent || '')", timeout=60000)
                again_text = await pg.evaluate("() => document.querySelector('#toast').textContent")
                again_versions = len(json.loads(og_json.read_text()).get("versions") or [])
            except Exception as e:
                print("same download:", e)
            check("the same Printables files downloaded again aren't added as another version",
                  "has these files already" in again_text and again_versions == 2, (again_text, again_versions))
            # (0.4.20, UI-38) the page's own Updated isn't a second Updated filter; Published is grouped like Updated
            og_filters = []
            try:
                await pg.goto(B + f"#/browse/source/{og['id']}")
                await pg.click("[data-source-tab=items]", timeout=20000)    # the About tab, opened above, has no filters
                await pg.wait_for_selector(".results [data-item]", timeout=30000)
                # (0.4.26) a project of 12 items or fewer shows no filters; the whole library's list has them
                if not await pg.locator(".filters").count():
                    await pg.goto(B + "#/browse/parts")
                    await pg.wait_for_selector(".results [data-item]", timeout=30000)
                og_filters = await pg.evaluate("() => [...document.querySelectorAll('[data-filter]')].map((b) => b.dataset.filter)")
                await pg.goto(B + f"#/browse/source/{og['id']}")   # (Read again, below, is on the project's page)
                await pg.wait_for_selector(".source-panel", timeout=20000)
            except Exception as e:
                print("filters:", e)
            check("a Printables project's Updated is never two filters, and Published is among the filters", og_filters.count("updated") <= 1 and "f:Updated" not in og_filters and "f:Published" in og_filters, og_filters)
            # (0.4.18, F-24) Read again reads the page PDF again: details lost (or never read) come back
            refreshed = {}
            try:
                s = json.loads(og_json.read_text())
                s["origin"].pop("site", None)
                s["detected"].pop("from", None)
                s["detected"]["name"] = "Opengrid"
                og_json.write_text(json.dumps(s, indent=1))
                await pg.click("[data-act=reread]")
                for _ in range(240):
                    refreshed = json.loads(og_json.read_text())
                    if (refreshed.get("origin") or {}).get("site"):
                        break
                    await pg.wait_for_timeout(500)
            except Exception as e:
                print("read again:", e)
            check("Read again reads a project's Printables PDF again",
                  ((refreshed.get("origin") or {}).get("site") or {}).get("name") == "printables"
                  and (refreshed.get("detected") or {}).get("name") == "openGrid - Wall/Desk mounting framework and ecosystem",
                  (refreshed.get("origin"), (refreshed.get("detected") or {}).get("name")))

            # 13l. (0.4.17) a collection: a part (from the inspector), a made part and a model with changed
            # settings (from their pages); quantities and printed; exported as one ZIP with a parts list
            coll_rows, coll_names, coll_csv, coll_saved, printed_text = [], [], "", None, ""
            try:
                await pg.goto(B + "#/")
                await pg.click("#new-collection")
                await pg.fill("#collect-name", "Test bench")
                await pg.click("#collect-save")
                await pg.wait_for_selector(".nav-collections [data-scope='coll:test-bench']", timeout=10000)
                await pg.goto(B + "#/browse/parts")
                await pg.evaluate("() => import('/ui/state.js').then((m) => m.ui.set({ selection: ['part:folder-drop-test/latch'], inspector: true }))")
                await pg.click("[data-act=collect]")
                await pg.click("[data-collection=test-bench]")
                await pg.click("#collect-save")
                made = await pg.evaluate("() => window.__workshop.index.items.find((i) => i.kind === 'part' && i.madeFrom)?.href")
                await pg.goto(B + made)
                await pg.wait_for_selector("#part-collect:not([hidden])", timeout=20000)
                await pg.click("#part-collect")
                await pg.click("[data-collection=test-bench]")
                await pg.click("#collect-save")
                await pg.goto(B + "#/m/local-bevel/bevel-gear")
                await pg.wait_for_function("()=>document.body.dataset.model==='local-bevel/bevel-gear'", timeout=60000)
                await pg.evaluate("() => { const i = document.querySelector('#params input[type=number]'); i.value = String(Number(i.value) + 2); i.dispatchEvent(new Event('input', { bubbles: true })); i.dispatchEvent(new Event('change', { bubbles: true })); }")
                await pg.click("#model-collect")
                await pg.click("[data-collection=test-bench]")
                await pg.click("#collect-save")
                await pg.goto(B + "#/browse/coll/test-bench")
                await pg.wait_for_function("() => document.querySelectorAll('#collection-items tbody tr').length === 3", timeout=20000)
                await pg.fill("#collection-items tbody tr:first-child [data-qty]", "3")
                await pg.press("#collection-items tbody tr:first-child [data-qty]", "Tab")
                await pg.check("#collection-items tbody tr:nth-child(2) [data-printed]")
                await pg.wait_for_function("() => document.querySelector('#collection-printed')?.textContent === '1 of 5 printed'", timeout=10000)
                printed_text = await pg.inner_text("#collection-printed")
                coll_rows = await pg.evaluate("() => [...document.querySelectorAll('#collection-items tbody tr')].map((r) => r.dataset.ref)")
                await pg.screenshot(path=str(out / "m5-05-collection.png"))
                for z in saved_stl.glob("Test bench*.zip"):
                    z.unlink()
                await pg.click("#collection-export")
                for _ in range(600):
                    found = sorted(saved_stl.glob("Test bench*.zip"))
                    if found:
                        coll_saved = found[0]
                        break
                    await pg.wait_for_timeout(500)
                if coll_saved:
                    await pg.wait_for_timeout(500)
                    zf = zipfile.ZipFile(coll_saved)
                    coll_names = zf.namelist()
                    coll_csv = zf.read("parts.csv").decode()
            except Exception as e:
                print("collections:", e)
            stored = json.loads((library / "collections/test-bench/collection.json").read_text()) if (library / "collections/test-bench/collection.json").is_file() else {}
            check("a collection keeps a part, a made part and a model with its settings, by id, with quantities and what's printed",
                  len(coll_rows) == 3 and coll_rows[0] == "part:folder-drop-test/latch" and coll_rows[2] == "gen:local-bevel/bevel-gear"
                  and printed_text == "1 of 5 printed" and [i.get("quantity") for i in stored.get("items", [])] == [3, 1, 1]
                  and bool((stored.get("items") or [{}] * 3)[2].get("values")), (coll_rows, printed_text, stored.get("items")))
            stls = [n for n in coll_names if n.lower().endswith((".stl", ".3mf"))]
            check("Export as ZIP: a file per copy (models made at their settings), and parts.csv with each item's quantity and license",
                  len(stls) == 5 and "parts.csv" in coll_names and "README.txt" in coll_names and "Latch,3," in coll_csv and "Bevel" in coll_csv,
                  (coll_names, coll_csv[:300]))
            # (0.4.20, UI-40) several copies are counted as they're printed; (F-27) a model entry keeps the files it
            # was added with, and says when its model has changed; (UI-42) Home lists collections
            count_text, count_stored, stale, kept_id, home_coll, one_file = "", None, "", None, "", []
            coll_json = library / "collections/test-bench/collection.json"
            try:
                await pg.goto(B + "#/browse/coll/test-bench")
                await pg.wait_for_selector("#collection-items tbody tr:first-child [data-printed-more]", timeout=20000)
                await pg.click("#collection-items tbody tr:first-child [data-printed-more]")
                await pg.wait_for_function("() => document.querySelector('#collection-printed')?.textContent === '2 of 5 printed'", timeout=10000)
                count_text = await pg.evaluate("() => document.querySelector('#collection-items tbody tr:first-child [data-printed-count]').textContent.replace(/\\s+/g, ' ').trim()")
                count_stored = json.loads(coll_json.read_text())["items"][0].get("printed")
                c = json.loads(coll_json.read_text())
                kept_id = c["items"][2].get("files_id")
                c["items"][2]["files_id"] = "000000000000"
                coll_json.write_text(json.dumps(c, indent=2))
                await pg.evaluate("() => import('/ui/collections.js').then((m) => m.refreshCollections())")
                await pg.wait_for_selector("[data-stale]", timeout=10000)
                stale = await pg.inner_text("[data-stale]")
                await pg.screenshot(path=str(out / "m6-02-collection.png"))
                await pg.click("[data-use-current]")
                await pg.wait_for_function("() => !document.querySelector('[data-stale]')", timeout=10000)
                # (UI-41) a part with one file: Download with its format named beside it
                made_href = await pg.evaluate("() => window.__workshop.index.items.find((i) => i.kind === 'part' && i.madeFrom && i.files?.length === 1)?.href")
                await pg.goto(B + made_href)
                await pg.wait_for_selector("#part-download", timeout=20000)
                one_file = await pg.evaluate("() => [document.querySelector('#part-download').textContent, document.querySelector('#part-format-label')?.textContent]")
                await pg.goto(B + "#/")
                await pg.wait_for_selector("#home-collections", timeout=10000)
                home_coll = await pg.evaluate("() => document.querySelector('#home-collections').textContent")
            except Exception as e:
                print("collections, 0.4.20:", e)
            now_id = json.loads(coll_json.read_text())["items"][2].get("files_id") if coll_json.is_file() else None
            check("a collection counts printed copies (1 of 3), and a model entry keeps its files and says when they've changed",
                  "1 of 3" in count_text and count_stored == 1 and bool(kept_id) and "earlier version" in stale.lower() and now_id == kept_id,
                  (count_text, count_stored, kept_id, stale, now_id))
            check("Home lists collections with how many are printed", "Test bench" in home_coll and "2 of 5 printed" in home_coll, home_coll)
            check("a part with one file has Download (size) with its format beside it", bool(one_file) and one_file[0].startswith("Download (") and one_file[1] in ("STL", "3MF"), one_file)

            # (0.4.22) Check library: every file the tests' app wrote is in this version's format;
            # a planted unfinished copy is repaired, a planted entry that refers to nothing is listed
            checked = {}
            try:
                (library / "sources" / ".adding-99999" / "files").mkdir(parents=True, exist_ok=True)
                put_coll = "(c) => import('/ui/library.js').then((m) => m.api('collection_put', { collection: c }))"
                await pg.evaluate(put_coll, {"name": "Planted", "items": [{"id": "x1", "ref": "gen:no-such-project/none", "name": "Nothing", "quantity": 1}]})
                await pg.goto(B + "#/library-settings")
                await pg.click("#library-check", timeout=20000)
                await pg.wait_for_selector("#check-result", timeout=60000)
                listed = lambda sel: pg.evaluate(f"() => [...document.querySelectorAll('{sel} li')].map((l) => l.dataset.checkPath)")
                checked["repair"], checked["look"] = await listed("#check-repair"), await listed("#check-look")
                await pg.screenshot(path=str(out / "lf-01-check.png"))
                await pg.click("#library-repair", timeout=10000)
                await pg.wait_for_function("() => !document.querySelector('#library-repair')", timeout=60000)
                checked["after"] = await listed("#check-look")
            except Exception as e:
                print("check library:", e)
            print("check library:", json.dumps(checked, indent=1))
            recipe_files = list((library / "recipes").glob("*.json")) + list((library / "local").rglob("*.recipe.json"))
            shapes = [json.loads(f.read_text()) for f in recipe_files]
            check("every library file the app wrote says its format; Check library repairs an unfinished copy and lists an entry that refers to nothing",
                  checked.get("repair") == ["sources/.adding-99999"] and "collections/planted/collection.json" in checked.get("look", [])
                  and not (library / "sources" / ".adding-99999").exists() and "collections/planted/collection.json" in checked.get("after", [])
                  and bool(shapes) and all(r.get("format") == 1 and r.get("kind") in ("settings", "part") and "version" not in r and "date" not in r for r in shapes)
                  and json.loads((library / "library.json").read_text()).get("format") == 2,
                  (checked, [(f.name, r.get("format"), r.get("kind")) for f, r in zip(recipe_files, shapes) if r.get("format") != 1 or r.get("kind") not in ("settings", "part")]))

            # 14. (0.4.3) back up the library to a ZIP, then restore it as a new library
            backup = home / "backups" / "library backup.zip"
            backup.parent.mkdir(parents=True, exist_ok=True)
            await pg.goto(B + "#/library-settings")
            await pg.wait_for_selector("#backup-leave-out")
            urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(backup)}).encode(), method="POST"))
            await pg.click("#backup-leave-out")
            await pg.click("#library-backup")
            for _ in range(600):
                if backup.is_file():
                    break
                await pg.wait_for_timeout(500)
            names = zipfile.ZipFile(backup).namelist() if backup.is_file() else []
            thumbs_in = any("/thumbs/" in n for n in names)
            check("Back up library: one ZIP of the library folder, without thumbnails when asked",
                  "library.json" in names and any(n.startswith("recipes/") for n in names) and any(n.startswith("sources/") for n in names) and not thumbs_in,
                  (len(names), thumbs_in))
            n_sources = await pg.evaluate("() => window.__workshop.catalog.sources.length")
            restored_dir = home / "restored library"
            for pick in (backup, restored_dir):
                urllib.request.urlopen(urllib.request.Request(B + "test/pick", data=json.dumps({"path": str(pick)}).encode(), method="POST"))
            await pg.click("#library-restore")
            try:
                await pg.wait_for_function(f"() => window.__workshop?.catalog?.library?.path?.includes('restored library')", timeout=180000)
            except Exception:
                pass
            await pg.wait_for_function("() => !!window.__workshop?.catalog", timeout=60000)
            r_path = await pg.evaluate("() => window.__workshop.catalog.library?.path")
            r_sources = await pg.evaluate("() => window.__workshop.catalog.sources.length")
            check("Restore from backup: a new library folder, opened, with the same projects",
                  (restored_dir / "library.json").is_file() and "restored library" in (r_path or "") and r_sources == n_sources, (r_path, r_sources, n_sources))
            await pg.screenshot(path=str(out / "p4-06-restored.png"))
            await b.close()
    finally:
        server.terminate()
        server.wait(timeout=20)

    # 11. portable: the library opens the same after moving it
    moved = home / "moved" / "My library"
    shutil.copytree(library, moved)

    def summary(path):
        r = subprocess.run([a.cli, "library-summary", "--library", str(path), "--app-site", a.app_site], capture_output=True, text=True)
        if r.returncode:
            print(r.stderr)
        return json.loads(r.stdout)
    s1, s2 = summary(library), summary(moved)
    check("library opens the same after moving", s1 == s2, f"{len(s1['sources'])} projects, {len(s1['models'])} models")
    (out / "library-summary.json").write_text(json.dumps(s1, indent=1))

    # (0.4.22) a library in format 1 (as 0.4.21 left it) is upgraded when it's opened: its files
    # backed up first, recipes in one shape, and Check library finds nothing left to repair
    old = home / "format 1 library"
    for rel, text in {
        "library.json": '{"format": 1, "id": "lib-old", "name": "Old", "favourites": []}',
        "recipes/a1.json": '{"id": "a1", "model": "zip-x/box", "name": "Wide", "values": {"w": 30}, "version": "v1"}',
        "local/Made parts/Box/Box 2.recipe.json": '{"model": "zip-x/box", "values": {}, "date": "2026-10-01T00:00:00Z", "format": "3mf"}',
        "local/Made parts/Box/Box 2.3mf": "x",
        "collections/c/collection.json": '{"id": "c", "name": "C", "items": []}',
    }.items():
        (old / rel).parent.mkdir(parents=True, exist_ok=True)
        (old / rel).write_text(text)
    r = subprocess.run([a.cli, "library-call", "--library", str(old), "--app-site", a.app_site, "library_check", "{}"], capture_output=True, text=True)
    try:
        oc = json.loads(r.stdout)
    except Exception:
        oc = {"stdout": r.stdout[-400:], "stderr": r.stderr[-400:]}
    a1 = json.loads((old / "recipes/a1.json").read_text())
    box = json.loads((old / "local/Made parts/Box/Box 2.recipe.json").read_text())
    check("a format 1 library is upgraded when opened: backed up first, recipes in one shape, nothing left to repair",
          json.loads((old / "library.json").read_text()).get("format") == 2 and len(list((old / "backups").glob("format-1-*.zip"))) == 1
          and a1.get("project_version") == "v1" and "version" not in a1 and box.get("output") == "3mf" and box.get("created") == "2026-10-01T00:00:00Z"
          and box.get("name") == "Box 2" and oc.get("repair") == [],
          (oc, a1, box))
    shutil.make_archive(str(out / "library"), "zip", library)

    for c in calls:
        print("FAILED CALL:", c)
    for e in errors:
        print("PAGE ERROR:", e)
    failed = [n for n, ok in results if not ok]
    print(f"{len(results) - len(failed)} of {len(results)} checks passed")
    return 1 if failed or errors else 0


sys.exit(asyncio.run(main()))
