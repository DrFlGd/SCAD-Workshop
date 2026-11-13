#!/usr/bin/env python3
"""Pictures of the components the app ships with, made when the app is built (Review 7, item 6).

    python3 tools/component_thumbs.py --cli desktop/target/release/workshop-cli --desktop build/desktop \\
        --engine build/native/squashfs-root [--only BOSL2] [--port 8796]

Starts the app's backend (`workshop-cli serve`) with an empty library, opens the page in
headless Chromium and presses Library settings › Make previews, so the pictures are made
the way the app makes them (the same renders, viewer and size). Then copies them into
<desktop>/libs/thumbs/<library>/<module>.webp, where the app finds them for its own copy
of each library. A component that waits for a value of the user's, or doesn't render, has
none. Writes <desktop>/libs/thumbs/summary.json (made, and without one, per library).
"""
import argparse
import asyncio
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

from playwright.async_api import async_playwright

ROOT = Path(__file__).resolve().parent.parent
ap = argparse.ArgumentParser()
ap.add_argument("--cli", type=Path, required=True)
ap.add_argument("--desktop", type=Path, default=Path("build/desktop"), help="the app's files (ui/, site/, libs/)")
ap.add_argument("--engine", type=Path, required=True)
ap.add_argument("--port", type=int, default=8796)
ap.add_argument("--only", help="one library's name (testing)")
ap.add_argument("--timeout", type=int, default=45, help="minutes")
a = ap.parse_args()
libs = a.desktop / "libs"
if not (libs / "libraries.json").is_file():
    sys.exit(f"no libraries in {libs} (tools/fetch_libraries.py, then tools/build_desktop.py)")
B = f"http://127.0.0.1:{a.port}/"


def start(home: Path):
    log = open(home / "serve.log", "w")
    p = subprocess.Popen([str(a.cli), "serve", "--ui", str(a.desktop / "ui"), "--app-site", str(a.desktop / "site"),
                          "--engine", str(a.engine), "--home", str(home), "--library", str(home / "library"),
                          "--port", str(a.port)], stdout=log, stderr=log)
    for _ in range(150):
        try:
            urllib.request.urlopen(B, timeout=1)
            return p
        except Exception:
            time.sleep(0.2)
    p.kill()
    sys.exit("the server didn't start: " + (home / "serve.log").read_text()[-2000:])


async def make(home: Path):
    async with async_playwright() as pw:
        b = await pw.chromium.launch(args=["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"])
        ctx = await b.new_context(viewport={"width": 1400, "height": 900})
        await ctx.add_init_script(path=str(ROOT / "tests/tauri_shim.js"))
        pg = await ctx.new_page()
        pg.on("pageerror", lambda e: print("page error:", e, file=sys.stderr))
        await pg.goto(B)
        await pg.wait_for_function("() => window.__workshop?.index?.items?.length > 0", timeout=120000)
        comps = await pg.evaluate("""(only) => window.__workshop.index.items.filter((i) => i.kind === 'component' && (!only || i.project === only))
            .map((i) => ({ key: i.key, library: i.project, module: i.module, waits: !!i.needs?.length }))""", a.only)
        if a.only:
            # the others count as having one already, so only this library's are made
            await pg.evaluate("(only) => window.__workshop.index.items.forEach((i) => { if (i.kind === 'component' && i.project !== only) i.thumb = 'skip'; })", a.only)
        await pg.goto(B + "#/library-settings/libraries")
        button = pg.locator("#component-previews")
        await button.wait_for(timeout=60000)
        print(await button.inner_text(), flush=True)
        t0 = time.time()
        await button.click()
        await pg.wait_for_function("() => /Making component previews/.test(document.body.textContent)", timeout=60000)
        last, said = "", time.time()
        while True:
            text = await pg.evaluate("() => (document.body.textContent.match(/Making component previews · \\d+ of \\d+/) || [''])[0]")
            if not text:
                break
            last = text
            if time.time() - t0 > a.timeout * 60:
                sys.exit(f"not done after {a.timeout} minutes ({last})")
            if time.time() - said > 60:
                print(f"{time.time() - t0:.0f} s: {text}", flush=True)
                said = time.time()
            await asyncio.sleep(2)
        print(f"done in {time.time() - t0:.0f} s", flush=True)
        await b.close()
        return comps


def main():
    home = Path(tempfile.mkdtemp(prefix="component-thumbs-"))
    out = libs / "thumbs"
    shutil.rmtree(out, ignore_errors=True)  # (pictures from before would count as made)
    server = start(home)
    try:
        comps = asyncio.run(make(home))
    finally:
        server.terminate()
        server.wait(10)
    made_dir = home / "library/thumbs/components"
    summary = {}
    for c in comps:
        slug = c["key"].split("/")[0].lstrip("@")
        s = summary.setdefault(c["library"], {"made": 0, "waits": 0, "none": []})
        found = list(made_dir.glob(f"{slug}/app@*/{c['module']}.webp"))
        if found:
            (out / slug).mkdir(parents=True, exist_ok=True)
            shutil.copy2(found[0], out / slug / f"{c['module']}.webp")
            s["made"] += 1
        elif c["waits"]:
            s["waits"] += 1
        else:
            s["none"].append(c["module"])
    out.mkdir(parents=True, exist_ok=True)
    (out / "summary.json").write_text(json.dumps(summary, indent=1) + "\n")
    size = sum(p.stat().st_size for p in out.rglob("*.webp"))
    for name, s in sorted(summary.items()):
        print(f"{name}: {s['made']} pictures, {s['waits']} wait for a value, {len(s['none'])} without one{': ' + ', '.join(s['none'][:12]) if s['none'] else ''}")
    print(f"{sum(s['made'] for s in summary.values())} pictures, {size / 1e6:.1f} MB -> {out}")
    shutil.rmtree(home, ignore_errors=True)


if __name__ == "__main__":
    main()
