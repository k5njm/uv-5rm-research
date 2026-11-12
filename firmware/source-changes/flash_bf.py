"""Send a one-region .BF to the KDH bootloader. Same frame sequence as tools/flash.py in bf-5rh-firmware-open.

  flash_bf.py FILE.BF PORT SHA256_PREFIX --board BOARD --variant default|bk4819 [--check-only]

BOARD is the text the radio shows when you hold 8 while powering on (e.g. 5RHNRFB-YP07). It must be
one of the boards that VARIANT supports (firmware/variants.json); otherwise nothing is opened or sent.
--check-only validates the file and the board gate and exits without touching the serial port.
The SHA must match the file; that and the board text are what stop a wrong image reaching a radio.
UNVERIFIED on real hardware.
"""
import hashlib, json, os, struct, sys, time

_args = sys.argv[1:]
CHECK_ONLY = "--check-only" in _args
_args = [a for a in _args if a != "--check-only"]


def _opt(name):
    if name in _args:
        i = _args.index(name)
        v = _args[i + 1] if i + 1 < len(_args) else None
        del _args[i:i + 2]
        return v
    return None


BOARD, VARIANT = _opt("--board"), _opt("--variant")
if len(_args) != 3 or not BOARD or not VARIANT:
    raise SystemExit(__doc__)
BF, PORT, EXPECT_SHA = _args
with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "variants.json")) as _f:
    _VARIANTS = json.load(_f)
if VARIANT not in _VARIANTS:
    raise SystemExit(f"unknown variant {VARIANT!r}; choose one of {sorted(_VARIANTS)}")
if BOARD not in _VARIANTS[VARIANT]["boards"]:
    raise SystemExit(f"BOARD GATE: variant {VARIANT!r} supports {_VARIANTS[VARIANT]['boards']}, not {BOARD!r}. "
                     "Nothing was opened or sent. Hold 8 while powering on and type exactly what the screen shows.")
HEADER, FOOTER = 0xAA, 0xEF
CMD_HANDSHAKE, CMD_UPDATE, CMD_PACKAGES, CMD_END = 0x01, 0x03, 0x04, 0x45
ACK, CHUNK, RETRIES, TIMEOUT = 0x06, 1024, 5, 2.0
ERRORS = {0xE1: "handshake/command", 0xE2: "sequence or length", 0xE3: "flash write failed", 0xE5: "unknown command"}


def crc16(data):
    crc = 0
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) if crc & 0x8000 else crc << 1
        crc &= 0xFFFF
    return crc


def pack(cmd, args, data):
    body = bytes([cmd, args, len(data) >> 8, len(data) & 0xFF]) + data
    c = crc16(body)
    return bytes([HEADER]) + body + bytes([c >> 8, c & 0xFF, FOOTER])


raw = open(BF, "rb").read()
sha = hashlib.sha256(raw).hexdigest()
assert sha.startswith(EXPECT_SHA), f"file hash {sha[:16]} != {EXPECT_SHA}"
count, len_fw, len_data = struct.unpack_from(">BLL", raw, 0)
assert count == 1 and len_data == 0 and len(raw) == 16 + len_fw and 0 < len_fw <= 0xF000, (count, len_fw, len_data)
region = raw[16:]
assert region[3] == 0x20 and region[7] == 0x08, "vector table doesn't look like an application"
pkgs = -(-len_fw // CHUNK)
print(f"file ok: sha {sha[:16]}, {len_fw} bytes, {pkgs} packages; board {BOARD} accepted for {VARIANT}")
if CHECK_ONLY:
    print("check only: serial port not opened")
    sys.exit(0)
import serial

port = serial.Serial(PORT, 115200, timeout=0.03)
port.dtr = True
port.rts = True


def read_frame(deadline):
    buf = bytearray()
    while time.monotonic() < deadline:
        buf += port.read(64)
        while buf and buf[0] != HEADER:
            buf.pop(0)
        if len(buf) >= 5:
            n = buf[3] << 8 | buf[4]
            if len(buf) >= 8 + n:
                body = bytes(buf[1:5 + n])
                got = buf[5 + n] << 8 | buf[6 + n]
                return ("frame", body[0], body[1]) if got == crc16(body) else ("bad-crc",)
    return ("timeout", bool(buf))


def send(cmd, args, data, retries=RETRIES, tolerate_silence=False):
    last, heard = "no attempts", False
    frame = pack(cmd, args, data)
    for _ in range(retries):
        port.reset_input_buffer()
        port.write(frame)
        r = read_frame(time.monotonic() + TIMEOUT)
        if r[0] == "frame":
            heard = True
            if r[2] == ACK:
                return True
            name = ERRORS.get(r[1], f"code {r[1]:#x}")
            if r[1] != 0xE2:
                raise RuntimeError(f"radio refused cmd {cmd:#x}: {name}")
            last = name
        elif r[0] == "timeout":
            heard = heard or r[1]
            last = "timeout"
        else:
            heard, last = True, "bad crc"
    if tolerate_silence and not heard:
        return False
    raise RuntimeError(f"cmd {cmd:#x} failed after {retries} tries: {last}")


try:
    send(CMD_HANDSHAKE, 0, b"BOOTLOADER")
    print("handshake ok; writing starts now")
    send(CMD_PACKAGES, 0, bytes([pkgs]))
    start = time.monotonic()
    for seq in range(pkgs):
        chunk = region[seq * CHUNK:(seq + 1) * CHUNK].ljust(CHUNK, b"\x00")
        send(CMD_UPDATE, seq & 0xFF, chunk)
        if (seq + 1) % 10 == 0 or seq + 1 == pkgs:
            print(f"  {seq + 1}/{pkgs}")
    print(f"sent all packages in {time.monotonic() - start:.1f}s")
    acked = send(CMD_END, 0, b"", retries=2, tolerate_silence=True)
    print("end acknowledged" if acked else "end sent; radio likely rebooted before replying")
    print("FLASH COMPLETE")
except Exception as e:
    print(f"FLASH FAILED: {e}")
    sys.exit(1)
finally:
    port.close()
