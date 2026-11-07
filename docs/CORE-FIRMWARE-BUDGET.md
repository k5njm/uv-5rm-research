# Core firmware budget, and how to add modem features efficiently

Measured 2026-11-07 on the default core image (61,352 bytes) with `scripts/dev.sh size-report`. **Measured** = from the build;
**estimate** = my reading, not built. Nothing here has run on a radio. Changing the core is a stop-and-ask gate for this project.

## Where the 60 KB goes
`.text` 54,404, `.rodata` 6,656, vectors 212, `.data` 80. Free: 88 bytes.

| Area (measured symbol bytes) | Bytes | Note |
|---|---|---|
| Overlay API implementation (`app::overlay`) | ~7,400 code | 75 wrappers, avg 74 B; `draw_line` 648, `draw_circle` 324 |
| Main loop (`__cortex_m_rt_main`) | 6,748 | everything inlined |
| `ui::draw` + `ui::standby` + `ui::list` | 9,174 | the standby screen and lists |
| `embedded_graphics` | 3,370 (+ fonts in `.rodata`) | text drawing |
| `core::fmt` + `core::str` | ~4,200 | **suspicious for a firmware that formats little** |
| CPS (programming port) | 2,398 | needed: apps are loaded through it |
| `Settings` parse/serialise, storage, wear-levelling | ~3,600 | needed at boot |

The ABI surface is **not** where the fat is: 73 functions, 4,600 B, and every large one is used by two or more apps. Only
`side_cfg_*` (514 B, settings app), `fm_*` (~320 B + the driver, fm app), `blit` (spectrum) and two small ones serve a single app.

## Levers, best first

1. **Toolchain (measured, no source change): core 61,352 -> 57,820 bytes (3,620 free).** Nightly toolchain + `-Cpanic=immediate-abort` +
   `-Zbuild-std=core` (so `core` is compiled for size and loses its panic-message formatting). Apps shrink too: arena use TEXT
   8,119 -> 7,055 (73 B spare -> 1,137), hamtools 4,723 -> 3,855, chanmgr 7,834 -> 6,984, spectrum 7,438 -> 7,116.
   Repro: `scripts/dev.sh exec "bash firmware/tools/experiments/nightly_size.sh"`.
   Costs and risks: needs a nightly **pinned by date** (the flag was renamed this season); changes the core image, so it
   needs hardware verification and a re-check of cross-architecture reproducibility (the BK4819 image already differs between
   arm64 and x86, AUDIT 18); on a panic the core would trap instead of looping (both hang; unverified which the radio
   handles better). CI and the dev image would need the nightly + `rust-src`.
2. **Remove the rest of `core::fmt` (estimate 1.5-2 KB; still 2,482 B after lever 1).** The core formats list rows through
   `fmt::Write`; a tiny integer/decimal formatter would replace it. Behaviour-preserving source change in the core.
3. **Move the FM driver into `fm.app` (estimate ~1.5 KB).** Measured: 1,488 B in FM-related core symbols (RDA5807 driver, device,
   board, 8 `api_fm_*`). `fm.app` uses 3.7 KB of its 8 KB arena. Cost: the app must bit-bang the tuner's I2C itself, and it
   cannot be verified without a radio.
4. **Move the Band-key scan banks and scan UI to an app (estimate 1-1.5 KB).** Measured ~2.9 KB across `app::scan/modes/side*`,
   `ui::scan`, `keyfn`, `convert`; only part is movable and it changes how keys behave. Not recommended unless needed.
5. **`side_cfg_*` into the settings app (514 B).** Needs the record format in the app. Low value.

Not worth moving: CPS, settings parsing, storage, the drawing primitives (all used by 2+ apps), the standby UI.

## Adding modem features efficiently

What exists: apps have no modem interface. Four apps (`text`, and the unbuilt `aprs`, `horus`, `rs41`) each carry a private ~94-line
bit-banged copy of the RF chip's 3-wire bus, poking GPIO registers behind the core driver's back. The core driver
(`Fd6818::read_reg/write_reg`) already does the same job.

**Do not put a protocol modem in the core.** The chip's FSK engine is configured by plain register writes; AX.25, Horus and RS41
framing belong in host-testable crates linked into apps (the simulator and golden vectors then cover them). The core needs only
three tiny ABI additions (estimate ~100-150 B in total, which fits only after lever 1):

| New ABI function | Why | Already in core? |
|---|---|---|
| `rf_reg_read(addr)`, `rf_reg_write(addr, value)` | one shared, serialised path to the chip; each app drops its ~94-line bus copy; the simulator can mock the chip once | yes: private `Fd6818::read_reg/write_reg` |
| `delay_us(n)` | bit timing (1200 baud = 833 us); today only `delay_ms` | no |
| `play_cue(tones, len)` | a **speaker-only beep that never keys the transmitter**, so receive-only apps can alert (the ID timer is visual only today because `tone_on` transmits) | **yes: `Radio::play_cue`**, "never keys the transmitter", used only by the core's own mode menu; it also honours the user's beep setting |

Any `abi/src/lib.rs` edit changes the ABI hash, so every app must be rebuilt (all are in this repo; shipped binaries would stop
loading until rebuilt).

Feasibility, ranked (all UNVERIFIED hypotheses; flash space is not the real blocker for these):
1. **RS41 (4800 baud 2-level FSK): plausible first.** The chip's FSK receiver has programmable baud and sync words.
2. **APRS (1200/2200 Hz AFSK):** TX might step the tone generator with microsecond timing; RX needs audio samples and no route to them is known.
3. **Horus Binary (4FSK):** the chip's demodulator is 2-level, so probably not without raw samples.

The cheapest first experiment needs no new core code: a diagnostic **FSK probe app** that configures the chip, then dumps its FIFO
and registers for a known signal, on hardware day.

## Suggested order
1. Decide on lever 1 (largest, safest, no source change) and verify it on both architectures.
2. ABI additions (`rf_reg_*`, `delay_us`, `play_cue`) with a simulator mock; migrate TEXT onto them (removes its bus copy).
3. Host-testable protocol crates with reference vectors.
4. FSK probe app, then an RS41 decoder if the chip's behaviour allows it.
