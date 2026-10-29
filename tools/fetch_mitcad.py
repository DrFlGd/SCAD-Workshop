#!/usr/bin/env python3
"""Refresh the copy of MitCad's `.f3d` reader in desktop/core/mitcad/ (experimental, 0.4.27).

    python3 tools/fetch_mitcad.py                      # at the pinned commit (desktop/core/mitcad/PIN.json)
    python3 tools/fetch_mitcad.py --commit <sha>       # move the pin
    python3 tools/fetch_mitcad.py --from-git ~/Mitcad  # from a local clone instead of GitHub

MitCad (https://github.com/mitcad/Mitcad, MIT) reads Fusion 360's `.f3d` files
in pure Rust. The app uses its reader crates to list an `.f3d`'s user parameters
and its saved preview without MitCad installed; MitCad itself (`mitcad-cli`) is
only needed to generate. The crates are copied in, not fetched at build time, so
`cargo build` and the crates-vendor workflow (which copies desktop/core alone)
keep working offline.

What this does: downloads the repository at the pinned commit, copies the crates
listed below (their `src/`, `Cargo.toml` and README; not their corpus tests or
tools), rewrites each `Cargo.toml` so it builds inside this workspace (the
workspace keys MitCad inherits become explicit), applies the patches in
desktop/core/mitcad/patches/ (fixes waiting to go upstream), and records the
commit and the content hash in PIN.json. A build without the hash matching
isn't checked: PIN.json is for people and the next refresh.
"""
import argparse
import hashlib
import io
import json
import re
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "desktop" / "core" / "mitcad"
PIN = DEST / "PIN.json"
REPO = "mitcad/Mitcad"
# the reader (core/f3d-design and what it pulls in); the facade crate `core/f3d` (the inspection
# tool and the corpus tests) and the expression crate aren't needed
CRATES = ["vec3", "zip", "asm", "dump", "f3d-names", "f3d-file", "f3d-decode", "f3d-design"]
# versions the workspace's own pins allow (MitCad asks for the same crates, slightly older)
DEPS = {
    "serde": '{ version = "1", features = ["derive"] }',
    "serde_json": '{ version = "1", features = ["float_roundtrip"] }',
    "miniz_oxide": '{ version = "0.9", default-features = false, features = ["with-alloc"] }',
    "ruzstd": '{ version = "0.9", default-features = false, features = ["std"] }',
}


def fetch_tree(commit, from_git):
    """The repository's `core/` tree at the commit, as {relative path: bytes}."""
    files = {}
    if from_git:
        out = subprocess.run(["git", "-C", from_git, "archive", "--format=tar", commit, "core"], check=True, capture_output=True).stdout
        tf = tarfile.open(fileobj=io.BytesIO(out))
        prefix = ""
    else:
        url = f"https://github.com/{REPO}/archive/{commit}.tar.gz"
        print(f"downloading {url}")
        data = urllib.request.urlopen(url, timeout=120).read()
        tf = tarfile.open(fileobj=io.BytesIO(data), mode="r:gz")
        prefix = None
    for m in tf.getmembers():
        if not m.isfile():
            continue
        name = m.name
        if prefix is None:
            prefix = name.split("/")[0] + "/"
        if not name.startswith(prefix):
            continue
        rel = name[len(prefix):]
        if rel.startswith("core/"):
            files[rel[len("core/"):]] = tf.extractfile(m).read()
    return files


def rewrite_manifest(text, name):
    """MitCad's crates inherit version, edition and license from their workspace: made explicit here."""
    out = []
    for line in text.splitlines():
        s = line.strip()
        if s == "version.workspace = true":
            line = 'version = "0.0.0"'
        elif s == "edition.workspace = true":
            line = 'edition = "2024"'
        elif s == "license.workspace = true":
            line = 'license = "MIT"'
        elif s in ("rust-version.workspace = true", "publish.workspace = true"):
            continue
        else:
            m = re.match(r"^(\w+)\s*(?:\.workspace\s*=\s*true|=\s*\{.*\})\s*$", s)
            if m and m.group(1) in DEPS and not s.startswith("mitcad-"):
                line = f"{m.group(1)} = {DEPS[m.group(1)]}"
        out.append(line)
    text = "\n".join(out) + "\n"
    if "[dev-dependencies]" in text:
        # the corpus tests and their helpers aren't copied
        text = text.split("[dev-dependencies]")[0].rstrip() + "\n"
    return text


def content_hash(dest):
    h = hashlib.sha256()
    for p in sorted(x for x in dest.rglob("*") if x.is_file() and x.name != "PIN.json"):
        h.update(f"{p.relative_to(dest).as_posix()}\0".encode())
        h.update(hashlib.sha256(p.read_bytes()).hexdigest().encode())
        h.update(b"\n")
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--commit", help="the commit to pin (default: PIN.json's)")
    ap.add_argument("--from-git", help="a local clone of MitCad to take the files from")
    a = ap.parse_args()
    pin = json.loads(PIN.read_text()) if PIN.is_file() else {}
    commit = a.commit or pin.get("commit")
    if not commit:
        sys.exit("no commit: give --commit or keep desktop/core/mitcad/PIN.json")
    files = fetch_tree(commit, a.from_git)
    if not files:
        sys.exit("nothing fetched: is the commit right?")
    patches = sorted((DEST / "patches").glob("*.patch")) if (DEST / "patches").is_dir() else []
    keep = {p.relative_to(DEST) for p in patches} | {Path("README.md"), Path("PIN.json")}
    for p in list(DEST.rglob("*")):
        if p.is_file() and p.relative_to(DEST) not in keep:
            p.unlink()
    for crate in CRATES:
        n = 0
        for rel, data in files.items():
            top = rel.split("/")[0]
            if top != crate:
                continue
            inner = rel[len(crate) + 1:]
            if not (inner.startswith("src/") or inner in ("Cargo.toml", "README.md")):
                continue
            target = DEST / crate / inner
            target.parent.mkdir(parents=True, exist_ok=True)
            if inner == "Cargo.toml":
                target.write_text(rewrite_manifest(data.decode(), crate))
            else:
                target.write_bytes(data)
            n += 1
        if not n:
            sys.exit(f"crate {crate} not found at {commit}")
        print(f"{crate}: {n} files")
    for p in patches:
        print(f"applying {p.name}")
        subprocess.run(["patch", "-p1", "-d", str(DEST), "-i", str(p), "--no-backup-if-mismatch"], check=True)
    for d in list(DEST.rglob("*")):
        if d.is_dir() and not any(d.iterdir()):
            d.rmdir()
    pin = {"repository": f"https://github.com/{REPO}", "commit": commit, "crates": CRATES, "content_sha256": content_hash(DEST),
           "patches": [p.name for p in patches]}
    PIN.write_text(json.dumps(pin, indent=2) + "\n")
    print(f"pinned {commit[:12]} ({pin['content_sha256'][:12]}…)")


if __name__ == "__main__":
    main()
