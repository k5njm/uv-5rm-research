"""Restore the channel table from a radio_read.py image through the CUSTOM firmware's port.

  restore_channels.py PORT IMAGE --before BEFORE.bin            show what would change (writes nothing)
  restore_channels.py PORT IMAGE --before BEFORE.bin --write    do it

IMAGE is the file radio_read.py makes (0x8380 bytes) or just the 0x8000-byte channel table.
Only channel slots 0-999 (32 bytes each) are touched. BEFORE.bin receives the radio's current
table first (the undo file; an existing different file is never overwritten). Each changed slot is
read back; on any mismatch that slot is put back from the undo data and the tool exits 1.
Re-running after an interruption is safe: slots that already match are skipped.

This restores channels only. It cannot restore VFO, settings or DTMF/FM (use the structured tools),
and nothing here can write calibration. UNVERIFIED on real hardware.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

READ, WRITE = 0x52, 0x57
SLOT, SLOTS = 32, 1000
TABLE = SLOT * SLOTS
IMAGE_SIZES = (0x8380, 0x8000)

args = sys.argv[1:]
write = "--write" in args
args = [a for a in args if a != "--write"]
if "--before" not in args or len(args) != 4 or args.index("--before") != 2:
    raise SystemExit(__doc__)
port, image_path, before_path = args[0], args[1], args[3]

image = open(image_path, "rb").read()
if len(image) not in IMAGE_SIZES:
    raise SystemExit(f"{image_path} is {len(image)} bytes; expected {IMAGE_SIZES}. Nothing was touched.")
want = [image[i * SLOT:(i + 1) * SLOT] for i in range(SLOTS)]

cps = Cps(port)
before = []
for i in range(SLOTS):
    r = cps.send(READ, i * SLOT)[2]
    if len(r) != SLOT:
        raise SystemExit(f"SHORT READ at slot {i}")
    before.append(r)
changed = [i for i in range(SLOTS) if before[i] != want[i]]
print(f"{len(changed)} of {SLOTS} channel slots differ from the image")

if not write:
    print("dry run: nothing written (add --write to apply)")
    cps.end()
    raise SystemExit(0)

blob = b"".join(before)
if os.path.exists(before_path):
    if open(before_path, "rb").read() != blob:
        raise SystemExit(f"REFUSING TO OVERWRITE {before_path}: it holds a different undo file")
else:
    os.makedirs(os.path.dirname(os.path.abspath(before_path)), exist_ok=True)
    with open(before_path, "xb") as f:
        f.write(blob)
    print(f"current table saved to {before_path}")

bad = []
for n, i in enumerate(changed, 1):
    cps.send(WRITE, i * SLOT, want[i])
    if cps.send(READ, i * SLOT)[2] != want[i]:
        print(f"slot {i}: READ-BACK DIFFERS; putting the old contents back")
        cps.send(WRITE, i * SLOT, before[i])
        bad.append(i)
        break
    if n % 100 == 0:
        print(f"  {n}/{len(changed)}")
if bad:
    cps.end()
    raise SystemExit(f"RESTORE STOPPED at slot {bad[0]}; the rest of the table is as it was or restored. Undo file: {before_path}")
final = b"".join(cps.send(READ, i * SLOT)[2] for i in range(SLOTS))
if final != b"".join(want):
    cps.end()
    raise SystemExit("FINAL VERIFY FAILED")
print(f"restored {len(changed)} slots; full table verified")
cps.end()
print("done; radio restarting")
