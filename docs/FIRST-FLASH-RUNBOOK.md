# First-flash runbook (read-only steps first)

Status: **steps 1-7 have been done on a real `5RHNRFB-YP07` (V0.13, 2026-11-11, AUDIT 25); nothing has been written to a
radio yet.** Steps 8 onward and every write tool are still tested only against fakes (`scripts/dev.sh tools-test`). Anything
marked UNVERIFIED has not met real hardware. Do the first flash on a radio you can afford to lose.

## The cable
The programming port is the two-pin K-plug (speaker/mic jacks), not USB-C. Use a cable sold for Baofeng (the one that worked
has a CH340 chip). Before any software runs:
* The radio must **not** go into transmit when the cable is plugged in. One for another radio did; unplug and do not use it.
* Push the plug in until it is fully home. A half-seated plug echoes the computer's own bytes back (`ident ack: 50`) while
  the radio still shows its computer icon; re-seat it and try again.
* `radio_read.py` must print `ident ack: 06`. Anything else: stop.

## What is and is not recoverable
| Thing | Recoverable? | How / why not |
|---|---|---|
| Overlay apps | Yes | Re-write over the programming port; load-time ABI hash + CRC32 reject bad apps (never brick) |
| Boot logo | Yes | `set_boot_logo.py` saves the old logo first and verifies by readback |
| Channels (1000), VFO, 20 DTMF contacts | Yes, if backed up first | `radio_read.py` backup, then `restore_backup.py` (dry run by default, undo image, per-record readback and rollback). Records are restored through the port and verified *through the port*; the port speaks logical records, not raw flash |
| Settings (0x9000) | **No, by design** | the custom firmware keeps its own 168-byte record there; writing stock bytes could change it, for example clear the TX inhibit. Re-enter via SETTINGS |
| DTMF/ANI header, rest of 0xA000-0xA2C0, FM list, 0xD000 scratch | **No** | the port has no write command for them (first boot erases the DTMF and FM sectors) |
| Calibration (`0xF000-0xF260`) | Not touched by anything here | no write command exists; `dump_nor.py` records it **after** flashing. No pre-flash copy is possible with these tools |
| Core firmware | Re-flashable with another open build | bootloader is not overwritten (UNVERIFIED) |
| **Stock firmware** | **No** | no stock image exists for the YP07 (FD6818B); BK/NRF stock images do not restore RX/TX on it |

## Steps (stop at any surprise)
**A. Read-only, before anything is connected to a computer**
1. Hold `8` while powering on. Photograph the screen. It must be exactly a string in `firmware/variants.json`
   (`5RHNRFB-YP07` -> `default`, `5RHBK` -> `bk4819`). Anything else: stop.
2. Note the stock firmware version (`VER:` line). Use the listing's 180-day replacement window if the board is wrong.
3. Save the stock channels with CHIRP (`Baofeng_5RM` driver) as a second, independent backup. Without CHIRP installed:
   `scripts/chirp-backup.sh PORT` runs CHIRP's command-line tool in a container (needs the cable, so do it with step 5).

**B. Read-only, cable connected (stock firmware running)**
4. `scripts/dev.sh check` passes; `scripts/dev.sh gate` passes.
5. `python3 firmware/source-changes/radio_read.py PORT my-backup/factory_memory.img` (cannot write by construction).

**C. Offline checks of the image (no radio)**
6. Make the `.BF`: CI publishes raw `.bin` images, the flasher takes a wrapped `.BF`. Either `scripts/dev.sh bf` (builds
   and wraps; the `.bin` hashes must equal `firmware/expected-hashes.json`) or
   `python3 firmware/source-changes/wrap_bf.py fw-default.bin fw-default.BF` on a downloaded CI image. Then
   `python3 firmware/source-changes/flash_bf.py IMAGE.BF PORT SHA --board <screen text> --variant <variant> --check-only`
   Must print `board ... accepted` and `serial port not opened`. Use only a **CI-built** image; the committed `.BF` does not reproduce from source (AUDIT 6).

**D. First write: needs the owner's explicit go**
7. Enter flash mode: radio off, hold the **two small side buttons below PTT** (not PTT), power on. Then
   `bootloader_probe.py PORT` (handshake only; must print `BOOTLOADER ANSWERED`). To leave without flashing, power-cycle.
8. `flash_bf.py ... --board ... --variant ...` (no `--check-only`). Do not touch the radio or cable.
9. After the radio restarts: `dump_nor.py PORT my-backup/cal-after-first-flash.bin --cal`; record the sha256 in your notes.
   After every later flash, dump again; the hashes must match. A change means something wrote calibration: stop.
10. `setup_radio.py` (apps/logo) or the fewer-moving-parts `cps_tool.py`; then re-run `dump_nor.py --cal` once more.
11. Settings: `TXINH` stays ON until you have a dummy load on the antenna port.

**E. If something is wrong**
- Channels, VFO or contacts lost: `restore_backup.py PORT my-backup/factory_memory.img --before my-backup/undo.img` (dry run; lists what it will skip and why), add `--write`. The undo image can itself be restored the same way.
- App misbehaves: re-write the app slot; it cannot brick the radio.
- Core misbehaves (no RX/TX, wrong screen): re-enter flash mode and flash the other variant or a previous CI build.
  If the radio will not enter flash mode, stop: that is the unrecoverable case this runbook cannot cover.

## Never
Transmit during setup, flash without the board gate, or commit a backup (`my-backup/` is git-ignored).
