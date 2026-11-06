# Baofeng UV-5RM Custom Firmware Research & Validation

Comprehensive analysis, hardware specifications, host security audit, and flashing guide for the [UV-5RM-Radio-Mod](https://github.com/jakegarrett13-afk/UV-5RM-Radio-Mod) custom firmware.

> [!WARNING]
> ### HAB APPS (`aprs`, `horus`, `rs41`) DO NOT BUILD AND ARE NOT INSTALLABLE
> A Nov-2026 audit ([AUDIT.md](AUDIT.md)) found these three apps fail to compile, have no `.app` binaries, and contain protocol errors (wrong AX.25 CRC, no HDLC framing, wrong Horus/RS41 sync and modem settings, someone else's hardcoded callsign). They are **design sketches only**. Nothing in `firmware/apps/` includes them.
>
> The remaining apps come from the upstream mod, which reports testing on two radios. This repo has not yet tested anything on hardware. **Flash and transmit at your own risk.**

---

## At a Glance

| Item | Details |
|---|---|
| **Target Hardware** | Baofeng UV-5RM / UV-5RH (Tri-Band 10W, 1.77" Color TFT, FCC ID `2AJGM-5RM`) |
| **Amazon Listing** | ASIN: `B0CWV6GFZT` (Confirmed physical model match) |
| **Required Firmware ID** | **`5RHNRFB-YP07`** (Must verify by holding **`8`** while powering on) |
| **Host Code Safety** | **Verified**: no network calls in the HTML or Python tools (audited Nov 2026) |
| **Firmware Quality** | Core firmware builds; shipped `.BF` is **not** byte-reproducible yet (toolchain was unpinned); core image has only ~88 bytes of flash headroom. Zero unit tests. See [AUDIT.md](AUDIT.md) |
| **Reversibility** | **Effectively one-way for YP07.** The only public stock images (BK / NRF / AT1846S in amoxu's repo) target other RF chips; upstream reports they boot on a YP07 (FD6818B) but RX/TX fail. The official YP07 image has not been found. The bootloader is not overwritten, so you can always re-flash *something*, but a true factory restore is not available |
| **CHIRP Compatibility** | **BROKEN** (CHIRP cannot read/write; use included WebSerial HTML tools) |

---

## Quick Navigation

- [Balloonatics HAB Tracking Suite (APRS, Horus, RS41)](HAB-TRACKING-APPS.md)
- [Unique Firmware Features & Capabilities](FEATURES.md)
- [Upstream Lineage & Hardware References](REFERENCES.md)
- [Hardware Specs & Amazon Purchase Validation](HARDWARE.md)
- [Project Audit & Roadmap (Nov 2026)](AUDIT.md)
- [Host PC/Mac Security Audit](SECURITY-AUDIT.md)
- [Firmware Architecture & Modem Internals](FIRMWARE-INTERNALS.md)
- [macOS Flashing & Recovery Workflow](FLASHING-WORKFLOW.md)

---

## Executive Summary

### 1. Legitimacy
The upstream project [`UV-5RM-Radio-Mod`](https://github.com/jakegarrett13-afk/UV-5RM-Radio-Mod) is a genuine, community-developed fork of [`sophiel-meow/bf-5rh-firmware-open`](https://github.com/sophiel-meow/bf-5rh-firmware-open). It ports the UV-K5 community's FSK text messaging and spectrum scan capabilities to the newer ARM Cortex-M4 Baofeng hardware platform.

### 2. PC / Mac Host Safety
Both the Python flashing tools and browser-based HTML configuration tools have been inspected line-by-line:
- **Zero network calls:** No telemetry, sockets, or HTTP requests.
- **Self-contained HTML:** WebSerial channel uploader runs completely offline without CDNs or tracking.
- **Safe serial commands:** Tools only send formatted bootloader and CPS frames over USB-UART.

### 3. Hardware Suitability
The Amazon purchase (ASIN `B0CWV6GFZT`, Baofeng 5RM 10W Tri-Band) is the correct physical radio model. However, Baofeng manufactures several internal board revisions under the same outer shell. **You must verify the board identifier (`5RHNRFB-YP07`) before flashing.**

---

## Important Warnings

> [!CAUTION]
> **Treat flashing as permanent:** The stock images in [amoxu's repo](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE/tree/main/factory_firmware) are for the BK4819, AT1846S and FD6818 boards. Upstream issue [#18](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE/issues/18) says the YP07 uses the **FD6818B**, which is not register-compatible, so they will not restore a working YP07. The official YP07 firmware has not been found. Dump nothing you can't restore: you cannot read the internal flash (read-protected). First-boot also cleans stale factory records in SPI flash, so **take the backup first** and note that no restore script exists yet.

> [!WARNING]
> **CHIRP will stop working:** The custom firmware replaces factory memory structures with a modular app overlay (`OVL1`) architecture. Standard CHIRP is incompatible; channel programming must be done via the included WebSerial HTML tools.

> [!IMPORTANT]
> **Auto-transmit behavior:** When the `TEXT` app is open, the radio will **automatically transmit an acknowledgement burst** in response to any incoming message on that frequency. Always operate on simplex frequencies where you are licensed and permitted to transmit digital data.

---

## Credits & Lineage

This project stands on the shoulders of an incredible chain of open-source radio reverse-engineering:

- **[amoxu](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE)** & **OK2MOP**: Discovered the Artery AT32F421 MCU, dumped the KDH bootloader, and cracked the `.BF` firmware encryption format.
- **[sophiel-meow](https://github.com/sophiel-meow/bf-5rh-firmware-open)**: Created the bare-metal Rust OS base, peripheral drivers, and the `OVL1` dynamic 32 KB app overlay engine.
- **[kamilsss655](https://github.com/kamilsss655/uv-k5-firmware-custom)** & the **UV-K5 Community** (OneOfEleven, DualTachyon, egzumer): Researched the BK4819 transceiver registers, RSSI fast-sampling, and FSK modem modes.
- **[jakegarrett13-afk](https://github.com/jakegarrett13-afk/UV-5RM-Radio-Mod)**: Built the UV-5RM mod, the `TEXT` FSK messaging engine, RDF/sweep apps, and WebSerial tools.
- **[Project Horus](https://github.com/projecthorus)** & **[Balloonatics](https://github.com/k5rwk/balloonatics)**: The open Horus Binary telemetry protocols and ground station tracking stack.
- **Bob Bruninga WB4APR (SK)**: Creator of the Automatic Packet Reporting System (APRS).

