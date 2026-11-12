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
- [ ] **1a.** Battery reading (receive only; AUDIT 57). Watch the voltage at the right of the status bar for a minute with the radio idle on a quiet channel: it should hold steady, moving at most 0.01 or 0.02 V and only
  now and then, and the bars should not flicker. Note the voltage and the bars. Switch the backlight or the receive audio on and off: the number may drift over several seconds, never jump. (BATTERY CAL in SETTINGS is
  not smoothed; it keeps its quick, noisier reading.)
- [ ] **1b.** Low battery warning (AUDIT 58; nothing here has been seen on the radio). There is no way to drain a pack on
  demand, so make the radio believe the voltage is lower with BATTERY CAL. Write down the current BATTERY CAL value first
  (SETTINGS > SYSTEM). Leave SETTINGS with the radio on the main screen and a quiet channel. Set BATTERY CAL to `720`
  (type `7`, `2`, `0`: it takes the value at the third digit, and 7.20 V is under the 7.30 V first threshold). Within
  about ten seconds (the slow average has to move) expect the red `LOW BATTERY` box for four seconds and three falling
  tones from the speaker. No second warning should follow. Then type `690` (7.00 V is the second threshold: 6.90 V is below it): the notice and chime
  come once, then again 10 minutes later (leave it on the main screen to check). Type `780`: no warning. Type `720`
  again: a new warning (it re-armed above 7.45 V, which 7.80 V is). Check that KEY BEEP off silences the chime and still
  shows the notice, that a transmission on a dummy load (step 38) never shows it, and that with a station received the
  chime waits until the squelch closes. Put it back with `#` in BATTERY CAL (the nominal 2537) or by typing the true battery volts
  you measured; if the main screen's voltage has not moved after leaving SETTINGS, say so (the core reads the setting at boot, power-cycle to be sure). Report what you saw and heard.
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
- [ ] **8b.** DUAL WATCH: set side A to a quiet frequency and side B to NOAA 162.400 (`A/B` moves between sides). Hold `A/B` for a second or more: `DWR` appears in the status bar, the line at the left edge runs across both
  sides, and the radio stays on the side that is not the weather channel until it hears one, then it holds on the weather side (`RX` there) and goes back to swapping a moment after the signal ends. Hold `A/B` again: `DWR`
  goes and the line returns to the active side only. Check a short `A/B` still switches the side, and that SETTINGS > RADIO > DUAL WATCH agrees (also after a power cycle). Note whether the swapping makes a click, drops the first
  syllable of a call, or ever stays on the wrong side. (AUDIT 55)
- [ ] **8a.** Tune a steady signal (NOAA 162.400): the green `RX` marker beside the active side stays lit, without flicker, for as long as the squelch is open, and goes out when the signal goes. Then a weak signal and
  SQUELCH 0 (RADIO > SQUELCH): note whether `RX` still flickers. It followed a free-running 6 Hz blink before (the guide's marker row). (AUDIT 55)

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
- [ ] **23a.** On a strong station, press `UP` and `DOWN` several times, type a frequency, `#` and `*` between saved stations, and tune from the list: the audio must keep playing under every key
  beep (a short beep, then the broadcast again), not cut after the first key. The same after saving and after a cancelled delete. Report if there is a gap and how long. (AUDIT 55)
- [ ] **24.** Hold `UP` for a second on an empty spot: `SEEKING`, the audio mutes, and it stops on the next station or gives up after three seconds. Let go as soon as `SEEKING` shows. Write down whether the
  four bars match how strong the station is. (AUDIT 45)
- [ ] **25.** `MENU` opens STATIONS. `+ SAVE THIS` (the first row when the tuned frequency is not saved) saves the tuned frequency and the list stays on it (it reads `(unnamed)`); `*` then `EXIT` cancels; `*` then `MENU` deletes that new station
  (leave your other stations alone). Back in the tuner, `#` and `*` jump between saved stations (not named on the strip). (AUDIT 45)

## Part 5: SPECTRUM and BAND SWEEP (receive only)

- [ ] **26.** Tune to a frequency with a known signal (NOAA 162.400 or a local repeater) and open SPECTRUM (`MENU`, `4`). The signal is marked and `PEAK` reads its
  frequency. On a quiet frequency, no peak is marked from noise at the default settings. (AUDIT 50)
- [ ] **27.** With the signal marked, `MENU` tunes the VFO to the peak and leaves, to the main screen (even when SPECTRUM was opened from the APPS list); it shows that frequency in VFO mode. Then open SPECTRUM on a quiet frequency and press `MENU`: `NO SIGNAL` shows clearly
  over the drawing and SPECTRUM stays open. (AUDIT 50)
- [ ] **28.** In SPECTRUM press `A/B`: it stops on the signal and you hear it; `MENU` tunes to it. (AUDIT 50)
- [ ] **29.** BAND SWEEP (`MENU`, `5`; tune the main screen to 162.400 first and it opens on `148-174`): after a pass or two the list shows hits. `MENU` hears one, `MENU` again stops listening and the sweep
  carries on (the pass counter keeps going), `EXIT` while listening does the same. The title bar and strip are legible. An amber triangle with a line down to the trace marks the selected hit on the graph, moves with `UP`/`DOWN`, stays while listening, and sits on the peak (a hit at the band's first or last column keeps its triangle whole). (AUDIT 50, 55, 57)
- [ ] **29a.** In BAND SWEEP press `BAND` a few times: the title bar names each preset (`AIR`, `136-144`, `2M`, `148-174`, `MURS`, `MARINE`, `WX` ...) and the sweep restarts; hold `BAND` goes back one. Open it from the
  APPS list, hear a hit, press `VFO/MR`: you land on the main screen (not the APPS list) on that frequency in VFO mode. Try `VFO/MR` from the sweep view with a hit selected. (AUDIT 55)

## Part 6: FSK PROBE (receive only; steps in [SONDE-APP.md](SONDE-APP.md))

- [ ] **30.** `MENU`, `8` opens FSK PROBE on 404.800 MHz at 4800 baud, the option rows `RS41 FSK LEN316 TH1` and `INV- BWAUTO S4`. `MENU` turns the modem on; `DOWN` seven times from REG 02 to REG 59, which should read `1008` (the value the app writes), and four more to REG 5D, `3B20`. A
  different read-back is a finding. (AUDIT 44)
- [ ] **31.** Press `#`. `ERASING` shows (it counts sectors up to 32): write down how long it lasts, and that the keys come back. Then `REC` counts up. Wait 30 seconds, press `#`: `MODEM ON  SAVED n`. Leave
  the app with `EXIT`. (AUDIT 40, 44)
- [ ] **32.** On the computer: `python3 firmware/source-changes/probe_capture.py PORT noise-a.bin` prints a summary and writes a `.csv` (the radio must be on its main screen, not in an app). Then follow steps (a) in
  SONDE-APP.md (S4 against S2) to settle the sync-width question, and (b) at a radiosonde launch (about 11:00 and 23:00 UTC, from SONDE-APP.md; not checked here). (AUDIT 44)

## Part 6b: FSK PROBE on APRS, 144.390 MHz (receive only; the whole procedure, commands and result table are in [APRS-TEST.md](APRS-TEST.md))

Nothing in this part has been seen on a radio. It is the stand-in for the radiosonde test (steps 30 to 32) until a launch can be watched.

- [ ] **32a.** On the Mac, before touching the radio: `brew install librtlsdr direwolf`, save `sdr.conf` and run the `rtl_fm ... | direwolf ... | tee aprs-reference.txt` line from APRS-TEST.md. Wait until it prints decoded packets with timestamps.
- [ ] **32b.** FSK PROBE (`MENU`, `8`), press `0` once: `144.390`, `1200 baud`, `SYNC FEFE`, and the option rows `APRS F1224 LEN256 TH1` and `INV- BWAUTO S4`. `MENU`: modem on. Write down `SYN` and `WRD` after two minutes against the number of packets the reference decoded, and what REG 6E reads
  (`UP` from REG 02: the tone in Hz) while a packet is on the air. Press `0` on the preset item again and it returns to `RS41`, 404.800 MHz, 4800 baud.
- [ ] **32c.** Back on `APRS`: press `#`, write the Mac's clock to the second, wait 3 minutes, press `#`, `EXIT`, and run `python3 firmware/source-changes/probe_capture.py PORT aprs-a.bin`. It prints the summary and then `AX.25 decode`: either `PASS` lines with the packet text and a time after the start, or
  `why no frame passed` with the bit statistics. Match every `PASS` to a line of `aprs-reference.txt` by time. Note the three timing lines: time between words (13.3 ms expected), most words read in one interrupt, frames ended by count or by the chip's done flag.
- [ ] **32d.** Repeat 32c for `F1218` and for plain `FSK` (`A/B`, then `0` steps the mode), and once more on `F1224` with `S2` (five `A/B` from `MODE`). Read the result row for each in APRS-TEST.md's table and write the next step it names. A capture that finds nothing is still a result: record `SYN`, `WRD` and the table.

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

- [ ] **40a.** Battery sag under load (dummy load, TX INHIBIT off as in step 38; do this with a fully charged pack): note the idle voltage on the status bar, key `PTT` on the dummy load for about 5 seconds, and watch it. Within about a second of keying the
  voltage should drop (a fraction of a volt, the "sag") and then hold at the lower value while keyed, and the bars may drop by one. On release it climbs back to the idle number over about two seconds, without a jump, and
  then stays calm. Report the idle voltage, the keyed voltage and how long each change took. (AUDIT 57; the filter is `firmware-source/src/app/battery_filter.rs`.)

The fault screen (`APP FAULT` / `CORE FAULT`) and the transmitter dropping at a fault (AUDIT 43) can be exercised only with the test app `apps/hello --features
fault-test`, which is in no release and whose key `5` lets the app transmit. That is a dummy-load test and a separate decision; it is not part of this walk-through.
