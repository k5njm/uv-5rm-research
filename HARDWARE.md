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

### Result on the purchased radio (2026-11-11)
* Hold-8 screen: **`5RHNRFB-YP07`**, `Ver: V0.13`. This is the string the `default` build is gated to.
* Programming port (K-plug, CH340 cable, 115200 baud): ident reply `5RH     +L00000`; band limits 136-174, 400-520, 220-260 MHz.
* Factory memory holds 22 test channels (452.125 MHz ... 258.925 MHz); backups are in the git-ignored `my-backup/`.
* The KDH bootloader answers the flasher's handshake (two small side buttons held at power-on).
* Not confirmable on stock firmware: the RF chip ID (FD6818B, 0x4829). Details: AUDIT 25.

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
* **SRAM:** 16 KB (top of stack: `0x20004000`). Core firmware layout (`firmware-source/memory.x`): `.data`+`.bss` at `0x20000000` (460 bytes), 256-byte pad for the core's statics to grow into (the build fails when under 64 bytes of it is left), the 8 KB overlay-app arena at `0x200002D0-0x200022D0` (`abi::ARENA_ADDR`, pinned), and the stack from `0x200022D0` up to `0x20004000` (7,472 bytes).
* **Note on ISP:** The chip's native Artery system ISP bootloader mode (`0x1FFFE400`) is repurposed by Baofeng for configuration storage; external updates run through Baofeng's KDH bootloader over the K-plug UART.

### RF Transceiver
* **Chip:** **Beken BK4819**, or **Fudan Micro FD6818B** (the YP07 board's chip, ID `0x4829`). The FD6818B is *not* a drop-in register clone: upstream ported a separate driver for it (see section 4), and the register map below has to be read from Fudan's document, not Beken's.
* **Bus Interface:** 3-wire bit-banged serial bus connected to AT32F421:
  - `PC13`: Latch Enable (LE)
  - `PA2`: Serial Clock (SCK)
  - `PA3`: Serial Data (SDA)
* **Modulation Modes:** FM, NFM, AM (airband), and FSK modem (used by the `TEXT` app and the FSK PROBE app).
* **Power Amplifiers:** Discrete dual-band power amp stages for VHF/UHF yielding ~8-10W on high, ~5W on mid, ~1-2W on low.

### External Memory
* **Chip:** XMC **25QH16CJIG** (or Winbond equivalent 25Q16)
* **Capacity:** 16 Mbit (2 MByte) SPI NOR Flash
* **Usage:**
  - `0x000000 - 0x01FFFF`: Channel table (1,000 x 32 bytes at 0x0000) and radio configurations (VFO 0x8000, settings 0x9000, DTMF 0xA000, stock FM list 0xC000, calibration 0xF000)
  - `0x0C0000`: Boot logo bitmap (`160x128` RGB565 raw pixels = 40,960 bytes)
  - `0x14C000+`: Modular app overlay slots (`OVL1` format, 9 slots of 32 KB each, to 0x194000)
  - App data region after the slots (`flash_map.rs` is the authority; each user checks its own range): satellite table `0x194000`, FSK PROBE capture
    `0x1B0000-0x1D0000`, FM station names `0x1D8000` (two 4 KB halves), channel details `0x1E0000` (20 bytes per channel), TEXT message log `0x1F0000`

### Display & Peripherals
* **LCD:** 1.77-inch TFT color display (160 x 128 resolution, RGB565, ST7735-compatible controller).
* **Audio:** Internal speaker driven by audio amp; FM broadcast receiver is a separate tuner chip, an RDA5807 per the core's driver (`src/drivers/rda5807.rs`), which is why broadcast FM is the FM app and not a VFO band.
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

**Better K6 source, from upstream issue #18:** the YP07 author points to [`sophiel-meow/bf-k61-firmware-aura`](https://github.com/sophiel-meow/bf-k61-firmware-aura), open-source Rust firmware for the K6 family. It has a working [FD6818 driver](https://github.com/sophiel-meow/bf-k61-firmware-aura/blob/master/src/driver/fd6818.rs) and an [FD6818B driver on its `fd6818b` branch](https://github.com/sophiel-meow/bf-k61-firmware-aura/blob/fd6818b/src/drivers/fd6818b.rs), which the issue says was being ported into the 5RH firmware. **Compared (2026-11-08, Aura `fd6818b` branch, last driver commit `de011b9` of 2026-09-03, GPL-3.0 like this repo):** our `src/drivers/fd6818b.rs` is a port of it. All 143 shared constants and the FSK, modulation, squelch, sub-audio, AGC and TX paths are identical apart from rustfmt reflow and the bus plumbing (AT32F421 GPIO instead of KD32F328). Aura's FD6818B values (`REG_37` `0x9D1F`/`0x9F1F`, `REG_14`/`REG_49` `0x0210`/`0x2AB2`) are our default-build values; the `0x1D0F`/`0x0019` set exists only in our `bk4819` feature. Ours adds: separate RX (GPIO5/6) and TX (GPIO3/4) band switches (Aura drives GPIO2/3 for the PA path and GPIO5 as the K6's TX LED), `set_agc_fix`, `pa_target`/`set_band_quiet`, `reg_read`/`reg_write` (the new ABI), and long provenance comments on the PA gain word. Nothing in Aura's driver is missing from ours, so there is nothing to merge, and Aura has not touched the file since. The core's FSK reference init (`enter_fsk_mode`, unused: `REG_72 = 0x3065` = 1200 baud, `REG_5D = 0x0F00`) independently confirms the `baud * 10.32444` formula and the 13/26 MHz crystal assumption. **Caveat:** since the driver is a K6 port, "matches Aura" says the code was ported faithfully, not that the YP07 behaves like a K6 (upstream reports TX/RX working on a YP07; this repo has verified nothing on hardware).


## 6. FSK modem registers (FSK PROBE, 2026-11-12)

Written to find out why the first FSK PROBE capture (404.800 MHz, noise, 4800 baud, sync 0x086D 0x5388) held only a sync
interrupt and, 119.7 ms later, one word with the done flag. Sources, in order of trust:

* **FD** = Fudan "FD6818B Registers Definition" 2025/1/11 (`build-out/refs/FD6818B.Registers.Definition.pdf`, sha256 `3a2d14e1...`,
  pages 3, 4, 8, 10, 11, 12), read from the rendered pages because the text layer loses the table layout. A vendor register
  list for this chip, but unofficial in provenance and **not measured on the YP07**.
* **TEXT** = `apps/textapp/src/logic.rs`: sends and receives 32-byte FSK packets on this very radio at 450 baud (owner-tested).
* **K6** = Baofeng's `cnt7/BAOFENG-UV-K6-Firmware` `src/Driver/DevFD6818.c` (read, nothing copied; licence BFPL-1.0) and our
  `src/drivers/fd6818b.rs` (a port of the Aura driver). Different board, same chip family.
* **K5** = `egzumer/uv-k5-firmware-custom` `app/aircopy.c`, `app/app.c`, `driver/bk4819.c`, `driver/bk4819-regs.h` (Apache-2.0
  code for the BK4819, a relative). The BK4819 PDF in section 5 is a product brief with no register map.
* **RS41** = rdz_ttgo_sonde `RX_FSK/src/RS41.cpp` (read for facts only).

"Confirmed" means the cited source says it; **nothing is verified on the YP07 except what TEXT does and the one capture**.

| Register | Field | Meaning | Source | Status |
|---|---|---|---|---|
| 0x02 (read; write 0 latches/clears) | 15 / 14 / 13 / 12 / 1 | FSK TX finished / FIFO almost empty / RX finished (done) / FIFO almost full / RX sync found | FD p3; K5 `bk4819-regs.h`; TEXT | confirmed (TEXT uses 15, 13, 12, 1 on this chip) |
| 0x02 | write 0 then read | the read returns the flags latched by the write | TEXT `poll_modem`, K5 | confirmed by TEXT |
| 0x0C | 0 | interrupt request (any enabled flag) | FD p4; TEXT | confirmed |
| 0x0B (read) | 7 / 6 / 4 | FSK sync negative found / positive found / CRC pass | FD p4 | unverified (never read before; FSK PROBE now records bits 7:6 at each sync) |
| 0x3F | 15 / 14 / 13 / 12 / 1 | enable for the same flags as 0x02 | FD p8; K6 comment `0x3000 rx sucs/fifo_af irq mask`; TEXT writes 0x3002 | confirmed |
| 0x58 | 0 | FSK enable | FD p10; K6 | confirmed |
| 0x58 | 3:1 | RX bandwidth: 000 FSK 1.2K, 100 FSK 2.4K, 001 FFSK 1200/1800, 010 NOAA SAME | FD p10 | confirmed (meaning); which suits 4800 baud RS41 is **unverified** (probe default now 2.4K) |
| 0x58 | 5:4 | preamble type 11=0xAA, 10=0x55, 00 by sync byte 0 MSB | FD p10 | confirmed; TX only, unused |
| 0x58 | 9:8 | "FSK RX gain" (values not given). 0x0301 sets both | FD p10; TEXT value | unverified; kept from TEXT, which works at 450 baud |
| 0x58 | 12:10 / 15:13 | RX mode / TX mode (000 = FSK 1.2K/2.4K) | FD p10 | confirmed, left 000 |
| 0x59 | 15 / 14 | clear TX FIFO / clear RX FIFO, "1 = clear" | FD p11; K6 `REG_59 \| 0x4000` then `\| 0x1000`; TEXT `rx_restart`; K5 | confirmed. Whether the clear is level (held) or edge is **unverified** |
| 0x59 | 13 | FSK scramble | FD p11 | confirmed; left 0 (RS41 is scrambled by the sonde, descrambled in software) |
| 0x59 | 12 / 11 | enable FSK RX / TX | FD p11; K6; TEXT `CTRL_RX_ON = 1<<12` | confirmed |
| 0x59 | 10 / 9 | invert FSK data on RX / TX | FD p11 | confirmed (meaning); which polarity an RS41 needs is **unverified** (option INV) |
| 0x59 | 8 | not in the document | FD p11 | the old probe set it; no longer |
| 0x59 | 7:4 | TX preamble length, 0 = 1 byte ... 15 = 16 | FD p11; TEXT TX uses 15<<4 | confirmed, unused on RX |
| 0x59 | 3 | sync length: 0 = 2 bytes (REG_5A), 1 = 4 bytes (5A, 5B) | FD p11; TEXT comment "4-byte sync" and `CTRL_RX = 1<<3`; K5 0x0068; K6 0x0028 | **confirmed** |
| 0x5A, 0x5B | 15:8, 7:0 | sync bytes 0,1 and 2,3; "Sync Byte 0 first" | FD p11 | confirmed (meaning); bit order within a byte and word order in the FIFO **unverified** (keys 8 / 9) |
| 0x5C | 6 | CRC enable. 0x5625 (TEXT) = off, 0x5665 (K5, K6) = on | FD p11; TEXT; K6 | confirmed for bit 6; other bits undocumented, left as TEXT |
| 0x5D | 15:8 and 7:5 | packet length in bytes minus one, 11 bits (low 8, high 3); "0xF means 16 bytes" | FD p11; K5 (0x4700 = 72 bytes); K6 (`(8*2-1)<<8`) | confirmed for the low 8 bits by two drivers; **the high 3 bits are only in FD, unverified on the chip** |
| 0x5E | 9:3 / 2:0 | TX FIFO (128 words) almost-empty threshold, default 64 / RX FIFO (8 words) almost-full threshold, default 4 | FD p11 | confirmed (meaning). TEXT writes (64<<3)\|1 and gets one word per interrupt: **confirmed on this chip for threshold 1**. K5 leaves the default 4 and reads 4 words per interrupt. Whether "almost full" means >= or > the threshold, and whether the flag repeats while the FIFO stays above it, is **unverified** |
| 0x5F | 15:0 | FIFO word. First byte on air in the low byte (TEXT's packing); a read removes a word | FD p11; TEXT | unverified for received data; never peeked |
| 0x70 | 15 / 14:8 / 7 / 6:0 | TONE1 enable / gain / TONE2 enable / TONE2-FSK gain | FD p12; K6 writes 0x00E0 for FSK; TEXT (1<<7)\|96 | confirmed |
| 0x71 | | TONE1/scramble frequency word | FD p12 | not touched by the probe |
| 0x72 | 15:0 | TONE2/FSK frequency word = baud * 10.32444 (13/26 MHz crystal) or * 10.48576 | FD p12; K6 0x3065 = 1200; TEXT 4646 = 450 | confirmed formula; the crystal is **unverified** (TEXT would agree with itself either way) |
| 0x73 | 4 / 13:11 | AFC disable / AFC range | FD p12 | **not on the allow-list**; AFC is on by default (0 = enabled), range at maximum. Not changed |
| 0x67 | 8:0 | RSSI = value/2 - 160 dBm | FD p11 | confirmed (already used) |

RS41 needs (RS41.cpp): 4800 baud 2-level FSK; the header `08 6D 53 88 44 69 48 1F`; 320 bytes (`RS41MAXLEN`); the SX1278 is told 8 sync
bytes and then 312 payload bytes, with AFC and AGC on; bits are reversed and XORed with a 64-byte mask afterwards. The FD6818B
matches at most 4 sync bytes, so the first four bytes after the sync (`44 69 48 1F`) arrive as data and **316 bytes** follow
the sync, if the chip does not deliver the sync bytes (as in K5's aircopy, unverified here).

### What explains the first capture

1. **Sync looser than 32 bits: high confidence.** The old probe wrote REG_59 as bits 12, 10, 8 and never bit 3. FD, TEXT, K5
   and K6 all say bit 3 selects 4 sync bytes, so the chip matched only 16 bits (0x086D). A 16-bit match on noise is about
   1 in 65,536 per bit position, 4800 positions a second: about 0.2 events in 3 s. One sync in 2.93 s fits. No register for
   bit-error tolerance exists in FD.
2. **Words not streaming: likely, not proven.** The second REG_59 write was `CTRL_RX | (1<<14)`: the constant named
   `CTRL_RX_ON` was bit 14, which is *clear RX FIFO*, not the enable (bit 12). So the FIFO was told to clear on every write and
   never released. If the clear is level, the FIFO stays empty; that matches a sync, no almost-full interrupts, and a done
   flag. TEXT, K5 and K6 all write the bit back to 0 before enabling. The threshold (1) and the one-word-per-interrupt read
   were not the problem: they are TEXT's. The single word 0xFCB7 with the done flag is not explained (whatever the done path
   leaves in the FIFO).
3. **Reception ends at the length: confirmed as designed, the width is unverified.** REG_5D was 64<<8, i.e. 65 bytes
   (minus-one encoding), 108 ms at 4800 baud; the capture shows 119.7 ms, about 11 ms or 7 bytes more, **unexplained**
   (baud/crystal error, sync bytes counted, or tick latency on the sync record). An RS41 frame needs 316 bytes: FD says the
   field is 11 bits (0x3B20). If the chip ignores the top three bits, 316 would wrap to 60 bytes.

FSK PROBE now sets bit 3, releases the clear bit using TEXT's restart sequence, writes REG_5D as 0x3B20, counts words itself
and ends a frame at 158 words whatever the chip does, and offers options for the open readings (LEN, TH, INV, BW, SYNC).
