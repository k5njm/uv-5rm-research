"""Install a ready-made 160x128 RGB565 boot logo (for example build-out/personal/boot-logo.rgb565). No Pillow needed.

  boot_logo_raw.py PORT LOGO.rgb565 BACKUP.rgb565 [--dry-run]

Saves the logo that is in the radio to BACKUP first (an existing BACKUP with different contents is never overwritten),
skips the write if the radio already has this logo, reads all 40,960 bytes back after writing, and puts the old logo
back if the readback differs. Sends ONE handshake: if the reply is not heard (plug not fully seated) it stops.
--dry-run reads the logo and says whether it would write; it sends no write command and saves no backup.
To undo: run it again with BACKUP as the logo. For photos and other image files use set_boot_logo.py (needs Pillow).
Radio on, at its main frequency screen, running the open firmware. UNVERIFIED on real hardware.
"""
import os, sys

here = os.path.dirname(os.path.abspath(__file__))
source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
src = source[:source.index("def do_logo")]
assert "for _ in range(5):" in src
exec(src.replace("for _ in range(5):", "for _ in range(1):", 1))

DRY = "--dry-run" in sys.argv
_argv = [a for a in sys.argv[1:] if a != "--dry-run"]
if len(_argv) != 3:
    raise SystemExit(__doc__)
port, logo_path, backup_path = _argv
with open(logo_path, "rb") as f:
    logo = f.read()
if len(logo) != LOGO_SIZE:
    raise SystemExit(f"{logo_path} is {len(logo)} bytes; a boot logo is exactly {LOGO_SIZE}")
cps = Cps(port)


def read_logo():
    got = bytearray()
    for off in range(0, LOGO_SIZE, CHUNK):
        got += cps.send(READ_LOGO, off)[2]
    return bytes(got[:LOGO_SIZE])


def write_logo(img):
    for off in range(0, LOGO_SIZE, CHUNK):
        cps.send(WRITE_LOGO, off, img[off:off + CHUNK], ERASE_TIMEOUT if off == 0 else FRAME_TIMEOUT)


current = read_logo()
if DRY:
    print("the radio already has this logo; nothing written" if current == logo else "dry run: the logo differs; it would be replaced (nothing written)")
    cps.end()
    print("done; radio restarting")
    raise SystemExit(0)
if os.path.exists(backup_path):
    with open(backup_path, "rb") as f:
        if f.read() != current:
            cps.end()
            raise SystemExit(f"{backup_path} exists and is not the radio's current logo; choose a new file name. Nothing written.")
    print(f"backup already matches the radio's logo: {backup_path}")
else:
    with open(backup_path, "wb") as f:
        f.write(current)
    print(f"saved the radio's current logo to {backup_path}")
if current == logo:
    print("the radio already has this logo; nothing written")
else:
    write_logo(logo)
    if read_logo() != logo:
        write_logo(current)
        ok = read_logo() == current
        cps.end()
        raise SystemExit("LOGO VERIFY FAILED; the old logo was " + ("put back" if ok else "NOT restored: run again with the backup"))
    print(f"logo written and verified: {LOGO_SIZE} bytes")
cps.end()
print("done; radio restarting")
