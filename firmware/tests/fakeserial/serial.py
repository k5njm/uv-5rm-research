"""Fake `serial` module: a software UV-5RM custom-firmware CPS radio, for host tests.

Drop this file next to the script under test (sys.path[0]) in place of pyserial.
State persists in the directory named by $FAKE_RADIO_DIR (logo.bin, settings.bin,
faults.json) so multi-step flows (backup, upload, restore) work across processes.

Emulates only what source-changes/cps_tool.py frames: handshake, READ_LOGO/WRITE_LOGO,
READ/WRITE of the 168-byte settings record at 0x9000, END. Frame layout matches
cps_tool.Cps (A6 cmd addr16 len16 data crc16-xmodem).

Also models the 2 MiB SPI flash in nor.bin: channel read/write (32-byte slots 0..999 below 0x8000),
VFO (0x8000, 64 bytes, read and write), 20 DTMF contacts (16 bytes at 0xA020, read and write) and the raw read 'D' (64-byte chunks, addr*64) over the whole chip. The logo
(0xC0000) and settings (0x9000) overlay their own files. Every command is appended to log.txt:
"R/W/D <addr>" (and "F <offset>" for the FM station table) so tests can assert exactly what a tool touched (e.g. calibration at 0xF000 is never written).

Fault injection ($FAKE_RADIO_DIR/faults.json, consumed once each unless noted):
  {"corrupt_channel_write": true}   flip one byte of the next channel write
  {"corrupt_vfo_write": true}       flip one byte of the next VFO write
  {"corrupt_contact_write": true}   flip one byte of the next DTMF contact write
  {"die_after_writes": N}           (persistent) stop answering after N channel/VFO/contact writes: a pulled cable
  {"corrupt_logo_write": true}      flip one byte of the next full logo write
  {"corrupt_settings_write": true}  flip one byte of the next settings write
  {"silent": true}                  never answer the handshake
  {"corrupt_app_write": true}       flip one byte of the next app write ('a')
  {"corrupt_extra_write": true}     flip one byte of the next extras-table chunk ('X')
  {"corrupt_fm_write": true}        flip one byte of the next FM station chunk ('F')
  {"corrupt_read_once": true}       flip one byte of the next raw read ('D') reply: a read that does not repeat

App slots (cps/mod.rs handle_app_erase / handle_app_write): 'A' (addr = slot 0-8) erases the slot's 32 KB at 0x14C000 + slot * 0x8000
and selects it for this session; 'a' (addr = offset) writes exactly 512 bytes at a 512-aligned offset inside the selected slot, and
answers error 02 with no slot selected. Flash programming can only clear bits (norflash write_bytes does not erase first), so the
fake ANDs the data into what is there: a write into a slot that was not erased shows up as wrong bytes, as on the chip.
Every command that writes is logged: "A <slot>", "a <slot> <offset>", "X <offset>", "l <offset>", "W", "F".
"""
import json, os, struct

HEADER, ACK, ERR = 0xA6, 0x06, 0xEE
READ_LOGO, WRITE_LOGO, END, READ, WRITE = 0x4C, 0x6C, 0x45, 0x52, 0x57
LOGO_SIZE, SETTINGS_ADDR, SETTINGS_BYTES = 160 * 128 * 2, 0x9000, 168
READ_RAW, NOR_SIZE, LOGO_ADDR, VFO_ADDR, CHAN_SIZE, MAX_CHAN = 0x44, 2 * 1024 * 1024, 0xC0000, 0x8000, 32, 999
CONTACT_ADDR, CONTACT_SIZE, CONTACTS = 0xA020, 16, 20
APP_ERASE, APP_WRITE, APP_BASE, APP_SLOT, APP_SLOTS, APP_CHUNK = 0x41, 0x61, 0x14C000, 32 * 1024, 9, 512
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
        self.erased_sectors = set()   # channel sectors erased in this session
        self.timeout = timeout
        self.is_open = True
        self.dtr = self.rts = True
        self._rx = bytearray()   # bytes the host will read
        self._tx = bytearray()   # bytes the host wrote
        self.logo = bytearray(_load("logo.bin", b"\x00" * LOGO_SIZE))
        self.settings = bytearray(_load("settings.bin", bytes(range(SETTINGS_BYTES))))
        self.nor = bytearray(_load("nor.bin", b"\xff" * NOR_SIZE))
        self.chan_writes = 0
        self.app_slot = None          # selected by 'A', per session (cps/mod.rs app_slot)
        self.log = os.path.join(_dir(), "log.txt")

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
        n = _faults().get("die_after_writes")
        if n is not None and self.chan_writes >= n:
            self._note("fault: radio went silent (cable pulled)")
            return
        if cmd in (READ, WRITE, READ_RAW):
            self._note(f"{chr(cmd)} {addr:#x}" if cmd != READ_RAW else f"D {addr * 64:#x}")
        if cmd == READ_RAW:
            a = addr * 64
            if a + 64 > NOR_SIZE:
                return self._reply(ERR, addr, b"\x02")
            chunk = bytearray(self.nor[a:a + 64])
            if _faults().get("corrupt_read_once"):
                chunk[0] ^= 0xFF
                _clear_fault("corrupt_read_once")
                self._note("fault: corrupted a raw read")
            for base, blob in ((SETTINGS_ADDR, self.settings), (LOGO_ADDR, self.logo)):
                lo, hi = max(a, base), min(a + 64, base + len(blob))
                if lo < hi:  # overlay the part of the chunk covered by that region
                    chunk[lo - a:hi - a] = blob[lo - base:hi - base]
            return self._reply(cmd, addr, bytes(chunk))
        if cmd == READ and addr < VFO_ADDR and addr % CHAN_SIZE == 0 and addr // CHAN_SIZE <= MAX_CHAN:
            return self._reply(cmd, addr, bytes(self.nor[addr:addr + CHAN_SIZE]))
        if cmd == READ and addr == VFO_ADDR:
            return self._reply(cmd, addr, bytes(self.nor[addr:addr + 64]))
        if cmd == READ and CONTACT_ADDR <= addr < CONTACT_ADDR + CONTACTS * CONTACT_SIZE and (addr - CONTACT_ADDR) % CONTACT_SIZE == 0:
            return self._reply(cmd, addr, bytes(self.nor[addr:addr + CONTACT_SIZE]))
        if cmd == WRITE and addr == VFO_ADDR and len(data) == 64:
            blk = bytearray(data)
            if _faults().get("corrupt_vfo_write"):
                blk[3] ^= 0xFF
                _clear_fault("corrupt_vfo_write")
                self._note("fault: corrupted vfo write")
            self.nor[addr:addr + 64] = blk
            self.chan_writes += 1
            self._persist()
            return self._reply(cmd, addr)
        if cmd == WRITE and CONTACT_ADDR <= addr < CONTACT_ADDR + CONTACTS * CONTACT_SIZE and (addr - CONTACT_ADDR) % CONTACT_SIZE == 0 and len(data) == CONTACT_SIZE:
            blk = bytearray(data)
            if _faults().get("corrupt_contact_write"):
                blk[2] ^= 0xFF
                _clear_fault("corrupt_contact_write")
                self._note("fault: corrupted contact write")
            self.nor[addr:addr + CONTACT_SIZE] = blk
            self.chan_writes += 1
            self._persist()
            return self._reply(cmd, addr)
        if cmd == WRITE and addr < VFO_ADDR:
            if addr % CHAN_SIZE or len(data) != CHAN_SIZE or addr // CHAN_SIZE > MAX_CHAN:
                return self._reply(ERR, addr, b"\x02")
            blk = bytearray(data)
            if _faults().get("corrupt_channel_write"):
                blk[5] ^= 0xFF
                _clear_fault("corrupt_channel_write")
                self._note("fault: corrupted channel write")
            # As the firmware (storage.rs write_channel): the first write into a 4 KB sector in a session erases
            # the whole sector, so 127 neighbouring channels vanish unless the host sends them again.
            sector = addr // 4096
            if sector not in self.erased_sectors:
                self.erased_sectors.add(sector)
                self.nor[sector * 4096:(sector + 1) * 4096] = b"\xff" * 4096
            self.nor[addr:addr + CHAN_SIZE] = blk
            self.chan_writes += 1
            self._persist()
            return self._reply(cmd, addr)
        if cmd == APP_ERASE:
            if addr >= APP_SLOTS:
                return self._reply(ERR, addr, b"\x02")
            self._note(f"A {addr}")
            self.app_slot = addr
            lo = APP_BASE + addr * APP_SLOT
            self.nor[lo:lo + APP_SLOT] = b"\xff" * APP_SLOT
            self._persist()
            return self._reply(cmd, addr)
        if cmd == APP_WRITE:
            if self.app_slot is None or addr % APP_CHUNK or len(data) != APP_CHUNK or addr + APP_CHUNK > APP_SLOT:
                return self._reply(ERR, addr, b"\x02")
            self._note(f"a {self.app_slot} {addr}")
            blk = bytearray(data)
            if _faults().get("corrupt_app_write"):
                blk[7] ^= 0x10
                _clear_fault("corrupt_app_write")
                self._note("fault: corrupted app write")
            lo = APP_BASE + self.app_slot * APP_SLOT + addr
            for i, b in enumerate(blk):
                self.nor[lo + i] &= b          # NOR programming clears bits only
            self._persist()
            return self._reply(cmd, addr)
        if cmd == 0x58:  # channel extras table: 512-byte chunks, the chunk at offset 0 erases the table
            base, size = 0x1E0000, 20480
            if addr % 512 or len(data) != 512 or addr + 512 > size:
                return self._reply(ERR, addr, b"\x02")
            self._note(f"X {addr:#x}")
            if addr == 0:
                self.nor[base:base + size] = b"\xff" * size
            data = bytearray(data)
            if _faults().get("corrupt_extra_write"):
                data[3] ^= 0x01
                _clear_fault("corrupt_extra_write")
                self._note("fault: corrupted extras write")
            self.nor[base + addr:base + addr + 512] = data
            self._persist()
            return self._reply(cmd, addr)
        if cmd == 0x46:  # FM station table (cps/mod.rs CMD_FM_WRITE): two 4 KB halves, the chunk at offset 0 erases both
            self._note(f"F {addr:#x}")
            base, size = 0x1D8000, 0x2000
            if addr % 512 or len(data) != 512 or addr + 512 > size:
                return self._reply(ERR, addr, b"\x02")
            if addr == 0:
                self.nor[base:base + size] = b"\xff" * size
            data = bytearray(data)
            if _faults().get("corrupt_fm_write"):
                data[5] ^= 0x01
                _clear_fault("corrupt_fm_write")
                self._note("fault: corrupted fm write")
            self.nor[base + addr:base + addr + 512] = data
            self._persist()
            return self._reply(cmd, addr)
        if cmd == READ_LOGO:
            self._reply(cmd, addr, bytes(self.logo[addr:addr + 512]))
        elif cmd == WRITE_LOGO:
            if addr + len(data) > LOGO_SIZE:
                return self._reply(ERR, addr, b"\x02")
            self._note(f"l {addr:#x}")
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
