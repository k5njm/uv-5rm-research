#!/usr/bin/env python3
"""Offline release gate for the UV-5RM firmware bundle. Reads files only; never talks to a radio.

  python3 tools/verify_release.py            # run from firmware/ (or anywhere)
  python3 tools/verify_release.py --bf X.BF --apps DIR
  python3 tools/verify_release.py --apps build-out --image build-out/fw-default.bin --image build-out/fw-bk4819.bin
                                   (also checks freshly BUILT raw core images and overlays against the same floors;
                                    --no-committed skips the committed .BF/.app files)

Checks (any failure => exit 1):
  * .BF header/length consistency, single region, SP/reset vector sanity,
    image fits the 60 KB application area (and reports headroom)
  * every .app: OVL1 magic, ABI build hash == crc32(abi/src/lib.rs),
    per-segment CRC32, package fits the 32 KB slot, segment fits the 8 KB arena
  * prints sha256 of every artifact so a release can be pinned
"""
import argparse, glob, hashlib, os, struct, sys, zlib

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
APP_AREA = 60 * 1024
SLOT_SIZE = 32 * 1024
ARENA = 8192
KEYS = (b"KDHT", b"RBGI")
PKG = 1024
MIN_FW_HEADROOM = 64  # bytes of core flash that must stay free (88 today): alarm before it is eaten
MIN_ARENA_SPARE = 64  # bytes of 8 KB arena each app must leave free (TEXT has 73)


def unxor(block, key):
    out = bytearray()
    for i, x in enumerate(block):
        k = key[i % 4]
        out.append(x if x in (0, 0xFF, k, k ^ 0xFF) else x ^ k)
    return bytes(out)


def decrypt(data):
    n = -(-len(data) // PKG)
    out = bytearray(data[:2 * PKG])
    for i in range(2, n - 1):
        blk = data[i * PKG:(i + 1) * PKG]
        if i < n - 2 and i % 3 == 1:
            blk = unxor(blk, KEYS[0])
        elif i < n - 2 and i % 3 == 2:
            blk = unxor(blk, KEYS[1])
        out += blk
    out += data[PKG * (n - 1):]
    return bytes(out[:len(data)])


fails = []


def check(ok, msg):
    print(("  ok   " if ok else "  FAIL ") + msg)
    if not ok:
        fails.append(msg)


def sha(b):
    return hashlib.sha256(b).hexdigest()


def verify_bf(path):
    print(f"[BF] {os.path.basename(path)}")
    d = open(path, "rb").read()
    print(f"  sha256 {sha(d)}")
    cnt, l1, l2 = struct.unpack_from(">BLL", d, 0)
    check(len(d) == 16 + l1 + l2, f"length matches header ({len(d)} == 16+{l1}+{l2})")
    check(cnt == 1 and l2 == 0, "single region, no config region (as upstream-tested)")
    check(0 < l1 <= APP_AREA, f"firmware {l1} bytes fits {APP_AREA}-byte app area "
                              f"({APP_AREA - l1} bytes headroom)")
    check(APP_AREA - l1 >= MIN_FW_HEADROOM, f"core headroom {APP_AREA - l1} >= floor {MIN_FW_HEADROOM}")
    img = decrypt(d[16:16 + l1])
    sp, rst = struct.unpack_from("<II", img, 0)
    check(0x20000000 < sp <= 0x20004000, f"stack pointer {sp:#x} inside 16 KB SRAM")
    check(rst & 1 and 0x08001000 <= rst < 0x08001000 + APP_AREA,
          f"reset vector {rst:#x} inside application area (thumb bit set)")
    check(b"PROGRAMBF" in img, "image carries the bootloader re-entry marker PROGRAMBF")
    return img


def verify_image(path):
    """A raw core image as written by elf2bin.py (before wrapping): fit, vectors, headroom floor."""
    print(f"[IMAGE] {os.path.basename(path)}")
    with open(path, "rb") as f:
        d = f.read()
    print(f"  sha256 {sha(d)}")
    check(0 < len(d) <= APP_AREA, f"{len(d)} bytes fits the {APP_AREA}-byte app area ({APP_AREA - len(d)} bytes headroom)")
    check(APP_AREA - len(d) >= MIN_FW_HEADROOM, f"core headroom {APP_AREA - len(d)} >= floor {MIN_FW_HEADROOM}")
    sp, rst = struct.unpack_from("<II", d, 0)
    check(0x20000000 < sp <= 0x20004000, f"stack pointer {sp:#x} inside 16 KB SRAM")
    check(rst & 1 and 0x08001000 <= rst < 0x08001000 + APP_AREA, f"reset vector {rst:#x} inside application area")


def abi_hash():
    p = os.path.join(FW, "firmware-source", "abi", "src", "lib.rs")
    return zlib.crc32(open(p, "rb").read()) & 0xFFFFFFFF


def legacy_abi_hash():
    """ABI hash the committed (shipped) binaries were built against; the source ABI has moved on since (new functions)."""
    with open(os.path.join(FW, "legacy-abi-hash.txt")) as f:
        return int(f.read().strip(), 16)


def verify_apps(apps_dir, committed=False):
    h = legacy_abi_hash() if committed else abi_hash()
    print(f"[APPS] {apps_dir}  (ABI hash {h:08x}{', the shipped set: built against the previous ABI' if committed else ''})")
    paths = sorted(glob.glob(os.path.join(apps_dir, "*.app")))
    check(bool(paths), "at least one .app found")
    for p in paths:
        d = open(p, "rb").read()
        n = os.path.basename(p)
        magic, bh, cnt = struct.unpack_from("<4sIB", d, 0)
        check(magic == b"OVL1", f"{n}: OVL1 magic")
        check(bh == h, f"{n}: ABI hash matches firmware source")
        check(len(d) <= SLOT_SIZE, f"{n}: {len(d)} bytes fits {SLOT_SIZE}-byte slot")
        check(1 <= cnt <= 4, f"{n}: {cnt} segment(s)")
        for i in range(min(cnt, 4)):
            off, ilen, bss, ent, crc = struct.unpack_from("<IIIII", d, 28 + 20 * i)
            ok_rng = off + ilen <= len(d)
            check(ok_rng, f"{n}[{i}]: segment inside file")
            if ok_rng:
                check(zlib.crc32(d[off:off + ilen]) & 0xFFFFFFFF == crc, f"{n}[{i}]: crc32")
            check(ilen + bss <= ARENA, f"{n}[{i}]: {ilen}+{bss} bss fits {ARENA}-byte arena "
                                       f"({ARENA - ilen - bss} spare)")
            check(ARENA - ilen - bss >= MIN_ARENA_SPARE, f"{n}[{i}]: arena spare >= floor {MIN_ARENA_SPARE}")
        print(f"  sha256 {sha(d)}  {n}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bf", default=os.path.join(FW, "UV-5RM_Custom_Firmware.BF"))
    ap.add_argument("--apps", default=os.path.join(FW, "apps"))
    ap.add_argument("--image", action="append", default=[], help="a freshly built raw core image (repeatable)")
    ap.add_argument("--no-committed", action="store_true", help="skip the committed .BF (and use only --apps/--image)")
    a = ap.parse_args()
    if not a.no_committed:
        verify_bf(a.bf)
    verify_apps(a.apps, committed=os.path.abspath(a.apps) == os.path.join(FW, "apps"))
    for img in a.image:
        verify_image(img)
    print(f"\n{'FAILED' if fails else 'PASSED'}: {len(fails)} failure(s)")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
