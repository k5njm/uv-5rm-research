# Balloonatics HAB Suite: APRS, Horus & RS41

Custom High-Altitude Balloon (HAB) and Radiosonde tracking suite developed for the Baofeng UV-5RM custom firmware fork.

> [!WARNING]
> ### NOT IMPLEMENTED: DESIGN SKETCHES THAT DO NOT COMPILE
> As of the Nov-2026 audit ([AUDIT.md](AUDIT.md)) none of `aprs`, `horus`, `rs41` compile, no `.app` exists, and `setup_balloon_radio.py` will refuse to run (missing overlays). Feature descriptions below are **intent, not behaviour**. Known defects:
> * **APRS:** CRC is MSB-first `0x1021` (AX.25 needs reflected `0x8408`); no NRZI, bit-stuffing or flags; odd-length frame drops the last CRC byte; BK4819 FSK is not shown to be a 1200/2200 Hz Bell 202 modem; hardcoded callsign `K5RWK-7` and Richardson TX coordinates (must be user-configured).
> * **Horus:** uses a `$$` sync and 1200 baud; Horus Binary needs a 4FSK modem, Golay FEC and its own unique word.
> * **RS41:** sync words and "PRBS-15" scrambler claim are unverified against `radiosonde_auto_rx`; uses `f64::sqrt` (not available in `no_std`).

---

## Suite Overview

Designed to bridge your handheld radio directly with the [Richardson Area Balloonatics (`balloonatics`)](https://github.com/k5rwk/balloonatics) ground station ecosystem.

| App | Frequency | Protocol | Target & Purpose |
|---|---|---|---|
| **`aprs.app`** | 144.390 MHz | 1200 baud Bell 202 AFSK (AX.25) | Transceive: Decode balloon APRS beacons & transmit chase vehicle position telemetry. |
| **`horus.app`** | 434.xxx MHz | 4-FSK / 2-FSK Horus Binary v2 | Live stratospheric balloon telemetry: Altitude, climb/descent rate, Lat/Lon, battery & temps. |
| **`rs41.app`** | 400–406 MHz | 4800 baud GFSK (PRBS-15) | Decode NWS weather balloon radiosondes (PTU, altitude, serial number, GPS position). |

---

## 1. APRS Transceiver (`aprs.app`)

### Capabilities
* **1200 Baud Bell 202 Demodulation:** Operates over the BK4819 transceiver's FSK engine with hardware HDLC flag detection (`0x7E`).
* **AX.25 Packet Decoder:**
  - Extracts source callsign and SSID (e.g. `K5RWK-11`, `W5HAB-9`).
  - Unpacks destination, digipeater path (`WIDE1-1, WIDE2-1`).
  - Parses APRS position strings (`!DDMM.hhN/DDDMM.hhW$`).
  - Extracts altitude tags (`/A=084210`).
  - Stores the latest received packets in an on-screen scrollable buffer.
* **Position Beaconing (Transmitter):**
  - Sends uncompressed APRS position frames with callsign and customizable comment.
  - Automatically calculates CRC-16 CCITT (polynomial `0x8408` with `0xFFFF` preset) and bit-stuffing.
  - Toggle between **Manual Beacon** (`Menu` key) or **Auto-Beacon** (every 60s, toggled with `*`).
  - Toggle TX Power High (10W) / Low (1W) with `A/B`.

---

## 2. Project Horus Binary v2 Decoder (`horus.app`)

### Capabilities
* **Direct HAB Telemetry on Handheld:** Eliminates the need to carry a laptop or check a web map while hiking through fields or brush to recover a landed payload.
* **Telemetry Fields Decoded:**
  - **Payload ID & Sequence Count:** Tracks missing packets.
  - **Altitude:** Displayed in large high-contrast numerals (Feet & Meters).
  - **Vertical Climb Rate:** Real-time calculation showing ascending/descending trends (e.g. `▲ +5.4 m/s` under balloon, `▼ -14.2 m/s` under parachute).
  - **GPS Position:** Latitude & Longitude in decimal degrees.
  - **Payload Health:** Satellite lock count, battery millivolts, and internal/external temperatures.
  - **Peak Altitude Record:** Tracks balloon apogee (`MAX ALT`).
* **Thermal Drift Tuning:**Stratospheric crystal drift can shift transmissions by several kHz. Use `Up` / `Down` arrow keys to nudge the receiver in **5 kHz increments** to re-center the payload carrier.

---

## 3. Vaisala RS41 Radiosonde Decoder (`rs41.app`)

### Capabilities
* **4800 Baud GFSK Native Reception:** The BK4819 transceiver hardware FSK clock divider (`BAUD_4800 = 1161`) natively supports the exact baud rate used by National Weather Service (NWS) radiosondes (such as launches from Fort Worth / Dallas FWD).
* **PRBS-15 Descrambler:** Implements the LFSR whitening polynomial ($x^{15} + x^{14} + 1$) to reveal raw telemetry frames.
* **Sub-Block Parsing:**
  - `0x79` Status: Sonde Serial Number (e.g., `R3320145`), frame number, battery voltage.
  - `0x7B` GPS: ECEF XYZ coordinates converted to WGS-84 altitude, climb rate, and coordinates.
* **Meteorological Band Coverage:** Fast 25 kHz stepping across the 400.000 – 406.000 MHz radiosonde allocation.

---

## 4. App Slot Layout & Installation

The custom firmware allocates **9 app slots** (32 KB each) in external SPI NOR flash starting at address `0x14C000`.

### Balloonatics Habitat Slot Layout:
```text
Slot 0: SETTINGS      (settings.app)
Slot 1: APRS          (aprs.app)
Slot 2: HORUS         (horus.app)
Slot 3: RS41 SONDE    (rs41.app)
Slot 4: FOX HUNT      (foxhunt.app)
Slot 5: SPECTRUM      (spectrum.app)
Slot 6: TEXT          (text.app)
Slot 7: BAND SWEEP    (bandsweep.app)
Slot 8: CHAN MGR      (chanmgr.app)
```

### Flashing the Balloonatics Suite to the Radio
From the `firmware/` directory on your Mac:
```bash
cd ~/repos/uv-5rm-research/firmware

# Run the dedicated Balloonatics radio installer:
python3 source-changes/setup_balloon_radio.py /dev/cu.usbserial-XXXX skull_logo.rgb565 64 apps
```
*(Replace `/dev/cu.usbserial-XXXX` with your actual serial port adapter).*
