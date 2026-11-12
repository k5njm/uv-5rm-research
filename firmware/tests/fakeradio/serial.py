"""Fake `serial` module for the installer tests: ONE port whose radio is the stock firmware, the custom firmware or the KDH
bootloader, and which changes between them the way the real radio does.

Put this file, fake_cps.py (a copy of ../fakeserial/serial.py) and fake_bl.py (a copy of ../fakebootloader/serial.py) next to
the script under test, in place of pyserial (tests/test_install.py does this). It adds no protocol of its own for the custom
firmware or the bootloader: it hands the bytes to those two fakes. State lives in $FAKE_RADIO_DIR, the directory fake_cps.py
already uses (nor.bin, settings.bin, logo.bin, faults.json, log.txt); the bootloader's state is in $FAKE_RADIO_DIR/bl.

  mode          text file: stock | custom | bootloader. Tests write it to say what the radio is doing when the test starts.
  stock         what the stock firmware answers to radio_read.py (ident PROGRAMBFNORMALU -> 06, the three magic frames, 'R'
                read frames; data XOR-coded except the calibration page, AUDIT 26). It reads the SAME nor.bin as the custom
                firmware: one SPI flash, so channels survive a conversion. Any other byte is logged as "S ??" (a write would show).
  flash         when the bootloader fake has acknowledged END after all packages, the radio is "rebooted" into the custom
                firmware at the next open; sha256 of what the bootloader received is recorded in running_core.txt.

Behaviours of the real radio this models (and what is only a guess):
  reboot_silent  (dispatch.json) a custom-firmware session that was acknowledged and closed leaves the radio deaf to the next N
                handshake attempts: cps/mod.rs run_session resets after END or after 2 s idle. N is a guess (the real time is
                not measured); 0 = no delay.
  boot_silent    deaf to N handshakes after a core flash (boot time; a guess).
  echo           a half-seated cable: every byte written is echoed back and nothing else answers (AUDIT 25: first reply byte 50).
  user_enters_bootloader_after  K: the person holding the two side buttons gets the radio into the bootloader on the (K+1)th
                KDH handshake frame it sees: stands in for the human, not for hardware.
  stock_noise_on_session  K: in the Kth stock session one read comes back with a flipped byte (a read that does not repeat).
  cal_changes_on_flash  the flash flips a byte of the calibration page (for the test that a changed calibration must stop).
Settings come from dispatch.json in $FAKE_RADIO_DIR.
"""
import hashlib, json, os, struct, sys

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)
import fake_bl, fake_cps  # noqa: E402

CUSTOM_HS = b"PROGRAMBF-5RH"
STOCK_IDENT = b"PROGRAMBFNORMALU"
LONG_MAGIC = (b"\x53\x45\x4E\x44\x21\x05\x0D\x01\x01\x01\x04\x11\x08\x05"
              b"\x0D\x0D\x01\x11\x0F\x09\x12\x09\x10\x04\x00")
KEY = b"CO 7"


def _rd():
    d = os.environ.get("FAKE_RADIO_DIR")
    if not d:
        raise SystemExit("fake radio: set FAKE_RADIO_DIR")
    return d


def _json(name, default):
    p = os.path.join(_rd(), name)
    if os.path.exists(p):
        with open(p) as f:
            return json.load(f)
    return default


def _save(name, obj):
    with open(os.path.join(_rd(), name), "w") as f:
        json.dump(obj, f)


def _text(name, default=None):
    p = os.path.join(_rd(), name)
    if os.path.exists(p):
        with open(p) as f:
            return f.read().strip()
    return default


def _note(line):
    with open(os.path.join(_rd(), "log.txt"), "a") as f:
        f.write(line + "\n")


def crypt(buf):
    out = bytearray()
    for i, b in enumerate(buf):
        k = KEY[i % 4]
        out.append(b ^ k if (k != 32 and b not in (0, 255) and b != k and b != k ^ 255) else b)
    return bytes(out)


class _Stock:
    """What radio_read.py talks to. UNVERIFIED model: it answers what the tool sends, nothing more."""

    def __init__(self):
        self._tx, self._rx = bytearray(), bytearray()
        self.prog = False
        n = _json("stock_sessions.json", {"n": 0})["n"] + 1
        _save("stock_sessions.json", {"n": n})
        self.noisy = _json("dispatch.json", {}).get("stock_noise_on_session") == n
        with open(os.path.join(_rd(), "nor.bin"), "rb") as f:
            self.nor = f.read()

    def reset_input_buffer(self):
        self._rx.clear()

    def read(self, n=1):
        out = bytes(self._rx[:n])
        del self._rx[:n]
        return out

    def flush(self):
        pass

    def close(self):
        pass

    def write(self, data):
        self._tx += data
        while self._tx:
            t = bytes(self._tx)
            if t.startswith(STOCK_IDENT):
                del self._tx[:len(STOCK_IDENT)]
                self.prog = True
                self._rx.append(0x06)
            elif STOCK_IDENT.startswith(t):
                break
            elif not self.prog:
                _note("S ?? " + t.hex())
                self._tx.clear()
            elif t[0] == 0x46:
                del self._tx[:1]
                self._rx += b"5RH     +L00000".ljust(16, b"\x00")
            elif t[0] == 0x4D:
                del self._tx[:1]
                self._rx += b"\x00" * 15
            elif t.startswith(LONG_MAGIC[:1]) and LONG_MAGIC.startswith(t[:len(LONG_MAGIC)]):
                if len(t) < len(LONG_MAGIC):
                    break
                del self._tx[:len(LONG_MAGIC)]
                self._rx.append(0x06)
            elif t[0] == 0x52 and len(t) >= 4:
                _, addr, n = struct.unpack(">BHB", t[:4])
                del self._tx[:4]
                _note(f"S R {addr:#x}")
                blk = bytearray(self.nor[addr:addr + n])
                if self.noisy and addr == 0x40:
                    blk[0] ^= 0xFF
                self._rx += struct.pack(">BHB", 0x57, addr, n) + (blk if addr >= 0xF000 else crypt(blk))
            elif t[0] == 0x52:
                break
            else:
                _note("S ?? " + t.hex())
                self._tx.clear()
        return len(data)


class Serial:
    def __init__(self, name=None, baudrate=115200, timeout=None, **kw):
        os.environ["FAKE_BL_DIR"] = os.path.join(_rd(), "bl")
        os.makedirs(os.environ["FAKE_BL_DIR"], exist_ok=True)
        self.timeout = timeout
        self.dtr = self.rts = True
        self._echo = bytearray()
        self.cfg = _json("dispatch.json", {})
        self.mode = _text("mode", "custom")
        if self.mode == "bootloader":
            st = _json("bl/state.json", {})
            if st.get("ended") and st.get("got") == st.get("total"):
                self._complete_flash()
        self.impl = self._make()
        self.acked = False

    def _make(self):
        if self.mode == "bootloader":
            return fake_bl.Serial()
        if self.mode == "stock":
            return _Stock()
        return fake_cps.Serial()

    def _complete_flash(self):
        """The bootloader has everything and has acknowledged END: the radio restarts into what it was sent."""
        with open(os.path.join(_rd(), "bl", "flash.bin"), "rb") as f:
            blob = f.read()
        with open(os.path.join(_rd(), "running_core.txt"), "w") as f:
            f.write(hashlib.sha256(blob).hexdigest())
        if self.cfg.get("cal_changes_on_flash"):
            p = os.path.join(_rd(), "nor.bin")
            with open(p, "rb") as f:
                nor = bytearray(f.read())
            nor[0xF010] ^= 0x01
            with open(p, "wb") as f:
                f.write(nor)
        with open(os.path.join(_rd(), "mode"), "w") as f:
            f.write("custom")
        self.mode = "custom"
        _save("silent.json", {"left": self.cfg.get("boot_silent", 0)})
        # the new core's own state replaces the old firmware's fault list; nothing else to reset

    def reset_input_buffer(self):
        self._echo.clear()
        self.impl.reset_input_buffer()

    def flush(self):
        pass

    def read(self, n=1):
        if self.cfg.get("echo"):
            out = bytes(self._echo[:n])
            del self._echo[:n]
            return out
        return self.impl.read(n)

    def close(self):
        if self.mode == "custom" and self.acked:
            _save("silent.json", {"left": self.cfg.get("reboot_silent", 0)})
        self.impl.close()

    def write(self, data):
        if self.cfg.get("echo"):
            self._echo += data
            return len(data)
        if data[:2] == b"\xaa\x01" and self.mode in ("custom", "stock") and "user_enters_bootloader_after" in self.cfg:
            n = _json("kdh_probes.json", {"n": 0})["n"] + 1
            _save("kdh_probes.json", {"n": n})
            if n > self.cfg["user_enters_bootloader_after"]:
                with open(os.path.join(_rd(), "mode"), "w") as f:
                    f.write("bootloader")
                self.mode = "bootloader"
                self.impl = self._make()
        if self.mode in ("custom", "stock") and (data.startswith(CUSTOM_HS) or data.startswith(STOCK_IDENT)):
            s = _json("silent.json", {"left": 0})
            if s["left"] > 0:
                s["left"] -= 1
                _save("silent.json", s)
                return len(data)
        if self.mode == "custom" and data.startswith(CUSTOM_HS):
            self.acked = not fake_cps._faults().get("silent")
        return self.impl.write(data)
