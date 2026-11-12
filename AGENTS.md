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
- Development is local-first. Flash only an image built by `scripts/dev.sh release` whose sha256 equals the pin in
  `firmware/expected-hashes.json`, recorded by a passing `scripts/dev.sh check` on the commit being flashed. Never flash a
  hand-built image or a committed `.BF` (AUDIT.md finding 6). GitHub CI repeats the check on a clean machine for pushes to
  `main` and for pull requests; it is not a gate before flashing the owner's radio, and it is the gate for other people's PRs.
- Push a branch with an explicit destination (`git push origin BRANCH:BRANCH`) and create branches with `--no-track`:
  a branch made from `origin/main` tracks it, and a bare push then goes to `main` (AUDIT 41).
- Do not merge or push to upstream (jakegarrett13-afk) repos; import their PRs as source on a branch, tested.

## Build environment
Build and test in the container, not on the host: `scripts/dev.sh check` (OrbStack/Docker; image in `dev/Dockerfile`, pinned Rust,
cache volumes per architecture). Core firmware is always built under linux/amd64 emulation (the CI environment): the BK4819 image built on native arm64 differs from x86 (AUDIT 18). `scripts/dev.sh repro` builds both and requires everything to match except that one known image.
Hardware-facing steps (flashing, serial, webcam, injector) cannot run in a container and stay on the host.
`scripts/dev.sh bf` builds the core variants and wraps them as flashable `.BF` files. `scripts/chirp-backup.sh PORT` is the one
exception to "serial stays on the host": CHIRP runs in a container and reaches the port through `scripts/serial_bridge.py` (loopback only).

CI (`.github/workflows/ci.yml`) is path-filtered: the `changes` job runs `scripts/ci_changed.py` (a table of which files each
job reads, found from the code: the simulator's `#[path]` includes, what `mkapp.py` and `build_core.sh` consume, what the tool
tests open) and a pull request runs only the jobs its files can affect (`gate`, `sim`, `tools`, `hil`, `images`, `web`). A push
to `main` runs all of them; so does a change to a workflow, `scripts/dev.sh`, the Dockerfile, the toolchain pin, the ABI file,
or a file the table does not list. `ci-ok` always runs and is the one check to require. When a step starts reading a new path,
add it to `RULES` in `scripts/ci_changed.py`; `firmware/tests/test_ci_filters.py` (in `tools-test`) fails for an unlisted
tracked file and checks example changes against the real workflow. The two-architecture rebuild is `repro.yml`.

CI can be rehearsed locally before pushing (and when pushing is blocked): `scripts/ci-replay.sh gate|sim|tools|hil|images|web|all`
(no argument prints the list) runs the workflow's own `run:` steps in a clean x86_64 Ubuntu container with a runner-like Rust
(stable, no ARM target); it does not evaluate the path filter. It found nothing wrong after the toolchain fix; the one real CI
failure so far (pinned toolchain not selected when a step runs outside `firmware-source/`) was found by the first GitHub run.

Releases: `scripts/dev.sh bundle` builds the install bundle (core `.BF`, the launcher apps and extras for the current ABI,
`layout.json`, `SHA256SUMS`, the host tools, `channels/`, the runbook, `INSTALL.md`; `firmware/tools/bundle_release.py`) into
`build-out/bundle/`, and refuses a core whose hash differs from `firmware/expected-hashes.json`. `.github/workflows/release.yml`
runs that same command in the dev container on a `v*` tag (or as a dry run by hand) and attaches the result to a GitHub release.
Nothing personal may be bundled (`.env`, `my-backup/`, `build-out/personal`): the script fails if it finds any.

## Before a PR
`scripts/dev.sh check` (release gate, unit tests, scenarios for every style, simulator suite, overlay builds, core firmware, HIL tests).
