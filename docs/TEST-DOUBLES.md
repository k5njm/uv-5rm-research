# Test doubles against the code they stand in for

Nothing in this file has been checked on a radio. "Real" means what the firmware source says (file:line in
`firmware/firmware-source/src/`, branch `test/fakes-audit`); "unknown" means the source cannot say (the vendor bootloader,
the chip, timing).

## Why this exists

Four faults reached the owner's machine or radio because a stand-in was kinder than the thing it stood in for:

| # | What happened | The stand-in that was kinder |
|---|---|---|
| 1 | `restore_backup.py` wrote only the channels that differed; the firmware erases a channel's whole 4 KB sector on the first write of a session, so unchanged neighbours were wiped (AUDIT 37) | the fake radio stored single records and never erased |
| 2 | The core's statics grew, the app arena moved, every app hung the radio (AUDIT 31); the first diagnosis read a stale ELF from the wrong architecture's cache | nothing in the simulator runs the core; apps run as host code |
| 3 | FSK PROBE held the RX-FIFO clear bit and matched 16 bits of sync; scenarios showed packets arriving (AUDIT 44) | the mock modem delivered a word per interrupt whatever was written to its registers |
| 4 | `scripts/dev.sh unit` and `check` passed on one machine and failed to compile for everyone else: a test fixture, `fm-stations.bin`, was hidden by the `*.bin` rule in `.gitignore` | the working tree (a local file git did not carry) |

The audit below was done by reading each double next to the code it replaces. The dangerous class is **fake is more
forgiving**: it accepts, ignores or succeeds where the real code refuses, erases, truncates, clamps, wraps, blocks or
faults. Every such row has an action; the ones that could hide a bug in a tool or app were fixed in the fake, with a test
that fails against the old fake (the commands are at the end).

Verdicts: **same**, **diff** (different, neither clearly kinder), **stricter** (fake refuses more than the real code),
**forgiving** (fake kinder than real), **unknown** (the source cannot say).

## What the audit found (read this first)

Real defects in tools, apps and the core, exposed by making the doubles faithful:

1. **Three apps never showed their notices on the radio.** The core hands apps Press and Release events around every
   Single (`device/keypad.rs:169-215` pushes Single, Release, Press in one poll; `app/mod.rs:866-871` dispatches all of
   them; `app/overlay.rs:459` forwards every kind). CHAN MGR (`SAVED CH01`), FM (`DELETED`, `SAVED`, the out-of-band
   notice) and HAM TOOLS (`HOLD EXIT: TIMER RUNS`) cleared their notice on any key event, so the Release that follows
   the Single that raised the notice removed it before the next draw. The simulator sent Single only, so six golden
   frames showed notices that cannot be seen on a radio. Fixed in the three apps (ignore kinds below Single), the
   simulator now sends the full sequence, the golden frames are unchanged. **Core suggestion:** filter Press (0) and
   Release (1) out in `overlay::dispatch_key` (one line, `app/overlay.rs:459`), which fixes every present and future app;
   no app uses them (`grep KIND_PRESS` finds nothing).
2. **Long MENU does not exist.** `supports_long` (`device/keypad.rs:40-55`) has no `Menu`, so no long MENU event is ever
   sent. Dead code: Band Sweep's "hold MENU: open in Spectrum" (`apps/bandsweep/src/logic.rs:605`) and the mode menu's
   "hold MENU: pick the group and start scanning" (`app/modes.rs:160`). Not fixed (it is a core or a design question:
   add `Menu` to `supports_long`, or move the action to a key that has a long press). The simulator test for it is
   skipped, not deleted, and `known_gaps_are_still_gaps` fails the day the keypad can send it.
3. **The HIL rig's `hold` pressed for 900 ms; the radio's long press starts at 1000 ms** (`device/keypad.rs:9`). On
   hardware a `hold` step would have been read as a plain press. Now 1300 ms (the repeat starts at 1500 ms), with a
   `repeat` step added, a parser error for `hold` on a key with no long press, and a test that reads the thresholds from
   the keypad source.
4. **`restore_backup.py` could not roll back a channel.** "Putting the old contents back" writes into a sector that was
   already erased this session; flash programs only clear bits, so the old contents cannot be restored (the fake used to
   overwrite). The tool now says the channel is wrong until it is run again (a new session erases the sector and rewrites
   every channel of the image in it) and stops; the test runs the re-run and checks the repair.
5. **`cps_tool.py` died on one damaged frame.** The firmware answers a bad CRC with `ERR 0x01` and does not act on the
   frame (`cps/mod.rs:83-85`); the tool treated every `ERR` as a refusal and quit. The old fake answered a bad CRC with
   silence, which the tool retried. Now `ERR 0x01` is retried like silence. All tools that `exec()` its `Cps` class inherit it.
6. **Simulator checks that passed because the simulator invented events:** Search/QT Scan's "a long press doesn't switch"
   (a held `1` is a Single on release: it switches), and the `hold MENU` above.

Core defects found (not changed in this task; `file:line` and a suggested fix):

* `app/overlay.rs:787-813`: `api_nor_write` and `api_nor_erase_sector` refuse addresses below `0x14C000` but have no upper
  bound. An address at or above `0x200000` goes out as a 24-bit SPI address (`drivers/norflash.rs:42-46`), which a 2 MB chip
  decodes modulo its size (unknown for this part) or ignores. Suggested: reject `addr as u64 + len as u64 > 0x20_0000`.
  `api_nor_read` has no bound either (a read past the end returns garbage, harmless).
* `app/overlay.rs:844-856`: `api_chan_read` does not check `num` against 999; channel 1000 and up read the VFO, settings and
  DTMF area as if they were channel records and return true. `api_chan_write` does check (`:960-965`). Suggested:
  `num > super::MAX_CHANNEL_NUM` returns false.
* `app/overlay.rs:459`: Press/Release forwarded to apps (item 1).
* `device/keypad.rs:40-55` / `app/modes.rs:160`: dead long MENU (item 2).

Differences that stay recorded, because the real behaviour is unknown or the cost is not worth it, are in the tables.

## 1. The fake radio on the programming port

`firmware/tests/fakeserial/serial.py` stands in for `src/cps/{mod,frame}.rs`, `src/device/storage.rs`,
`src/hal/wear_leveled.rs`, `src/drivers/norflash.rs` and (for settings) `flash_map.rs`. It is now a port of those: the
byte-at-a-time frame reader, a session state machine, and a flash that ANDs on program and sets bits only on erase.
`firmware/tests/test_fake_fidelity.py` checks every constant and the settings clamp table against the Rust source, so a
change there fails a test rather than silently widening the gap. Real line numbers are `cps/mod.rs` unless stated.

| Behaviour | Real | Fake (before, now) | Verdict |
|---|---|---|---|
| Frame before any handshake | only `HandshakeDetector` runs outside a session (`:27-44`, polled at `main.rs:396-399`); the bytes are consumed, nothing is answered | answered every command, no handshake needed; now ignored (`serial.py:263`) | **forgiving, fixed** |
| Second handshake inside a session | not recognised (the detector is not running); bytes go to the frame reader (`:47-130`) | ACKed again; now junk bytes (`serial.py:248-261`) | **forgiving, fixed** |
| Which screens answer the handshake | the detector is polled every main-loop iteration in every mode, including a running app (`main.rs:396-399`); the docs say "main frequency screen" | any state; one state ("standby" = any non-session state) | unknown (docs and source differ; the radio is not asked) |
| RX byte ring | 256 bytes, oldest dropped on overflow (`hal/uart.rs:6-31`): a busy radio can lose bytes of a handshake | unbounded | unknown; hosts retry five times |
| Bad CRC | `ERR 0x01` with the frame's address (`:83-85`) | silence, "host retries"; now `ERR 0x01` (`serial.py:289-294`) | **forgiving, fixed** (exposed tool defect 5) |
| Length over 512 | header dropped silently, reader resyncs on the next `0xA6` in the data (`frame.rs:121-122`) | waited for the bytes, then consumed them; now a port of the reader (`serial.py:151-192`) | **forgiving, fixed**; no tool sends one |
| Junk before a header | skipped (`frame.rs:99-103`) | stalled forever if the buffer did not start with `0xA6`; now skipped | **forgiving, fixed** |
| Unknown command | `ERR 0x03` (`:128`) | `ERR 0x05`; now `0x03` | diff, fixed |
| Bad address / length | `ERR 0x02` for reads (`:170`), writes (`:232`), logo, apps, tables | `0x05` for reads and VFO/contact/settings length mismatches; now `0x02` | diff, fixed |
| Idle in a session | 2000 ms without a byte: `SCB::sys_reset()` (`:15`, `:68-73`) | never; now the radio goes down after `FAKE_RADIO_IDLE_S` (default 2.0) | **forgiving, fixed**. No tool sleeps in a session today (checked); a future one that waits on a person would hit it |
| END | reply, flush, `sys_reset()` (`:122-127`) | replied and carried on; now the instance never answers again | **forgiving, fixed** |
| Time to restart after END / after the idle reset | a full boot (logo, tune); host tools return at once and a second tool may run into it | not modelled; `faults.json` `boot_delay_s` ignores everything for S seconds after the port opens | **unknown**; configurable. `Cps` tries the handshake five times with a 1 s read each, so it survives about 4 s |
| Port read timeout | blocks for the timeout | returns at once; `FAKE_RADIO_BLOCK=1` waits it out | diff, configurable (tests that depend on elapsed time set it) |
| Per-session erase record | `reset_channel_write_session()` at session start (`:53`, `storage.rs:192`) | per `Serial` object; now reset at each handshake | same / fixed |
| First channel write erases its sector | `storage.rs:196-207` | already modelled (AUDIT 37) | same |
| Channel numbers | 0..=999, multiple of 32, exactly 32 bytes (`:174-189`, `:236-250`); 1000..1023 of the table area refused | same | same |
| Flash program | ANDs into what is there; only an erase sets bits (`drivers/norflash.rs:71-95`) | overwrote for channels (after erase: same), logo, tables, app slots; now ANDs | **forgiving, fixed** (exposed defect 4) |
| Page crossing | the driver splits every write at 256-byte pages (`norflash.rs:82-95`), so crossing is safe | no pages | same in effect |
| Erase address | any address; the chip erases the containing 4 KB sector (`norflash.rs:97-109`, no check) | aligned down | same |
| Erase time | 4 KB sector erase blocks until the chip is ready or 1,000,000 polls have passed, then carries on silently (`norflash.rs:62-69`) | zero | **unknown**; tools allow 20 s for the first logo/app chunk and 2 s elsewhere (`cps_tool.py`, `channel_extras.py`, `fm_stations.py` pass `ERASE_TIMEOUT` for table offset 0) |
| Settings write | `Settings::from_bytes` clamps (`flash_map.rs:555-614`), then a wear-levelled save (`storage.rs:51`) | stored raw (tests used patterns no radio would keep); now clamped by a table checked against `flash_map.rs` | **forgiving, fixed**; two tune-tool fixtures held settings the radio would have changed |
| Blank settings read | `load_settings().unwrap_or(DEFAULT)` (`:140-147`) | `bytes(range(168))`; now `Settings::DEFAULT.to_bytes()` | diff, fixed |
| Wear-levelled VFO, settings, FM list | bitmap header + CRC slots, erase when full (`hal/wear_leveled.rs`, `storage.rs:9-25`); a raw dump of `0x8000-0x8FFF`, `0x9000-0x9FFF`, `0xC000-0xCFFF` shows the framing | plain 64-byte / 168-byte / 60-byte records at those addresses; the settings file is overlaid on raw reads | **diff, not fixed**: tests that compare raw dumps of those pages with records would pass here and fail on a radio. The record-level commands behave the same |
| Contact write | read-modify-write of the whole `0xA000` sector through the `0xD000` scratch sector (`storage.rs:209-264`) | single record; scratch untouched; now both sectors rewritten | diff, fixed; `restore_backup` tests now expect the scratch to change |
| Logo chunks | exactly 512 bytes at multiples of 512 (`:264-299`); the chunk at 0 erases all ten logo sectors, the others are programs | accepted any length and offset, overwrote; now refused, ANDed | **forgiving, fixed** |
| App slots `A`/`a`, satellite table `S` | slot < 9 else `0x02`; `a` needs the slot from a prior `A` in this session (`:301-340`); `S`: 5120-byte table, erase at offset 0 (`:342-387`) | not implemented (`ERR 0x05`); no tool that installs apps (`cps_tool.py app`, `install_apps.py`, `install_new_apps.py`, `replace_app.py`, `arrange_apps.py`) was ever tested against it | **forgiving, fixed** (implemented) |
| Table writes `X`, `F` | 512-byte chunks inside the table, erase of every sector at offset 0, later chunks are programs (`:358-387`) | overwrote; now programs | **forgiving, fixed** |
| `R 0xC000` (FM list) | 60 bytes through the wear-levelled region (`:155-165`) | `ERR 0x05`; now plain bytes | diff, fixed |
| Raw read `D` | 64-byte chunks, `addr * 64 + 64 <= 2 MiB` (`:389-398`) | same | same |
| Lost frame inside a session | no resend by the radio | not modelled beyond `die_after_writes` | same |
| Radio busy while a frame arrives | bytes queue in the 256-byte ring; a 520-byte frame sent while the radio is erasing would overflow it | none | unknown; hosts wait for each reply |
| Stale reply after a host timeout | possible if an erase outlasts the host timeout: the repeated frame is answered twice, and `Cps.send` accepts any reply frame (it does not compare command or address) | not modelled | unknown; tool weakness recorded, harmless while commands are idempotent and the final read-back compares data |

Not changed: the fake still answers `READ` of `0xA000-0xA01F`/`0xA160+` with `ERR 0x02` (the real code does the same, `:170`).

## 2. The fake bootloader

`firmware/tests/fakebootloader/serial.py` stands in for the vendor KDH bootloader, which is closed. Known: six real
flashes succeeded with `flash_bf.py`, so the happy path (handshake `BOOTLOADER`, package count, 1 KB packages with
sequence numbers, END) is right. Everything else is the author's model, now said at the top of the file.

| Behaviour | Real | Fake | Verdict |
|---|---|---|---|
| Happy path frames, CRC, sequence `& 0xFF` | proven by six flashes | same | same |
| Error codes `E1`/`E2`/`E3`/`E5` and their meaning | names written in `flash_bf.py:41` from the earlier project; never observed | same names | unknown |
| A repeated package (host retried after a lost ACK) | unknown: ACK or `E2` | `E2`; configurable `duplicate_package: "ack"` | **unknown, now both covered**: with `E2` one lost ACK stops the flash (the radio already holds the package; a rerun restarts at the handshake and completes); with ACK it is survived |
| END reply | the radio may reboot before replying (`flash_bf.py:130` tolerates it) | always replied; `no_end_reply` for the other case | unknown, both covered |
| Time per package (erase + write) | unknown; the host allows 2 s per attempt, five attempts | zero | unknown |
| What the image must look like | unknown beyond what `flash_bf.py` checks itself (size, vector table, board gate) | accepts any 1 KB packages | unknown; the host-side gate is the only one tested |
| Handshake in the middle of a transfer | unknown | restarts the transfer | unknown |
| What is written | to `0x08001000..` (HARDWARE.md:62), presumably erased first | the bytes received, nothing else | unknown |

## 3. The simulator's mock `Api`

`firmware/source-changes/simulator/main.rs` implements `bf5rh_abi::Api` (the `API` table, `main.rs:~905`) for apps that
run as host code. Real code: `src/app/overlay.rs`. `test_mock_fidelity` (sim suite) and `fidelity_tests` (`cargo test`) check the
behaviours below; the tables copied from the core are checked against the core source.

| Behaviour | Real (`overlay.rs`) | Mock | Verdict |
|---|---|---|---|
| `nor_read` | no range check, null buffer false (`:787-795`) | slices the 2 MB array: out of range panics (`main.rs:366`) | stricter |
| `nor_write` limits | refuses below `0x14C000`; no upper bound, no alignment or length limit; the driver splits at pages (`:797-805`, `norflash.rs:82-95`) | asserts the address is in one of three app ranges and the write inside one page (`main.rs:371-386`) | stricter; the core's missing upper bound is not seen (core defect above) |
| Program semantics | AND | AND (`main.rs:381`) | same |
| `nor_erase_sector` | refuses below `0x14C000`; any address erases its sector (`:807-814`) | asserts alignment and an app range | stricter |
| Flash time | an erase or a program blocks the whole radio (keys, ticks, draws) for ms to hundreds of ms | zero (the clock only moves on calls that advance it) | **unknown**: apps that write from a key handler show no pause in a frame; `chan_write` is two sector erases and thirty-two page programs |
| `chan_read` | false for a buffer under 32 bytes or an all-`0xFF` slot (`:844-856`); **no check of `num`** | same two rules (it used to ignore both); false for `num > 999` where the real one reads other flash | diff, fixed; core defect noted |
| `chan_write` | false for null, `num > 999`, length under 32 (`:960-977`); then sector read-modify-write and a display refresh | always true, asserted length 32; now refuses the same three | **forgiving, fixed**; CHAN MGR ignores the result (`chanmgr/src/main.rs:929`) |
| `settings_read` | at least 168 bytes (`:1388-1395`) | exactly 168 | stricter, fixed |
| `settings_preview` / `commit` | parse through `Settings::from_bytes` (clamps), apply to the hardware; commit also saves (`:1411-1433`) | stored raw, preview = commit; now clamped (table in `main.rs`, checked against `flash_map.rs`) | **forgiving, fixed**; preview without commit is not modelled separately |
| The record at start | `Settings::DEFAULT` (TX inhibit on, TOT 8, squelch 3 ...) | all zero | diff, fixed (settings goldens changed, below) |
| `settings_get` ids | 0-6 are fields of the live record, 7 the chip id (`:858-872`) | 0 returned 3, 5/6 from a scenario variable, the rest 0; now the record, 7 returns 0 | diff, fixed; chip id **unknown** |
| `side_cfg_read` / `write` | derived from the master side and applied (`:1435-1499`) | a plain 14-byte buffer | diff, not fixed (the settings segments run on it) |
| List widget, toast | the same source files, included by path (`ui/list.rs`, `ui/toast.rs`) | same code | same |
| `list_hint` | truncates to 26 bytes and stops at a non-ASCII byte (`:1013-1028`) | asserts | stricter |
| `handoff_write` / `read` | 64 bytes, false above (`:1150-1180`) | same | same |
| Group functions | the same `group_table.rs` | same | same |
| `utc_get`/`set`, `battery_raw12_avg`, `read_rf_gains`, `fm_power_off`, `tx_state` bit 1 | real clock, ADC, gains, FM park/unpark, TX-prohibited bit | 0 / 0 / false / nop / bit 0 only | diff, not fixed (apps check `read_rf_gains` for false, so the true path is untested) |
| `play_cue` | subject to the user's beep setting (`:1553`) | always recorded and played | diff |
| FM tuner | 100 kHz steps from 76 MHz (65 MHz below), channel field of 10 bits (`drivers/rda5807.rs:151-165`); readback is the quantised value | stored the frequency as given; now quantised | **forgiving, fixed** (an app showing 94.55 MHz reads 94.5) |
| FM seek | asynchronous, flags in a status word; duration unknown | 200 ms, five stations on the air | unknown |
| `fm_channels_save` / `load` | wear-levelled region | panics on save (the FM app keeps its own table) | stricter |
| RF register access | allow-list from the ABI crate on writes, unrestricted reads (`:1530-1545`) | the same allow-list, same crate | same |
| The modem | the chip | a model written from the register document (AUDIT 44); every unverified item in HARDWARE.md section 6 is still unverified | unknown |
| Key events | `Press`, `Single` (on release, if no Long fired), `Release`; `Long` only for Up, Down, VM, Band, `*`, `#`, `0`, Exit, Side1, Side2 after 1000 ms; `Repeat` for Up/Down from 1500 ms, every 100 ms (`device/keypad.rs:9-12`, `:40-58`, `:169-215`); a key must read the same on two polls | only `Single` and `Long`, `Long` for any key | **forgiving, fixed** (exposed defects 1, 2, 6). `hold` on a key without a long press is now a plain press, `repeat K N` added |
| Key timing | events are queued during a poll and dispatched together at the top of the loop (`main.rs:396`, `app/mod.rs:866`); at least two polls (about 4 ms) to register | delivered at once | diff |
| Tick and draw | the overlay tick runs every fifth loop iteration (at least 10 ms) with `dt` in 100 us units; the draw every fifteenth (at least 30 ms, display time on top): about three ticks per draw (`main.rs:388-468`) | a tick every 5 ms and a draw after each tick (`run`, `main.rs:~985`) | **unknown**: the real loop period depends on how long a frame takes to draw; apps that count ticks instead of using `dt` would differ |
| `uptime_100us` / `delay_ms` | free-running 100 us counter / busy wait | each call advances one tick; `delay_ms` advances the clock | diff (a wait loop terminates in both) |
| PTT, TX permission | `set_ptt` goes through `App::set_ptt` and its checks; `tone_on` keys the radio | counters; the scenario runner fails a receive-only app that calls either | same in what is enforced |
| `app_fault` | transmitter off, message with the slot and line, reset (`:1272`) | panics | stricter |
| The core itself | runs: main screen, launcher, group menu, notices, the port, the loader | **not run at all** (apps only) | see section 5 |

Settings goldens changed (`settings_menu_groups`, `settings_menu_keys`, `settings_radio_items`, `settings_system_guards`,
`settings_system_items`; 19 frames): the record now starts as the default, so BACKLIGHT shows `10 SEC`, SQUELCH 3, TX TIMEOUT
`120 SEC`, TX INHIBIT ON, SIDE2 SHORT `LIGHT`, TX BANDS `FCC HAM`, BOOT SCREEN `LOGO`, BOOT SOUND ON and the position `----`
instead of zeros, `0.0000N` and `OFF`. Each changed frame was read next to its golden; they differ only in those values. No
other golden changed: the notice frames that would have changed (item 1) changed back once the three apps were fixed.

## 4. The HIL fakes

`hil/injector.py` `FakeInjector` and `hil/vision.py` `synthetic_camera` only claim to model the rig, not the radio.

| Behaviour | Real | Fake | Verdict |
|---|---|---|---|
| Key press length for `hold` | at least 1000 ms for a Long (`keypad.rs:9`), under 1500 ms to avoid the Up/Down repeat | recorded 900 ms | **forgiving, fixed** (1300 ms; test reads the thresholds from the source) |
| Which keys can be held | `supports_long` | any | fixed (parser rejects the rest; `repeat` added) |
| PTT | refused by the injector and, in firmware, by the host tool | refused | same |
| Debounce, key bounce | a key must read the same on two scans | none | unknown (the press is 120 ms) |
| The webcam | a real camera, glare, LCD refresh banding, backlight timeout | a rectified, blurred, noised copy of the golden with a fixed perspective | diff by design; two "known limit" tests record what it cannot tell apart |
| Injector firmware | MicroPython on a board, untested on hardware (README) | not exercised | unknown |

## 5. Proposal: run the core on the host

Nothing runs the core itself on the host: the main screen, the launcher, the group menu, notices, the programming-port
session and the loader were found by hand on the radio.

What already compiles for the host through `#[path]` includes in `simulator/main.rs`: `fmtlite.rs`, `fault_text.rs`,
`ui/list.rs`, `ui/toast.rs`, `app/group_table.rs` (and the ABI crate). They are the files with no hardware types.

What blocks the rest, by module (`firmware-source/src`, sizes in lines):

* `device/keypad.rs` (235), `device/storage.rs` (307), `hal/wear_leveled.rs`, `cps/{mod,frame}.rs` (590), `flash_map.rs`:
  logic is pure except for the board and flash calls; blockers are `board::*` (GPIO), `hal::{uptime,delay,uart}`,
  `drivers::norflash` (PAC SPI registers), `cortex_m::peripheral::{SYST,SCB}` and the `App` type (`run_session` takes
  `&mut App` but uses only `storage_mut()` and `radio_mut().rf_off`).
* `app/{mod,keys,modes,scan,side,side_ops,tx,keyfn,convert,settings,launcher,input}.rs` (about 2,900 lines): `App<'a>`
  owns `Radio<'a>`, `Keypad<'a>`, `Storage<'a>`, `Flashlight<'a>`, each borrowing PAC peripherals; 65 distinct `Radio`
  methods are called; `&mut SYST` threads through about 160 call sites; `overlay.rs` holds `static mut` arena state and
  jumps into machine code at a fixed address.
* `ui/{mod,standby,icons,launcher,modes}.rs` (about 1,500 lines): draw to an `embedded-graphics` `DrawTarget` through
  `device::display::Display`; `standby.rs` uses no `SYST` and only `device::radio::{Modulation, Power, SubAudio}` plain
  types, so it is the cheapest to bring over.

Staged plan; no stage needs a change to the core's behaviour:

1. **The programming port on the host (first slice, 3-4 days, smallest and highest value).** A new crate beside the
   simulator that `#[path]`-includes `cps/{mod,frame}.rs`, `device/storage.rs`, `hal/wear_leveled.rs`, `flash_map.rs` and
   supplies shims at the same paths: an in-memory `NorFlash` (AND on program, sector erase, a page-split check) in place of
   `drivers/norflash.rs`; `hal::uart` as two queues; `cortex_m::peripheral::{SYST,SCB}` where `SCB::sys_reset()` unwinds
   to the harness (the session is `-> !`); a small `App` that exposes `storage_mut()` and `radio_mut().rf_off()`; no-op
   `ui::cps::draw_programming`. Expose it on a loopback socket and give the host tools a `serial` shim that connects to it.
   Then every tool test runs against the real `run_session`, and `fakeserial/serial.py` becomes a differential partner
   (the same script against both, results compared) instead of the only copy of the protocol. This would have caught
   AUDIT 37 without anyone remembering the sector rule. `flash_map.rs` needs `crate::app::KEYFN_MAX`: provide it in the shim.
2. **The keypad (1-2 days).** Include `device/keypad.rs` with a `board` shim that exposes a key matrix the test drives and
   a clock the test advances. Real Press/Single/Long/Repeat/Release sequences and timing replace the hand-copied
   `keypad_long` table and its drift test.
3. **Key handling and the main screen (about 2 weeks).** Include `app/{keys,modes,scan,side,side_ops,tx,keyfn,convert,
   settings,launcher,input}.rs`, `ui/{mod,standby,icons,launcher,modes}.rs` over a host `App` that replaces `Radio`
   with a recording double of the 65 methods (settings, channel and VFO state, call log), `device::display::Display` as a
   frame buffer, and `overlay` as in-process function calls (no arena). `SYST` becomes a unit type with the methods the
   core calls. Coverage: the main screen, the BAND menu, scanning, the launcher list, notices, the TX-block message, key
   lock.
4. **What stays on hardware and in the link checks:** the loader's address handling and the arena (AUDIT 31 is a link and a
   load-address fault; `memory.x`, `build_core.sh`, `mkapp.py` and the loader's own check cover it), RF behaviour and
   the modem.

First slice to build if only one is built: stage 1. It turns the programming port from a copy into the real code, and it
is the part where a stand-in has already cost a data-loss bug.

## 6. What stays unknown without hardware

* The vendor bootloader: error behaviour, repeated packages, END, timing, image checks (section 2).
* How long the radio takes to restart after END and after the idle reset; whether a second handshake can arrive during
  the boot logo.
* Flash erase and program times of the part on this board, and what the host sees when they exceed its 2 s timeout.
* The real tick and draw period of the main loop.
* Whether a 2 MB chip on this board decodes only 21 address bits (core defect above).
* The FM tuner's seek time and RSSI scale; the RF chip's behaviour behind the modem model (HARDWARE.md section 6).
* Whether the handshake is accepted while the radio is transmitting, scanning, in power-save sleep or drawing.
* Debounce and key-bounce behaviour of the physical keypad, and the injector firmware.

## 7. The fourth instance: an ignored file

`firmware/tests/fixtures/fm-stations.bin` was written, included by the simulator's unit tests, and hidden by the
`*.bin` rule in `.gitignore`; the tests compiled only where the file happened to exist. Fixed on the base branch (the file
is tracked, with an exception rule for `firmware/tests/fixtures/*.bin`).

`scripts/dev.sh tree` (first step of `check`) now fails when a source, test or tool directory holds an ignored or
untracked file other than caches and build output; `TREE_ALLOW=1` prints them and goes on (work in progress). It would have
named this file. Not built, proposed: a stricter step that exports the tree with `git archive HEAD` into a scratch
directory and runs `unit` and `tools-test` there, which also catches a file that is tracked but stale; it needs a second
container volume for the cargo cache and doubles the time, so it is better placed in the CI replay
(`scripts/ci-replay.sh`, a clean machine already) than in `check`.

## 8. How the old fakes were shown to fail

* Fake radio: `git show 3a31439:firmware/tests/fakeserial/serial.py > /tmp/old_serial.py`, then
  `FAKE_SERIAL_PATH=/tmp/old_serial.py python3 firmware/tests/test_fake_fidelity.py` (copy the file under the repo, the
  container sees only `/work`). 20 of the 23 tests at the time failed against the old fake; all pass against the new one.
  The tool-level tests (`test_recovery_tools.py` channel rollback and garbled frame, `test_boot_logo_tune_tools.py`
  fixtures) fail against the old tools: `TOOLS_DIR` points them at a copy of `source-changes/` with the previous
  `cps_tool.py` (garbled frame: `RADIO REFUSED cmd 0x44 addr 0x3c0: error 01`).
* Simulator mock: run with the new mock and the old apps, `scripts/dev.sh scenarios` failed six frames
  (`chanmgr_save_notice`, `fm_delete`, `fm_no_table`, `fm_save`, `fm_type`, `hamtools_idtimer`: the notice is missing) and the
  suite failed two checks (`a long press doesn't switch`, `42 switches land where expected`); with the apps fixed the six
  frames match their old goldens.
