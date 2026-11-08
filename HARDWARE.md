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
* **Chip:** **Beken BK4819**, or **Fudan Micro FD6818B** (the YP07 board's chip, ID `0x4829`). The FD6818B is *not* a drop-in register clone: upstream ported a separate driver for it (see section 4), and the register map below has to be read from Fudan's document, not Beken's.
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

### Amazon listing text review (ASIN `B0CWV6GFZT`)

Supplied by the owner (page itself not fetchable). **Consistent with the right family, but cannot identify the RF chip or board revision.**

| Listing says | Reading |
|---|---|
| Names "5RM / AR-5RM / UV-5RM", text mentions 1.25 m (VHF2), 10 W, USB-C battery, 1.77" colour TFT, 999 ch, airband | Matches the amoxu tri-band profile; rules out the `L` (AT1846S) profile (UV5RL / no 220 MHz). Frequency line lists only 144-174 / 420-470 (marketing copy omits 220-225). |
| "Support CHIRP (UV-17Pro or 5RM)" | Stock firmware is programmed through CHIRP's UV-17Pro-family driver (same KDH platform as the bootloader). Says nothing about the RF chip. **Custom firmware breaks CHIRP.** |
| "Upgraded chip" | Marketing; unverifiable. |
| FCC ID `2AJGM-5RM` (latest application 2024-02-23) | One grant covers many synonym models; implies production after early 2024, i.e. newer than the Oct/Nov-2023 stock images in amoxu's repo. Newer batches *may* be the YP07/FD6818B board; not provable from the listing. |
| No firmware version, no chip, no PCB revision stated | Unknown until the `8`-key screen is read on the physical unit. |

**Practical steps:** (1) on arrival read the `8`-key screen and photograph it; (2) before anything else, save a CHIRP image of the stock channel memory; (3) the listing's 180-day replacement offer means a non-YP07 unit can be exchanged, so **do not flash until the screen is verified**; (4) keep the original radio as stock only if you have a second one for flashing, since no stock YP07 image is available to restore.

### BK4819 build (upstream PR #3, `bk4819` feature)

Upstream PR [#3](https://github.com/jakegarrett13-afk/UV-5RM-Radio-Mod/pull/3) adds a build for boards whose `8`-key screen reads **`5RHBK`** (BK4819), versus **`5RHNRFB-YP07`** (FD6818B) for the default build. That gives the screen string a documented meaning: it selects the firmware variant. Build it with `--features bk4819`; the default build is unchanged.

Validation done here (static, no hardware): 15 of its 17 new register constants appear in Baofeng's stock `5RH_BK_V0.14` image (the other two, `0x06D8` and the already-existing `0x13A0`, could not be located by a simple scan). The YP07 values it replaces appear in **neither** stock image, which fits upstream's note that the YP07 driver is a BK4829 port. Surprisingly the stock **NRF** V0.14 image contains the same BK4819 values, so a plain-NRF (FD6818) board may also work with this build; that is a hypothesis, not tested. The PR author reports one `5RHBK` radio tested, with output power not measured.

## 5. Reference documents (linked, not stored; not redistributed here)

| Document | Use | Notes |
|---|---|---|
| [AT32F421 datasheet v2.02](https://www.arterychip.com/download/DS/DS_AT32F421_V2.02_EN.pdf) (Artery) | The core's MCU: flash/RAM layout, clocks, peripherals | Vendor download |
| FD6818B Registers Definition, 2025/1/11 (Fudan; 12 pages; sha256 `3a2d14e1debbd12a...` of the copy used here), found via [cnt7/BAOFENG-UV-K6-Firmware issue 1](https://github.com/cnt7/BAOFENG-UV-K6-Firmware/issues/1) | **The register map for the chip on this board.** Used for the FSK probe (REG_58 to REG_5F, REG_70 to REG_72, REG_67 RSSI) | Not linked directly: the issue's `file://` path is the reporter's local copy. Provenance is a third party's download, so treat it as unofficial |
| [BK4819 datasheet](https://www.qsl.net/g0ftd/other/Quansheng%2520UV5R%2520Plus%2520and%2520variants/Quansheng%2520UV5R%2520and%2520variants%2520stuff/Parts%2520and%2520Misc%2520stuff/BK4819.PDF) (hobbyist mirror) | Analogy only, for the `bk4819` build variant | **Wrong chip for the default YP07 build**; do not treat its values as this board's spec |

**`cnt7/BAOFENG-UV-K6-Firmware`** is Baofeng's published source for a different radio (the UV-K6 family, a different MCU, the `KD32F328`). It has its own FD6818 driver (`src/Driver/DevFD6818.c`, `DevFD6818Data.c`), the KD32F328 and SC5260 datasheets and a K6 schematic, so it may show how the vendor initialises the same RF chip. **Not used so far:** its licence is Baofeng's own BFPL-1.0, which I have only skimmed, so check its terms before copying anything; reading it for reference needs no copy. Its register-poking is for a different board and chip revision, so anything taken from it is a hypothesis until measured on the YP07.

**Better K6 source, from upstream issue #18:** the YP07 author points to [`sophiel-meow/bf-k61-firmware-aura`](https://github.com/sophiel-meow/bf-k61-firmware-aura), open-source Rust firmware for the K6 family. It has a working [FD6818 driver](https://github.com/sophiel-meow/bf-k61-firmware-aura/blob/master/src/driver/fd6818.rs) and an [FD6818B driver on its `fd6818b` branch](https://github.com/sophiel-meow/bf-k61-firmware-aura/blob/fd6818b/src/drivers/fd6818b.rs), which the issue says was being ported into the 5RH firmware. This repo's `src/drivers/fd6818b.rs` probably descends from it, but **I have not compared them**. That comparison (register init values, the FSK blocks) is the useful next read, and it is the same author as our upstream, so the licence question is simpler than for the Baofeng C source above. Check the repo's licence before copying.

