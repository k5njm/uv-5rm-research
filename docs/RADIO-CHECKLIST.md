# What to check on the radio after the next flash

One ordered walk-through for the owner. It lists what has **not yet been seen on the radio**, taken from the "Open: unseen on
the radio" items of [AUDIT.md](../AUDIT.md) rows 40 to 51 (plus a few from rows 33 to 39 that no row records as seen). What each
screen should look like is in [USER-GUIDE.md](USER-GUIDE.md). Every key, text and number below was checked against the source on
2026-11-12 ([GUIDE-VERIFICATION.md](GUIDE-VERIFICATION.md) says what was corrected). Tick a box when you saw what is written; where
something else happens, write down what you saw (a photo helps) and stop that section.

* **Parts 1 to 7 only receive.** Keep TX INHIBIT **on** throughout.
* **Part 8** is the installer and the host tools. They talk to the radio but never transmit.
* **Part 9 transmits, or can. It needs a dummy load on the antenna port and your decision to switch TX INHIBIT off.** Do not do it
  without both.

Start each part from the main screen. `EXIT` backs out of a menu or an app (from a settings list it takes one `EXIT` per level).
Steps that need VFO mode say so: if the top block shows a channel number instead of `VFO`, press `VFO/MR`.

**Before you start.** Flash an image from `scripts/dev.sh release` whose hash equals `firmware/expected-hashes.json`, reinstall the apps
from the same release (`python3 firmware/source-changes/install_apps.py PORT build-out/release`, or the installer), then run
`python3 firmware/source-changes/dump_nor.py PORT cal-after.bin --cal` and check that `shasum -a 256 cal-after.bin` equals the
calibration copy you already hold. If it differs, **stop**: do not transmit.

## Part 1: main screen, VFO, groups and scanning (receive only)

- [ ] **1.** In VFO mode type `300000`. An amber box says `OUT OF RX RANGE` (it goes away after about two seconds), and the frequency does not change. (AUDIT 39, 40)
- [ ] **2.** Type `098100`. The FM app opens already tuned to 98.1 MHz (it is in slot 9 of the release). `EXIT` returns to the main screen. (AUDIT 40)
- [ ] **3.** Type `174000`, then `UP`: the frequency goes to `200.000` (the start of the next receive band), and `DOWN` goes back to `174.000`.
  Type `520000`, `UP`: it wraps to `108.000`. (AUDIT 43)
- [ ] **4.** Type `146520` and hold `#` (about a second). The status bar reads `SCAN UP EXIT` (just `SCAN UP` if a `PS`, `DWR` or `VOX` flag is on) and the
  frequency moves inside 144 to 148 MHz only, never outside it. If a signal opens the squelch the scan waits on it as SCAN RESUME says (CARRIER
  by default): that is not a fault. `UP` and `DOWN` change the direction (`SCAN DN EXIT`). `EXIT` stops it and the tag reads `2M`. Say whether the
  label and the tag crowd each other. (AUDIT 34, 35, 43, 50)
- [ ] **5.** Type `162400` (a NOAA channel). In SETTINGS > RADIO set SCAN RESUME to STOP (three `UP` from the first row reach it; `MENU`, `UP`, `MENU`), go
  back to the main screen and hold `#`: the scan stops on the signal and ends (the scan label goes away). Write down how long it stayed on the
  signal before it ended (the source's comments say about a second; the real time is unmeasured). Set SCAN RESUME back to CARRIER (`MENU`, `DOWN`, `MENU`). (AUDIT 50)
- [ ] **6.** `BAND` then `1` selects REPEATERS (if the group has channels, the radio goes to memory mode on its first one, or stays on the current one if it is in the group):
  the status-bar tag reads `RPT` and the channel numbers read `01`, `02`... not `234`. `BAND` then `0` (ALL) shows the radio's own number. Leave the radio in memory mode for step 7. (AUDIT 34, 36)
- [ ] **7.** Memory mode on a channel that has a detail text (a place or a note; the channels from `channels/` have one, with CH DISPLAY on its default NAME+FREQ): the
  text sits right-aligned on the channel-number row. (AUDIT 33)
- [ ] **8.** Hold `*` locks the keypad (padlock in the status bar, a beep if KEY BEEP is on); hold `*` again unlocks it.

## Part 2: SETTINGS (receive only)

- [ ] **9.** `MENU`, `1` opens SETTINGS. Seven groups. `MENU` on RADIO: a small grey scroll mark is at the right edge and moves as you go down. The strip
  under the list explains each setting and is not cut off. (AUDIT 42, 51)
- [ ] **10.** DISPLAY > AUTO KEYLOCK (from the group list `DOWN`, `DOWN`, `MENU`, `DOWN`): `MENU` (amber row, `^ OFF v`), `UP` to `5 SEC`, `MENU`. `EXIT` three times to the main screen, on a
  frequency with no signal, and touch nothing: the padlock should appear after **about 5 seconds**, not half a second, with a beep (it does not lock while a signal
  opens the squelch). Hold `*` to unlock, then set AUTO KEYLOCK back to OFF. (AUDIT 51)
- [ ] **11.** RADIO > SQUELCH: first set it to 5 (`MENU`, `UP`, `UP`, `MENU`) so it is not at its default. Then `MENU`, `DOWN`, `#` (the value goes to 3), `EXIT` (the 5 comes
  back). Then `MENU`, `#`, `MENU` saves 3. `#` should act when you let go of an ordinary press, not only on a hold. (AUDIT 51)
- [ ] **12.** SYSTEM > TX BANDS (the first row): `MENU`, `DOWN` (CE HAM), `MENU`; then `MENU` again and `#`. The strip says `Choose TX BANDS by hand` and the value stays CE HAM. `UP` to
  FCC HAM and `MENU` to put it back by hand. (AUDIT 51)
- [ ] **13.** SYSTEM > LATITUDE (`DOWN` from TX BANDS): `MENU`, type `407100`. The text cursor after the last digit is fully visible, not half off the screen. `EXIT` deletes one digit
  each time: press it six times to empty the field and a seventh time to leave without storing anything. (AUDIT 42)
- [ ] **14.** SYSTEM > GRID LOCATOR: `MENU` opens a different screen (a short pause is expected; note any flash or glitch). Type `3 3`, `6`, `1`, `2`, `5 5`, `9 9` for `EM12kx` (`#` is not needed:
  every letter here is followed by a different key; it is only for two letters on the same key), then `MENU`. You return to SYSTEM on the same row, the GRID LOCATOR row reads `EM12kx`,
  and LATITUDE and LONGITUDE are set. Clear both (on each: `MENU`, `#`, `MENU`). (AUDIT 51)
- [ ] **15.** SYSTEM > VERSION: `MENU` opens ABOUT with seven rows. Write down the RF CHIP value (HARDWARE.md expects `0x4829`) and the APP ABI value (the apps open only if it matches, so this is for your notes). `EXIT` returns to SYSTEM on the VERSION row. (AUDIT 51)
- [ ] **16.** PRESETS > SCANNING: `MENU` shows a preview titled `SCANNING: n CHANGES` (`1 CHANGE` if n is 1); `EXIT` cancels and nothing changes. Open it again and `MENU` applies it
  (`PRESET APPLIED`). Then DEFAULTS: apply it to restore SQUELCH, SCAN RESUME and BACKLIGHT (it also puts DUAL WATCH, BATTERY SAVE, KEY BEEP, VOX, SQ TAIL ELIM and ROGER BEEP back to their
  defaults, so note any you had changed). (AUDIT 51)
- [ ] **17.** SYSTEM > RESET ALL: `MENU` arms it (amber, `HOLD #`). A second `MENU` does nothing; a short `#` does nothing; `EXIT` cancels. **Do not hold `#`**
  unless you are ready to re-enter your settings. (AUDIT 42, 47)

## Part 3: CHAN MGR (receive only)

- [ ] **18.** From the main screen `MENU`, `2` opens CHAN MGR on the current group. Open a channel with `MENU`: note any flash or glitch at the switch to the editor; `EXIT`
  returns to the **same row**. (AUDIT 47)
- [ ] **19.** Open a channel, `DOWN` five times to TX POWER, change it (`MENU`, `UP`, `MENU`; use `DOWN` if it already reads MID), then `EXIT`: the selected row turns amber and reads `DISCARD CHANGES?`. Every hint strip fits its line.
  `EXIT` keeps editing; `EXIT` asks again; `MENU` discards and returns to the list. (AUDIT 47)
- [ ] **20.** `+ NEW` (it opens the editor on a new channel at the main screen's frequency, LOW power), edit nothing, go to SAVE (`UP` from the first row), `MENU`: a green `SAVED nn` box is **visible** for about a second and a
  half (nn is the new channel's number in the group). (It was reported on an earlier build; AUDIT 49 says it may never have been drawn.)
- [ ] **21.** The list is on the new channel: `MENU` opens it, `UP` once from the first row reaches DELETE (the blank row between SAVE and DELETE is skipped), `MENU` (amber `HOLD #`), a second `MENU` does nothing, then **hold `#`**:
  the radio beeps and the channel is deleted after about a second. Check the numbers of the others (a gap is expected after a delete in the middle). (AUDIT 47)

## Part 4: FM (receive only)

- [ ] **22.** `MENU`, `9` opens FM on 100.0 MHz. The large digits, the name line, the four signal bars and the tuning scale read
  well at real size; the speaker mark shows in the station list (step 25). (AUDIT 45)
- [ ] **23.** Type `981`: it tunes at once. Type `1100`: `RANGE 87.5-108.0`. `UP` and `DOWN` move 0.1 MHz. (AUDIT 45)
- [ ] **24.** Hold `UP` for a second on an empty spot: `SEEKING`, the audio mutes, and it stops on the next station or gives up after three seconds. Let go as soon as `SEEKING` shows. Write down whether the
  four bars match how strong the station is. (AUDIT 45)
- [ ] **25.** `MENU` opens STATIONS. `+ SAVE THIS` (the first row when the tuned frequency is not saved) saves the tuned frequency and the list stays on it (it reads `(unnamed)`); `*` then `EXIT` cancels; `*` then `MENU` deletes that new station
  (leave your other stations alone). Back in the tuner, `#` and `*` jump between saved stations (not named on the strip). (AUDIT 45)

## Part 5: SPECTRUM and BAND SWEEP (receive only)

- [ ] **26.** Tune to a frequency with a known signal (NOAA 162.400 or a local repeater) and open SPECTRUM (`MENU`, `4`). The signal is marked and `PEAK` reads its
  frequency. On a quiet frequency, no peak is marked from noise at the default settings. (AUDIT 50)
- [ ] **27.** With the signal marked, `MENU` tunes the VFO to the peak and leaves; the main screen shows that frequency in VFO mode. Then open SPECTRUM on a quiet frequency and press `MENU`: `NO SIGNAL` shows clearly
  over the drawing and SPECTRUM stays open. (AUDIT 50)
- [ ] **28.** In SPECTRUM press `A/B`: it stops on the signal and you hear it; `MENU` tunes to it. (AUDIT 50)
- [ ] **29.** BAND SWEEP (`MENU`, `5`; it starts on VHF, which holds NOAA 162.400): after a pass or two the list shows hits. `MENU` hears one, `MENU` again tunes the VFO to it and leaves, `EXIT` instead goes back
  to the sweep. The title bar and strip are legible. (AUDIT 50)

## Part 6: FSK PROBE (receive only; steps in [SONDE-APP.md](SONDE-APP.md))

- [ ] **30.** `MENU`, `8` opens FSK PROBE on 404.800 MHz at 4800 baud, options `LEN316 TH1 INV- BW2.4 S4`. `MENU` turns the modem on; `DOWN` seven times from REG 02 to REG 59, which should read `1008` (the value the app writes), and four more to REG 5D, `3B20`. A
  different read-back is a finding. (AUDIT 44)
- [ ] **31.** Press `#`. `ERASING` shows (it counts sectors up to 32): write down how long it lasts, and that the keys come back. Then `REC` counts up. Wait 30 seconds, press `#`: `MODEM ON  SAVED n`. Leave
  the app with `EXIT`. (AUDIT 40, 44)
- [ ] **32.** On the computer: `python3 firmware/source-changes/probe_capture.py PORT noise-a.bin` prints a summary and writes a `.csv` (the radio must be on its main screen, not in an app). Then follow steps (a) in
  SONDE-APP.md (S4 against S2) to settle the sync-width question, and (b) at a radiosonde launch (about 11:00 and 23:00 UTC, from SONDE-APP.md; not checked here). (AUDIT 44)

## Part 7: extras, if installed (receive only)

- [ ] **33.** HAM TOOLS (not in a slot in the release: put it in one with the installer's app menu, or open it with a SIDE key set to `APP n`): arm the ID timer (`*`) and press `EXIT` once: the notice `HOLD EXIT: TIMER RUNS` is visible. Then hold `EXIT` to leave. (AUDIT 49)

## Part 8: installer and host tools (the radio is read, never made to transmit)

- [ ] **34.** `python3 firmware/source-changes/install.py --dry-run --port PORT` (in an unpacked bundle: `python3 install.py --dry-run --port PORT`) answers its menus, then identifies the radio correctly (this firmware, not stock, not "bootloader") and runs
  each tool in its read-only mode. A dry run makes no backup (it says so) and sends no write or erase command. Note anything it says it did not expect. It has never run against a real radio. (AUDIT 48)
- [ ] **35.** After a real install of your choosing (your decision): every step says verified, the calibration is identical afterwards, how long each wait lasted is in the report, and the radio shows the new
  apps, channels and boot screen. (AUDIT 48)
- [ ] **36.** `python3 firmware/source-changes/fm_stations.py build my.csv -o my.fmst`, then `python3 firmware/source-changes/fm_stations.py install PORT my.fmst --before fm-before.bin`: write down how long the install takes; the names then show in the FM
  station list. (AUDIT 45)

## Part 9: the transmitter (dummy load, and your decision)

**Stop here unless a dummy load is on the antenna port and you have decided to transmit.** Leave TX INHIBIT on until step 38.

- [ ] **37.** With TX INHIBIT **on**, press `PTT`: `TX INHIBIT IS ON` and no carrier. (Seen before; check it still holds on this image.)
- [ ] **38.** Switch TX INHIBIT **off** (SETTINGS > RADIO, `DOWN` six times to TX INHIBIT, `MENU`, `UP`, `MENU`). Set TX BANDS to CE HAM (144 to 146 and 430 to 440 MHz), go to VFO mode, type `446000` (outside it), hold `PTT`: `OUTSIDE TX BANDS` and no
  carrier. An air-band frequency (type `118000`) shows the same, but no TX BANDS choice includes the air band, so that does not test the AM rule alone. To test it alone: SETTINGS > KEYS > SIDE1 SHORT = AM/FM (`MENU`, then `DOWN` until it reads `AM/FM`: it starts on NONE, which is the retired value 5, so that is two presses; `MENU`),
  type `145500` (inside CE HAM), press `SIDE1` so the mode line reads `AM`, hold `PTT`: `OUTSIDE TX BANDS` and no carrier. Put SIDE1 SHORT back with `MENU`, `#`, `MENU` and retune. (AUDIT 29, 42)
- [ ] **39.** Memory mode on a weather channel (`BAND`, `8`; it has no transmit frequency): `PTT` shows `RX-ONLY CHANNEL`. Then BUSY LOCKOUT on (SETTINGS > RADIO, five `DOWN`, `MENU`, `UP`, `MENU`), VFO mode on NOAA 162.400 (a signal present), `PTT`: `CHANNEL BUSY`
  (on the weather channel itself the receive-only refusal wins, because it is checked first).
- [ ] **40.** TEXT (an extra: put it in a slot first). With TX INHIBIT **on**, send a message: `TX BLOCKED: SEE SETTINGS`, nothing transmitted. Only if you
  decide to transmit, with the dummy load: switch TX INHIBIT off and choose a TX BANDS value that includes the main-screen frequency (FCC HAM includes 146.520), with a second radio you own on the same frequency close enough
  to hear (a dummy load radiates almost nothing; whether a message crosses that way is untested): the title bar is red and reads `TX` only while the transmitter is keyed, and the
  message arrives. Note whether the redraw during the transmission disturbs the message. (AUDIT 50)
- [ ] **41.** Set TX INHIBIT back **on**, TX BANDS to FCC HAM and BUSY LOCKOUT to OFF when you are done.

The fault screen (`APP FAULT` / `CORE FAULT`) and the transmitter dropping at a fault (AUDIT 43) can be exercised only with the test app `apps/hello --features
fault-test`, which is in no release and whose key `5` lets the app transmit. That is a dummy-load test and a separate decision; it is not part of this walk-through.
