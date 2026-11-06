# Baofeng UV-5RM Custom Firmware Research & Validation

Comprehensive analysis, hardware specifications, host security audit, and flashing guide for the [UV-5RM-Radio-Mod](https://github.com/jakegarrett13-afk/UV-5RM-Radio-Mod) custom firmware.

---

## At a Glance

| Item | Details |
|---|---|
| **Target Hardware** | Baofeng UV-5RM / UV-5RH (Tri-Band 10W, 1.77" Color TFT, FCC ID `2AJGM-5RM`) |
| **Amazon Listing** | ASIN: `B0CWV6GFZT` (Confirmed physical model match) |
| **Required Firmware ID** | **`5RHNRFB-YP07`** (Must verify by holding **`8`** while powering on) |
| **Host Code Safety** | **SAFE** (No outbound network calls, zero malware, local serial communication only) |
| **Firmware Quality** | **SOUND** (Clean `no_std` Rust embedded codebase built on `bf-5rh-firmware-open`) |
| **Reversibility** | **ONE-WAY** (No stock firmware dump exists; flashing cannot be reversed) |
| **CHIRP Compatibility** | **BROKEN** (CHIRP cannot read/write; use included WebSerial HTML tools) |

---

## Quick Navigation

- [Balloonatics HAB Tracking Suite (APRS, Horus, RS41)](HAB-TRACKING-APPS.md)
- [Unique Firmware Features & Capabilities](FEATURES.md)
- [Hardware Specs & Amazon Purchase Validation](HARDWARE.md)
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
> **Flashing is permanent:** There is currently no publicly available stock factory firmware for the `YP07` board revision. Once flashed, you cannot revert to the factory Baofeng OS.

> [!WARNING]
> **CHIRP will stop working:** The custom firmware replaces factory memory structures with a modular app overlay (`OVL1`) architecture. Standard CHIRP is incompatible; channel programming must be done via the included WebSerial HTML tools.

> [!IMPORTANT]
> **Auto-transmit behavior:** When the `TEXT` app is open, the radio will **automatically transmit an acknowledgement burst** in response to any incoming message on that frequency. Always operate on simplex frequencies where you are licensed and permitted to transmit digital data.
