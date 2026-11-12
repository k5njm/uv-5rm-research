"""Read-only dump of the radio's SPI flash through the CUSTOM firmware's programming port.

  dump_nor.py PORT OUT.bin [--cal | --config | --full]

  --cal     0xF000-0x10000   calibration (PA tables, squelch, battery, band/model): default
  --config  0x00000-0x10000  channels, VFO, settings, DTMF, FM, calibration
  --full    the whole 2 MiB chip (slow: about 32,000 round trips)

Only the handshake, raw-read ('D') and the final END frame can leave this script: send() refuses
anything else, so there is no path that writes to the radio. Run it right after the first flash
(and again after any flash): the firmware has no command that writes calibration, so the dump
must stay identical. It cannot be used BEFORE the first flash (the stock firmware does not speak
this protocol), and radio_read.py's stock backup does NOT include calibration.
"""
import hashlib, os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
exec(source[:source.index("def do_logo")])

RANGES = {"--cal": (0xF000, 0x10000), "--config": (0x0, 0x10000), "--full": (0x0, 0x200000)}
RAW_CHUNK = 64  # bytes per 'D' frame, address = offset / 64

args = sys.argv[1:]
flags = [a for a in args if a.startswith("--")]
pos = [a for a in args if not a.startswith("--")]
if len(pos) != 2 or len(flags) > 1 or any(f not in RANGES for f in flags):
    raise SystemExit(__doc__)
port, out = pos
start, end = RANGES[flags[0] if flags else "--cal"]

if os.path.exists(out):
    raise SystemExit(f"REFUSING TO OVERWRITE {out}: choose a new file name")

cps = Cps(port)
_send = cps.send


def guarded_send(cmd, addr, data=b"", timeout=FRAME_TIMEOUT):
    if cmd not in (READ_RAW, END) or (cmd == READ_RAW and data):
        raise SystemExit(f"REFUSED to send command {cmd:#x}: this tool is read-only")
    return _send(cmd, addr, data, timeout)


cps.send = guarded_send

blob = bytearray()
for a in range(start, end, RAW_CHUNK):
    chunk = cps.send(READ_RAW, a // RAW_CHUNK)[2]
    if len(chunk) != RAW_CHUNK:
        raise SystemExit(f"SHORT READ at {a:#x}: got {len(chunk)} bytes")
    blob += chunk
    if (a - start) % 0x4000 == 0:
        print(f"\r  {a - start:#x}/{end - start:#x}", end="", flush=True)
print()
os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
with open(out, "xb") as f:
    f.write(blob)
print(f"read {len(blob)} bytes {start:#x}-{end:#x} -> {out}")
print("sha256", hashlib.sha256(blob).hexdigest())
cps.end()
print("done; radio restarting")
