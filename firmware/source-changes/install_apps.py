"""Install the launcher apps of a release folder (scripts/dev.sh release) into the radio's app slots. Apps only: no logo.

  install_apps.py PORT RELEASE_DIR [--dry-run]   RELEASE_DIR holds layout.json (slot order) and the .app files

A null entry in layout.json's "slots" leaves that slot EMPTY: the slot is erased if it holds anything (the launcher shows
it as empty). --dry-run only reads: it says which slots it would write or erase and sends no erase or write command.

Writes a slot only when it differs, then reads every slot back in full and compares. Sends ONE handshake: if the
radio's reply is not heard (plug not fully seated: the cable echoes) it stops, instead of cycling the radio through
programming mode five times. Radio on, at its main frequency screen. Worked on a 5RHNRFB-YP07 (2026-11-11).
The apps must be built for the core that is on the radio (same ABI hash), or the launcher shows them as EMPTY.
"""
import json, os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
src = source[:source.index("def do_logo")]
assert "for _ in range(5):" in src
exec(src.replace("for _ in range(5):", "for _ in range(1):", 1))

SLOT_BASE, RAW = 0x14C000, 64
DRY = "--dry-run" in sys.argv
_argv = [a for a in sys.argv[1:] if a != "--dry-run"]
if len(_argv) != 2:
    raise SystemExit(__doc__)
port, rel = _argv
with open(os.path.join(rel, "layout.json")) as f:
    LAYOUT = json.load(f)["slots"]
apps = [None if n is None else open(os.path.join(rel, n), "rb").read() for n in LAYOUT]
assert len(LAYOUT) == SLOT_COUNT and all(a is None or (a[:4] == b"OVL1" and len(a) <= SLOT_SIZE) for a in apps)
assert len({a[4:8] for a in apps if a is not None}) <= 1, "apps were built for different ABIs"
cps = Cps(port)


def read_raw(addr, length):
    out = bytearray()
    for i in range(-(-length // RAW)):
        out += cps.send(READ_RAW, addr // RAW + i)[2]
    return bytes(out[:length])


def name(d):
    return d[12:28].split(b"\0")[0].decode("ascii", "replace")


for slot, data in enumerate(apps):
    if data is None:
        if read_raw(SLOT_BASE + slot * SLOT_SIZE, 512) == b"\xff" * 512:
            print(f"slot {slot + 1}: empty already")
        elif DRY:
            print(f"slot {slot + 1}: would erase (left empty)")
        else:
            cps.send(APP_ERASE, slot, b"", ERASE_TIMEOUT)
            print(f"slot {slot + 1}: erased (left empty)", flush=True)
        continue
    if read_raw(SLOT_BASE + slot * SLOT_SIZE, len(data)) == data:
        print(f"slot {slot + 1}: {name(data)} already in place")
        continue
    if DRY:
        print(f"slot {slot + 1}: would write {name(data)}, {len(data)} bytes")
        continue
    cps.send(APP_ERASE, slot, b"", ERASE_TIMEOUT)
    padded = data.ljust(-(-len(data) // CHUNK) * CHUNK, b"\xff")
    for off in range(0, len(padded), CHUNK):
        cps.send(APP_WRITE, off, padded[off:off + CHUNK])
    print(f"slot {slot + 1}: wrote {name(data)}, {len(data)} bytes", flush=True)
if DRY:
    cps.end()
    print("dry run: nothing written\ndone; radio restarting")
    raise SystemExit(0)
print("read back in full:")
for slot, data in enumerate(apps):
    if data is None:
        if read_raw(SLOT_BASE + slot * SLOT_SIZE, SLOT_SIZE) != b"\xff" * SLOT_SIZE:
            raise SystemExit(f"SLOT {slot + 1} VERIFY FAILED (should be empty)")
        print(f"  {slot + 1}. (empty)  verified")
        continue
    if read_raw(SLOT_BASE + slot * SLOT_SIZE, len(data)) != data:
        raise SystemExit(f"SLOT {slot + 1} VERIFY FAILED")
    print(f"  {slot + 1}. {name(data)}  verified")
cps.end()
print("done; radio restarting")
