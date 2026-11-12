"""Install the channel extras table (each channel's Detail text) that channel_plan.py builds.

  channel_extras.py PORT TABLE.extras [--before OLD.extras]

TABLE.extras is 20,480 bytes: a 20-byte record per channel slot (16 bytes of text, 3 reserved, 0xA5). The radio keeps
it in the data region of its flash at 0x1E0000 and shows the text on the main screen in memory mode.
Reads the table that is on the radio first (saved to --before if given), skips the write if it is already the same,
writes the whole table (the first chunk erases the old one) and reads all of it back. Sends ONE handshake.
Needs a core with the `X` port command (AUDIT 33); an older core answers with an error and nothing is written.
Radio on, at its main frequency screen. UNVERIFIED on real hardware until AUDIT says otherwise.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
src = source[:source.index("def do_logo")]
assert "for _ in range(5):" in src
exec(src.replace("for _ in range(5):", "for _ in range(1):", 1))

EXTRA_WRITE, BASE, SIZE, RAW = 0x58, 0x1E0000, 20480, 64
args = [a for a in sys.argv[1:] if a != "--before"]
before = sys.argv[sys.argv.index("--before") + 1] if "--before" in sys.argv else None
if before:
    args.remove(before)
if len(args) != 2:
    raise SystemExit(__doc__)
port, path = args
with open(path, "rb") as f:
    table = f.read()
if len(table) != SIZE:
    raise SystemExit(f"{path} is {len(table)} bytes; the extras table is exactly {SIZE}")
cps = Cps(port)


def read_table():
    out = bytearray()
    for i in range(SIZE // RAW):
        out += cps.send(READ_RAW, BASE // RAW + i)[2]
    return bytes(out)


current = read_table()
if before:
    if os.path.exists(before) and open(before, "rb").read() != current:
        cps.end()
        raise SystemExit(f"{before} exists and is not what the radio has now; choose a new file name. Nothing written.")
    with open(before, "wb") as f:
        f.write(current)
    print(f"saved the radio's current table to {before}")
if current == table:
    print("the radio already has this table; nothing written")
else:
    for off in range(0, SIZE, CHUNK):
        cps.send(EXTRA_WRITE, off, table[off:off + CHUNK], ERASE_TIMEOUT if off == 0 else FRAME_TIMEOUT)
    if read_table() != table:
        cps.end()
        raise SystemExit("EXTRAS VERIFY FAILED: the table on the radio differs from the file. Run again.")
    print(f"extras table written and verified: {SIZE} bytes")
cps.end()
print("done; radio restarting")
