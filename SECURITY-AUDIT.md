# Host PC / Mac Security Audit

A comprehensive code safety and security analysis was performed across all host-facing scripts and tools in the repository before testing.

---

## 1. Python Toolchain Analysis

The repository provides several Python scripts in `source-changes/` and `firmware-source/tools/` for flashing, backups, and app installation.

### Network Activity Audit
* **Check:** Searched across all `.py` files for socket, networking, or remote request libraries (`socket`, `urllib`, `requests`, `http.client`, `aiohttp`, etc.).
* **Finding:** **ZERO network imports or calls.** The scripts are strictly offline utilities that operate only on local disk files and USB serial ports (`pyserial`).

### Execution & Subprocess Audit
* **Check:** Searched for arbitrary code execution primitives (`os.system`, `subprocess`, `eval`, `exec`).
* **Finding:**
  1. `subprocess` appears only in build tools (`firmware-source/tools/mkapp.py`, `check_app.py`, `push_sat.py`) where it invokes `arm-none-eabi-objcopy` or `cargo build` during local compilation.
  2. The pattern `exec(source[:source.index("def do_logo")])` is present in several helper scripts:
     - `setup_radio.py`
     - `backup_radio.py`
     - `set_boot_tune.py`
     - `set_radio2_mario_tune.py`
     - `arrange_apps.py`
     - `install_new_apps.py`
     - `replace_app.py`
  * **Evaluation of `exec()` pattern:**
    This pattern reads the first half of the local `cps_tool.py` file to load the `Cps` serial protocol class and constants without creating a formal Python module/package. It does **not** evaluate untrusted input, download payloads, or execute obfuscated code. It is safe, albeit crude scripting.

### File System Operations
* The scripts only access the explicit files passed on the command line (e.g. firmware binary, logo image, or backup destination path).
* There are no file deletion commands outside of standard build output directories.

---

## 2. Browser HTML Tools (`WebSerial`)

The repository includes two single-page HTML applications:
1. `UV-5RM_Channel_Uploader.html`: Manages channels, names, frequencies, tones, and uploads to the radio over WebSerial.
2. `UV-5RM_Scan_Builder.html`: Generates CSV lists and toggles scan skip bits.

### Analysis & Sandbox Review
* **Zero Remote Assets:**
  - No `<script src="...">` or `<link rel="stylesheet">` tags pointing to external CDNs.
  - No embedded `<iframe>` elements.
  - The background image is an inline Base64 PNG.
* **Zero Telemetry / Phoning Home:**
  - Grepped for `fetch()`, `XMLHttpRequest`, `WebSocket`, `navigator.sendBeacon`, or image pixel pings.
  - **Result:** None. Both tools work completely offline with your internet connection disabled.
* **WebSerial Safety:**
  - Uses the W3C standard `navigator.serial` API.
  - The browser enforces an explicit user permission prompt whenever connecting to a serial port.
  - Communication is strictly standard 115,200 baud UART frames adhering to the Baofeng CPS command framing.

---

## 3. Host Safety Verdict

| Component | Status | Details |
|---|---|---|
| **Python Flashing Scripts** | **SAFE** | Local UART framing only. Requires only `pyserial`. |
| **WebSerial HTML Pages** | **SAFE** | Offline local execution; no tracking, no external calls. |
| **Compiled Binaries** | **PARTIAL** | Only structure is verified (`tools/verify_release.py`: header, vectors, ABI hash, CRCs). A rebuild with a current toolchain differs in ~15.7 KB, so the shipped `.BF` is not independently reproducible yet. |
