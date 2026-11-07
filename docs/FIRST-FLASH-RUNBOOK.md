# First-flash runbook (read-only steps first)

Status: **written before the radio exists; every tool below is tested only against fakes**
(`scripts/dev.sh tools-test`). Anything marked UNVERIFIED has not met real hardware. Do the first flash
on a radio you can afford to lose.

## What is and is not recoverable
| Thing | Recoverable? | How / why not |
|---|---|---|
| Overlay apps | Yes | Re-write over the programming port; load-time ABI hash + CRC32 reject bad apps (never brick) |
| Boot logo | Yes | `set_boot_logo.py` saves the old logo first and verifies by readback |
| Channel table | Yes, if backed up first | `radio_read.py` backup, then `restore_channels.py` (dry run by default, undo file, readback) |
| VFO, settings, DTMF/ANI, FM | **No** (not restorable by these tools) | first boot erases those sectors; settings re-created from defaults |
| Calibration (`0xF000-0xF260`) | Not touched by anything here | no write command exists; `dump_nor.py` records it **after** flashing. No pre-flash copy is possible with these tools |
| Core firmware | Re-flashable with another open build | bootloader is not overwritten (UNVERIFIED) |
| **Stock firmware** | **No** | no stock image exists for the YP07 (FD6818B); BK/NRF stock images do not restore RX/TX on it |

## Steps (stop at any surprise)
**A. Read-only, before anything is connected to a computer**
1. Hold `8` while powering on. Photograph the screen. It must be exactly a string in `firmware/variants.json`
   (`5RHNRFB-YP07` -> `default`, `5RHBK` -> `bk4819`). Anything else: stop.
2. Note the stock firmware version (`VER:` line). Use the listing's 180-day replacement window if the board is wrong.
3. Save the stock channels with CHIRP (UV-17Pro-family driver) as a second, independent backup.

**B. Read-only, cable connected (stock firmware running)**
4. `scripts/dev.sh check` passes; `scripts/dev.sh gate` passes.
5. `python3 firmware/source-changes/radio_read.py PORT my-backup/factory_memory.img` (cannot write by construction).

**C. Offline checks of the image (no radio)**
6. `python3 firmware/source-changes/flash_bf.py IMAGE.BF PORT SHA --board <screen text> --variant <variant> --check-only`
   Must print `board ... accepted` and `serial port not opened`. Use only a **CI-built** image; the committed `.BF` does not reproduce from source (AUDIT 6).

**D. First write: needs the owner's explicit go**
7. Enter flash mode (both side buttons), `bootloader_probe.py PORT` (handshake only).
8. `flash_bf.py ... --board ... --variant ...` (no `--check-only`). Do not touch the radio or cable.
9. After the radio restarts: `dump_nor.py PORT my-backup/cal-after-first-flash.bin --cal`; record the sha256 in your notes.
   After every later flash, dump again; the hashes must match. A change means something wrote calibration: stop.
10. `setup_radio.py` (apps/logo) or the fewer-moving-parts `cps_tool.py`; then re-run `dump_nor.py --cal` once more.
11. Settings: `TXINH` stays ON until you have a dummy load on the antenna port.

**E. If something is wrong**
- Channels lost: `restore_channels.py PORT my-backup/factory_memory.img --before my-backup/undo.bin` (dry run), add `--write`.
- App misbehaves: re-write the app slot; it cannot brick the radio.
- Core misbehaves (no RX/TX, wrong screen): re-enter flash mode and flash the other variant or a previous CI build.
  If the radio will not enter flash mode, stop: that is the unrecoverable case this runbook cannot cover.

## Never
Transmit during setup, flash without the board gate, or commit a backup (`my-backup/` is git-ignored).
