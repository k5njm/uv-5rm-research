#!/usr/bin/env python3
"""Rebuild every shipped overlay (firmware/apps/*.app) from firmware-source and compare bytes.

  python3 firmware/tools/provenance.py [--strict]     (run inside the dev container: scripts/dev.sh provenance)

Prints IDENTICAL / DIFFERENT per package. --strict exits 1 if a package listed in EXPECTED_IDENTICAL
differs (a regression); packages not listed are reported as known skew (AUDIT.md finding 6).
"""
import os, struct, subprocess, sys, tempfile

FW = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(FW, "firmware-source")
# package file -> (display name in the header, crate dirs in segment order)
PACKAGES = {
    "bandsweep.app": ("BAND SWEEP", ["apps/bandsweep"]),
    "chanmgr.app": ("CHAN MGR", ["apps/chanmgr"]),
    "fm.app": ("FM", ["apps/fm"]),
    "foxhunt.app": ("FOX HUNT", ["apps/foxhunt"]),
    "rptfinder.app": ("RPT FINDER", ["apps/rptfind"]),
    "scanqt.app": ("SCANQT", ["apps/scanqt"]),
    "search.app": ("SEARCH", ["apps/search"]),
    "searchqt.app": ("SEARCH/QT SCAN", ["apps/qtscan2", "apps/search2"]),
    "settings.app": ("SETTINGS", ["apps/settings/menu", "apps/settings/radio", "apps/settings/system"]),
    "spectrum.app": ("SPECTRUM", ["apps/spectrum"]),
    "text.app": ("TEXT", ["apps/textapp"]),
}
EXPECTED_IDENTICAL = {  # measured 2026-11: these rebuild byte-identically; a change here is a regression
    "bandsweep.app", "fm.app", "foxhunt.app", "rptfinder.app", "scanqt.app",
    "searchqt.app", "settings.app", "spectrum.app", "text.app",
}  # chanmgr.app and search.app are known skew (AUDIT.md finding 14)


def segments(d):
    n = d[8]
    return [struct.unpack_from("<IIIII", d, 28 + 20 * i) for i in range(n)]


def explain(a, b):
    if a == b:
        return "IDENTICAL"
    sa, sb = segments(a), segments(b)
    if len(sa) != len(sb):
        return f"DIFFERENT: {len(sa)} vs {len(sb)} segments"
    parts = []
    for i, ((ao, al, ab, ae, _), (bo, bl, bb, be, _)) in enumerate(zip(sa, sb)):
        ia, ib = a[ao:ao + al], b[bo:bo + bl]
        if (al, ab) != (bl, bb):
            parts.append(f"seg{i} size {al}+{ab} vs {bl}+{bb}")
        else:
            nd = sum(x != y for x, y in zip(ia, ib))
            parts.append(f"seg{i} {nd} code bytes differ" if nd else f"seg{i} code identical" + ("" if ae == be else f", entry {ae:#x} vs {be:#x}"))
    return "DIFFERENT: " + "; ".join(parts)


def main():
    strict = "--strict" in sys.argv
    env = dict(os.environ)
    results = {}
    with tempfile.TemporaryDirectory() as tmp:
        for pkg, (name, dirs) in PACKAGES.items():
            out = os.path.join(tmp, pkg)
            p = subprocess.run([sys.executable, os.path.join(SRC, "tools", "mkapp.py"), *[os.path.join(SRC, d) for d in dirs],
                                "--name", name, "-o", out], capture_output=True, text=True, env=env)
            if p.returncode != 0:
                results[pkg] = f"BUILD FAILED: {p.stderr.strip().splitlines()[-1] if p.stderr.strip() else p.stdout[-200:]}"
                continue
            with open(out, "rb") as f, open(os.path.join(FW, "apps", pkg), "rb") as g:
                results[pkg] = explain(g.read(), f.read())
    bad = []
    for pkg, r in results.items():
        print(f"  {pkg:15} {r}")
        if r != "IDENTICAL" and pkg in EXPECTED_IDENTICAL:
            bad.append(pkg)
    n = sum(r == "IDENTICAL" for r in results.values())
    print(f"{n}/{len(results)} shipped packages rebuild byte-identically")
    return 1 if (strict and bad) else 0


if __name__ == "__main__":
    sys.exit(main())
