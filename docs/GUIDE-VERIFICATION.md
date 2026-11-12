# Line-by-line check of the user guide and the radio checklist

Date: 2026-11-12. Checked against commit `764011e` (main, "Merge pull request #16 from k5njm/docs/guide-refresh").
Documents checked: [USER-GUIDE.md](USER-GUIDE.md), [RADIO-CHECKLIST.md](RADIO-CHECKLIST.md), and where the same statement appears
[../README.md](../README.md) and [../channels/README.md](../channels/README.md). This is the record the first guide-writing run (AUDIT 52)
was stopped before it could make. Nothing here was run on the radio.

## Method

Every factual statement (a key and what it does, a screen text, a label, a title, a hint strip, a default, a limit, a count, an address, a
command line, the order of rows) was looked up and given a verdict: CONFIRMED, WRONG (corrected in the document), MISLEADING (true but
incomplete in a way that would mislead; corrected) or UNVERIFIABLE (no source or frame decides it; the text now says so).
Evidence, in order: the source (`apps/*`, `src/app/*`, `src/ui/*`, `src/device/keypad.rs`, `src/app/group_table.rs`, `src/flash_map.rs`,
`apps/settings/common.rs`, `firmware/tools/build_release.py`, `src/device/radio.rs`), then the host tools' own usage text, then the simulator's
golden frames, then AUDIT rows 25 to 52 for what has been seen on the radio. All 44 pictures in `docs/img` were opened and compared with their
captions and the text around them: every caption matches its frame. A few facts rest on arithmetic done by hand on the source (the grid
locator example, the FSK PROBE register values, the up/down band wrap); they are marked as worked through. Nothing was executed on a
serial port, and no firmware, app, tool, test, script or golden frame was changed.

## Statements checked

Counts are of distinct statements (a key and its action, one label, one default, one number, one command line); they are approximate to a
few percent because some table rows carry several.

| Section | Checked | WRONG | MISLEADING | UNVERIFIABLE |
|---|---|---|---|---|
| USER-GUIDE: intro, status, reading this guide | 22 | 1 | 1 | 0 |
| Main screen (schematic, elements, notices, channel numbers) | 46 | 3 | 1 | 0 |
| Keys on the main screen, typed frequencies, keypad lock | 34 | 2 | 1 | 1 |
| Colours, hints, scroll mark | 12 | 0 | 1 | 0 |
| Channel groups, CH GROUPS menu, CUSTOM RANGE | 50 | 0 | 2 | 1 |
| Scanning | 20 | 1 | 0 | 1 |
| APPS list | 16 | 1 | 0 | 0 |
| SETTINGS (changing a value, presets, about, grid locator, reset) | 58 | 1 | 2 | 0 |
| CHAN MGR | 48 | 0 | 0 | 0 |
| SEARCH / QT SCAN | 14 | 0 | 1 | 0 |
| SPECTRUM | 28 | 1 | 0 | 1 |
| BAND SWEEP | 20 | 0 | 0 | 0 |
| FOX HUNT | 14 | 0 | 1 | 0 |
| RPT FINDER | 16 | 0 | 1 | 0 |
| FSK PROBE | 30 | 1 | 0 | 1 |
| FM | 38 | 1 | 1 | 2 |
| HAM TOOLS | 14 | 0 | 0 | 0 |
| TEXT | 16 | 0 | 1 | 0 |
| Settings reference (62 rows x values, default, help line) | 186 | 0 | 0 | 0 |
| Transmit safety (check order, 10 TX BANDS rows) | 32 | 0 | 0 | 1 |
| Loading from a computer (commands, options, what each load replaces) | 36 | 2 | 0 | 0 |
| Backup and recovery | 8 | 0 | 1 | 0 |
| Troubleshooting | 22 | 0 | 2 | 0 |
| **USER-GUIDE total** | **~780** | **14** | **16** | **8** |
| RADIO-CHECKLIST (41 steps, preamble, closing paragraph) | ~175 | 8 | 13 | 5 |
| README.md | 26 | 0 | 0 | 0 |
| channels/README.md | 22 | 0 | 0 | 1 |

(One wrong statement that appears in two places, for example the two-second notice or the 108.0 MHz edge, is counted once. The
UNVERIFIABLE column counts the statements listed in the second table below.)

The settings reference is the one section that was already right: every label, value list, default and help line matches
`apps/settings/common.rs` and `Settings::DEFAULT` (and `test_docs.py` checks the labels). The launcher table, the channel-group table and the TX BANDS
table also match the source exactly.

## WRONG and MISLEADING statements (corrected)

WRONG: false, or a step that cannot show what it says. MISLEADING: true but incomplete in a way that would mislead. The guide has 14 WRONG and 16
MISLEADING rows here, the checklist 8 WRONG and 13 MISLEADING (a row that stands for the same fix in two places counts once).

| Where | Type | What it said | What is true | Evidence |
|---|---|---|---|---|
| Guide, status | WRONG | AUDIT rows "25 to 51" | The record now runs to row 52 | `AUDIT.md` |
| Guide, reading this guide | MISLEADING | Beeps "on a press and on a hold" | A short press beeps on release, a hold beeps when it fires, auto-repeat is silent | `app/keys.rs` (`Single`, `Long` only) |
| Guide, main-screen schematic | WRONG | `PS DWR SCAN UP EXIT` on one line | With a `PS`, `DWR` or `VOX` flag the label is `SCAN UP` / `SCAN DN` without `EXIT` (the guide's own table said so) | `src/ui/standby.rs` `draw_flags` (`roomy`) |
| Guide, schematic and Row 2 | WRONG | `147.12000` small frequency beside a name | Four decimals, `147.1200`; with CH DISPLAY NAME the name can be 16 characters | `standby.rs` `draw_row2` |
| Guide, notices and groups | WRONG | `NO CH IN ...` goes away "after a few seconds" | About two seconds (200 ticks of 10 ms); the same for `OUT OF RX RANGE` and `NO CHANNELS` | `app/mod.rs` `NO_CHANNELS_HOLD_TICKS`, `main.rs` `n10` |
| Guide, Row 1 | MISLEADING | The channel's place in its group | A channel outside the current group shows the radio's own number | `app/mod.rs` `put_channel_number` |
| Guide, typed frequencies; troubleshooting | WRONG | 87.5 to 108.0 MHz opens the FM app | 87.5 up to but not including 108.0; `108000` is the VFO's 108.000 MHz | `side_ops.rs` `commit_input` (`rx_allowed` first) |
| Guide, keypad lock | WRONG | "The radio gives no message when it locks or unlocks" | No message, but it beeps (KEY BEEP on), by hand and by AUTO KEYLOCK | `app/keys.rs` `dispatch`; `app/mod.rs` `poll_auto_lock` |
| Guide, keypad lock | MISLEADING | AUTO KEYLOCK locks "after 5, 10 or 15 seconds without a key" | Only on the main screen: never while scanning, while a signal opens the squelch, while transmitting, or in a menu or app | `poll_auto_lock` |
| Guide, colours | MISLEADING | "Red is meant to mean only" transmitting or a refusal | True of the shared screens; the older-look apps also use red (title bars, FOX HUNT bar, RPT FINDER selection, FSK PROBE `REC`) | frames `foxhunt`, `rptfinder`, `fskprobe-capture` |
| Guide, CH GROUPS menu | MISLEADING | "stays on the current channel ... otherwise goes to the first stored channel" | It also leaves VFO mode for memory mode; with no channel in the group it only shows the notice and stays where it was | `app/modes.rs` `select`, `Side::load_channel` |
| Guide, CUSTOM RANGE | MISLEADING | "Type six digits for each" | Up to six; missing digits count as zeros (`144` = 144.000); an empty field keeps the old value; `BAD RANGE` also for anything outside 16 to 560 MHz | `modes.rs` `commit_range_field`, `range_valid` |
| Guide, scanning | WRONG | "No on-screen hint says which keys work while scanning" | The status-bar label names `EXIT` when no `PS`/`DWR`/`VOX` flag shares the line; `UP`, `DOWN` and held `#` are not named | `standby.rs` |
| Guide, APPS list | WRONG | Strip `MENU or 1-9 open  EXIT back` (two spaces) | One space: `MENU or 1-9 open EXIT back` | `src/ui/launcher.rs` |
| Guide, SETTINGS | WRONG | Typed settings: "`MENU`, type digits, `MENU`", and one strip for all | RPT OFFSET (7 digits) and BATTERY CAL (3 digits) accept themselves at the last digit; LATITUDE and LONGITUDE need `MENU`; each has its own strip (`0-9 # default  MENU ok`, `0-9 * N/S # clear MENU ok`) | `common.rs` `dispatch_offset_input`, `dispatch_battery_input`, `draw` |
| Guide, SETTINGS | MISLEADING | `0` is unsafe on TX INHIBIT only | `0` also selects HIGH on TX POWER (first choice), in SETTINGS and in the channel editor | `common.rs` `scalar_floor`, `edit.rs` |
| Guide, grid locator | MISLEADING | `EM12kx` is `3 3`, `6`, `1`, `2`, `5 5`, `#`, `9 9` | Produces `EM12kx` (worked through `Loc::press`), but `#` is not needed: `9` after `5` is a different key and accepts the K; `#` is only for two letters on the same key | `common.rs` `Loc::press`, `Loc::next` |
| Guide, SEARCH | MISLEADING | `MENU` puts the main screen on the frequency "with the tone found" | The tone becomes both the receive and the transmit tone | `overlay.rs` `api_save_master_vfo` |
| Guide, SPECTRUM | WRONG | "The strip says what MENU will do" | With no peak the strip reads `UP DN pan  5 freq  EXIT` and does not mention `MENU` (which still works and shows `NO SIGNAL`); line 1 reads `NO SIGNAL  PAN 800k` | `spectrum/src/main.rs` `draw_hint`, `draw_status`; frame `spectrum-no-signal` |
| Guide, FOX HUNT | MISLEADING | Words `CLOSER +`, `FARTHER -` | `CLOSER +n`, `FARTHER -n` with the dB change; also `LAST n DBM` and `NO SIGNAL`; `NOT A VALID VALUE` was missing | `foxhunt/src/logic.rs` |
| Guide, RPT FINDER | MISLEADING | Titled "RPT FINDER" | That is the launcher name; the screen is titled `REPEATER FINDER`; candidates read `SEEN n` | `rptfind/src/logic.rs`; frame `rptfinder` |
| Guide, FSK PROBE | WRONG | `SAVED` shown "when `#`, `MENU` or `EXIT` ends" a capture | `EXIT` leaves the app at the same moment, so `SAVED` is seen only after `#` or `MENU`; `LEN` reads 318 with a two-byte sync | `fskprobe/src/logic.rs` `key`, `Opts::frame_bytes` |
| Guide, FM | WRONG | `UP`/`DOWN` "holding repeats", and a hold seeks | A hold is a seek after one second; repeat steps happen only if the key is still down when the seek ends (they start at 1.5 s) | `keypad.rs` (`Long` at 1 s, `Repeat` at 1.5 s), `fm/src/logic.rs` `key` |
| Guide, FM | MISLEADING | `NO SAVED STATIONS` "if none" | Also when the only saved station is the tuned one; `#` and `*` are not on the hint strip; the screen is titled `FM RADIO` | `fm/src/logic.rs` `neighbour`, `draw` |
| Guide, TEXT | MISLEADING | `TX BLOCKED: SEE SETTINGS` when TX INHIBIT or TX BANDS stops it | BUSY LOCKOUT stops it too; more notices exist (`WAIT: STILL SENDING`, `MESSAGES ERASED`, `CAN'T REPLY: TX BLOCKED`) | `app/tx.rs` (external path), `textapp/src/logic.rs` |
| Guide, loading | WRONG | `python3 install.py ...` | `install.py` is at the root of a bundle only; in a checkout it is `firmware/source-changes/install.py`. A dry run makes no backup | `bundle_release.py`, `install.py` docstring and `execute` |
| Guide, loading table | WRONG | Settings "never written by any tool here" | `set_boot_tune.py` rewrites the tune bytes of the settings record (the row above says so) | `set_boot_tune.py` |
| Guide, backup | MISLEADING | "Apps, the boot logo, channels, the VFO and the 20 DTMF contacts can be restored from a backup" | `restore_backup.py` restores channels, the VFO and contacts; the logo comes from the file its tool saved; apps are reinstalled from the release | `restore_backup.py` docstring |
| Guide, troubleshooting | MISLEADING | Fault second-line examples without `CODE` | FSK PROBE gives `SLOT 8 CODE 1769472` (a flash address) | `src/fault_text.rs` |
| Guide, troubleshooting | MISLEADING | `Keys do nothing` = locked | AUTO KEYLOCK may have locked them | `poll_auto_lock` |
| Checklist, step 13 | WRONG | "`EXIT` to leave" the LATITUDE entry | `EXIT` deletes one digit per press (six, then a seventh to leave) | `common.rs` `dispatch_coord_input` |
| Checklist, step 14 | MISLEADING | `#` between `5 5` and `9 9` | Harmless but not needed (see above) | `Loc::press` |
| Checklist, step 11 | WRONG | Change SQUELCH, `#` (goes to 3), `EXIT` (old value comes back) | Unobservable when the old value already is 3; the step now sets 5 first | `common.rs` `back_to_default`, snapshot |
| Checklist, step 34 | WRONG | The dry run "reads the calibration twice" and the waits are timed; `python3 install.py` | A dry run makes no backup (no double read) and flashes nothing, so there are no waits; it identifies the radio and runs the tools' read-only modes. `python3 install.py` fails in a checkout | `install.py` `execute` (`not done in a dry run`) |
| Checklist, step 38 | WRONG | An air-band PTT shows `OUTSIDE TX BANDS` "(AM never transmits)" | True, but every TX BANDS choice excludes the air band, so it does not isolate the AM rule; an AM test on 145.500 with CE HAM does (added, optional) | `radio.rs` `tx_ranges`, `enter_tx` |
| Checklist, step 39 | WRONG | `CHANNEL BUSY` with BUSY LOCKOUT on, on a weather channel | On a receive-only channel `RX-ONLY CHANNEL` is checked first; use the VFO | `app/tx.rs` order |
| Checklist, step 24 | WRONG | Stops "within three seconds" | Stops on a station or gives up after three seconds; a key held past the end of the seek repeats the 0.1 MHz step | `fm/src/logic.rs` |
| Checklist, step 19 | WRONG | Change TX POWER with `MENU`, `UP`, `MENU` | The row is the sixth (five `DOWN`), and `UP` does nothing on MID | `edit.rs` `adjust` (clamped) |
| Checklist, step 36 | WRONG | `fm_stations.py install ... my.fmst` | The `.fmst` must be built first (`build`) | `fm_stations.py` docstring |
| Checklist, step 25 | MISLEADING | `*` `MENU` "deletes a station" | It would delete one of your stations; it now deletes the one just saved (the list stays on it) | `save_current` sets `a.sel` |
| Checklist, step 27 | MISLEADING | One step for the peak and the no-signal case | The second case needs SPECTRUM reopened on a quiet frequency (the first `MENU` leaves) | `go_to_target` |
| Checklist, step 33 | MISLEADING | HAM TOOLS test, no way to open it, no way to leave | Not in a slot in the release; `EXIT` is a hold while the timer runs | `build_release.py`, `hamtools/src/logic.rs` |
| Checklist, step 40 | MISLEADING | TEXT "with the dummy load and a second radio" | TX BANDS must include the main-screen frequency (earlier steps leave CE HAM); a dummy load radiates almost nothing (see UNVERIFIABLE) | `app/tx.rs`, `radio.rs` |
| Checklist, step 41 | MISLEADING | Restore TX INHIBIT and TX BANDS | Also BUSY LOCKOUT (step 39) and SIDE1 SHORT (step 38) | steps 38 and 39 |
| Checklist, steps 1, 3, 38 | MISLEADING | VFO mode assumed | Step 6 leaves the radio in memory mode; the preamble now says to press `VFO/MR` | `modes.rs` `select` |
| Checklist, step 10 | MISLEADING | Padlock "after about 5 seconds" | Also needs a quiet squelch and the main screen; there is a beep | `poll_auto_lock` |
| Checklist, step 16 | MISLEADING | DEFAULTS restores SQUELCH, SCAN RESUME, BACKLIGHT | It also resets DUAL WATCH, BATTERY SAVE, KEY BEEP, VOX, SQ TAIL ELIM and ROGER BEEP | `common.rs` `PRESETS[4]` |
| Checklist, steps 20, 21 | MISLEADING | `UP` "past the blank row" to DELETE | One `UP` from the first row reaches DELETE (the blank row is skipped); `nn` in `SAVED nn` is the number in the group | `edit.rs` `field_at` loop |
| Checklist, step 5 | MISLEADING | Scan "stops ... after about a second"; SCAN RESUME changed without keys | Timing UNVERIFIABLE (below); `UP` reaches STOP from CARRIER, `DOWN` goes back; three `UP` from the first row reach the setting | `scan.rs`, `common.rs` `adjust` |
| Checklist, step 30 | MISLEADING | `UP`/`DOWN` to REG 59 | Seven `DOWN` from REG 02; REG 5D four more. `1008` and `3B20` worked through from `Opts::ctrl` and `len_field` | `fskprobe/src/logic.rs` |
| Checklist, "Before you start" | MISLEADING | `dump_nor.py ... --cal` and "check its sha256" | Right; added the `shasum` command and the `install_apps.py` command for the reinstall | tools' usage text |

## UNVERIFIABLE (text reworded or marked)

| Where | Statement | Why it cannot be checked | Now says |
|---|---|---|---|
| Guide, scanning; checklist step 5 | TIMED about 5 s, CARRIER 5 s grace, STOP about 1 s | Constants are in poll ticks with comments assuming 50 ms; `main.rs` polls the scan about every 10 ms (`delay_ms(2)` x 5 plus work). Never measured (AUDIT 50) | States both, says unmeasured |
| Guide, keypad lock; checklist step 10 | AUTO KEYLOCK 5, 10, 15 seconds | Counted in the same polls as the backlight, assumed 10 ms each; AUDIT 51 lists "the key lock's real delay" as open | Says assumed 10 ms, unmeasured |
| Guide, channel groups | "A radio from the factory has its test channels in slots 1 to 22" | No source or AUDIT row records it | Removed; says stock channels sit wherever they were put |
| Guide, SPECTRUM; checklist step 26 | no peak from noise at the default trigger | Needs real RSSI (AUDIT 50) | Stays as a thing to check |
| Guide, FM | signal bar thresholds | A guess (AUDIT 45) | Says so |
| Guide, FSK PROBE | crystal, sync order, whether the modem hears a signal | AUDIT 44 | Already marked unverified |
| Guide, transmit safety | the ALL limits are "what the stock firmware reports" | A source comment, not checked against stock | Left as written (already hedged in the text) |
| Checklist step 32 | radiosonde launches about 11:00 and 23:00 UTC | From `SONDE-APP.md`, an outside fact | Marked "from SONDE-APP.md; not checked here" |
| Checklist step 40 | a message crosses from a keyed radio into a dummy load to a second radio | No source decides it | Says it is untested |
| channels/README | the national calling frequencies etc. are "the same everywhere in the US" | An outside fact | unchanged |
| Guide, FM | `hold` repeat after seek | From the source only; never seen | Says "by the source" |

## Problems in the code or its comments (not fixed)

These are the most useful findings: the guide now describes what the code does, but these deserve a decision.

1. **FM seek leaves a strip that promises keys that do nothing.** While `SEEKING` shows, only `EXIT` is heard (`fm/src/logic.rs` `key`), yet the strip still
   reads `UP/DN step  MENU stations` (frame `fm-seek.png`).
2. **Hold on FM `UP`/`DOWN` is both a seek and a repeat.** The keypad sends `Long` at 1 s and `Repeat` from 1.5 s (`keypad.rs`); the app treats `Long` as seek and
   `Repeat` as a 0.1 MHz step, so a key held past the end of a seek starts stepping.
3. **Choosing a channel group in VFO mode (`BAND`, digit) switches to memory mode without saving the VFO** (`modes.rs` `select` calls
   `load_channel_num`; `vfo_backup` is only written in `toggle_vfo_channel`). `VFO/MR` afterwards restores the VFO as it was at the last `VFO/MR` press, not the frequency
   you were on. From reading the code; not run.
4. **Scan timing constants and their comments assume a 50 ms poll; the loop calls the scan about every 10 ms** (`scan.rs` comments "5.0s", "200ms"; `main.rs`
   `fast_tick % 5` with `delay_ms(2)`). Either the comments or the durations are wrong; the guide cannot give seconds.
5. **QT SCAN and SEARCH show `MENU SAVE` from the start, though `MENU` does nothing until a tone is found**; SPECTRUM's strip does the opposite (omits `MENU` when no
   peak, though it works).
6. **Dead code:** `modes.rs` `(Long, Menu)` in the CH GROUPS menu ("hold MENU: pick the group and start scanning"); `keypad.rs` never sends a long `MENU`
   (also in `TEST-DOUBLES.md`). `keyfn.rs` `KeyFunction::label()` is unused and its words (`WIDE/NAR`, `MODE`, `POWER`) differ from the ones SETTINGS shows
   (`BANDWIDTH`, `AM/FM`, `TX POWER`).
7. **Tool docstrings say "UNVERIFIED on real hardware"** for `restore_backup.py`, `channel_extras.py`, `boot_logo_raw.py` and the follow-up note of
   `channel_plan.py`, but AUDIT 30, 32, 35, 37 and 40 record each of them used on the radio. Only `fm_stations.py` and `install.py` are unseen.
8. **`0` while changing TX POWER selects HIGH** (and `0` on TX INHIBIT selects OFF; AUDIT 51 flagged only the second). Both loosen a transmit setting by one key
   (`MENU` still has to confirm).
9. **Red is not only for transmit and refusals** (the `standby.rs` comment says it is): FOX HUNT's bar, RPT FINDER's selection and FSK PROBE's `REC` count use it.
10. **The default SIDE1 SHORT is the retired value 5**, which shows as NONE, and `#` writes it back (known, AUDIT 51).
11. **The fault screen's `CODE n` is a decimal number** for FSK PROBE's refused flash address (`1769472` = `0x1B0000`).

## What the checker did not change

`AUDIT.md` row 52 still says the guide "has been skimmed in review, not checked line by line"; it should now point to this file. The documents say nothing is
verified on the radio that AUDIT does not record as seen.
