#!/usr/bin/env python3
"""Compare build outputs with firmware/expected-hashes.json (or rewrite it with --update).

  pin_hashes.py DIR            every *.bin / *.app in DIR must match the pinned sha256 (and nothing may be missing)
  pin_hashes.py DIR --update   record the current hashes (do this only for an intentional change, and say why in the PR)

Builds are reproducible across hosts (checked arm64 vs amd64, scripts/dev.sh repro), so a hash that moves
means source, toolchain or ABI changed. The pins are for images built by CI/the container, never the committed .BF.
"""
import hashlib, json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
PINS = os.path.join(os.path.dirname(HERE), "expected-hashes.json")
args = [a for a in sys.argv[1:] if a != "--update"]
update = "--update" in sys.argv[1:]
if len(args) != 1:
    raise SystemExit(__doc__)
found = {}
for n in sorted(os.listdir(args[0])):
    if n.endswith((".bin", ".app")):
        with open(os.path.join(args[0], n), "rb") as f:
            found[n] = hashlib.sha256(f.read()).hexdigest()
if update:
    with open(PINS, "w") as f:
        json.dump(found, f, indent=2, sort_keys=True)
        f.write("\n")
    print(f"pinned {len(found)} images")
    raise SystemExit(0)
with open(PINS) as f:
    want = json.load(f)
bad = [n for n in sorted(set(want) | set(found)) if want.get(n) != found.get(n)]
for n in sorted(set(want) | set(found)):
    print(("  ok   " if n not in bad else "  DIFF ") + n + ("" if n not in bad else f"  pinned={str(want.get(n))[:12]} built={str(found.get(n))[:12]}"))
print("PINS MATCH" if not bad else f"{len(bad)} hash(es) differ")
sys.exit(1 if bad else 0)
