"""Fake `serial` module: a software UV-5RM custom-firmware CPS radio, for host tests.

Drop this file next to the script under test (sys.path[0]) in place of pyserial.
State persists in the directory named by $FAKE_RADIO_DIR (logo.bin, settings.bin,
faults.json) so multi-step flows (backup, upload, restore) work across processes.

Emulates only what source-changes/cps_tool.py frames: handshake, READ_LOGO/WRITE_LOGO,
READ/WRITE of the 168-byte settings record at 0x9000, END. Frame layout matches
cps_tool.Cps (A6 cmd addr16 len16 data crc16-xmodem).

Fault injection ($FAKE_RADIO_DIR/faults.json, consumed once each):
  {"corrupt_logo_write": true}      flip one byte of the next full logo write
  {"corrupt_settings_write": true}  flip one byte of the next settings write
  {"silent": true}                  never answer the handshake
"""
import json, os, struct

HEADER, ACK, ERR = 0xA6, 0x06, 0xEE
READ_LOGO, WRITE_LOGO, END, READ, WRITE = 0x4C, 0x6C, 0x45, 0x52, 0x57
LOGO_SIZE, SETTINGS_ADDR, SETTINGS_BYTES = 160 * 128 * 2, 0x9000, 168
HANDSHAKE = b"PROGRAMBF-5RH"


def crc16(data):
    crc = 0
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) if crc & 0x8000 else crc << 1
        crc &= 0xFFFF
    return crc


def _dir():
    d = os.environ.get("FAKE_RADIO_DIR")
    if not d:
        raise SystemExit("fake radio: set FAKE_RADIO_DIR")
    return d


def _load(name, default):
    p = os.path.join(_dir(), name)
    return open(p, "rb").read() if os.path.exists(p) else default


def _faults():
    p = os.path.join(_dir(), "faults.json")
    return json.load(open(p)) if os.path.exists(p) else {}


def _clear_fault(key):
    f = _faults()
    f.pop(key, None)
    json.dump(f, open(os.path.join(_dir(), "faults.json"), "w"))


class Serial:
    def __init__(self, name=None, baudrate=115200, timeout=None, **kw):
        self.timeout = timeout
        self.is_open = True
        self.dtr = self.rts = True
        self._rx = bytearray()   # bytes the host will read
        self._tx = bytearray()   # bytes the host wrote
        self.logo = bytearray(_load("logo.bin", b"\x00" * LOGO_SIZE))
        self.settings = bytearray(_load("settings.bin", bytes(range(SETTINGS_BYTES))))
        self.log = os.path.join(_dir(), "log.txt")

    def _note(self, s):
        open(self.log, "a").write(s + "\n")

    def _persist(self):
        open(os.path.join(_dir(), "logo.bin"), "wb").write(self.logo)
        open(os.path.join(_dir(), "settings.bin"), "wb").write(self.settings)

    def reset_input_buffer(self):
        self._rx.clear()

    def flush(self):
        pass

    def close(self):
        self.is_open = False

    def read(self, n=1):
        out = bytes(self._rx[:n])
        del self._rx[:n]
        return out

    def write(self, data):
        self._tx += data
        self._pump()
        return len(data)

    def _reply(self, cmd, addr, data=b""):
        body = struct.pack(">BHH", cmd, addr, len(data)) + data
        self._rx += bytes([HEADER]) + body + struct.pack(">H", crc16(body))

    def _pump(self):
        if bytes(self._tx).startswith(HANDSHAKE):
            del self._tx[:len(HANDSHAKE)]
            if not _faults().get("silent"):
                self._rx.append(ACK)
            return
        while len(self._tx) >= 8 and self._tx[0] == HEADER:
            cmd, addr, ln = struct.unpack(">BHH", bytes(self._tx[1:6]))
            if len(self._tx) < 6 + ln + 2:
                return
            data = bytes(self._tx[6:6 + ln])
            crc = struct.unpack(">H", bytes(self._tx[6 + ln:8 + ln]))[0]
            del self._tx[:8 + ln]
            if crc != crc16(struct.pack(">BHH", cmd, addr, ln) + data):
                continue  # bad frame: no reply, host retries
            self._handle(cmd, addr, data)

    def _handle(self, cmd, addr, data):
        if cmd == READ_LOGO:
            self._reply(cmd, addr, bytes(self.logo[addr:addr + 512]))
        elif cmd == WRITE_LOGO:
            if addr + len(data) > LOGO_SIZE:
                return self._reply(ERR, addr, b"\x02")
            if addr == 0:
                self.logo[:] = b"\xff" * LOGO_SIZE  # erase on first block
                self._note("logo erase")
            blk = bytearray(data)
            if _faults().get("corrupt_logo_write") and addr == 0:
                blk[0] ^= 0xFF
                _clear_fault("corrupt_logo_write")
                self._note("fault: corrupted logo write")
            self.logo[addr:addr + len(blk)] = blk
            self._persist()
            self._reply(cmd, addr)
        elif cmd == READ and addr == SETTINGS_ADDR:
            self._reply(cmd, addr, bytes(self.settings))
        elif cmd == WRITE and addr == SETTINGS_ADDR and len(data) == SETTINGS_BYTES:
            blk = bytearray(data)
            if _faults().get("corrupt_settings_write"):
                blk[100] ^= 0xFF
                _clear_fault("corrupt_settings_write")
                self._note("fault: corrupted settings write")
            self.settings[:] = blk
            self._persist()
            self._note("settings write")
            self._reply(cmd, addr)
        elif cmd == END:
            self._reply(cmd, addr)
        else:
            self._reply(ERR, addr, b"\x05")
