# MitCad's `.f3d` reader (a pinned copy)

These crates are [MitCad](https://github.com/mitcad/Mitcad)'s reader for Fusion 360 files
(`.f3d`, `.f3z`), copied from the commit in `PIN.json` by `tools/fetch_mitcad.py`. MitCad is
MIT-licensed (`Copyright (c) 2026 MitCad contributors`); the crates keep their `SPDX-License-Identifier`
headers and READMEs. Nothing here is edited by hand: a change goes in `patches/` and the copy is refreshed.

The app uses them in `desktop/core/src/f3d.rs` to list an `.f3d`'s user parameters (the settings form)
and its saved preview, without MitCad installed, and to hand MitCad the design as this copy reads it
(`mitcad-cli import-f3d --dump`, 0.4.28): so a patch here reaches MitCad without changing MitCad.
Generating needs MitCad's own `mitcad-cli` (`desktop/core/src/mitcad.rs`), which Settings › Engine can
download: the release `PIN.json` records, the one this copy comes from. Experimental:
docs/DESKTOP_PLAN.md, "Fusion 360 files through MitCad".

| Crate | What |
| --- | --- |
| `vec3` | `[f64; 3]` helpers |
| `zip` | The zip container (deflate and zstd, pure Rust) |
| `asm` | The ASM B-rep reader (the bodies; needed by the names and the design) |
| `dump` | The design dump IR: parameters, components, timeline, as Rust types |
| `f3d-names` | The names of B-rep entities that feature inputs refer to |
| `f3d-file` | The document inside the container: entries, previews, display meshes |
| `f3d-decode` | The design streams decoded: records, parameters, sketches |
| `f3d-design` | The design segment built into the dump IR (the entry points `package`, `decode_streams`) |

**Patches** (held back from upstream for now: owner, 2026-10-29, test internally first):

- `0001-user-parameters-before-version-8.patch`: user parameters of older writers (class version 5,
  Fusion 2.0.13881 of 2022) keep their comment before the number; the reader looked for the number
  within 24 bytes and so read none of them. With it, every parameter of the owner's test file reads,
  and MitCad builds the file with them from the app's reading (0.4.28).

**Refreshing:** `python3 tools/fetch_mitcad.py` (the pinned commit), or move the pin to a MitCad release
with `--commit <the tag's commit> --release <tag>` (the reader and the MitCad the app downloads move
together: the dump the app hands over is MitCad's internal format); `--from-git <clone>` takes the files
from a local clone. The script rewrites each `Cargo.toml` (MitCad's
crates inherit version, edition and license from their workspace), drops the corpus tests, applies the
patches and records the content hash. When a patch no longer applies, upstream has changed that code:
check whether the fix landed there and drop the patch.
