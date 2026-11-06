# Project Audit & Roadmap (2026-11-06)

Static audit plus builds; **no radio was available**, so nothing here is hardware-verified.
Method: cloned all five upstream repos, decrypted factory/custom `.BF` files with an independent
decoder, built the core firmware, apps and host simulator (Rust 1.99.1), ran the simulator.

## Findings

| # | Finding | Status |
|---|---|---|
| 1 | amoxu repo ships factory images (BK, AT1846S, NRF_FD6818 V0.13/V0.14). "No stock firmware exists" was false. Stock images have a second region (~1.3 KB Thumb code, written to the system-bootloader area) that our `.BF` omits. | Docs fixed; revert path still to build/prove |
| 2 | `aprs`, `horus`, `rs41` fail to compile; no `.app`; `setup_balloon_radio.py` aborts before writing (safe). | Docs fixed; CI tracks as non-blocking |
| 3 | APRS: wrong CRC, no HDLC/NRZI, dropped last byte, modem unproven, hardcoded third-party callsign and coordinates. | Open |
| 4 | Horus/RS41: sync words, baud, scrambler and FEC do not match the real protocols (checked from memory; verify against `horusdemodlib`, `radiosonde_auto_rx`). | Open |
| 5 | Core image 61,352 of 61,440 bytes (88 free). New features must be overlay apps. TEXT arena use 8,119 of 8,192. | Open (now reported by `verify_release.py`) |
| 6 | Toolchain unpinned (`stable`); shipped `.BF` not reproducible; `Cargo.lock` was gitignored. | Fixed going forward (pin 1.99.1, lockfiles tracked) |
| 7 | No CI, no Rust unit tests. Simulator exists (324 checks pass). JS uploader tests need an uncommitted radio dump (`RADIO_DUMP_B64`). | CI added for gate+sim+build |
| 8 | Hardware ID `5RHNRFB-YP07` unverified; mod README says to refuse `…NRF`. | Open; verify on the radio |
| 9 | `patch_fw.py` makes first boot clean stale factory records in SPI flash; no restore tool exists for the backup. | Open |
| 10 | Host tools: no network calls (HTML and Python). `exec()` helper pattern is local-only but crude. | Confirmed |
| 11 | Shipped `.app` files: ABI hash and CRCs valid; firmware re-validates at load. TX is locked by default and gated by firmware (`tx_forbid`, band lock). | Confirmed |

## Roadmap (suggested order)

1. **Revert path:** script wrapping stock NRF V0.14 (both regions), a SPI-backup restore tool, and a round-trip test on a sacrificial radio.
2. **Reproducibility:** record the toolchain per release; rebuild `.BF` and apps in CI and compare hashes.
3. **Existing features:** extend simulator coverage, add size-budget enforcement, resync with upstream `bf-5rh-firmware-open`.
4. **HAB apps:** move protocol code to host-testable crates with golden vectors; prove the BK4819 modem can do AFSK/4FSK before any RF code; user-configurable callsign.
5. **HIL:** sacrificial radio on a UART/CPS cable for flash/readback loops, then a second radio or SDR for over-the-air checks (dummy load, low power).

Tools: `firmware/tools/verify_release.py` (offline release gate), `.github/workflows/ci.yml`.
