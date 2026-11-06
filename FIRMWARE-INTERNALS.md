# Firmware Architecture & Modem Internals

## 1. Upstream Foundation
The mod is built on top of [`bf-5rh-firmware-open`](https://github.com/sophiel-meow/bf-5rh-firmware-open) created by `sophiel-meow`. 

* **Language:** 100% Rust (`no_std`, bare-metal embedded).
* **Target Architecture:** `thumbv7em-none-eabi` (ARM Cortex-M4 without hardware FPU enabled).
* **Firmware Vector Inspection:**
  - Initial Stack Pointer: `0x20004000` (matches 16 KB SRAM limit of AT32F421).
  - Reset Handler: `0x080010d5` (starts at 4 KB offset above the factory bootloader).
  - Binary Size: ~61.3 KB (fits comfortably inside the 60 KB application area).

---

## 2. Bootloader & Flashing Encryption

Baofeng's built-in KDH bootloader expects incoming firmware files (`.BF`) to follow a specific framing and light obfuscation scheme:

* **File Header (16 bytes):**
  - Offset `0x00`: Region count (`0x01`)
  - Offset `0x01-0x04`: Length of firmware region (Big Endian)
  - Offset `0x05-0x08`: Length of config data (0 for standard flashes)
  - Offset `0x09-0x0F`: Reserved padding
* **Payload Encryption:**
  - Payload is split into 1024-byte packages (`PKG = 1024`).
  - Packages 0, 1, and the last two packages remain **unencrypted**.
  - Intermediate packages are XORed against 4-byte keys depending on package index:
    - Index % 3 == 1: XORed with ASCII key `KDHT`
    - Index % 3 == 2: XORed with ASCII key `RBGI`
* **Transport Framing:**
  - Header byte: `0xAA`
  - Tail byte: `0xEF`
  - CRC: Standard CRC16-CCITT / XMODEM (`polynomial 0x1021`)
  - Speed: 115,200 baud, 8N1.

---

## 3. Modular App Overlay Architecture (`OVL1`)

Unlike stock firmware where the entire UI is hardcoded into one monolithic image, this firmware implements an overlay application system:

* **App Slot Base:** Located in external SPI flash starting at address `0x14C000`.
* **Slots:** 9 app slots, each allocated 32 KB (`0x8000`).
* **Header:** Each app binary starts with the magic 4-byte ASCII string `OVL1`.
* **Standard App Layout:**
  1. `settings.app` (Radio settings, backlight, timeouts, TX inhibit)
  2. `chanmgr.app` (On-device channel editor)
  3. `searchqt.app` (Sub-audio tone scanner / frequency counter)
  4. `spectrum.app` (Fast RF spectrum waterfall / panadapter)
  5. `bandsweep.app` (Wideband activity sweep)
  6. `foxhunt.app` (Direction-finding signal strength meter)
  7. `rptfinder.app` (Repeater tone and offset identifier)
  8. `text.app` (FSK peer-to-peer messaging)
  9. `fm.app` (Commercial broadcast FM radio receiver)

---

## 4. The `TEXT` App: Modem & RF Mechanics

The headline feature of this mod is the standalone text messaging app.

### FSK Modem Register Control
The firmware gives app overlays no direct modem API. Therefore, `text.app` directly manipulates the transceiver registers via the 3-wire bus (`PC13`/`PA2`/`PA3`):
* **Baud Rate:** 1200 baud.
* **Sync Word:** Standard BK4819 sync sequence (`0x3072`, `0x576C`).
* **Packet Structure:**
  - Length: 25-byte packets (max message length 50 characters across 2 packets).
  - Packet Header: Magic byte, Sender ID, Message ID, Sequence & Part count.
  - Payload: ASCII text chunk.
  - Trailing: XOR checksum + 2-byte tail.
  - Data Scrambling: Simple XOR whitening to maintain DC balance.

### Transmit & Receive Flow
1. **Keying:** The app calls `api.set_tx_freq(hz)` and enables PTT via `api.set_ptt(true)`.
2. **TX Inhibit Safeguard:** Before sending data, it inspects `(api.tx_state)()`. If transmit is forbidden by settings (`TXINH`), PTT is immediately dropped and transmission is marked `Blocked`.
3. **Automatic Acknowledgement (ACK):**
   - When a packet is successfully decoded by a listening radio running the `TEXT` app, it **immediately and automatically keys up to send an ACK reply packet**.
   - The sending radio retries up to 8 times with jittered backoff until receiving this confirmation.

---

## 5. Regulatory & Operational Implications

> [!IMPORTANT]
> **Station Identification:**
> Amateur Radio regulations (FCC Part 97 in the US) require station identification (your callsign) at regular intervals during transmission. The `TEXT` app does **not** inject an automated callsign into packets. Operators must periodically identify by voice over the channel.

> [!WARNING]
> **No Encryption:**
> Messages are stored in plain text and transmitted over unencrypted FSK (scrambling is for RF DC balance only, not cryptographic security). Any listening radio running this app or an SDR can intercept all text messages.

> [!NOTE]
> **Simplex Only:**
> The protocol is strictly point-to-point simplex. Do not attempt to use repeaters, as repeater hang-time, CTCSS decode delay, and squelch tails will cause packet corruptions and retry storms.
