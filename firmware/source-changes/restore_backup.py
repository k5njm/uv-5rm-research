"""Restore what the custom firmware's port can safely write from a radio_read.py image.

  restore_backup.py PORT IMAGE --before UNDO.img            show what would change (writes nothing)
  restore_backup.py PORT IMAGE --before UNDO.img --write    do it

IMAGE is the 0x8380-byte file radio_read.py makes (or just its first 0x8000 bytes, the channel table).
Restored, each through the port and verified by reading it back through the port:
  channels   slots 0-999, 32 bytes each           (image 0x0000-0x7D00)
  vfo        64 bytes                              (image 0x8000)
  contacts   20 DTMF contacts, 16 bytes each at logical 0xA020   (image 0x80A0-0x81E0)
NOT restored, and reported as skipped, with the reason:
  settings   (image 0x8040): the custom firmware keeps a 168-byte record there; stock bytes in its first 64 could
             change settings it interprets differently (for example clear the TX inhibit). Re-enter via SETTINGS.
  DTMF/ANI header and the rest of 0xA000-0xA2C0: the port has no write command for them.
  0xD000 scratch: firmware-internal wear-levelling scratch, meaningless to restore.
  calibration: nothing here can write it.
The port speaks LOGICAL records, not raw flash, so "byte-identical" here means the records read back through the
port equal the image. UNDO.img (same format as IMAGE) receives the radio's current values first, so
`restore_backup.py PORT UNDO.img --before UNDO2.img --write` puts things back; an existing different file is never
overwritten. Each changed record is read back; on a mismatch that record is put back and the tool exits 1.
Re-running after an interruption is safe (matching records are skipped). UNVERIFIED on real hardware.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

READ, WRITE = 0x52, 0x57
IMAGE_SIZES = (0x8380, 0x8000)
SLOT, SLOTS = 32, 1000
VFO_OFF, VFO_LEN = 0x8000, 0x40
DTMF_OFF, DTMF_ADDR = 0x8080, 0xA000          # where the 0xA000-0xA2C0 range sits in the image, and its radio address
CONTACT_ADDR, CONTACT, CONTACTS = 0xA020, 16, 20

SKIPPED = [
    ("settings (0x9000)", "the custom firmware's 168-byte record; stock bytes could change TX inhibit and other settings"),
    ("DTMF/ANI header and 0xA160-0xA2C0", "the port has no write command for them"),
    ("0xD000 scratch", "firmware-internal wear-levelling scratch"),
    ("calibration (0xF000-0xF260)", "not in the image and not writable by anything here"),
]


def items(image):
    """(label, wire address, bytes) for every record this tool restores, from an image."""
    out = [(f"channel {i}", i * SLOT, image[i * SLOT:(i + 1) * SLOT]) for i in range(SLOTS)]
    if len(image) >= 0x8380:
        out.append(("vfo", VFO_OFF, image[VFO_OFF:VFO_OFF + VFO_LEN]))
        for i in range(CONTACTS):
            off = DTMF_OFF + (CONTACT_ADDR - DTMF_ADDR) + i * CONTACT
            out.append((f"contact {i}", CONTACT_ADDR + i * CONTACT, image[off:off + CONTACT]))
    return out


args = sys.argv[1:]
write = "--write" in args
args = [a for a in args if a != "--write"]
if "--before" not in args or len(args) != 4 or args.index("--before") != 2:
    raise SystemExit(__doc__)
port, image_path, before_path = args[0], args[1], args[3]

image = open(image_path, "rb").read()
if len(image) not in IMAGE_SIZES:
    raise SystemExit(f"{image_path} is {len(image)} bytes; expected {IMAGE_SIZES}. Nothing was touched.")
want = items(image)

cps = Cps(port)
before = []
for label, addr, _ in want:
    r = cps.send(READ, addr)[2]
    if len(r) != len(_):
        raise SystemExit(f"SHORT READ of {label}: got {len(r)} bytes")
    before.append(r)
changed = [i for i in range(len(want)) if before[i] != want[i][2]]
# The firmware erases a channel's whole 4 KB flash sector (128 channels) the first time a session writes into it
# (storage.rs write_channel), then stores only what it is sent. So every channel the image has in a touched sector
# must be sent, changed or not, or the unchanged ones are lost. Found on a radio (AUDIT 37): a load that changed 65
# channels wiped the untouched ones beside them. The fake radio now erases the same way.
SECTOR_CHANNELS = 4096 // SLOT
touched = {i // SECTOR_CHANNELS for i in changed if i < SLOTS}
to_write = [i for i in range(len(want)) if (i >= SLOTS and i in changed)
            or (i < SLOTS and i // SECTOR_CHANNELS in touched and (want[i][2] != b"\xff" * SLOT or i in changed))]
groups = {}
for i in changed:
    groups.setdefault(want[i][0].split()[0], []).append(i)
print(f"{len(changed)} of {len(want)} records differ from the image: " +
      (", ".join(f"{k} {len(v)}" for k, v in groups.items()) or "none"))
for name, why in SKIPPED:
    print(f"  skipped: {name}: {why}")

if not write:
    print("dry run: nothing written (add --write to apply)")
    cps.end()
    raise SystemExit(0)

undo = bytearray(b"\xff" * len(image))        # an image in the same format holding the radio's current records
pos = {}
for k, (label, addr, _) in enumerate(want):
    if label.startswith("channel"):
        off = addr
    elif label == "vfo":
        off = VFO_OFF
    else:
        off = DTMF_OFF + (addr - DTMF_ADDR)
    undo[off:off + len(before[k])] = before[k]
undo = bytes(undo)
if os.path.exists(before_path):
    if open(before_path, "rb").read() != undo:
        raise SystemExit(f"REFUSING TO OVERWRITE {before_path}: it holds a different undo file")
else:
    os.makedirs(os.path.dirname(os.path.abspath(before_path)), exist_ok=True)
    with open(before_path, "xb") as f:
        f.write(undo)
    print(f"current records saved to {before_path}")

for n, i in enumerate(to_write, 1):
    label, addr, data = want[i]
    cps.send(WRITE, addr, data)
    if cps.send(READ, addr)[2] != data:
        if label.startswith("channel"):
            # A channel sector is erased once per session; a second write to the same record only clears bits
            # (flash programs AND into what is there), so the old contents cannot be put back from here. The next
            # session erases the sector again and rewrites every channel the image has in it: just run this again.
            print(f"{label}: READ-BACK DIFFERS; this session cannot rewrite it")
            cps.end()
            raise SystemExit(f"RESTORE STOPPED at {label}, which was written wrongly and stays wrong until this tool is run "
                             f"again (it is safe to re-run: a new session erases the sector and rewrites all of it). "
                             f"Undo file: {before_path}")
        print(f"{label}: READ-BACK DIFFERS; putting the old contents back")
        cps.send(WRITE, addr, before[i])
        cps.end()
        raise SystemExit(f"RESTORE STOPPED at {label}; everything else is as it was or restored. Undo file: {before_path}")
    if n % 100 == 0:
        print(f"  {n}/{len(to_write)}")
for label, addr, data in want:
    if cps.send(READ, addr)[2] != data:
        cps.end()
        raise SystemExit(f"FINAL VERIFY FAILED at {label}")
print(f"restored {len(changed)} records ({len(to_write)} written: whole sectors); all {len(want)} verified through the port")
cps.end()
print("done; radio restarting")
