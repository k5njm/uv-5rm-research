# Upstream Lineage & Hardware References

This document catalogs the upstream source repositories, hardware reverse-engineering research, and community firmware projects that form the foundation of our Baofeng UV-5RM firmware and HAB tracking suite.

---

## Reference Repositories

| Repository | Author | Upstream Link | Description & Role |
|---|---|---|---|
| **[Baofeng-UV-5RM-5RH-RE](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE)** | `amoxu` | [amoxu/Baofeng-UV-5RM-5RH-RE](https://github.com/amoxu/Baofeng-UV-5RM-5RH-RE) | Canonical reverse-engineering documentation, teardown photos, MCU pinouts, KDH bootloader dumping, and `.BF` packaging crypto. |
| **[bf-5rh-firmware-open](https://github.com/sophiel-meow/bf-5rh-firmware-open)** | `sophiel-meow` | [sophiel-meow/bf-5rh-firmware-open](https://github.com/sophiel-meow/bf-5rh-firmware-open) | The upstream open-source bare-metal Rust OS base for the Baofeng UV-5RH/5RM platform (`thumbv7em-none-eabi`). |
| **[bf-k61-firmware-aura](https://github.com/sophiel-meow/bf-k61-firmware-aura)** | `sophiel-meow` | [sophiel-meow/bf-k61-firmware-aura](https://github.com/sophiel-meow/bf-k61-firmware-aura) | Next-generation "Aura" open firmware architecture for Baofeng radios, featuring modular apps, DSP filters, and satellite tracker. |
| **[UV-5RM-Radio-Mod](https://github.com/jakegarrett13-afk/UV-5RM-Radio-Mod)** | `jakegarrett13-afk` | [jakegarrett13-afk/UV-5RM-Radio-Mod](https://github.com/jakegarrett13-afk/UV-5RM-Radio-Mod) | The fork that added the 1200 baud FSK `TEXT` messaging app, band sweep, foxhunt, repeater finder, and WebSerial tools. |
| **[balloonatics](https://github.com/k5rwk/balloonatics)** | `k5rwk` | [k5rwk/balloonatics](https://github.com/k5rwk/balloonatics) | Richardson Area Balloonatics ground station reference design (KA9Q-radio, HorusDemodLib, Wenet, Chasemapper). |

---

## 1. `amoxu/Baofeng-UV-5RM-5RH-RE`

The definitive reverse-engineering repository for the UV-5RM and UV-5RH series.

### Key Contributions & Insights:
1. **Hardware Identification:**
   - Identified the MCU as an **Artery AT32F421C8T7** disguised under custom manufacturer silkprint `SYN2A-000` (120 MHz Cortex-M4, 64 KB Flash, 16 KB SRAM).
   - Identified the transceiver as a **Beken BK4819** or compatible **Fudan Micro FD6818B**.
   - Identified the external memory as a **16 Mbit (2 MB) XMC 25QH16CJIG** SPI NOR flash.
2. **Bootloader Dump & Cryptanalysis:**
   - Dumped the 4 KB factory bootloader at `0x08000000` via a custom micro-firmware payload (< 2 KB).
   - Decoded the `.BF` firmware encryption format: XORing packages by index % 3 with 4-byte keys `KDHT` and `RBGI`.
   - Documented why hardware ISP doesn't work: Baofeng remapped the `0x1FFFE400` system bootloader region to store radio configuration data.
3. **Factory Firmware Archives:**
   - Houses pristine factory firmware `.BF` files for several hardware versions (V01, V02) and CPS tools.

---

## 2. `sophiel-meow/bf-5rh-firmware-open`

The parent open-source firmware project upon which `UV-5RM-Radio-Mod` was built.

### Key Contributions & Insights:
1. **Pure Embedded Rust Base:**
   - Implements a complete `no_std` runtime with memory map targeting `thumbv7em-none-eabi`.
   - Hardware abstraction layer (HAL) for AT32F421 peripherals, timer interrupts, display drawing, and keypad debouncing.
2. **Modular ABI Architecture (`abi/src/lib.rs`):**
   - Introduced the dynamic `OVL1` overlay engine, enabling 32 KB sub-applications to live in external SPI flash and execute inside an 8 KB RAM arena.
   - Standardized the `Api` struct providing hooks for RF retuning, PTT control, NOR flash reads/writes, and LCD blitting.
3. **Driver Implementations:**
   - Clean register-level drivers for FD6818B / BK4819 (`src/drivers/fd6818b.rs`).
   - ST7735 color TFT display driver (`src/drivers/st7735.rs`).
   - NorFlash SPI driver (`src/device/storage.rs`).

---

## 3. `sophiel-meow/bf-k61-firmware-aura`

The advanced "Aura" open-source firmware project developed by `sophiel-meow` for the Baofeng K6x / K61 family.

### Key Contributions & Insights:
1. **UI & Architectural Paradigms:**
   - Sophisticated menu hierarchy, font rendering pipelines, and low-latency panadapters.
   - Advanced DSP audio processing routines.
2. **Satellite & Tracking Modules:**
   - Contains on-device satellite pass prediction routines (SGP4 orbital propagator) and automatic Doppler retuning models.
   - Useful reference for celestial and telemetry algorithms on memory-constrained ARM Cortex-M microcontrollers.

---

## 4. How They Connect to Our UV-5RM HAB Fork

```text
[ amoxu RE Research ] ---------> Hardware pinouts, memory maps & .BF crypto
       |
[ bf-5rh-firmware-open ] ------> Base Rust embedded OS & OVL1 app overlay ABI
       |
[ UV-5RM-Radio-Mod ] ----------> 1200 baud FSK modem, WebSerial uploader & UI mods
       |
[ uv-5rm-research / HAB ] -----> Our Balloonatics suite:
                                 - aprs.app (AX.25 1200 baud transceive & beacon)
                                 - horus.app (Horus Binary v2 4-FSK HAB decoder)
                                 - rs41.app (Vaisala 4800 baud radiosonde tracker)
```
