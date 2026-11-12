# Baofeng UV-5RM custom firmware: research, tools and prototype

Custom firmware, overlay apps and host tools for the Baofeng UV-5RM / UV-5RH handheld (board `5RHNRFB-YP07`, 1.77" colour
screen, tri-band, 10 W). The firmware is built on the community's open [UV-5RM-Radio-Mod](https://github.com/jakegarrett13-afk/UV-5RM-Radio-Mod),
which is built on [bf-5rh-firmware-open](https://github.com/sophiel-meow/bf-5rh-firmware-open). This repository audits that
code, rebuilds it reproducibly, adds tests, a simulator and tools, and develops changes on top.

> [!WARNING]
> **HAB apps (`aprs`, `horus`, `rs41`) do not build and are not installable.** The [audit](AUDIT.md) found that they fail to
> compile, have no `.app` binaries and contain protocol errors (wrong AX.25 CRC, no HDLC framing, wrong Horus/RS41 sync words and
> modem settings, someone else's hardcoded callsign). They are design sketches only. Nothing in `firmware/apps/` includes them.

> [!CAUTION]
> **Treat flashing as permanent.** No stock image exists for the YP07 board: the images in
> [amoxu's repo](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE/tree/main/factory_firmware) are for other RF chips (upstream issue
> [#18](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE/issues/18): the YP07 uses the FD6818B) and will not restore a working YP07.
> The bootloader is not overwritten, so you can always flash another open build, but a factory restore is not available.
> The internal flash cannot be read. First boot also erases stale factory records in SPI flash, so **take a backup first**.
> Flash only an image built by CI, never the `.BF` committed here ([AUDIT.md](AUDIT.md), row 6). Confirm the board by holding
> `8` while powering on: the screen must say `5RHNRFB-YP07`.

> [!IMPORTANT]
> **Transmit safety.** The firmware refuses to transmit by default (TX INHIBIT is on, and PTT shows a notice instead of transmitting). The
> **TEXT** app, when open, **automatically transmits an acknowledgement** to any incoming message on its frequency. Settings can
> limit the transmitter but do not make a transmission legal: you need the licence and the right to use the frequency. This radio
> is not certified for FRS, GMRS or MURS. See the [user guide](docs/USER-GUIDE.md#transmit-safety).

> [!WARNING]
> **CHIRP is not expected to work with the custom firmware** (the upstream project's report; not tested here). CHIRP does read the
> *stock* firmware, which is how the first backup of the test radio was made. The custom firmware replaces the factory memory
> layout with an overlay-app design, so channels are programmed with the tools in this repository (`channels/` and
> `channel_plan.py`) or the WebSerial HTML tools.

## Status (2026-11-11)

One radio, a `5RHNRFB-YP07` on stock V0.13, has been flashed once, with an early build (commit `aaa3e04`, essentially the upstream
mod's source built by this project's CI). It boots, receives, and refuses to transmit by default. The calibration area was
checked before and after and is unchanged. Everything built since (readable setting names, the colour list style, the key-hint
strip, channel groups in CHAN MGR, assignable long-presses, the `SAVED` notice, the transmit-range choice and the channel
lists) passes the simulator and the tool tests but **has not run on the radio**. Nothing has been transmitted on the air with
this firmware, and the host tools that write to a radio (`restore_backup.py`, the logo and tune tools) are tested only against
fakes. See [AUDIT.md](AUDIT.md), rows 25 to 28, for the record.

| | |
|---|---|
| ![The SETTINGS list](docs/img/settings-groups.png) | ![CHAN MGR listing the REPEATERS channel group](docs/img/chanmgr-group-list.png) |
| SETTINGS, the list the apps share | CHAN MGR, showing one channel group |
| ![SPECTRUM, a trace and waterfall with two signals](docs/img/spectrum.png) | ![FOX HUNT signal meter](docs/img/foxhunt.png) |
| SPECTRUM | FOX HUNT |

These pictures are rendered by the host simulator from the firmware's own drawing code, not photographed from the radio.

## Read next

* **[User guide](docs/USER-GUIDE.md):** the keys, screens, settings, channel groups, transmit safety and every app.
* **[First-flash runbook](docs/FIRST-FLASH-RUNBOOK.md):** read-only steps first, the cable, what can and cannot be restored.
* [HARDWARE.md](HARDWARE.md): the radio's hardware and the purchase check. [AUDIT.md](AUDIT.md): findings and what is verified.
* [channels/README.md](channels/README.md): loading channel lists (weather, FRS, GMRS, MURS, repeaters) from CSV files.
* More: [FEATURES.md](FEATURES.md), [FIRMWARE-INTERNALS.md](FIRMWARE-INTERNALS.md), [FLASHING-WORKFLOW.md](FLASHING-WORKFLOW.md),
  [REFERENCES.md](REFERENCES.md), [SECURITY-AUDIT.md](SECURITY-AUDIT.md), [HAB-TRACKING-APPS.md](HAB-TRACKING-APPS.md),
  [docs/CHANNEL-GROUPS.md](docs/CHANNEL-GROUPS.md), [docs/CORE-FIRMWARE-BUDGET.md](docs/CORE-FIRMWARE-BUDGET.md),
  [docs/WEB-BUILDER.md](docs/WEB-BUILDER.md).

## At a glance

| Item | Details |
|---|---|
| Target hardware | Baofeng UV-5RM / UV-5RH (FCC ID `2AJGM-5RM`, Amazon ASIN `B0CWV6GFZT`) |
| Required firmware ID | **`5RHNRFB-YP07`** (hold `8` while powering on) |
| Host tools | No network calls in the Python tools or HTML pages (audited Nov 2026) |
| Reproducibility | CI-built images are the reference. The core image is built under linux/amd64 (the BK4819 variant differs on arm64, AUDIT row 18). The committed `.BF` does not reproduce from source |
| Tests | Unit tests, a host simulator with scripted scenarios and golden frames, tool tests against a fake radio, and a release gate. See [AGENTS.md](AGENTS.md); run `scripts/dev.sh check` (needs Docker or OrbStack). CI runs only the jobs a pull request's files can affect |
| Releases | A `v*` tag builds the install bundle (core `.BF`, apps, host tools, channel lists, runbook, checksums) with `scripts/dev.sh bundle`, in CI and locally; the core must equal the pinned hash. Experimental: flash at your own risk, back up calibration first |
| Reversibility | Effectively one way for the YP07 (see the caution above) |

## Credits and licence

This project stands on a chain of open-source radio work:

* **[amoxu](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE)** and **OK2MOP**: found the Artery AT32F421 MCU, dumped the KDH bootloader and worked out the `.BF` firmware format.
* **[sophiel-meow](https://github.com/sophiel-meow/bf-5rh-firmware-open)**: the bare-metal Rust base, the peripheral drivers and the `OVL1` app-overlay engine.
* **[kamilsss655](https://github.com/kamilsss655/uv-k5-firmware-custom)** and the UV-K5 community (OneOfEleven, DualTachyon, egzumer): research on the BK4819 radio chip, fast RSSI sampling and FSK modes.
* **[jakegarrett13-afk](https://github.com/jakegarrett13-afk/UV-5RM-Radio-Mod)**: the UV-5RM mod, the `TEXT` FSK messaging app, the direction-finding and sweep apps and the WebSerial tools.
* **[Project Horus](https://github.com/projecthorus)** and **[Balloonatics](https://github.com/k5rwk/balloonatics)**: the open Horus Binary telemetry protocols and ground-station stack.
* **Bob Bruninga WB4APR (SK)**: creator of the Automatic Packet Reporting System (APRS).

The firmware and tools are licensed under the **GNU General Public License, version 3**: see [LICENSE](LICENSE) and
[firmware/source-changes/LICENSE-GPL3](firmware/source-changes/LICENSE-GPL3). The upstream projects named above carry their own
licences; [HARDWARE.md](HARDWARE.md) and [REFERENCES.md](REFERENCES.md) note them where they matter. Upstream repositories
are not pushed to from here; their pull requests are imported as source on a branch and tested.
