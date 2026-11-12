#!/usr/bin/env python3
"""Build the set of files one radio needs: the nine launcher apps (in slot order) plus the extras, for the CURRENT ABI.

  python3 firmware/tools/build_release.py OUT_DIR      (in the dev container: scripts/dev.sh release)

Apps are built from source with the same toolchain mode as the core (`mkapp.py` default), so their ABI hash matches a
core built from this tree. The committed firmware/apps/*.app are NOT used: they carry the legacy ABI hash and only load
on the legacy core. Writes OUT_DIR/layout.json (slot order for install_apps.py). The core image is added by dev.sh.
"""
import json, os, subprocess, sys
from provenance import PACKAGES, SRC

LAYOUT = ["settings.app", "chanmgr.app", "searchqt.app", "spectrum.app", "bandsweep.app",
          "foxhunt.app", "rptfinder.app", "text.app", "fm.app"]          # launcher slots 1-9, as setup_radio.py
EXTRAS = {"hamtools.app": ("HAM TOOLS", ["apps/hamtools"]), "fskprobe.app": ("FSK PROBE", ["apps/fskprobe"])}


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    todo = {n: PACKAGES[n] for n in LAYOUT} | EXTRAS
    for pkg, (name, dirs) in todo.items():
        p = subprocess.run([sys.executable, os.path.join(SRC, "tools", "mkapp.py"), *[os.path.join(SRC, d) for d in dirs],
                            "--name", name, "-o", os.path.join(out, pkg)], capture_output=True, text=True)
        lines = [l for l in p.stdout.splitlines() if l.startswith("[OK")]
        if p.returncode != 0:
            print(f"  {pkg:14} BUILD FAILED\n{p.stdout[-600:]}\n{p.stderr[-600:]}")
            return 1
        for l in lines[:-1]:
            print(f"  {pkg:14} {l}")
    with open(os.path.join(out, "layout.json"), "w") as f:
        json.dump({"slots": LAYOUT, "extras": sorted(EXTRAS)}, f, indent=2)
        f.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
