# What to check on the radio after the next flash

One ordered walk-through for the owner. It lists what has **not yet been seen on the radio**, taken from the "Open: unseen on
the radio" items of [AUDIT.md](../AUDIT.md) rows 40 to 51 (plus a few from rows 33 to 39 that no row records as seen). What each
screen should look like is in [USER-GUIDE.md](USER-GUIDE.md). Tick a box when you saw what is written; where something else
happens, write down what you saw (a photo helps) and stop that section.

* **Parts 1 to 7 only receive.** Keep TX INHIBIT **on** throughout.
* **Part 8** is the installer and the host tools. They talk to the radio but never transmit.
* **Part 9 transmits, or can. It needs a dummy load on the antenna port and your decision to switch TX INHIBIT off.** Do not do it
  without both.

**Before you start.** Flash an image from `scripts/dev.sh release` whose hash equals `firmware/expected-hashes.json`, reinstall the apps from the
same release, then run `python3 firmware/source-changes/dump_nor.py PORT cal-after.bin --cal` and check its sha256 equals the calibration
copy you already hold. If it differs, **stop**: do not transmit.

## Part 1: main screen, VFO, groups and scanning (receive only)

- [ ] **1.** Type `300000` in VFO mode. An amber box says `OUT OF RX RANGE`, and the frequency does not change. (AUDIT 39, 40)
- [ ] **2.** Type `098100`. The FM app opens already tuned to 98.1 MHz. `EXIT` returns to the main screen. (AUDIT 40)
- [ ] **3.** Type `174000`, then `UP`: the frequency goes to `200.000` (the start of the next receive band), and `DOWN` goes back to `174.000`.
  Type `520000`, `UP`: it wraps to `108.000`. (AUDIT 43)
- [ ] **4.** Type `146520` and hold `#`. The status bar reads `SCAN UP EXIT` and the frequency moves inside 144 to 148 MHz only, never
  outside it. `UP` and `DOWN` change the direction (`SCAN DN EXIT`). `EXIT` stops it and the tag reads `2M`. Say whether the label and the
  tag crowd each other. (AUDIT 34, 35, 43, 50)
- [ ] **5.** Tune to a NOAA channel (`162400`) with SCAN RESUME on STOP (SETTINGS > RADIO) and hold `#`: the scan stops on a
  signal after about a second and ends. Set SCAN RESUME back to CARRIER. (AUDIT 50)
- [ ] **6.** `BAND` then `1` selects REPEATERS (if the group has channels, the radio goes to memory mode on its first one): the status-bar tag reads `RPT` and the channel
  numbers read `01`, `02`... not `234`. `BAND` then `0` (ALL) shows the radio's own number. (AUDIT 34, 36)
- [ ] **7.** Memory mode on a channel that has a detail text (a place or a note): the text sits right-aligned on the channel-number
  row. (AUDIT 33)
- [ ] **8.** Hold `*` locks the keypad (padlock in the status bar); hold `*` again unlocks it.

## Part 2: SETTINGS (receive only)

- [ ] **9.** `MENU`, `1` opens SETTINGS. Seven groups. Open RADIO: a small grey scroll mark is at the right edge and moves as you go down. The strip
  under the list explains each setting and is not cut off. (AUDIT 42, 51)
- [ ] **10.** DISPLAY > AUTO KEYLOCK: `MENU` (amber row, `^ OFF v`), `UP` to `5 SEC`, `MENU`. `EXIT` until you are on the main screen and touch nothing: the
  padlock should appear after **about 5 seconds**, not half a second. Hold `*` to unlock, then set AUTO KEYLOCK back to OFF. (AUDIT 51)
- [ ] **11.** RADIO > SQUELCH: `MENU`, change it, `#` (it goes to 3), `EXIT` (the old value comes back). Then `MENU`, `#`, `MENU` saves 3. `#` should act when you let go of an ordinary press. (AUDIT 51)
- [ ] **12.** SYSTEM > TX BANDS: `MENU`, `DOWN` (CE HAM), `MENU`; then `MENU` again and `#`. The strip says `Choose TX BANDS by hand` and the value stays CE HAM. `UP` to
  FCC HAM and `MENU` to put it back by hand. (AUDIT 51)
- [ ] **13.** SYSTEM > LATITUDE: `MENU`, type `407100`. The text cursor after the last digit is fully visible, not half off the screen. `EXIT` to
  leave. (AUDIT 42)
- [ ] **14.** SYSTEM > GRID LOCATOR: `MENU` opens a different screen (a short pause is expected; note any flash or glitch). Type `3 3`, `6`, `1`, `2`, `5 5`, `#`,
  `9 9` for `EM12kx`, then `MENU`. You return to SYSTEM on the same row and LATITUDE and LONGITUDE are set. Clear them (`MENU`, `#`, `MENU`). (AUDIT 51)
- [ ] **15.** SYSTEM > VERSION: `MENU` opens ABOUT with seven rows. Write down the RF CHIP value (HARDWARE.md expects `0x4829`) and the APP ABI value (the apps open only if it matches, so this is for your notes). `EXIT` returns to SYSTEM on the VERSION row. (AUDIT 51)
- [ ] **16.** PRESETS > SCANNING: `MENU` shows a preview titled `SCANNING: n CHANGES`; `EXIT` cancels and nothing changes. Open it again and `MENU` applies it
  (`PRESET APPLIED`). Then DEFAULTS: apply it to restore SQUELCH, SCAN RESUME and BACKLIGHT. (AUDIT 51)
- [ ] **17.** SYSTEM > RESET ALL: `MENU` arms it (amber, `HOLD #`). A second `MENU` does nothing; a short `#` does nothing; `EXIT` cancels. **Do not hold `#`**
  unless you are ready to re-enter your settings. (AUDIT 42, 47)

## Part 3: CHAN MGR (receive only)

- [ ] **18.** `MENU`, `2` opens CHAN MGR on the current group. Open a channel with `MENU`: note any flash or glitch at the switch to the editor; `EXIT`
  returns to the **same row**. (AUDIT 47)
- [ ] **19.** Open a channel, change TX POWER (`MENU`, `UP`, `MENU`), then `EXIT`: the selected row turns amber and reads `DISCARD CHANGES?`. Every hint strip fits its line.
  `EXIT` keeps editing; `EXIT` asks again; `MENU` discards and returns to the list. (AUDIT 47)
- [ ] **20.** `+ NEW`, edit nothing, go to SAVE (`UP` from the first row), `MENU`: a green `SAVED nn` box is **visible** for about a second and a
  half. (It was reported on an earlier build; AUDIT 49 says it may never have been drawn.)
- [ ] **21.** Open that new channel, `UP` to DELETE (past the blank row), `MENU` (amber `HOLD #`), a second `MENU` does nothing, then **hold `#`**:
  the radio beeps and the channel is deleted after about a second. Check the numbers of the others (a gap is expected after a delete in the middle). (AUDIT 47)

## Part 4: FM (receive only)

- [ ] **22.** `MENU`, `9` opens FM on 100.0 MHz. The large digits, the name line, the speaker mark in the list, the four signal bars and the tuning scale read
  well at real size. (AUDIT 45)
- [ ] **23.** Type `981`: it tunes at once. Type `1100`: `RANGE 87.5-108.0`. `UP` and `DOWN` move 0.1 MHz. (AUDIT 45)
- [ ] **24.** Hold `UP` on an empty spot: `SEEKING`, the audio mutes, and it stops on the next station within three seconds. Write down whether the
  four bars match how strong the station is. (AUDIT 45)
- [ ] **25.** `MENU` opens STATIONS. `+ SAVE THIS` saves the tuned frequency (it reads `(unnamed)`); `*` then `MENU` deletes a station; `*` then `EXIT` cancels.
  `#` and `*` in the tuner jump between saved stations. (AUDIT 45)

## Part 5: SPECTRUM and BAND SWEEP (receive only)

- [ ] **26.** Tune to a frequency with a known signal (NOAA 162.400 or a local repeater) and open SPECTRUM (`MENU`, `4`). The signal is marked and `PEAK` reads its
  frequency. On a quiet frequency, no peak is marked from noise at the default settings. (AUDIT 50)
- [ ] **27.** `MENU` tunes the VFO to the peak and leaves; the main screen shows that frequency in VFO mode. Without a signal, `MENU` shows `NO SIGNAL` clearly
  over the drawing. (AUDIT 50)
- [ ] **28.** In SPECTRUM press `A/B`: it stops on the signal and you hear it; `MENU` tunes to it. (AUDIT 50)
- [ ] **29.** BAND SWEEP (`MENU`, `5`): after a pass or two the list shows hits. `MENU` hears one, `MENU` again tunes the VFO to it and leaves, `EXIT` instead goes back
  to the sweep. The title bar and strip are legible. (AUDIT 50)

## Part 6: FSK PROBE (receive only; steps in [SONDE-APP.md](SONDE-APP.md))

- [ ] **30.** `MENU`, `8` opens FSK PROBE on 404.800 MHz at 4800 baud, options `LEN316 TH1 INV- BW2.4 S4`. `MENU` turns the modem on; `UP`/`DOWN` to REG 59 reads `1008`, REG 5D
  reads `3B20`. (AUDIT 44)
- [ ] **31.** Press `#`. `ERASING` shows: write down how long it lasts, and that the keys come back. Then `REC` counts up. Wait 30 seconds, press `#`: `SAVED n`. Leave
  the app. (AUDIT 40, 44)
- [ ] **32.** On the computer: `python3 firmware/source-changes/probe_capture.py PORT noise-a.bin` prints a summary and writes a `.csv`. Then follow steps (a) in
  SONDE-APP.md (S4 against S2) to settle the sync-width question, and (b) at a radiosonde launch (about 11:00 and 23:00 UTC). (AUDIT 44)

## Part 7: extras, if installed (receive only)

- [ ] **33.** HAM TOOLS: arm the ID timer (`*`) and press `EXIT` once: the notice `HOLD EXIT: TIMER RUNS` is visible. (AUDIT 49)

## Part 8: installer and host tools (the radio is read, never made to transmit)

- [ ] **34.** `python3 install.py --dry-run --port PORT` identifies the radio correctly (this firmware, not stock, not "bootloader"), reads the calibration twice and
  sends no write or erase. Note anything it says it did not expect, and how long each wait lasted. It has never run against a real radio. (AUDIT 48)
- [ ] **35.** After a real install of your choosing (your decision): every step says verified, the calibration is identical afterwards, and the radio shows the new
  apps, channels and boot screen. (AUDIT 48)
- [ ] **36.** `python3 firmware/source-changes/fm_stations.py install PORT my.fmst --before fm-before.bin`: write down how long it takes; the names then show in the FM
  station list. (AUDIT 45)

## Part 9: the transmitter (dummy load, and your decision)

**Stop here unless a dummy load is on the antenna port and you have decided to transmit.** Leave TX INHIBIT on until step 38.

- [ ] **37.** With TX INHIBIT **on**, press `PTT`: `TX INHIBIT IS ON` and no carrier. (Seen before; check it still holds on this image.)
- [ ] **38.** Switch TX INHIBIT **off** (SETTINGS > RADIO, `MENU`, `UP`, `MENU`). Set TX BANDS to CE HAM, tune the VFO to `446000` (outside it), press `PTT`: `OUTSIDE TX BANDS` and no
  carrier. On an air-band frequency, `PTT`: `OUTSIDE TX BANDS` (AM never transmits). (AUDIT 29, 42)
- [ ] **39.** On a weather channel in memory mode (no transmit frequency): `PTT` shows `RX-ONLY CHANNEL`. With BUSY LOCKOUT on and a signal present: `CHANNEL BUSY`.
- [ ] **40.** TEXT (an extra: install it in a slot first). With TX INHIBIT **on**, send a message: `TX BLOCKED: SEE SETTINGS`, nothing transmitted. Only if you
  decide to transmit, with the dummy load and a second radio you own on the same frequency: the title bar is red and reads `TX` only while the transmitter is keyed, and the
  message arrives. Note whether the redraw during the transmission disturbs the message. (AUDIT 50)
- [ ] **41.** Set TX INHIBIT back **on** and TX BANDS to FCC HAM when you are done.

The fault screen (`APP FAULT` / `CORE FAULT`) and the transmitter dropping at a fault (AUDIT 43) can be exercised only with the test app `apps/hello --features
fault-test`, which is in no release and whose key `5` lets the app transmit. That is a dummy-load test and a separate decision; it is not part of this walk-through.
