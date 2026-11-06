# Hardware Specifications & Amazon Purchase Validation

## 1. Amazon Purchase Validation

### Purchased Item Details
* **Amazon ASIN:** [`B0CWV6GFZT`](https://www.amazon.com/dp/B0CWV6GFZT)
* **Title:** *Ham Radio BAOFENG Radio 5RM 10W Handheld Long Range Walkie Talkies NOAA Emergency Two Way Radio with 999CH, USB-C 2500mAh Battery, 6.1" & 15.12" 771 Antenna - 1 Pack*
* **Seller / Shipper:** TIDRADIO Direct / Amazon Fulfillment
* **FCC ID:** `2AJGM-5RM`
* **Aliases:** Baofeng UV-5RM, AR-5RM, 5RM, UV-5RH (M Edition)

### Why This Is The Correct Hardware
1. **Radio Model Match:** The custom firmware is engineered specifically for the Baofeng **UV-5RM / UV-5RH** platform.
2. **Key Physical Indicators Present:**
   - 1.77-inch Color TFT display.
   - 999 memory channels.
   - Tri-Band coverage: VHF (144–174 MHz), 1.25M VHF2 (220–225 MHz), UHF (420–470 MHz) + AM Airband reception.
   - USB-C rechargeable 2500mAh battery pack.

---

## 2. Mandatory Verification Step (Upon Delivery)

Even though the radio model is correct, Baofeng frequently updates internal PCB revisions (e.g. `5RHNRF`, `5RHBK`, `5RHNRFB-YP07`) and switches RF frontends without changing the external shell.

### The Power-On "8" Key Test
1. Make sure the radio is completely **turned off**.
2. Press and hold down the **`8`** key on the keypad.
3. While continuing to hold **`8`**, rotate the volume/power knob clockwise to turn the radio **ON**.
4. Read the text string displayed on the screen:

| Screen String | Compatibility | Action |
|---|---|---|
| **`5RHNRFB-YP07`** | **EXACT MATCH** | Safe to proceed with backup and flashing. |
| Any other string (e.g. `...NRF`, `...BK`, `...v0.06`, `...YP06`) | **INCOMPATIBLE** | **DO NOT FLASH.** Different GPIO mapping or transceiver chip. Flashing will soft-brick or break RX/TX. |

5. Turn off the radio without pressing any other keys.

### Physical Label Check
- Remove the battery pack.
- Inspect the specification sticker on the aluminum chassis:
  - Look for model name: `UV-5RM`.
  - Check for a small round quality control sticker: Often bears letters like `BK` (denoting BK4819/FD6818 transceiver).

---

## 3. Teardown & Component Specifications

Based on hardware reverse engineering by `amoxu` (`Baofeng-UV-5RM-5RH-RE`) and `sophiel-meow`:

### Microcontroller (MCU)
* **Part:** Artery Technology **AT32F421C8T7**
* **Custom Silkprint:** `SYN2A-000`
* **Core:** ARM 32-bit Cortex-M4 running up to 120 MHz
* **On-Chip Flash:** 64 KB (divided into 4 KB bootloader at `0x08000000` + 60 KB main application at `0x08001000`)
* **SRAM:** 16 KB (top of stack: `0x20004000`)
* **Note on ISP:** The chip's native Artery system ISP bootloader mode (`0x1FFFE400`) is repurposed by Baofeng for configuration storage; external updates run through Baofeng's KDH bootloader over the K-plug UART.

### RF Transceiver
* **Chip:** **Beken BK4819** or **Fudan Micro FD6818B** (fully register- and pin-compatible clone)
* **Bus Interface:** 3-wire bit-banged serial bus connected to AT32F421:
  - `PC13`: Latch Enable (LE)
  - `PA2`: Serial Clock (SCK)
  - `PA3`: Serial Data (SDA)
* **Modulation Modes:** FM, NFM, AM (airband), and FSK modem (used by `TEXT` app).
* **Power Amplifiers:** Discrete dual-band power amp stages for VHF/UHF yielding ~8-10W on high, ~5W on mid, ~1-2W on low.

### External Memory
* **Chip:** XMC **25QH16CJIG** (or Winbond equivalent 25Q16)
* **Capacity:** 16 Mbit (2 MByte) SPI NOR Flash
* **Usage:**
  - `0x000000 - 0x01FFFF`: Channel table and radio configurations
  - `0x14C000+`: Modular app overlay slots (`OVL1` format, 9 slots of 32 KB each)
  - Boot logo bitmap (`160x128` RGB565 raw pixels = 40,960 bytes)
  - App data storage (e.g. saved `TEXT` messages, scan lists)

### Display & Peripherals
* **LCD:** 1.77-inch TFT color display (160 x 128 resolution, RGB565, ST7735-compatible controller).
* **Audio:** Internal speaker driven by audio amp; FM broadcast radio IC (BK1088 or integrated).
* **Keypad:** Matrix keypad with LED backlight.
* **Flashlight:** High-brightness white LED top-mounted.

---

## 4. Variant Compatibility (Nov 2026 research)

Sources: upstream [amoxu README](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE), issues [#3](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE/issues/3), [#6](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE/issues/6), [#18](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE/issues/18).

| Variant | RF chip | Open firmware (`bf-5rh-firmware-open`) |
|---|---|---|
| `BK` / `FK` | BK4819 | Not the upstream target; amoxu's tools cover this one |
| `L` | AT1846S | Not supported |
| `NRF` (stock) | FD6818 | Driver "still being ported and validated" per upstream |
| **`YP07`** | **FD6818B** (chip ID `0x4829`, BK4829-like) | **Current upstream target** (TX/RX, CTCSS/DCS, scan, FM verified by upstream author) |

* Flashing BK/NRF firmware onto a YP07 boots, but RF RX/TX fail (upstream #18).
* The "hold `8` at power-on" screen shows model code, `VER:` and `BF` (issue #3). The exact `5RHNRFB-YP07` string appears only in the mod README; it is not documented upstream. Record the exact text from your radio.
* **The Amazon listing (ASIN `B0CWV6GFZT`) could not be fetched** (amazon.com is outside this session's network allow-list), and Amazon batches can differ. Whether it ships as YP07 is **unknown until the radio arrives and the `8`-key screen is read**.
* FCC ID `2AJGM-5RM` lists many synonym models (UV-5RM Plus, UV-16M, K5PLUS, etc.), so the ID does not identify the PCB or RF chip.
