# macOS Flashing & Setup Workflow

Step-by-step instructions for backing up, flashing, and configuring the Baofeng UV-5RM from a Mac (`nm1p`).

---

## 1. Prerequisites

### Hardware
* Baofeng UV-5RM radio (Confirmed with `5RHNRFB-YP07` boot check).
* Baofeng K-plug programming cable (FTDI, Prolific, or WCH CH340).
* Mac running macOS with Python 3 installed.

### Software Environment
Install the `pyserial` dependency:
```bash
python3 -m pip install pyserial
```

Locate your USB serial device port:
```bash
ls -l /dev/cu.*
```
Common serial device names:
* `/dev/cu.usbserial-*` (FTDI)
* `/dev/cu.wchusbserial*` (CH340)

For the examples below, replace `/dev/cu.usbserial-XXXX` with your actual device name.

---

## 2. Pre-Flight Verification & Backup

### Step A: Verify Hardware Revision
1. Power off the radio.
2. Hold down the **`8`** key and turn the radio on.
3. Confirm the screen displays:
   ```text
   5RHNRFB-YP07
   ```
4. Turn off the radio. **If the screen says anything else, STOP. Do not flash.**

### Step B: Read & Backup Factory Memory (Point of No Return)
While this tool cannot dump the factory executable ROM (due to MCU read protection), it **does** dump your calibration data, serial records, and factory channel mapping:

1. Turn on the radio normally and leave it on the main frequency display.
2. Connect the programming cable securely (ensure the 2-pin K-plug is pushed in all the way until it clicks).
3. Run the factory read script from the `UV-5RM Custom Firmware` directory:
   ```bash
   cd ~/repos/UV-5RM-Radio-Mod/"UV-5RM Custom Firmware"
   mkdir -p my-backup
   python3 source-changes/radio_read.py /dev/cu.usbserial-XXXX my-backup/factory_memory.img
   ```
4. Verify that `my-backup/factory_memory.img` was generated (~33 KB).

---

## 3. Flashing Custom Firmware

### Step A: Enter Bootloader Mode
1. Turn off the radio.
2. Press and hold **both side buttons** simultaneously (the PTT button is NOT one of them; use the two smaller buttons below PTT: Moni and Flashlight/Alarm).
3. While holding both side buttons, rotate the power knob ON.
4. The screen should remain completely black / blank. The radio is now in KDH bootloader mode.

### Step B: Probe the Bootloader
Test communication with the bootloader (this is read-only and does not alter flash):
```bash
python3 source-changes/bootloader_probe.py /dev/cu.usbserial-XXXX
```
You should receive a clean handshake acknowledgment.

### Step C: Flash the Firmware Image
Execute the flash script with the expected SHA256 checksum:
```bash
python3 source-changes/flash_bf.py UV-5RM_Custom_Firmware.BF /dev/cu.usbserial-XXXX 4340f205430f0741
```
* **Duration:** ~45 seconds.
* **Caution:** Do not touch the radio or bump the cable during this process.
* When finished, the radio will automatically reboot.

---

## 4. Install Apps & Custom Logo

Once the radio reboots to its main screen:

1. Write the boot logo and install the 9 application overlays into external flash:
   ```bash
   python3 source-changes/setup_radio.py /dev/cu.usbserial-XXXX skull_logo.rgb565 64 apps
   ```
   This will format the overlay slots and upload:
   - `settings.app`
   - `chanmgr.app`
   - `searchqt.app`
   - `spectrum.app`
   - `bandsweep.app`
   - `foxhunt.app`
   - `rptfinder.app`
   - `text.app`
   - `fm.app`

2. *(Optional)* Set custom start-up tune:
   ```bash
   python3 source-changes/set_boot_tune.py /dev/cu.usbserial-XXXX my-backup/settings_backup.bin
   ```

---

## 5. Post-Flash Configuration

1. **Re-Enable Transmit:**
   - Press **Menu** on the radio and navigate to `SETTINGS`.
   - Find `TXINH` (Transmit Inhibit) and set it to **`OFF`**.
2. **Channel Management:**
   - Launch Google Chrome or Microsoft Edge on your Mac.
   - Open `firmware/UV-5RM_Channel_Uploader.html` in your browser.
   - Click **Connect Radio**, select your USB serial adapter, and configure your preferred frequency channels.
