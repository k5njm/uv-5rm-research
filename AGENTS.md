# AGENTS.md

Conventions for this repo (borrowed from the sibling `beepy-ui` project's workflow).

## Layout
- `firmware/firmware-source/` firmware + overlay apps (`apps/<name>/src/logic.rs` is host-testable; `main.rs` is the no_std shell).
- `firmware/source-changes/simulator/` host simulator, scenarios (`tests/scenarios`), golden frames (`tests/golden`).
- `hil/` hardware-in-the-loop host tools and the injector firmware (untested on hardware).
- `firmware/tools/verify_release.py` offline release gate; `AUDIT.md` findings and roadmap.

## Rules
- App logic is pure and integer-only where possible; hardware access lives behind the `Api`. Put unit tests in `logic.rs`
  (`cargo test --release` in the simulator crate runs them).
- Every new app: `#[path]` module in the simulator, an entry in `scenario.rs::lookup`, scenarios, **look at the PNG frames**
  before `run_scenarios.sh --update`.
- Visual styles are cargo features (`style-d74|proxx|ft5d`, default classic) of an app; build with `MKAPP_FEATURES=style-d74 mkapp.py ...`; each style has its own golden dir. Classic must stay pixel-identical unless intentionally changed.
- Receive-only apps must never call `api.tone_on` (it keys the transmitter) or key PTT. Scenarios enforce this.
- App arena is 8,192 bytes; core firmware has ~88 bytes of flash free: new features are overlay apps.
- Build overlays with `firmware/firmware-source/tools/mkapp.py` (needs `rust-objcopy`, `rust-nm`, `rust-size` on PATH; llvm-* symlinks work).
- Never commit radio backups (`my-backup/`, `*.img`), secrets, or third-party logos/music.
- Ship only CI-built images; do not trust a committed `.BF` (see AUDIT.md finding 6).
- Do not merge or push to upstream (jakegarrett13-afk) repos; import their PRs as source on a branch, tested.

## Build environment
Build and test in the container, not on the host: `scripts/dev.sh check` (OrbStack/Docker; image in `dev/Dockerfile`, pinned Rust,
cache volumes per architecture). Core firmware is always built under linux/amd64 emulation (the CI environment): the BK4819 image built on native arm64 differs from x86 (AUDIT 18). `scripts/dev.sh repro` builds both and requires everything to match except that one known image.
Hardware-facing steps (flashing, serial, webcam, injector) cannot run in a container and stay on the host.
`scripts/dev.sh bf` builds the core variants and wraps them as flashable `.BF` files. `scripts/chirp-backup.sh PORT` is the one
exception to "serial stays on the host": CHIRP runs in a container and reaches the port through `scripts/serial_bridge.py` (loopback only).

CI can be rehearsed locally before pushing (and when pushing is blocked): `scripts/ci-replay.sh verify|web` runs the workflow's own
`run:` steps in a clean x86_64 Ubuntu container with a runner-like Rust (stable, no ARM target). It found nothing wrong after the
toolchain fix; the one real CI failure so far (pinned toolchain not selected when a step runs outside `firmware-source/`) was
found by the first GitHub run.

## Before a PR
`scripts/dev.sh check` (release gate, unit tests, scenarios for every style, simulator suite, overlay builds, core firmware, HIL tests).
