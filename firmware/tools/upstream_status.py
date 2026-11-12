#!/usr/bin/env python3
"""Is our firmware-source still at upstream bf-5rh-firmware-open's head, and what have we patched?

  python3 firmware/tools/upstream_status.py [--clone DIR]     (needs git and network; read-only; runs no upstream code)

Compares every file of upstream HEAD with firmware/firmware-source and prints: the baseline we were
last checked against, whether upstream has moved since, and the files that differ (the patch set a future
merge must reconcile). Use a 3-way merge with base = BASELINE when upstream does move.
"""
import os, subprocess, sys, tempfile

URL = "https://github.com/sophiel-meow/bf-5rh-firmware-open"
BASELINE = "e52b236"   # upstream head our tree was based on (2026-09-06 "spectrum app"), checked 2026-11-07
SKIP = ("at32f421-pac/", ".lock", "flake.")
OURS = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "firmware-source")


def git(repo, *a):
    return subprocess.run(["git", "-C", repo, *a], capture_output=True, text=True, check=True).stdout


def main():
    args = sys.argv[1:]
    dest = args[args.index("--clone") + 1] if "--clone" in args else tempfile.mkdtemp(prefix="bf5rh-")
    if not os.path.exists(os.path.join(dest, ".git")):
        subprocess.run(["git", "clone", "-q", URL, dest], check=True)
    else:
        subprocess.run(["git", "-C", dest, "fetch", "-q", "origin"], check=True)
    head = git(dest, "rev-parse", "--short=7", "origin/HEAD" if "origin/HEAD" in git(dest, "branch", "-r") else "HEAD").strip()
    print(f"upstream head {head}; our baseline {BASELINE}: " + ("UNCHANGED, nothing to resync" if head == BASELINE else "UPSTREAM HAS MOVED"))
    if head != BASELINE:
        print(git(dest, "log", "--oneline", f"{BASELINE}..{head}") or "(baseline not in history)")
    differ, missing = [], []
    for f in git(dest, "ls-tree", "-r", "--name-only", head).split():
        if any(s in f for s in SKIP):
            continue
        p = os.path.join(OURS, f)
        if not os.path.exists(p):
            missing.append(f)
        elif git(dest, "hash-object", p).strip() != git(dest, "rev-parse", f"{head}:{f}").strip():
            differ.append(f)
    print(f"{len(differ)} upstream files differ from ours (our patches):")
    for f in differ:
        print("  " + f)
    if missing:
        print(f"{len(missing)} upstream files are missing from ours:")
        for f in missing:
            print("  " + f)
    return 0 if head == BASELINE else 2


if __name__ == "__main__":
    sys.exit(main())
