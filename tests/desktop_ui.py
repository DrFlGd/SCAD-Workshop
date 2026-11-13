#!/usr/bin/env python3
"""Drive the built desktop app through tauri-driver (WebDriver) and save screenshots.

    tauri-driver &                     # needs WebKitWebDriver (Linux) or msedgedriver (Windows)
    python3 tests/desktop_ui.py --app desktop/target/release/scad-workshop --out shots

Before the app starts, its library (~/SCAD Workshop unless --library) is made from
tests/old-library (two starter projects of an earlier app) with tests/projects in its
local/ folder, unless it's there already.

Checks: the library interface loads (every model with a thumbnail), search answers
quickly, adding a project from GitHub works, native renders work, opened models
become tabs, saved settings land in the library folder, the settings page shows the
library and engine, a ready-made part previews, Components (the bundled libraries'
modules: listed by topic, a BOSL2 bevel gear and a threads-scad bolt render
natively, the libraries in Library settings), and the night theme applies. Exits
non-zero on any failure.
"""
import argparse
import json
import os
import re
import shutil
import sys
import time
from pathlib import Path

from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.common.options import ArgOptions

ap = argparse.ArgumentParser()
ap.add_argument("--app", required=True)
ap.add_argument("--out", default="shots")
ap.add_argument("--library", "--workspace", dest="library", help="expected library folder (default: ~/SCAD Workshop)")
ap.add_argument("--driver", default="http://127.0.0.1:4444")
a = ap.parse_args()
out = Path(a.out)
out.mkdir(parents=True, exist_ok=True)
library = Path(a.library) if a.library else Path.home() / "SCAD Workshop"
TESTS = Path(__file__).resolve().parent
if not (library / "library.json").is_file():
    shutil.copytree(TESTS / "old-library", library, dirs_exist_ok=True)
    for project in sorted((TESTS / "projects").iterdir()):
        if project.is_dir():
            shutil.copytree(project, library / "local" / project.name)

opts = ArgOptions()
opts.set_capability("browserName", "wry")
opts.set_capability("tauri:options", {"application": str(Path(a.app).resolve())})
print("starting a session…", flush=True)
d = webdriver.Remote(command_executor=a.driver, options=opts)
print("session started", flush=True)
d.set_script_timeout(30)
try:
    d.set_window_size(1400, 900)
except Exception as e:  # some drivers can't resize
    print("resize:", e, flush=True)
failures, report = [], {}


def shot(name):
    d.save_screenshot(str(out / f"{name}.png"))


def wait(js, timeout=180, what=""):
    print(f"waiting for {what or js[:60]}…", flush=True)
    end = time.time() + timeout
    while time.time() < end:
        try:
            if d.execute_script(f"return !!({js})"):
                return True
        except Exception:
            pass
        time.sleep(0.25)
    failures.append(f"timed out waiting for {what or js}")
    try:
        shot(f"timeout-{len(failures):02d}")
        print("page says:", d.execute_script("return document.body.innerText.slice(0, 400)"), flush=True)
        print("errors:", d.execute_script("return window.__errors || []"), flush=True)
    except Exception as e:
        print("couldn't inspect the page:", e, flush=True)
    return False


def text(sel):
    return d.execute_script(f"const n=document.querySelector({json.dumps(sel)});return n?n.textContent:''")


DONE = "(()=>{const s=document.querySelector('#status');return s&&(s.classList.contains('ok')||s.classList.contains('error'))})()"


def open_model(key, timeout=300):
    d.execute_script(f"location.hash = '#/m/{key}'")
    wait(f"document.body.dataset.model === '{key}'", 60, f"{key} to open")
    ok = wait(DONE, timeout, f"{key} to render")
    status = text("#status")
    report[key] = status
    print(("PASS " if "matches" in status else "FAIL ") + key + " :: " + status, flush=True)
    if not ok or "matches" not in status:
        failures.append(f"{key}: {status}")


try:
    if not wait("document.querySelector('.home .tile')", 90, "the home page"):
        raise SystemExit("the interface never appeared; skipping the rest")
    time.sleep(0.5)
    shot("00-home")
    report["engine"] = text("#engine")
    report["platform"] = d.execute_script("return document.body.dataset.platform || ''")
    print("engine:", report["engine"], "| platform:", report["platform"])
    if "native" not in report["engine"]:
        failures.append(f"engine label: {report['engine']}")

    # the library: every model with a thumbnail (your own projects' are made when they're first read), in all three views
    d.execute_script("location.hash = '#/browse/all'")
    wait("document.querySelector('.results [data-item]')", 30, "the generator list")
    shown = d.execute_script("return document.querySelectorAll('.results [data-item]').length")
    wait("document.querySelectorAll('.results .card img').length >= document.querySelectorAll('.results [data-item]').length", 180, "every model's thumbnail")
    thumbs = d.execute_script("return document.querySelectorAll('.results .card img').length")
    # thumbnails load lazily: the first row must actually have loaded
    loaded = wait("[...document.querySelectorAll('.results .card img')].slice(0, 3).every(i => i.complete && i.naturalWidth > 0)", 15, "thumbnails to load")
    report["library"] = {"generators": shown, "thumbnails": thumbs, "first_row_loaded": loaded}
    print("library:", report["library"], flush=True)
    if thumbs < shown:
        failures.append(f"thumbnails: {thumbs} of {shown}")
    shot("00-library-grid")
    for layout in ("list", "table"):
        d.execute_script(f"document.querySelector('button[data-layout=\"{layout}\"]').click()")
        if wait(f"document.querySelector('.results[data-layout=\"{layout}\"] [data-item]')", 10, f"{layout} view"):
            n = d.execute_script("return document.querySelectorAll('.results [data-item]').length")
            if n != shown:
                failures.append(f"{layout} view shows {n} of {shown}")
    shot("00-library-table")
    d.execute_script("document.querySelector('button[data-layout=\"grid\"]').click()")

    # search: typed into the top box, answered from the index
    d.execute_script("""const i = document.querySelector('#global-search'); i.value = 'tray';
      i.dispatchEvent(new Event('input', {bubbles: true}));""")
    wait("/ms$/.test(document.querySelector('.browse-count')?.textContent || '')", 10, "search results")
    report["search_tray"] = text(".browse-count")
    print("search 'tray':", report["search_tray"], flush=True)
    ms = report["search_tray"].split("·")[-1].strip().split()[0] if "·" in report["search_tray"] else "999"
    if float(ms.lstrip("<")) >= 100:
        failures.append(f"search took {report['search_tray']}")

    # the test projects (local/) and the starter projects (sources/) are in the library
    sources = sorted(p.name for p in (library / "sources").iterdir()) if (library / "sources").is_dir() else []
    report["library_sources"] = sources
    if not {"gridfinity-basket", "hsw", "local-bins", "local-labels", "local-parts"} <= set(sources):
        failures.append(f"the test projects aren't all in {library}: {sources}")

    # adding a project from GitHub: the app's own network access (TLS, GitHub's API, tarball)
    d.execute_script("document.querySelector('#add-project').click()")
    if wait("document.querySelector('#add-url')", 15, "the add dialog"):
        d.execute_script("""const i = document.querySelector('#add-url'); i.value = 'https://github.com/chrisspen/gears';
          i.dispatchEvent(new Event('input', {bubbles: true})); document.querySelector('#add-go').click();""")
        if wait("location.hash.includes('/source/')", 600, "the added project"):
            time.sleep(1)
            n = d.execute_script("return window.__workshop.index.items.filter(i => i.projectId === location.hash.split('/source/')[1]).length")
            report["added_from_github"] = n
            print("added from GitHub:", n, "items", flush=True)
            if not n:
                failures.append("the project added from GitHub has no items")
            shot("00-added-project")
        else:  # leave the page usable for the rest of the checks
            d.execute_script("document.querySelector('.dialog [aria-label=Close]')?.click()")

    open_model("hsw/grid")
    time.sleep(1)
    shot("01-starter-project")
    open_model("local-labels/label")
    time.sleep(1)
    shot("02-label")
    # a file included by the model's others: rendered with include once (Review 7, item 1), again after a change
    for i, w in enumerate(("2", "3")):
        if i == 0:
            open_model("local-bins/bin")
        else:
            d.execute_script(f"""const i = document.querySelector('[data-name="width"] input'); i.value = '{w}';
              i.dispatchEvent(new Event('input', {{bubbles: true}})); document.querySelector('#generate').click();""")
            time.sleep(0.5)
            wait(DONE, 120, "bin re-render")
        report[f"bin_{i + 1}"] = {"status": text("#status"), "dims": text("#dims")}
        print("bin:", report[f"bin_{i + 1}"], flush=True)
        if "matches" not in report[f"bin_{i + 1}"]["status"]:
            failures.append(f"bin at width {w}: {report[f'bin_{i + 1}']}")

    # saved settings go to the library as files
    open_model("local-bins/tray")
    d.execute_script("""
      const i = document.querySelector('#p-units_wide'); i.value = '4'; i.dispatchEvent(new Event('input', {bubbles: true}));""")
    d.find_element(By.ID, "settings-save").click()
    time.sleep(0.3)
    d.execute_script("document.querySelector('#save-name').value = 'CI four wide'")
    d.find_element(By.ID, "save-primary").click()
    wait("document.querySelector('#settings-note') && document.querySelector('#settings-note').textContent.startsWith('Saved')", 20, "save note")
    saved = list((library / "recipes").glob("*.json"))
    names = [json.loads(p.read_text()).get("name") for p in saved]
    report["saved_files"] = names
    print("saved settings files:", names)
    if "CI four wide" not in names:
        failures.append(f"saved settings not in {library}: {names}")

    d.execute_script("location.hash = '#/settings'")
    wait("document.querySelector('.profile-grid')", 20, "settings page")
    time.sleep(0.5)
    shot("03-settings")
    report["settings_text"] = text("#about-body")[:600]
    report["version"] = text("#app-version")
    if not re.search(r"SCAD Workshop \d+\.\d+\.\d+", report["version"]):
        failures.append(f"no version number in Settings: {report['version']!r}")

    # library settings: projects, categories, trash
    d.execute_script("location.hash = '#/library-settings'")
    if wait("document.querySelector('#ls-project-table tbody tr') && document.querySelector('#ls-category-table tbody tr')", 30, "library settings"):
        report["library_settings_projects"] = d.execute_script("return document.querySelectorAll('#ls-project-table tbody tr').length")
        time.sleep(0.5)
        shot("03b-library-settings")

    d.execute_script("location.hash = '#/parts/local-parts/hook'")
    if wait("!document.querySelector('#dims').hidden", 60, "part preview"):
        report["part_dims"] = text("#dims")
    time.sleep(0.5)
    shot("04-part")

    # Components: the libraries shipped with the app, their modules as forms, rendered natively
    n_comp = d.execute_script("return window.__workshop.index.items.filter(i => i.kind === 'component').length")
    heads = d.execute_script("return [...document.querySelectorAll('.sidebar [data-section-row] .nav-label')].map(e => e.textContent.trim())")
    report["components"] = {"count": n_comp, "menu": heads}
    print("components:", report["components"], flush=True)
    if n_comp < 700 or "Components" not in heads:
        failures.append(f"components: {report['components']}")
    open_model("@bosl2/bevel_gear")
    time.sleep(1)
    shot("04b-component-bevel-gear")
    # the side viewer (0.4): the component's code, and its library's README in a frame that can't run scripts
    d.execute_script("document.querySelector('#model-extra [data-side-open=code]')?.click()")
    if wait("/bevel_gear\\(/.test(document.querySelector('.side-panel[data-side-panel=code] .code')?.textContent || '')", 30, "the side viewer's Code"):
        code = text(".side-panel .code")
        d.execute_script("document.querySelector('[data-side-tab=readme]')?.click()")
        ok = wait("/BOSL2/.test(document.querySelector('.doc-frame')?.contentDocument?.querySelector('h1')?.textContent || '')", 30, "the BOSL2 README")
        report["side_viewer"] = {"code": code[:80], "readme": ok, "pdf_viewer": d.execute_script("return navigator.pdfViewerEnabled")}
        print("side viewer:", report["side_viewer"], flush=True)
        if "bevel_gear(" not in code:
            failures.append(f"side viewer code: {code[:120]}")
        shot("04c-side-viewer")
        # (0.4.10, UI-21) the side tabs' labels run down, in every web view: each tab is taller than it is wide
        tabs_wh = d.execute_script("return [...document.querySelectorAll('.side-tab')].map(b => [b.dataset.sideTab, Math.round(b.getBoundingClientRect().width), Math.round(b.getBoundingClientRect().height)])")
        report["side_tabs"] = tabs_wh
        print("side tabs:", tabs_wh, flush=True)
        if not tabs_wh or any(h <= w * 1.5 for _, w, h in tabs_wh):
            failures.append(f"side tabs aren't upright: {tabs_wh}")
        d.execute_script("document.querySelector('.side-close')?.click()")
    open_model("@threads-scad/MetricBolt")
    d.execute_script("location.hash = '#/library-settings'")
    if wait("document.querySelectorAll('#ls-libraries tbody tr').length >= 5", 30, "the libraries in Library settings"):
        report["libraries"] = d.execute_script("return [...document.querySelectorAll('#ls-libraries tbody tr')].map(r => r.dataset.library + ':' + r.dataset.provider)")
        print("libraries:", report["libraries"], flush=True)

    # every generator opened above stays open as a tab
    tabs = d.execute_script("return [...document.querySelectorAll('.tabbar [data-tab], .tabbar [data-tab-more]')].map(t => t.dataset.tab || t.dataset.tabMore)")
    report["tabs"] = tabs
    print("tabs:", tabs, flush=True)
    for key in ("hsw/grid", "local-labels/label", "local-bins/bin", "local-bins/tray"):
        if key not in tabs:
            failures.append(f"no tab for {key}")

    # (0.4.5) the default skin, its bundled font and its colours mixed in the top bar, in the real webview
    d.execute_script("location.hash = '#/browse/all'")
    wait("document.querySelector('.results [data-item]')", 20, "the library again")
    report["skin"] = d.execute_script("""return [document.documentElement.dataset.skin, [...document.fonts].some((f) => f.family.replace(/["']/g, "") === "Overpass" && f.status === "loaded"),
        getComputedStyle(document.querySelector('.topsearch')).borderTopColor]""")
    print("skin:", report["skin"], flush=True)
    if report["skin"][0] != "drafting" or not report["skin"][1] or report["skin"][2] in ("", "rgba(0, 0, 0, 0)"):
        failures.append(f"skin not applied as expected: {report['skin']}")
    shot("04d-drafting-skin")

    # night theme, on the library
    for _ in range(3):
        if d.execute_script("return document.documentElement.dataset.theme") == "night":
            break
        d.execute_script("document.querySelector('#theme-toggle').click()")
    report["theme"] = d.execute_script("return document.documentElement.dataset.theme")
    if report["theme"] != "night":
        failures.append(f"theme button never reached night: {report['theme']}")
    time.sleep(0.5)
    shot("05-night")
    errors = d.execute_script("return (window.__errors || []).slice(0, 20)")
    if errors:
        failures.extend(errors)
finally:
    (out / "desktop-ui.json").write_text(json.dumps({"report": report, "failures": failures}, indent=1))
    d.quit()

for f in failures:
    print("FAILED:", f)
sys.exit(1 if failures else 0)
