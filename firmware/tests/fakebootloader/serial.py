"""Fake `serial` module: a software KDH bootloader, for host tests of source-changes/flash_bf.py.

Frames match flash_bf.py: AA cmd args len16 data crc16-xmodem EF. A reply is AA cmd args(0x06=ACK)
len16 crc EF; errors put the error code (E1 handshake/command, E2 sequence/length, E3 flash write,
E5 unknown) in the cmd position. State lives in $FAKE_BL_DIR/state.json (+ flash.bin) so a second
process sees what the first one left, e.g. after an interrupted transfer. UNVERIFIED model: it
reproduces what flash_bf.py expects, not measured bootloader behaviour.

Faults in $FAKE_BL_DIR/faults.json (persistent):
  {"silent": true}                   never answers (radio not in flash mode / cable out)
  {"die_after_packages": N}          stops answering after N accepted packages (cable pulled)
  {"flash_fail_at": SEQ}             answers E3 for that package
  {"no_end_reply": true}             takes END and never answers it (the radio reboots first: flash_bf.py tolerates it)
  {"lose_ack_at": SEQ}               takes package SEQ but its ACK never reaches the host (once)
  {"duplicate_package": "e2"|"ack"}  what a repeat of the last accepted package gets. UNKNOWN for the real bootloader:
                                     "e2" (default) answers a sequence error, "ack" answers ACK and ignores it.
                                     flash_bf.py retries the same frame, so the two policies decide whether a lost ACK
                                     ends a flash that is in fact going fine (the host can simply start over: the
                                     handshake restarts the transfer)

Every command that reaches the model is appended to $FAKE_BL_DIR/log.txt as "cmd 0x01" (handshake), "cmd 0x04" (package
count), "cmd 0x03 <seq>" (a package: a flash write) and "cmd 0x45" (end), so a test can show that nothing but
handshakes were sent.

What is known (six real flashes with flash_bf.py): the happy path above. Not known, and not modelled: error behaviour in
general, flash write/erase time (the host allows 2 s per package), a package size other than 1 KB, a file longer than
the application area, a handshake in the middle of a transfer (this fake restarts it), what END does to a transfer that
is not complete (this fake answers E2).
"""
import json, os, struct

HEADER, FOOTER, ACK = 0xAA, 0xEF, 0x06
CMD_HANDSHAKE, CMD_UPDATE, CMD_PACKAGES, CMD_END = 0x01, 0x03, 0x04, 0x45
CHUNK = 1024


def crc16(data):
    crc = 0
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) if crc & 0x8000 else crc << 1
        crc &= 0xFFFF
    return crc


def _d():
    d = os.environ.get("FAKE_BL_DIR")
    if not d:
        raise SystemExit("fake bootloader: set FAKE_BL_DIR")
    return d


def _load(name, default):
    p = os.path.join(_d(), name)
    if not os.path.exists(p):
        return default
    with open(p) as f:
        return json.load(f)


class Serial:
    def __init__(self, name=None, baudrate=115200, timeout=None, **kw):
        self.timeout, self.is_open, self.dtr, self.rts = timeout, True, False, False
        self._rx, self._tx = bytearray(), bytearray()
        self.state = _load("state.json", {"handshakes": 0, "total": None, "got": 0, "ended": False, "opened": 0})
        self.state["opened"] += 1
        self.flash = bytearray()
        p = os.path.join(_d(), "flash.bin")
        if os.path.exists(p):
            with open(p, "rb") as f:
                self.flash = bytearray(f.read())
        self._save()

    def _save(self):
        with open(os.path.join(_d(), "state.json"), "w") as f:
            json.dump(self.state, f)
        with open(os.path.join(_d(), "flash.bin"), "wb") as f:
            f.write(self.flash)

    def reset_input_buffer(self):
        self._rx.clear()

    def close(self):
        self.is_open = False

    def read(self, n=1):
        out = bytes(self._rx[:n])
        del self._rx[:n]
        return out

    def _reply(self, cmd, args=ACK):
        body = bytes([cmd, args, 0, 0])
        self._rx += bytes([HEADER]) + body + struct.pack(">H", crc16(body)) + bytes([FOOTER])

    def write(self, data):
        self._tx += data
        while len(self._tx) >= 8 and self._tx[0] == HEADER:
            n = self._tx[3] << 8 | self._tx[4]
            if len(self._tx) < 8 + n:
                break
            body, got = bytes(self._tx[1:5 + n]), self._tx[5 + n] << 8 | self._tx[6 + n]
            del self._tx[:8 + n]
            if got == crc16(body):
                self._handle(body[0], body[1], body[4:])
        return len(data)

    def _handle(self, cmd, args, data):
        f = _load("faults.json", {})
        if f.get("silent"):
            return
        if "die_after_packages" in f and self.state["got"] >= f["die_after_packages"] and cmd == CMD_UPDATE:
            return
        with open(os.path.join(_d(), "log.txt"), "a") as lg:
            lg.write(f"cmd {cmd:#04x}" + (f" {self.state['got']}" if cmd == CMD_UPDATE else "") + "\n")
        if cmd == CMD_HANDSHAKE:
            if data != b"BOOTLOADER":
                return self._reply(0xE1, 0)
            self.state.update(handshakes=self.state["handshakes"] + 1, got=0, ended=False, total=None)
            self.flash = bytearray()
        elif cmd == CMD_PACKAGES:
            if self.state["handshakes"] == 0 or len(data) != 1:
                return self._reply(0xE1, 0)
            self.state["total"] = data[0]
        elif cmd == CMD_UPDATE:
            if (self.state["total"] is not None and len(data) == CHUNK and self.state["got"] > 0
                    and args == ((self.state["got"] - 1) & 0xFF) and f.get("duplicate_package") == "ack"):
                return self._reply(cmd)                      # a repeat of the last package: acknowledged, not stored again
            if self.state["total"] is None or len(data) != CHUNK or args != (self.state["got"] & 0xFF):
                return self._reply(0xE2, 0)
            if f.get("flash_fail_at") == self.state["got"]:
                return self._reply(0xE3, 0)
            self.flash += data
            self.state["got"] += 1
            if f.get("lose_ack_at") == self.state["got"] - 1 and not self.state.get("ack_lost"):
                self.state["ack_lost"] = True
                self._save()
                return
        elif cmd == CMD_END:
            if self.state["got"] != self.state["total"]:
                return self._reply(0xE2, 0)
            self.state["ended"] = True
            if f.get("no_end_reply"):
                self._save()
                return
        else:
            return self._reply(0xE5, 0)
        self._save()
        self._reply(cmd)
