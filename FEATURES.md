# Unique Firmware Features & Capabilities

Comprehensive breakdown of the custom features, applications, and operating modes introduced by the [UV-5RM-Radio-Mod](https://github.com/jakegarrett13-afk/UV-5RM-Radio-Mod) firmware.

---

## Feature Matrix

| Feature | Sub-App / Mode | Description |
|---|---|---|
| **Off-Grid Texting** | `text.app` | Standalone two-way FSK messaging with delivery receipts; no phone or extra hardware. |
| **Spectrum Panadapter** | `spectrum.app` | Real-time RF signal strength graph & waterfall across a configurable frequency slice. |
| **Band Sweep** | `bandsweep.app` | High-speed sweeping of entire amateur and commercial bands. |
| **Repeater Finder** | `rptfinder.app` | Automatically detects repeater input frequencies and CTCSS/DCS access tones. |
| **Direction Finding** | `foxhunt.app` | Attenuated signal strength meter for radio direction finding (T-hunting). |
| **Close-Call Capture** | `searchqt.app` | Near-field frequency counter and sub-audible tone decoder with one-touch toggle. |
| **App Overlays** | `OVL1` Engine | Modular application architecture loading 32 KB apps from 2 MB SPI NOR flash. |
| **Categorized Scan** | Quick `BAND` Key | One-button cycling through Weather, Airband, Marine, Police/Fire, Ham, and FRS/GMRS. |
| **Audio/Visual Themes** | Custom Assets | High-contrast black/red tactical UI, 16-bit RGB565 boot logo, and custom startup tunes (Doom / Mario). |

---

## 1. Standalone Off-Grid Text Messaging (`text.app`)

The headline capability of this firmware. It transforms the UV-5RM into a two-way text pager operating entirely off-grid without smartphones, Bluetooth, audio cables, or APRS/TNC hardware.

### Keypad Text Entry
* Multi-tap flip-phone text input using the numeric keypad (`2` for A-B-C, `3` for D-E-F, etc.).
* Dynamic character map displayed along the top header of the 1.77" color TFT screen.
* Maximum message length: **50 characters** (fragmented across two 25-byte packets).

### Modem Mechanics
* Operates directly over the BK4819/FD6818 transceiver’s built-in **1200 baud FSK** modem.
* Messages transmit on whichever simplex frequency the radio was tuned to before opening the app.
* Packet format: Magic byte (`0x4D`), Sender ID, Message ID, Packet Sequence/Part count, ASCII payload, XOR checksum, and tail markers with data scrambling for DC balance.

### Delivery Confirmation & Protocol (ARQ)
* Implements automatic stop-and-wait repeat request (ARQ) with random backoff jitter.
* Up to 8 automated transmission attempts until an acknowledgement is heard.
* **Auto-ACK:** Any listening radio with `TEXT` open automatically and silently keys up to send an acknowledgement burst upon receiving a valid packet.
* **Status Indicators:**
  - `>` : Transmitted, waiting for acknowledgment.
  - `+` : **Delivered & confirmed** by the remote radio.
  - `!` : Failed / unacknowledged after 8 retries.
  - `<` : Incoming message received.

### Storage & Alerts
* Persistent message inbox stored in external SPI NOR flash; survives reboots and power cycles.
* Audible chime/alert plays immediately when an incoming message is decoded.
* Pressing `*` sends a quick numbered diagnostic test ping.
* Holding `#` twice wipes the message inbox.

---

## 2. Real-Time Spectrum Panadapter (`spectrum.app`)

Replaces conventional blind dual-watch with a real-time RF panadapter:
* Samples the transceiver's fast hardware RSSI (Received Signal Strength Indicator) registers across a user-defined frequency slice.
* Draws a dynamic signal graph and waterfall display across the color TFT screen.
* Enables rapid identification of nearby transmissions, repeater activity, or transient interference.

---

## 3. High-Speed Band Sweep (`bandsweep.app`)

* Sweeps entire pre-set frequency bands (2m, 1.25m, 70cm, Marine VHF, Airband) in fractions of a second.
* Visually flags active frequency peaks and lets the operator jump directly to them using the arrow keys.

---

## 4. Repeater Auto-Finder (`rptfinder.app`)

Assists with identifying repeater configurations when travel details or directories are unavailable:
* Monitors the active repeater downlink/output frequency.
* Scans common repeater splits/offsets to locate the uplink/input frequency.
* Decodes the required CTCSS (PL) or DCS sub-audible access tone in real time.

---

## 5. Transmitter Hunting / Fox Hunt (`foxhunt.app`)

Designed for amateur radio hidden-transmitter hunts and interference locating:
* Provides a wide-dynamic-range relative signal strength meter.
* Includes progressive attenuation controls to prevent receiver front-end desensitization and overload when nearing the transmitter.
* Features coarse distance estimation indicators based on calibrated RSSI curves.

---

## 6. Near-Field Frequency & Tone Counter (`searchqt.app`)

* Rapidly locks onto strong local transmissions in close proximity (analogous to Uniden Close Call or frequency counters).
* Simultaneously measures carrier frequency and decodes any active CTCSS sub-tone or DCS digital code.
* Single-key toggle (**`1`**) instantly switches between carrier frequency search and sub-audible tone analysis.

---

## 7. Modular "App Overlay" System (`OVL1`)

Solves the embedded memory bottleneck of the Artery AT32F421 MCU:
* **The Problem:** The MCU contains only 64 KB internal flash (4 KB bootloader + 60 KB application area), making it impossible to compile all features into a single monolithic firmware binary.
* **The Solution:** The core OS runs out of internal flash, while secondary apps are compiled as modular 32 KB overlay files (`OVL1` header format) and flashed to the 2 MB external SPI NOR flash (`0x14C000`).
* When an app is launched from the radio menu, it is dynamically loaded into RAM.

---

## 8. Categorized Scan Banks

Pressing the **Band** key cycles the receiver through dedicated, pre-filtered monitoring banks:
* **NOAA Weather Radio** (7 national channels)
* **Aviation / Airband** (AM demodulation)
* **Marine VHF** (International & US channels)
* **Public Safety & Mutual Aid** (Police, Fire, EMS interop frequencies)
* **Amateur Radio Repeaters** (2m, 1.25m, 70cm)
* **Personal Radio Services** (FRS, GMRS, MURS)

---

## 9. Visual & Acoustic Customization

* **Tactical Red/Black Theme:** Replaces standard blue/white factory menus with a high-contrast dark palette optimized for low-light use.
* **16-bit Color Boot Logo:** Supports 160×128 RGB565 bitmaps (40,960 bytes). The default image in this build is a custom stylized skull.
* **Melodic Startup Riffs:** OpenGD77-style programmable note sequences. The repo includes tools to set:
  - **Doom E1M1** opening guitar riff (default boot tune).
  - **Super Mario Bros** theme song.
