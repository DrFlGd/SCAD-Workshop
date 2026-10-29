#!/usr/bin/env python3
"""A stand-in for MitCad's mitcad-cli (tests only): records what it was asked (MITCAD_STUB_LOG) and
writes a small STL or 3MF, one triangle per parameter set, so a test can tell the settings reached it."""
import json, shutil, struct, sys, os
args = sys.argv[1:]
log = open(os.environ.get("MITCAD_STUB_LOG", os.devnull), "a")
log.write(json.dumps(args) + "\n")
if args[0] == "import-f3d":
    src = args[1]; save = args[args.index("--save") + 1]; report = args[args.index("--report") + 1]
    print("importing", src)
    json.dump({"format": "mitcad", "version": 2, "parameters": [], "features": []}, open(save, "w"))
    json.dump({"counts": {"parametric": 70, "partial": 2, "fallback": 4}}, open(report, "w"))
    sys.exit(0)
if args[0] == "run":
    script = json.load(open(args[1])); opened = args[args.index("--open") + 1]
    assert os.path.isfile(opened), opened
    out = [s for s in script if s.get("cmd") == "export"][0]["path"]
    sets = {s["name"]: s["expression"] for s in script if s.get("cmd") == "set_parameter"}
    log.write("SET " + json.dumps(sets) + "\n")
    # one triangle per set parameter so the output reflects the settings
    n = max(1, len(sets))
    if out.endswith(".stl"):
        data = b"\0" * 80 + struct.pack("<I", n) + b"".join(struct.pack("<12fH", 0,0,1, 0,0,0, 1,0,0, 0,1,0, 0) for _ in range(n))
        open(out, "wb").write(data)
    else:
        open(out, "wb").write(b"PK3MF-stub " + str(n).encode())
    print(json.dumps({"ok": True, "set": len(sets)}))
    sys.exit(0)
print("mitcad-cli: unknown command", file=sys.stderr); sys.exit(1)
