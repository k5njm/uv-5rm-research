# Design: pick-your-features web builder + WebSerial flasher

Status: **design, styles prototype, manifest generator, and a phase-1 page** (`web/`, planning only: validates a selection and
writes to a *software* radio; it has no serial access, pinned by `firmware/tests/test_web_static.py`). The page is
tested in headless Chromium (`scripts/dev.sh web-test`, 27 checks including mutation-checked board gate, budget rules, tamper refusal and read-back on the fake radio); it has still never met a real radio or a real serial port. Nothing here has run on a real radio.

## Key insight: almost nothing needs a per-user compile

The firmware already splits in two:

| Part | What it is | Size / limits | Risk to change |
|---|---|---|---|
| **Core** (`.BF`) | boot, radio driver, launcher, menus | 60 KB area; **88 bytes free** when this was written, 1,372 bytes on 2026-11-12 (AGENTS.md) | flashed through the bootloader; wrong image = no RX/TX |
| **Overlay apps** (`.app`) | independent programs in SPI-flash slots | 8 KB arena, 32 KB slot, 9 slots; hash-pinned to the ABI and CRC-checked at load | written over the programming cable; a bad one is rejected at load, never bricks |

So a "custom build" is **core variant + a chosen set of prebuilt apps (each in a chosen style/locale)**.
CI builds the whole matrix once; the web page only *selects, validates and writes* prebuilt artifacts.
No server-side compiler, no per-user build queue.

```
CI matrix (apps x styles x locales, core x board x palette)
        └─► artifacts + manifest.json (sha256, ABI hash, sizes, board gate, licences)
                └─► static page (GitHub Pages): choose options ─► budget check ─► WebSerial write ─► read back & verify
```

## What is varied how
* **Style**: compile-time cargo feature per app (`style-d74`, `style-proxx`, `style-ft5d`; default classic). Only the chosen
  style is in the binary, so size stays flat (4.7-4.9 KB for HAM TOOLS). *Prototyped here.* The core's own UI (launcher/menus)
  needs core variants; palette-only variants are size-neutral, layout changes are not (the core has only about 1.4 KB of flash free).
* **Locale**: per-locale string tables as cargo features, same mechanism. The simulator already panics when text runs off the
  screen (it caught a real bug in HAM TOOLS), so it doubles as a **translation overflow lint** across a golden frame per locale.
  Limits: the built-in fonts are ASCII; Latin-1 languages are plausible (extra glyphs cost flash), Cyrillic/CJK are not practical in 8 KB.
* **Minimal builds**: the user's app set decides which slots are written. Removing unused core features would free space for
  core changes, but that is future work and each removal needs hardware validation.
* **Board variant**: `bk4819` vs default (FD6818B/YP07) are separate core images (upstream PR #3, imported on its own branch).
  Gate by the 8-key screen string the user types, never by guesswork.

Matrix size is small: e.g. 12 apps x 4 styles x 3 locales = 144 builds of about 10 s each; core 2 boards x 4 palettes = 8 images.

## WebSerial flasher
Feasible: the repo already has a browser WebSerial CPS tool (`UV-5RM_Channel_Uploader.html`) and a JS fake-radio test harness
(`firmware/source-changes/uploader_tests.js`), and the Python flashers document the frames. Chrome/Edge only.

Two flows with very different risk:
1. **Apps-only (default)**: write chosen `.app` files to slots, read back, verify CRC. Reversible, low risk.
2. **Core flash (guarded)**: behind a hard gate: typed board string matches the variant, SPI-flash backup downloaded first,
   sha256 checked in-page against the manifest, vector table sanity, explicit "no stock firmware for YP07" warning.
   **Do not build this until a restore tool and a proven recovery path exist** (AUDIT.md findings 1, 9).

Integrity and provenance: only CI-built artifacts; pin hashes in the manifest; consider build attestations. A committed `.BF`
is not trusted (the shipped one does not reproduce from source, AUDIT.md finding 6). GPL-3.0: custom builds come from public source.

## Testing without a radio
Headless Chromium (Playwright is available in this environment) driving the page with the JS fake radio replacing
`navigator.serial`: select options, check the slot/NOR budget validator, write, read back, inject a corrupt write and watch the
verify/restore path. Same philosophy as `firmware/tests/test_boot_logo_tune_tools.py`.

## Phases
0. Style infrastructure in apps *(done for HAM TOOLS; goldens per style; CI)*
1. `manifest.json` generator + CI matrix building every app/style (+ board core variants) with provenance
2. Static page: option picker, dependency and budget validation, dry-run against the fake radio (apps-only flow) *(page written and browser-tested against the fake radio)*
3. Real-radio apps-only flow on the sacrificial radio; HIL rig replays scenarios per style
4. Core flash flow behind the gate (after restore tooling exists)
5. Locales

## Style notes (from the three reference radios)
Current prototypes are palette and chrome variants only; they are honest approximations, not copies:
* **TH-D74**: black, dense rows, colour-coded labels, thin green rules, S-meter bars. Next: multi-row dense layout and meter bars.
* **Pro-X**: light screen and a tab strip echoing its tile menu. Next: a real tile launcher with 1-bit icons (core UI, size-limited).
* **FT-5D**: black, huge frequency digits, red/blue tags, soft-key boxes. Next: very large digits (needs an app-side bitmap font via
  `blit`, roughly 500 bytes for ten 16x24 digits) and per-screen soft-key labels.
Reference photos are of commercial products; only their general layout ideas are used, no logos, artwork or fonts.

## Open questions
* Which apps matter most to ship first (beyond HAM TOOLS)? Slots are scarce.
* Is a hosted page (GitHub Pages) acceptable, or should it be a single local HTML file like the existing uploader?
* Should core palette variants exist before the recovery path does? (They multiply images users could flash.)
