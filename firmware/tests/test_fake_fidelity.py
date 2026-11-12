"""The test doubles against the firmware they stand in for (docs/TEST-DOUBLES.md has the full comparison).

  scripts/dev.sh tools-test        (or: python3 firmware/tests/test_fake_fidelity.py)

Each test here is a behaviour of the real firmware (per the source, UNVERIFIED on hardware) that an older fake did
not have and that a tool could therefore ship a bug against: a fake that is kinder than the radio hides the bug
(AUDIT 37, 31, 44). The tests were run against the previous fake first and fail there; set FAKE_SERIAL_PATH to try
another fake file: `git show 3a31439:firmware/tests/fakeserial/serial.py > /tmp/old_serial.py` then
`FAKE_SERIAL_PATH=/tmp/old_serial.py python3 firmware/tests/test_fake_fidelity.py`.
"""
import importlib.util, json, os, re, shutil, struct, sys, tempfile, time, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
SRC = os.path.join(FW, "firmware-source", "src")
FAKE = os.environ.get("FAKE_SERIAL_PATH", os.path.join(HERE, "fakeserial", "serial.py"))
BL_FAKE = os.path.join(HERE, "fakebootloader", "serial.py")


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def text(*parts):
    with open(os.path.join(*parts), encoding="utf-8") as f:
        return f.read()


class Radio(unittest.TestCase):
    """Drives the fake in process, frame by frame, the way a host tool does."""

    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="fidelity-")
        self._env = dict(os.environ)
        os.environ["FAKE_RADIO_DIR"] = self.dir
        self.fake = load(FAKE, "fake_radio_under_test")
        self.ports = []

    def tearDown(self):
        os.environ.clear()
        os.environ.update(self._env)
        shutil.rmtree(self.dir, ignore_errors=True)

    def port(self):
        p = self.fake.Serial("FAKE", timeout=0.01)
        self.ports.append(p)
        return p

    def frame(self, cmd, addr, data=b"", crc=None):
        body = struct.pack(">BHH", cmd, addr, len(data)) + data
        return bytes([0xA6]) + body + struct.pack(">H", self.fake.crc16(body) if crc is None else crc)

    def session(self):
        p = self.port()
        p.write(self.fake.HANDSHAKE)
        self.assertEqual(p.read(1), b"\x06", "handshake")
        return p

    def ask(self, p, cmd, addr, data=b"", **kw):
        p.reset_input_buffer()
        p.write(self.frame(cmd, addr, data, **kw))
        out = bytes(p._rx)
        if not out:
            return None
        n = struct.unpack(">H", out[4:6])[0]
        return out[1], struct.unpack(">H", out[2:4])[0], out[6:6 + n]

    def nor(self):
        with open(os.path.join(self.dir, "nor.bin"), "rb") as f:
            return f.read()


class Session(Radio):
    def test_a_frame_before_the_handshake_is_not_answered(self):
        p = self.port()
        self.assertIsNone(self.ask(p, 0x52, 0))          # READ channel 0 with no session: the standby screen ignores it

    def test_a_second_handshake_inside_a_session_is_not_answered(self):
        p = self.session()
        p.write(self.fake.HANDSHAKE)                      # cps/mod.rs: only the standby loop recognises it; here it is junk bytes
        self.assertEqual(p.read(1), b"")
        self.assertIsNotNone(self.ask(p, 0x52, 0))        # and the session is still fine

    def test_a_bad_crc_is_answered_with_error_1_not_silence(self):
        p = self.session()
        r = self.ask(p, 0x52, 0, crc=0x1234)
        self.assertEqual(r[0], 0xEE)
        self.assertEqual(r[2], b"\x01")                   # frame.rs ERR_BAD_CRC

    def test_an_oversized_frame_is_dropped_silently_and_the_reader_resyncs(self):
        p = self.session()
        self.assertIsNone(self.ask(p, 0x57, 0, b"\x00" * 1024))   # length > 512: no reply of any kind
        self.assertIsNotNone(self.ask(p, 0x52, 0))                 # the next header byte starts a clean frame

    def test_junk_before_a_header_is_skipped(self):
        p = self.session()
        p.write(b"\x00\x01\x02")
        self.assertIsNotNone(self.ask(p, 0x52, 0))

    def test_end_answers_then_the_radio_restarts_and_answers_nothing_more(self):
        p = self.session()
        self.assertEqual(self.ask(p, 0x45, 0)[0], 0x45)
        self.assertIsNone(self.ask(p, 0x52, 0))                    # sys_reset(): gone until power-up
        p.write(self.fake.HANDSHAKE)
        self.assertEqual(p.read(1), b"")

    def test_two_seconds_of_silence_restart_the_radio(self):
        os.environ["FAKE_RADIO_IDLE_S"] = "0.2"
        p = self.session()
        self.assertIsNotNone(self.ask(p, 0x52, 0))
        time.sleep(0.3)                                            # run_session: IDLE_TIMEOUT_MS -> SCB::sys_reset()
        self.assertIsNone(self.ask(p, 0x52, 0))

    def test_a_radio_still_restarting_ignores_the_handshake(self):
        with open(os.path.join(self.dir, "faults.json"), "w") as f:
            json.dump({"boot_delay_s": 0.3}, f)
        p = self.port()
        p.write(self.fake.HANDSHAKE)
        self.assertEqual(p.read(1), b"")
        time.sleep(0.35)
        p.write(self.fake.HANDSHAKE)
        self.assertEqual(p.read(1), b"\x06")

    def test_unknown_command_is_error_3(self):
        p = self.session()
        self.assertEqual(self.ask(p, 0x99, 0), (0xEE, 0, b"\x03"))

    def test_read_and_write_errors_use_the_firmware_codes(self):
        p = self.session()
        self.assertEqual(self.ask(p, 0x52, 33)[2], b"\x02")                 # channel address not a multiple of 32
        self.assertEqual(self.ask(p, 0x52, 999 * 32 + 32)[2], b"\x02")      # channel 1000 does not exist
        self.assertEqual(self.ask(p, 0x57, 1000 * 32, b"\x00" * 32)[2], b"\x02")
        self.assertEqual(self.ask(p, 0x57, 0x8000, b"\x00" * 63)[2], b"\x02")   # VFO is exactly 64 bytes


class Flash(Radio):
    def test_a_program_only_clears_bits_and_only_an_erase_sets_them(self):
        p = self.session()
        # logo flash starts as all zero bits here: writing 0xFF over it without the erase that offset 0 does stays zero
        self.assertEqual(self.ask(p, 0x6C, 512, b"\xff" * 512)[0], 0x6C)
        self.assertEqual(self.ask(p, 0x4C, 512)[2], b"\x00" * 512)
        # offset 0 erases the whole logo, then writes
        self.assertEqual(self.ask(p, 0x6C, 0, b"\xa5" * 512)[0], 0x6C)
        self.assertEqual(self.ask(p, 0x4C, 512)[2], b"\xff" * 512)
        self.assertEqual(self.ask(p, 0x6C, 512, b"\xf0" * 512)[0], 0x6C)
        self.assertEqual(self.ask(p, 0x6C, 512, b"\x0f" * 512)[0], 0x6C)     # a second program over the first
        self.assertEqual(self.ask(p, 0x4C, 512)[2], b"\x00" * 512)           # F0 AND 0F

    def test_logo_chunks_must_be_whole_and_aligned(self):
        p = self.session()
        self.assertEqual(self.ask(p, 0x6C, 0, b"\x00" * 256)[2], b"\x02")    # cps/mod.rs handle_write_logo
        self.assertEqual(self.ask(p, 0x6C, 100, b"\x00" * 512)[2], b"\x02")
        self.assertEqual(self.ask(p, 0x4C, 100)[2], b"\x02")

    def test_the_first_channel_write_of_a_session_erases_its_sector_and_each_session_does_it_again(self):
        p = self.session()
        for n in range(4):
            self.ask(p, 0x57, n * 32, bytes([n + 1]) * 32)
        self.ask(p, 0x45, 0)
        q = self.session()                                                   # a new power-up and handshake
        self.ask(q, 0x57, 0, b"\x77" * 32)                                   # only channel 0, in a fresh session
        nor = self.nor()
        self.assertEqual(nor[:32], b"\x77" * 32)
        self.assertEqual(nor[32:128], b"\xff" * 96, "channels 1-3 were erased with the sector")
        self.assertEqual(nor[4096:4100], b"\xff" * 4)

    def test_a_second_write_to_a_channel_in_the_same_session_cannot_set_bits_again(self):
        p = self.session()
        self.ask(p, 0x57, 0, b"\x0f" * 32)
        self.ask(p, 0x57, 0, b"\xf3" * 32)                                   # the sector is not erased a second time
        self.assertEqual(self.nor()[:32], b"\x03" * 32)

    def test_app_slots_need_an_erase_in_this_session_first(self):
        p = self.session()
        self.assertEqual(self.ask(p, 0x61, 0, b"\x11" * 512)[2], b"\x02")    # 'a' with no 'A': handle_app_write
        self.assertEqual(self.ask(p, 0x41, 9)[2], b"\x02")                   # nine slots: 0-8
        self.assertEqual(self.ask(p, 0x41, 2)[0], 0x41)
        self.assertEqual(self.ask(p, 0x61, 0, b"\x3c" * 512)[0], 0x61)
        self.assertEqual(self.nor()[0x14C000 + 2 * 0x8000:][:512], b"\x3c" * 512)
        self.assertEqual(self.ask(p, 0x61, 100, b"\x00" * 512)[2], b"\x02")

    def test_a_contact_write_goes_through_the_scratch_sector(self):
        p = self.session()
        self.ask(p, 0x57, 0xA020, b"\x12" * 16)
        nor = self.nor()
        self.assertEqual(nor[0xD000:0xE000], nor[0xA000:0xB000])
        self.assertEqual(nor[0xA020:0xA030], b"\x12" * 16)

    def test_every_table_chunk_after_the_first_is_a_program_not_an_overwrite(self):
        p = self.session()
        self.ask(p, 0x58, 0, b"\xf0" * 512)
        self.ask(p, 0x58, 512, b"\xf0" * 512)
        self.ask(p, 0x58, 512, b"\x0f" * 512)                                # the table was erased only by offset 0
        self.assertEqual(self.nor()[0x1E0000 + 512:][:512], b"\x00" * 512)
        self.assertEqual(self.ask(p, 0x58, 20480, b"\x00" * 512)[2], b"\x02")
        self.assertEqual(self.ask(p, 0x53, 5120, b"\x00" * 512)[2], b"\x02")  # satellite table is 5120 bytes


class Settings(Radio):
    def test_the_radio_clamps_a_settings_record_before_it_keeps_it(self):
        p = self.session()
        rec = bytearray(self.fake.default_settings())
        rec[7] = 0       # vox_level 1-9
        rec[4] = 9       # key_auto_lock 0-3
        rec[1] = 0x42    # a bool
        rec[14] = 0x80   # rit_offset -128 -> -127
        rec[32:36] = struct.pack("<i", 99_000_000)                          # latitude out of range -> not set
        self.assertEqual(self.ask(p, 0x57, 0x9000, bytes(rec))[0], 0x57)
        back = self.ask(p, 0x52, 0x9000)[2]
        self.assertEqual((back[7], back[4], back[1], back[14]), (1, 3, 1, 0x81))
        self.assertEqual(back[32:36], struct.pack("<i", 0x7FFFFFFF))
        self.assertNotEqual(back, bytes(rec), "a tool that writes an arbitrary record cannot expect it back unchanged")

    def test_a_blank_radio_reports_the_firmware_default_record(self):
        p = self.session()
        back = self.ask(p, 0x52, 0x9000)[2]
        self.assertEqual((back[0], back[3], back[8], back[7]), (3, 1, 8, 1))   # sql 3, tx_forbid on, tot 8, vox level 1

    def test_the_fake_clamp_table_is_the_one_in_flash_map_rs(self):
        src = text(SRC, "flash_map.rs")
        body = src[src.index("pub fn from_bytes(buf: &[u8; SETTINGS_BYTES])"):src.index("pub fn to_bytes(self) -> [u8; SETTINGS_BYTES]")]
        keyfn = int(re.search(r"pub const KEYFN_MAX: u8 = (\d+)", text(SRC, "app", "keyfn.rs")).group(1))
        want = {}
        for off, rule in re.findall(r"buf\[(\d+)\]\s*(!= 0|\.min\([^)]*\)|\.clamp\([^)]*\))", body):
            off = int(off)
            if off == 14:
                continue                                                      # rit_offset: signed, checked in the test above
            if rule == "!= 0":
                want[off] = ("bool",)
            elif rule.startswith(".min"):
                arg = rule[5:-1].strip()
                want[off] = ("min", keyfn if arg == "crate::app::KEYFN_MAX" else int(arg))
            else:
                lo, hi = (int(x) for x in rule[7:-1].split(","))
                want[off] = ("clamp", lo, hi)
        self.assertEqual(want, self.fake.SETTINGS_RULES)

    def test_the_fake_default_record_matches_settings_default_in_flash_map_rs(self):
        src = text(SRC, "flash_map.rs")
        d = src[src.index("pub const DEFAULT: Settings"):src.index("pub fn from_bytes(buf: &[u8; SETTINGS_BYTES])")]
        for field, off in (("sql_level", 0), ("vox_level", 7), ("tot_level", 8), ("rtone", 12), ("scan_mode", 13),
                           ("backlight_time", 16), ("channel_display_mode", 17), ("vox_delay", 20),
                           ("side1_short", 23), ("side2_short", 25), ("band_short", 27), ("band_lock", 29),
                           ("boot_display_mode", 30)):
            v = int(re.search(rf"\b{field}: (\d+)", d).group(1))
            self.assertEqual(self.fake.default_settings()[off], v, field)
        for field, off in (("tail_elimination", 1), ("tx_forbid", 3), ("beeps_switch", 9), ("boot_sound_enabled", 31)):
            self.assertEqual(self.fake.default_settings()[off], int(re.search(rf"\b{field}: (true|false)", d).group(1) == "true"), field)
        self.assertEqual(self.fake.default_settings()[21:23], struct.pack("<H", int(re.search(r"battery_cal_raw: (\d+)", d).group(1))))


class BootloaderFake(unittest.TestCase):
    """The closed vendor bootloader: what is known (six real flashes with flash_bf.py) and what is not."""

    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="blfidelity-")
        self._env = dict(os.environ)
        os.environ["FAKE_BL_DIR"] = self.dir
        self.bl = load(BL_FAKE, "fake_bl_under_test")

    def tearDown(self):
        os.environ.clear()
        os.environ.update(self._env)
        shutil.rmtree(self.dir, ignore_errors=True)

    def send(self, p, cmd, args, data=b""):
        body = bytes([cmd, args, len(data) >> 8, len(data) & 0xFF]) + data
        p.reset_input_buffer()
        p.write(bytes([0xAA]) + body + struct.pack(">H", self.bl.crc16(body)) + b"\xef")
        r = bytes(p._rx)
        return (r[1], r[2]) if r else None

    def test_a_lost_acknowledgement_is_a_choice_the_fake_makes_explicit(self):
        # Unknown for the real bootloader: when the host repeats a package whose ACK was lost, does it ACK again
        # ("ack") or answer E2, sequence error ("e2")? flash_bf.py retries the same frame five times.
        faults = os.path.join(self.dir, "faults.json")
        for policy, want in (("e2", (0xE2, 0)), ("ack", (0x03, 6))):
            shutil.rmtree(self.dir)
            os.mkdir(self.dir)
            with open(faults, "w") as f:
                json.dump({"duplicate_package": policy, "lose_ack_at": 0}, f)
            p = self.bl.Serial("FAKE")
            self.assertEqual(self.send(p, 1, 0, b"BOOTLOADER"), (1, 6))
            self.assertEqual(self.send(p, 4, 0, bytes([1])), (4, 6))
            self.assertIsNone(self.send(p, 3, 0, b"\x00" * 1024), "the package was taken but the ACK was lost")
            self.assertEqual(self.send(p, 3, 0, b"\x00" * 1024), want, policy)

    def test_a_package_that_is_not_one_kilobyte_is_refused(self):
        p = self.bl.Serial("FAKE")
        self.send(p, 1, 0, b"BOOTLOADER")
        self.send(p, 4, 0, bytes([1]))
        self.assertEqual(self.send(p, 3, 0, b"\x00" * 100), (0xE2, 0))


if __name__ == "__main__":
    unittest.main(verbosity=2)
