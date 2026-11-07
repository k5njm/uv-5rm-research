#!/usr/bin/env python3
"""Describe a build directory as manifest.json for the web builder (no flashing, no network).

  make_manifest.py BUILD_DIR OUT.json

BUILD_DIR holds the CI/container outputs: fw-<variant>.bin and <app>-<style>.app files. Everything in
the manifest is read from the files themselves (sizes, hashes, ABI hash, arena use, segment CRCs) or
from firmware/variants.json, so the page can validate a selection without trusting file names.
The page's rules (slot count, slot size, arena, core headroom) come from `limits`, which this tool
takes from the same constants verify_release.py enforces.
"""
import hashlib, json, os, re, struct, sys, zlib

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
LIMITS = {"slots": 9, "slot_bytes": 32 * 1024, "arena_bytes": 8192, "core_area_bytes": 60 * 1024,
          "min_core_headroom": 64, "min_arena_spare": 64}


def sha(b):
    return hashlib.sha256(b).hexdigest()


def abi_hash():
    with open(os.path.join(FW, "firmware-source", "abi", "src", "lib.rs"), "rb") as f:
        return f"{zlib.crc32(f.read()) & 0xFFFFFFFF:08x}"


def read_app(path):
    with open(path, "rb") as f:
        d = f.read()
    magic, bh, cnt = struct.unpack_from("<4sIB", d, 0)
    if magic != b"OVL1":
        raise SystemExit(f"{path}: not an OVL1 package")
    segs = []
    for i in range(cnt):
        off, ilen, bss, ent, crc = struct.unpack_from("<IIIII", d, 28 + 20 * i)
        if zlib.crc32(d[off:off + ilen]) & 0xFFFFFFFF != crc:
            raise SystemExit(f"{path}: segment {i} crc mismatch")
        segs.append({"image_bytes": ilen, "bss_bytes": bss, "arena_use": ilen + bss})
    name = d[12:28].split(b"\0")[0].decode("ascii", "replace")
    return {"display_name": name, "abi_hash": f"{bh:08x}", "package_bytes": len(d), "segments": segs,
            "sha256": sha(d)}


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    build, out = sys.argv[1:]
    with open(os.path.join(FW, "variants.json")) as f:
        variants = json.load(f)
    cores = []
    for n in sorted(os.listdir(build)):
        m = re.fullmatch(r"fw-(\w+)\.bin", n)
        if not m:
            continue
        vid = "default" if m.group(1) == "default" else m.group(1)
        if vid not in variants:
            raise SystemExit(f"{n}: variant {vid!r} is not in variants.json")
        with open(os.path.join(build, n), "rb") as f:
            d = f.read()
        sp, rst = struct.unpack_from("<II", d, 0)
        cores.append({"id": vid, "file": n, "sha256": sha(d), "bytes": len(d),
                      "headroom_bytes": LIMITS["core_area_bytes"] - len(d),
                      "boards": variants[vid]["boards"], "rf_chip": variants[vid]["rf_chip"],
                      "vectors_ok": 0x20000000 < sp <= 0x20004000 and rst & 1 == 1})
    apps = {}
    for n in sorted(os.listdir(build)):
        m = re.fullmatch(r"(\w+?)-(classic|d74|proxx|ft5d)\.app", n)
        if not m:
            continue
        a = read_app(os.path.join(build, n))
        a["file"] = n
        app = apps.setdefault(m.group(1), {"id": m.group(1), "styles": {}})
        app["styles"][m.group(2)] = a
    manifest = {"schema": 1, "abi_hash": abi_hash(), "limits": LIMITS, "core": cores,
                "apps": sorted(apps.values(), key=lambda a: a["id"]),
                "notes": ["Images are raw (.bin) and overlay (.app) files built by CI/the container. "
                          "Core images still need wrapping into .BF before any flashing flow exists.",
                          "UNVERIFIED on real hardware."]}
    with open(out, "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")
    print(f"manifest: {len(cores)} core image(s), {sum(len(a['styles']) for a in apps.values())} app build(s) -> {out}")


if __name__ == "__main__":
    main()
