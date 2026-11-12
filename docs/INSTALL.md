# Installing the open firmware on a UV-5RM

For someone who is not the owner of this repository and has a Baofeng UV-5RM / UV-5RH. Read the risks first.

## Risks, stated plainly

* **The firmware is experimental.** It has been flashed on **one** radio, a board `5RHNRFB-YP07`, and some of what was built
  after that has not been on a radio at all (see [AUDIT.md](../AUDIT.md)). Nothing has been transmitted with it. By default it
  refuses to transmit (TX INHIBIT is on). You are responsible for transmitting only where you are licensed.
* **The installer itself has only been run against simulated radios** (`firmware/tests/test_install.py`) until someone runs it on a
  real one. The tools it drives have each been run on the one radio (the channel loader, the details-table writer and the boot-logo tool included),
  except the FM station table writer, which has only met simulations. If it does something unexpected, stop and unplug the cable.
* **Going back is not available for the YP07.** No stock firmware image exists for this board (it uses an FD6818B radio chip, and
  the images for other boards do not restore receive and transmit). The bootloader is not overwritten, so you can always flash
  another open build, but you cannot go back to stock.
* **Back up first.** The installer does it for you before it writes anything, and stops if it cannot verify the backup. The
  calibration page (what makes the radio's power and squelch right) cannot be restored if it is lost; the installer proves
  it was not touched.
* Use a radio you can afford to lose. If the radio will not enter flash mode, nothing in this repository can recover it.
* The board must read `5RHNRFB-YP07` (hold `8` while powering on, photograph the screen). Another board is refused.

## What you need

* The release zip (`uv5rm-VERSION.zip`), unpacked. Check it: `shasum -a 256 -c uv5rm-VERSION.zip.sha256` (macOS) or
  `sha256sum -c`. Inside, `release/SHA256SUMS` lists every file the installer will write; the installer checks them itself.
* A computer with **Python 3.9 or newer** and `pip install pyserial`. No other package is needed.
* The **programming cable**: for this radio it is the two-pin K-plug (the speaker/mic jacks), not USB-C. Use one sold for Baofeng;
  the one that worked here has a **CH340** chip. Before any software runs: the radio must **not** go into transmit when the cable
  is plugged in (one cable for another radio did; unplug it and do not use it). **Push the plug in until it is fully home.** A
  half-seated plug echoes the computer's own bytes back; the installer notices this and says "re-seat the cable".
* The serial port name: macOS `/dev/cu.usbserial-...` or `/dev/cu.wchusbserial-...`, Linux `/dev/ttyUSB0`, Windows `COM3`
  (the installer lists what it can see).
* The radio charged, on, at its normal frequency screen.

## The short version

```
python3 install.py --dry-run --port PORT       # everything except the writes; shows what would happen
python3 install.py --port PORT                 # menus, then the real thing after you type INSTALL
python3 install.py --config setup.json         # repeat a saved setup
```

Run it from the unpacked folder. In the repository it is `firmware/source-changes/install.py` and finds the release files in
`release/` or `build-out/release/` (or give `--release DIR`).

## Walk-through

The menus are plain numbered prompts (they work in any terminal, and in a pipe). Nothing is written until the last question.

```
UV-5RM installer. Nothing is written to the radio until you type INSTALL at the end.

Step 2 of 7: the board. Hold 8 while powering the radio on and type what the screen shows.
  Supported: 5RHNRFB-YP07, 5RHBK
board> 5RHNRFB-YP07

Step 3 of 7: apps in the nine launcher slots
  Available:
    1. BANDSWEEP        bandsweep.app      6600 bytes
    2. CHANMGR          chanmgr.app        7600 bytes
    ...
  Commands: SLOT NUMBER (put app NUMBER in slot SLOT)   SLOT 0 (empty the slot)   d (release defaults)   x (all empty)   ok
   slot 1: SETTINGS
   slot 2: CHANMGR
   ...
apps> 4 0              <- empty slot 4
apps> 4 6              <- or: put app 6 (HAMTOOLS) there; an app can be placed only once
apps> ok
```

* **Step 1** (the serial port) is asked unless you gave `--port`. Answer `none` to only plan; with no port the run is a dry run.
* **Step 3** the nine launcher slots, from every `.app` in the release folder (title and size are read from each package). Slots
  may be left empty (the installer erases a slot that holds something). The release's own layout is the default (`d`).
* **Step 4** channel lists:

  ```
    Lists (x = chosen):
     1. [ ] FRS              22 channels  channels/frs.csv
     2. [ ] SIMPLEX          12 channels  channels/simplex.csv
     3. [x] WEATHER           7 channels  channels/weather.csv
    Commands: NUMBER (choose or drop a list)   a PATH (your own CHIRP CSV)   n (none)   ok
  ```

  One list per channel group; a list replaces that group's slots, other channels on the radio are kept. `repeaters.csv` and
  `gmrs.csv` in the bundle are local to the Dallas area: use your own. `a PATH` adds your CSV (CHIRP format) and asks which
  group it is for; it is checked by the same rules as `channel_plan.py` (names at most 12 printable characters, frequencies in
  range, enough free slots, ...) and the tool's message is shown if it is refused. FRS, GMRS and MURS lists are for listening
  unless your licence and the rules allow transmitting; the radio's TX BANDS and TX INHIBIT settings decide, and the installer
  never changes them. See [channels/README.md](../channels/README.md).
* **Step 5** an FM broadcast station list for the FM app (`channels/fm/*.csv` or your own: `Name`, `Frequency` 87.5-108.0 in
  0.1 steps, optional `Detail`), or none. It replaces the list on the radio, including stations saved there.
* **Step 6** a finished boot screen file (160x128 RGB565, exactly 40,960 bytes), or none. The old screen is saved first.
* **Step 7** the core firmware: automatic (flash on a stock radio, ask on a radio already running this firmware), always, or never.
  The programming port cannot read which core build is running, so the installer cannot tell by itself whether a radio that already
  runs this firmware runs *this exact build*. If the apps in the launcher show as EMPTY after an install, the core is older than the
  apps: run again with "always flash".
* Then it offers to save the setup (see below), prints the **summary**, and asks you to type `INSTALL`:

```
WILL BE WRITTEN TO THE RADIO:
  core firmware fw-default.BF (default, board 5RHNRFB-YP07): decided when the radio has been identified; setting 'auto'
  app slot 1: SETTINGS (settings.app, 20480 bytes)
  ...
  channels, group WEATHER: 7 from channels/weather.csv
  channel details (the text shown beside the channel number): replaced for the channels above; other channels on the radio are kept
  FM station list: 4 stations from channels/fm/dallas.csv (replaces the list on the radio)

WILL NOT BE TOUCHED:
  settings, including TX INHIBIT and TX BANDS (nothing here writes them; the radio keeps refusing to transmit)
  calibration (read before and after to prove it)
  VFO, DTMF contacts, ANI
  channels outside the lists above
  the boot screen

Type INSTALL (capitals) to write all of this to the radio, anything else to stop.
```

### What happens after you type INSTALL

In this fixed order. Each step refuses to continue if the one before did not verify; the log of every tool run is kept in the
backup folder.

1. **Identify the radio**: stock firmware, this firmware, or flash (bootloader) mode, by the handshakes the tools use. A
   half-seated cable (the radio's reply is your own bytes) stops here with "re-seat the cable". A radio in flash mode cannot be
   backed up; power-cycle it, unless you are resuming a failed flash (below).
2. **Back up first**, into a new folder `uv5rm-backups/uv5rm-YYYYMMDD-HHMMSS/` (never overwriting anything):
   a stock radio with `radio_read.py` (channels, VFO, contacts, 0x8380 bytes) plus the calibration page, **read twice and
   compared**; a radio running this firmware with `dump_nor.py --config` (channels, settings, DTMF, calibration), twice and
   compared. The installer stops if the two reads differ. `backup.json` records the hashes.
3. **Check the files**: every app and the core against `release/SHA256SUMS` (a file that is not listed is refused), the core
   against `flash_bf.py`'s board and variant gate, and the generated channel image, details table and FM table.
4. **Flash the core, only if needed.** It tells you how to enter flash mode: radio off; hold the **two small side buttons below
   PTT** (not PTT) while switching on. It then probes for the bootloader again and again (a few seconds apart) until it answers or
   `--wait` seconds pass, flashes with `flash_bf.py`, and waits for the radio to restart.
5. **Calibration check**: `dump_nor.py --cal` again; it must be **byte-identical** to the copy from before. If not, the
   installer stops and nothing else is written. On a stock radio the "before" copy is the calibration page the stock firmware
   answers read frames for (the runbook records this working once); on a radio already running this firmware it is from the
   `dump_nor.py` backup.
6. **Write**: apps (`install_apps.py`, read back in full), channels (`restore_backup.py`, which rewrites whole 4 KB sectors
   because the firmware erases a channel's sector on the first write of a session; all 1000 slots are verified), channel details
   (`channel_extras.py`), FM stations (`fm_stations.py`), boot screen (`boot_logo_raw.py`). Each tool verifies itself by reading
   back; a step counts as done only when its tool says so. The radio restarts after each one, so you will see
   "the radio did not answer; it may still be restarting" lines: that is the installer waiting and trying again.
7. **Final calibration check**, and a **report**: what was written, where the backups are and, for a failed step, what state the radio is in
   and how to recover.

`--dry-run` goes through all of it and prints what would be written; no write or erase command of any kind is sent (the
bootloader is only probed with its handshake). Reads happen if a radio is attached; with no radio at all it still works
(without a `port` it is a dry run automatically).

## The setup file

The menus offer to save your choices as JSON, or pass `--save FILE`. Repeat it with `install.py --config FILE`: no questions
except the final confirmation (`--yes` skips that, for scripts). Relative paths in the file are relative to the file's folder.
Channels stay CSV: the file names them.

```json
{
  "schema": 1,
  "port": "/dev/cu.usbserial-1420",
  "board": "5RHNRFB-YP07",
  "release": "release",
  "core": "auto",
  "apps": ["settings.app", "chanmgr.app", "searchqt.app", null, "bandsweep.app",
           "foxhunt.app", "rptfinder.app", "fskprobe.app", "fm.app"],
  "channels": {"WEATHER": "channels/weather.csv", "FRS": "channels/frs.csv", "REPEATERS": "my-repeaters.csv"},
  "keep_existing_channels": true,
  "fm_stations": "channels/fm/dallas.csv",
  "boot_logo": null
}
```

| Key | Meaning |
|---|---|
| `schema` | must be `1`. Any other value is refused (so is an unknown key). |
| `port` | serial port, or `null` (then only a dry run is possible). `--port` overrides. |
| `board` | exactly what the radio shows when you hold `8` while powering on. Decides the core variant (`firmware/variants.json`). |
| `release` | folder with the `.BF`, the `.app` files, `layout.json` and `SHA256SUMS`; default `release/`. |
| `core` | `auto` (flash on stock, refuse to guess on this firmware when it cannot ask), `flash`, or `skip`. |
| `apps` | exactly nine entries, `.app` file names in the release folder or `null` for an empty slot. No name twice. |
| `channels` | group name (as in `channels/README.md`, any capitalisation) to a CSV file; at most one list per group. |
| `keep_existing_channels` | `true`: channels the lists do not fill are kept (read from the backup). `false`: they are erased. |
| `fm_stations` | CSV file or `null`. |
| `boot_logo` | a 40,960-byte `.rgb565` file or `null`. |

No personal details are stored by the installer. **Personalised boot screens are not made by it**: the picture with a callsign
and name is drawn by the Rust simulator (`scripts/dev.sh personalize`, using a git-ignored `.env`), which a release user does
not have. If you build one, give the finished `.rgb565` file as `boot_logo`.

## Options

`--config FILE`, `--dry-run`, `--yes`, `--port`, `--release DIR`, `--save FILE`, `--backup-dir DIR` (default `./uv5rm-backups`),
`--resume FOLDER`, and the waits `--settle S` (4), `--wait S` (180), `--retries N` (8), `--probe-timeout S` (2).
Exit status: `0` done, `1` a step failed (the report says what state the radio is in), `2` refused before anything was written.

## When something goes wrong

The report ends with the state of the radio and a command. The common cases:

| Where it stopped | State of the radio | What to do |
|---|---|---|
| refused before step 1 (checksum, bad CSV, board, an app twice, ...) | untouched | fix what it says; download the release again for a checksum |
| "re-seat the cable" | untouched | push the plug in fully, run again |
| backup differs between two reads | untouched | re-seat the cable and run again (a new backup folder). If it keeps differing, stop |
| the bootloader never answers | unchanged (still stock or this firmware) | radio off, hold the two small side buttons below PTT, switch on, run again |
| **the flash was interrupted** | **in the bootloader with a partly written core: it will not start normally, but flash mode still works** | leave it in flash mode and run `install.py --config SETUP.json --resume BACKUPFOLDER` (the folder the failed run named); or flash directly with the `flash_bf.py` command in the report |
| calibration differs | core flashed, nothing else written | **stop.** Do not transmit. Keep the backup folder and the differing dump and ask for help |
| apps | some slots empty or old; core and settings intact | run again (slots that match are skipped) |
| channels | a touched 4 KB sector may be partly written until it is run again; apps are done | run again. The previous channels are in `undo-channels-*.img` and the backup |
| details, FM stations | that table may be old or erased; channels are done | run again |
| boot screen | the tool puts the old screen back if the read-back differs; the old one is saved in the backup folder | `boot_logo_raw.py PORT logo-before-*.rgb565 NEWBACKUP.rgb565` restores it |

* **Channels, VFO or contacts lost**: `restore_backup.py PORT BACKUP.img --before UNDO.img` (dry run first, then `--write`);
  for a stock backup the image is `stock-memory.img`, for this firmware `channels-before.img`.
* **An app misbehaves**: write the slot again; an app cannot brick the radio (the load checks its ABI hash and checksum).
* **The core misbehaves** (no RX, wrong screen): re-enter flash mode and flash another open build; see the runbook.
* **Settings** (TX INHIBIT, TX BANDS) are never written by the installer; set them on the radio (SETTINGS). TX INHIBIT stays on
  until you have a dummy load on the antenna port.

The manual procedure the installer automates, step by step, is [FIRST-FLASH-RUNBOOK.md](FIRST-FLASH-RUNBOOK.md). It remains
the fallback and the explanation of why each step exists.

## What is and is not verified

Verified on a real radio (the owner's `5RHNRFB-YP07`, AUDIT rows 25-26, 30, 32, 35, 37, 40): the stock backup read, the bootloader
handshake, the core flash, the calibration read before and after, the app install, the boot logo, the channel loader (whole-sector writes, all 1,000
slots read back) and the channel extras table.
**Not verified on any radio**: this installer; the order of its probes (what a stock radio or the bootloader does when it hears a
handshake meant for something else); the stock radio's calibration read through `radio_read.py --cal` (done by hand once);
how long the radio takes to restart after each step (the installer waits and retries); the FM station table; and every
installer test is against a simulated radio, which has been wrong about hardware before (AUDIT 37).
