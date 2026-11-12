"""Fake `serial` module: a software UV-5RM custom-firmware CPS radio, for host tests.

Drop this file next to the script under test (sys.path[0]) in place of pyserial.
State persists in the directory named by $FAKE_RADIO_DIR (nor.bin, logo.bin, settings.bin,
faults.json, log.txt) so multi-step flows (backup, upload, restore) work across processes.

It is a port of firmware-source/src/cps/{mod,frame}.rs and the storage layer under it
(device/storage.rs, drivers/norflash.rs). docs/TEST-DOUBLES.md has the line-by-line comparison.
What it reproduces from the firmware source (UNVERIFIED on hardware, "per the source"):
  * the byte-at-a-time frame reader: junk before 0xA6 is skipped, a length over 512 drops the header and
    resyncs on the next 0xA6 in the data, a bad CRC is answered with ERR 0x01 (not silence)
  * a session: the handshake is only recognised from the standby screen (a second handshake inside a
    session is just junk bytes), it resets the per-session sector-erase record, END answers and then the
    radio restarts (this fake never answers again), and 2000 ms without a byte restarts the radio too
  * flash: a program ANDs into what is there (bits only clear), only an erase sets them, an erase takes the
    whole 4 KB sector that contains the address, the first channel write into a sector in a session erases it
  * the stored records are parsed and rebuilt like the core does: settings go through Settings::from_bytes
    (clamped), a blank settings record reads as the firmware default
  * command/length/address rules and error codes of every handler, including app slots ('A'/'a') and the
    satellite table ('S'), which the older fake did not have
Known simplifications (see TEST-DOUBLES.md): VFO and settings are plain records here, the real flash holds them in
wear-levelled slots, so raw dumps of 0x8000/0x9000/0xC000 differ from the real chip; erase/program take no time.

Environment:
  FAKE_RADIO_IDLE_S    seconds of silence that restart the radio inside a session (default 2.0, the firmware's)
  FAKE_RADIO_BLOCK=1   read() waits out its timeout when nothing is queued, like a real port (default: returns at once)

Every command is appended to log.txt: "R/W/D <addr>" ("F <offset>" for the FM station table) so tests can assert
exactly what a tool touched (e.g. calibration at 0xF000 is never written).

Fault injection ($FAKE_RADIO_DIR/faults.json, consumed once each unless noted):
  {"corrupt_channel_write": true}   flip one byte of the next channel write
  {"corrupt_vfo_write": true}       flip one byte of the next VFO write
  {"corrupt_contact_write": true}   flip one byte of the next DTMF contact write
  {"die_after_writes": N}           (persistent) stop answering after N channel/VFO/contact writes: a pulled cable
  {"corrupt_logo_write": true}      flip one byte of the next full logo write
  {"corrupt_settings_write": true}  flip one byte of the next settings write
  {"silent": true}                  never answer the handshake
  {"boot_delay_s": S}  (persistent) ignore everything until S seconds after the port was opened: the radio is
                                    still restarting from the previous session (the real time is unknown)
"""
import json, os, struct, time

HEADER, ACK, ERR = 0xA6, 0x06, 0xEE
READ_LOGO, WRITE_LOGO, END, READ, WRITE = 0x4C, 0x6C, 0x45, 0x52, 0x57
APP_ERASE, APP_WRITE, SAT_WRITE, READ_RAW = 0x41, 0x61, 0x53, 0x44
EXTRA_WRITE = 0x58
FM_WRITE = 0x46
ERR_BAD_CRC, ERR_BAD_ADDR, ERR_BAD_LEN = 0x01, 0x02, 0x03
MAX_DATA = 512
LOGO_SIZE, SETTINGS_ADDR, SETTINGS_BYTES = 160 * 128 * 2, 0x9000, 168
NOR_SIZE, LOGO_ADDR, VFO_ADDR, CHAN_SIZE, MAX_CHAN = 2 * 1024 * 1024, 0xC0000, 0x8000, 32, 999
CONTACT_ADDR, CONTACT_SIZE, CONTACTS = 0xA020, 16, 20
FM_ADDR, FM_BYTES = 0xC000, 60
SCRATCH_ADDR = 0xD000
SECTOR = 4096
SLOT_BASE, SLOT_SIZE, SLOT_COUNT = 0x14C000, 32 * 1024, 9
SAT_BASE, SAT_SIZE = SLOT_BASE + SLOT_SIZE * SLOT_COUNT, 5120
EXTRA_BASE, EXTRA_SIZE = 0x1E0000, 20480
FMTAB_BASE, FMTAB_SIZE = 0x1D8000, 0x2000
CHUNK = 512
IDLE_S = 2.0
HANDSHAKE = b"PROGRAMBF-5RH"
KEYFN_MAX = 19

# Settings::from_bytes (flash_map.rs): offset -> ("bool",) | ("min", n) | ("clamp", lo, hi) ; checked against the source by
# tests/test_fake_fidelity.py so it cannot drift.
SETTINGS_RULES = {
    1: ("bool",), 2: ("bool",), 3: ("bool",), 4: ("min", 3), 5: ("bool",), 6: ("bool",), 7: ("clamp", 1, 9),
    9: ("bool",), 10: ("min", 2), 12: ("min", 3), 13: ("min", 2), 15: ("min", 4), 16: ("min", 4), 17: ("min", 2),
    18: ("bool",), 19: ("min", 10), 20: ("min", 15),
    23: ("min", KEYFN_MAX), 24: ("min", KEYFN_MAX), 25: ("min", KEYFN_MAX), 26: ("min", KEYFN_MAX),
    27: ("min", KEYFN_MAX), 28: ("min", KEYFN_MAX), 29: ("min", 9), 30: ("min", 2), 31: ("bool",),
}


def default_settings():
    """Settings::DEFAULT.to_bytes()."""
    b = bytearray(SETTINGS_BYTES)
    for off, v in {0: 3, 1: 1, 3: 1, 7: 1, 8: 8, 9: 1, 12: 2, 13: 1, 16: 2, 17: 2, 20: 5, 23: 5, 25: 8, 27: 6,
                   29: 1, 30: 2, 31: 1}.items():
        b[off] = v
    b[21:23] = struct.pack("<H", 2537)
    b[32:36] = b[36:40] = struct.pack("<i", 0x7FFFFFFF)
    for i, (tone, dur) in enumerate([(1, 4), (13, 4), (20, 4), (25, 4), (32, 4), (37, 10)]):
        b[72 + i * 2], b[73 + i * 2] = tone, dur
    return bytes(b)


def normalize_settings(raw):
    """Settings::from_bytes then to_bytes: what the radio holds and reports back for any 168 bytes."""
    b = bytearray(raw)
    for off, rule in SETTINGS_RULES.items():
        if rule[0] == "bool":
            b[off] = 1 if b[off] else 0
        elif rule[0] == "min":
            b[off] = min(b[off], rule[1])
        else:
            b[off] = max(rule[1], min(b[off], rule[2]))
    v = b[14] - 256 if b[14] > 127 else b[14]
    b[14] = max(-127, min(v, 127)) & 0xFF
    for off, limit in ((32, 9000000), (36, 18000000)):
        c = struct.unpack("<i", bytes(b[off:off + 4]))[0]
        if not -limit <= c <= limit:
            b[off:off + 4] = struct.pack("<i", 0x7FFFFFFF)
    return bytes(b)


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


class FrameReader:
    """cps/frame.rs FrameReader, byte for byte."""

    def __init__(self):
        self.state, self.cmd, self.addr, self.len, self.data, self.crc_hi = "header", 0, 0, 0, bytearray(), 0

    def feed(self, byte):
        s = self.state
        if s == "header":
            if byte == HEADER:
                self.state = "cmd"
        elif s == "cmd":
            self.cmd, self.state = byte, "addr_hi"
        elif s == "addr_hi":
            self.addr, self.state = byte << 8, "addr_lo"
        elif s == "addr_lo":
            self.addr |= byte
            self.state = "len_hi"
        elif s == "len_hi":
            self.len, self.state = byte << 8, "len_lo"
        elif s == "len_lo":
            self.len |= byte
            if self.len > MAX_DATA:
                self.state = "header"           # oversized: silently dropped, resync on the next header byte
            else:
                self.data = bytearray()
                self.state = "crc_hi" if self.len == 0 else "data"
        elif s == "data":
            self.data.append(byte)
            if len(self.data) == self.len:
                self.state = "crc_hi"
        elif s == "crc_hi":
            self.crc_hi, self.state = byte, "crc_lo"
        else:
            rx = (self.crc_hi << 8) | byte
            self.state = "header"
            body = struct.pack(">BHH", self.cmd, self.addr, self.len) + bytes(self.data)
            if rx == crc16(body):
                return ("frame", self.cmd, self.addr, bytes(self.data))
            return ("crc", self.addr)
        return None


class Serial:
    def __init__(self, name=None, baudrate=115200, timeout=None, **kw):
        self.timeout = timeout
        self.is_open = True
        self.dtr = self.rts = True
        self._rx = bytearray()   # bytes the host will read
        self.logo = bytearray(_load("logo.bin", b"\x00" * LOGO_SIZE))
        self.settings = bytearray(_load("settings.bin", default_settings()))
        self.nor = bytearray(_load("nor.bin", b"\xff" * NOR_SIZE))
        self.chan_writes = 0
        self.log = os.path.join(_dir(), "log.txt")
        self.mode = "standby"            # standby | session | down (END or idle timeout: the radio restarted)
        self.matched = 0                 # HandshakeDetector position
        self.reader = FrameReader()
        self.erased_mask = set()         # channel-table sectors erased in this session (storage.rs channel_erased_mask)
        self.app_slot = None
        self.last_byte = time.monotonic()
        self.born = time.monotonic()
        self.idle_s = float(os.environ.get("FAKE_RADIO_IDLE_S", IDLE_S))

    # ---- port surface ----
    def _note(self, s):
        open(self.log, "a").write(s + "\n")

    def _persist(self):
        open(os.path.join(_dir(), "logo.bin"), "wb").write(self.logo)
        open(os.path.join(_dir(), "settings.bin"), "wb").write(self.settings)
        open(os.path.join(_dir(), "nor.bin"), "wb").write(self.nor)

    def reset_input_buffer(self):
        self._rx.clear()

    def flush(self):
        pass

    def close(self):
        self.is_open = False

    def _tick(self):
        """The idle timeout (cps/mod.rs run_session): 2000 ms without a byte restarts the radio."""
        if self.mode == "session" and time.monotonic() - self.last_byte >= self.idle_s:
            self._note("radio restarted: idle timeout")
            self.mode = "down"

    def read(self, n=1):
        self._tick()
        if not self._rx and os.environ.get("FAKE_RADIO_BLOCK") and self.timeout:
            time.sleep(self.timeout)
            self._tick()
        out = bytes(self._rx[:n])
        del self._rx[:n]
        return out

    def write(self, data):
        self._tick()
        booting = time.monotonic() - self.born < float(_faults().get("boot_delay_s", 0))
        for b in data:
            if self.mode == "standby":
                if not booting:
                    self._standby_byte(b)
            elif self.mode == "session":
                self.last_byte = time.monotonic()
                r = self.reader.feed(b)
                if r:
                    self._frame(r)
        return len(data)

    # ---- standby: the handshake (cps/mod.rs HandshakeDetector, run_session start) ----
    def _standby_byte(self, b):
        if b == HANDSHAKE[self.matched]:
            self.matched += 1
            if self.matched == len(HANDSHAKE):
                self.matched = 0
                if not _faults().get("silent"):
                    self._start_session()
        else:
            self.matched = int(b == HANDSHAKE[0])

    def _start_session(self):
        self.mode = "session"
        self.erased_mask = set()          # reset_channel_write_session
        self.reader = FrameReader()
        self.app_slot = None
        self.last_byte = time.monotonic()
        self._rx.append(ACK)

    # ---- frames ----
    def _reply(self, cmd, addr, data=b""):
        body = struct.pack(">BHH", cmd, addr, len(data)) + data
        self._rx += bytes([HEADER]) + body + struct.pack(">H", crc16(body))

    def _error(self, addr, code):
        self._reply(ERR, addr, bytes([code]))

    def _frame(self, r):
        if r[0] == "crc":
            return self._error(r[1], ERR_BAD_CRC)
        _, cmd, addr, data = r
        n = _faults().get("die_after_writes")
        if n is not None and self.chan_writes >= n:
            self._note("fault: radio went silent (cable pulled)")
            return
        if cmd in (READ, WRITE, READ_RAW):
            self._note(f"{chr(cmd)} {addr:#x}" if cmd != READ_RAW else f"D {addr * 64:#x}")
        self._handle(cmd, addr, data)

    # ---- flash (drivers/norflash.rs): program ANDs, erase sets a whole sector ----
    def _program(self, addr, data):
        for i, b in enumerate(data):
            self.nor[addr + i] &= b

    def _erase(self, addr):
        s = addr - addr % SECTOR
        self.nor[s:s + SECTOR] = b"\xff" * SECTOR

    def _erase_range(self, base, size):
        s = base
        while s < base + size:
            self._erase(s)
            s += SECTOR

    # ---- handlers (cps/mod.rs) ----
    def _handle(self, cmd, addr, data):
        if cmd == READ:
            return self._read(addr)
        if cmd == WRITE:
            return self._write(addr, data)
        if cmd == READ_LOGO:
            if addr % CHUNK or addr + CHUNK > LOGO_SIZE:
                return self._error(addr, ERR_BAD_ADDR)
            return self._reply(cmd, addr, bytes(self.logo[addr:addr + CHUNK]))
        if cmd == WRITE_LOGO:
            if addr % CHUNK or len(data) != CHUNK or addr + CHUNK > LOGO_SIZE:
                return self._error(addr, ERR_BAD_ADDR)
            if addr == 0:                       # erases every logo sector, only here
                self.logo[:] = b"\xff" * LOGO_SIZE
                self._note("logo erase")
            blk = bytearray(data)
            if _faults().get("corrupt_logo_write") and addr == 0:
                blk[0] ^= 0xFF
                _clear_fault("corrupt_logo_write")
                self._note("fault: corrupted logo write")
            for i, b in enumerate(blk):          # a program can only clear bits
                self.logo[addr + i] &= b
            self._persist()
            return self._reply(cmd, addr)
        if cmd == APP_ERASE:
            if addr >= SLOT_COUNT:
                self.app_slot = None
                return self._error(addr, ERR_BAD_ADDR)
            self._erase_range(SLOT_BASE + addr * SLOT_SIZE, SLOT_SIZE)
            self.app_slot = addr
            self._persist()
            return self._reply(cmd, addr)
        if cmd == APP_WRITE:
            if self.app_slot is None or addr % CHUNK or len(data) != CHUNK or addr + CHUNK > SLOT_SIZE:
                return self._error(addr, ERR_BAD_ADDR)
            self._program(SLOT_BASE + self.app_slot * SLOT_SIZE + addr, data)
            self._persist()
            return self._reply(cmd, addr)
        if cmd in (SAT_WRITE, EXTRA_WRITE, FM_WRITE):
            base, size = {SAT_WRITE: (SAT_BASE, SAT_SIZE), EXTRA_WRITE: (EXTRA_BASE, EXTRA_SIZE),
                          FM_WRITE: (FMTAB_BASE, FMTAB_SIZE)}[cmd]
            if cmd == FM_WRITE:
                self._note(f"F {addr:#x}")
            return self._table_write(cmd, addr, data, base, size)
        if cmd == READ_RAW:
            a = addr * 64
            if a + 64 > NOR_SIZE:
                return self._error(addr, ERR_BAD_ADDR)
            chunk = bytearray(self.nor[a:a + 64])
            for base, blob in ((SETTINGS_ADDR, self.settings), (LOGO_ADDR, self.logo)):
                lo, hi = max(a, base), min(a + 64, base + len(blob))
                if lo < hi:  # overlay the part of the chunk covered by that region
                    chunk[lo - a:hi - a] = blob[lo - base:hi - base]
            return self._reply(cmd, addr, bytes(chunk))
        if cmd == END:
            self._reply(cmd, addr)
            self.mode = "down"          # reply drained, then SCB::sys_reset(): nothing answers until a new power-up
            return
        self._error(addr, ERR_BAD_LEN)

    def _table_write(self, cmd, addr, data, base, size):
        if addr % CHUNK or len(data) != CHUNK or addr + CHUNK > size:
            return self._error(addr, ERR_BAD_ADDR)
        if addr == 0:
            self._erase_range(base, size)
        self._program(base + addr, data)
        self._persist()
        self._reply(cmd, addr)

    def _read(self, addr):
        if addr < VFO_ADDR:
            if addr % CHAN_SIZE or addr // CHAN_SIZE > MAX_CHAN:
                return self._error(addr, ERR_BAD_ADDR)
            return self._reply(READ, addr, bytes(self.nor[addr:addr + CHAN_SIZE]))
        if addr == VFO_ADDR:
            return self._reply(READ, addr, bytes(self.nor[addr:addr + 64]))
        if addr == SETTINGS_ADDR:
            return self._reply(READ, addr, normalize_settings(self.settings))
        if CONTACT_ADDR <= addr < CONTACT_ADDR + CONTACTS * CONTACT_SIZE and (addr - CONTACT_ADDR) % CONTACT_SIZE == 0:
            return self._reply(READ, addr, bytes(self.nor[addr:addr + CONTACT_SIZE]))
        if addr == FM_ADDR:
            return self._reply(READ, addr, bytes(self.nor[addr:addr + FM_BYTES]))
        self._error(addr, ERR_BAD_ADDR)

    def _write(self, addr, data):
        f = _faults()
        if addr < VFO_ADDR:                       # write_channel
            if addr % CHAN_SIZE or len(data) != CHAN_SIZE or addr // CHAN_SIZE > MAX_CHAN:
                return self._error(addr, ERR_BAD_ADDR)
            blk = bytearray(data)
            if f.get("corrupt_channel_write"):
                blk[5] ^= 0xFF
                _clear_fault("corrupt_channel_write")
                self._note("fault: corrupted channel write")
            # storage.rs write_channel: the first write into a 4 KB sector in a session erases the whole sector,
            # so 127 neighbouring channels vanish unless the host sends them again.
            sector = addr // SECTOR
            if sector not in self.erased_mask:
                self.erased_mask.add(sector)
                self._erase(sector * SECTOR)
            self._program(addr, blk)
        elif addr == VFO_ADDR and len(data) == 64:
            blk = bytearray(data)
            if f.get("corrupt_vfo_write"):
                blk[3] ^= 0xFF
                _clear_fault("corrupt_vfo_write")
                self._note("fault: corrupted vfo write")
            self.nor[addr:addr + 64] = blk       # plain record (the real flash keeps wear-levelled slots)
        elif addr == SETTINGS_ADDR and len(data) == SETTINGS_BYTES:
            blk = bytearray(normalize_settings(data))      # Settings::from_bytes clamps before it is stored
            if f.get("corrupt_settings_write"):
                blk[100] ^= 0xFF
                _clear_fault("corrupt_settings_write")
                self._note("fault: corrupted settings write")
            self.settings[:] = blk
            self._persist()
            self._note("settings write")
            return self._reply(WRITE, addr)
        elif (CONTACT_ADDR <= addr < CONTACT_ADDR + CONTACTS * CONTACT_SIZE
              and (addr - CONTACT_ADDR) % CONTACT_SIZE == 0 and len(data) == CONTACT_SIZE):
            blk = bytearray(data)
            if f.get("corrupt_contact_write"):
                blk[2] ^= 0xFF
                _clear_fault("corrupt_contact_write")
                self._note("fault: corrupted contact write")
            self._rmw(addr, bytes(blk))
        elif addr == FM_ADDR and len(data) == FM_BYTES:
            self.nor[addr:addr + FM_BYTES] = data
        else:
            return self._error(addr, ERR_BAD_ADDR)
        self.chan_writes += 1
        self._persist()
        self._reply(WRITE, addr)

    def _rmw(self, addr, record):
        """storage.rs rmw_sector_record: copy the sector to the scratch sector (patched), erase it, copy back."""
        s = addr - addr % SECTOR
        patched = bytearray(self.nor[s:s + SECTOR])
        patched[addr - s:addr - s + len(record)] = record
        self._erase(SCRATCH_ADDR)
        self._program(SCRATCH_ADDR, bytes(patched))
        self._erase(s)
        self._program(s, bytes(patched))
