"""Tests for fm_stations.py (FM station names: CSV -> table -> the radio) against the fake radio.

  scripts/dev.sh tools-test        (or: python3 firmware/tests/test_fm_stations.py)

The table layout is written by hand in fm_stations.py and read by the app (apps/fm/src/logic.rs). Two things tie them
together: tests/fixtures/fm-stations.bin, built here from fm-stations.csv and checked against the committed file, which a
unit test in logic.rs parses; and the constants below, compared with the source files that repeat them.
UNVERIFIED against real hardware.
"""
import importlib.util, os, re, shutil, struct, subprocess, sys, tempfile, unittest, warnings

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
ROOT = os.path.dirname(FW)
TOOLS = os.environ.get("TOOLS_DIR", os.path.join(FW, "source-changes"))
FIXTURES = os.path.join(HERE, "fixtures")
NOR = 2 * 1024 * 1024
TBL, TBL_BYTES, HALF, REC = 0x1D8000, 0x2000, 0x1000, 32

spec = importlib.util.spec_from_file_location("fm_stations", os.path.join(TOOLS, "fm_stations.py"))
fm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fm)


def rb(p):
    with open(p, "rb") as f:
        return f.read()


def wt(p, text):
    with open(p, "w", encoding="utf-8") as f:
        f.write(text)


class Box(unittest.TestCase):
    def setUp(self):
        self.box = tempfile.mkdtemp(prefix="fmbox-")
        self.addCleanup(shutil.rmtree, self.box, ignore_errors=True)
        self.radio = os.path.join(self.box, "radio")
        os.mkdir(self.radio)
        for f in ("cps_tool.py", "fm_stations.py"):
            shutil.copy(os.path.join(TOOLS, f), self.box)
        shutil.copy(os.path.join(HERE, "fakeserial", "serial.py"), self.box)

    def tool(self, *args, ok=True):
        p = subprocess.run([sys.executable, os.path.join(self.box, "fm_stations.py"), *args], cwd=self.box,
                           env=dict(os.environ, FAKE_RADIO_DIR=self.radio), capture_output=True, text=True, timeout=300)
        if ok and p.returncode != 0:
            self.fail(f"{args} -> {p.returncode}\n{p.stdout}\n{p.stderr}")
        return p

    def csv(self, name, text):
        p = os.path.join(self.box, name)
        wt(p, text)
        return p

    def radio_with(self, nor_bytes=None):
        nor = bytearray(b"\xff" * NOR)
        nor[TBL - 0x1000:TBL] = b"\x11" * 0x1000                       # neighbours the tool must never touch
        nor[TBL + TBL_BYTES:TBL + TBL_BYTES + 0x1000] = b"\x22" * 0x1000
        nor[0xC000:0xC000 + 60] = bytes(range(60))                     # the core's own bare FM list
        for addr, blob in (nor_bytes or {}).items():
            nor[addr:addr + len(blob)] = blob
        with open(os.path.join(self.radio, "nor.bin"), "wb") as f:
            f.write(nor)
        self.nor0 = bytes(nor)

    def nor_now(self):
        return rb(os.path.join(self.radio, "nor.bin"))

    def log(self):
        p = os.path.join(self.radio, "log.txt")
        if not os.path.exists(p):
            return []
        with open(p) as f:
            return [l for l in f.read().split("\n") if l]

    def build(self, text="Name,Frequency,Detail\nKERA,90.1,Public Radio\nWRR,101.1,Classical\n", name="s.csv"):
        out = os.path.join(self.box, "s.fmst")
        self.tool("build", self.csv(name, text), "-o", out)
        return out


class Build(Box):
    def test_the_committed_fixture_is_what_the_tool_builds(self):
        out = os.path.join(self.box, "f.bin")
        self.tool("build", os.path.join(FIXTURES, "fm-stations.csv"), "-o", out)
        self.assertEqual(rb(out), rb(os.path.join(FIXTURES, "fm-stations.bin")),
                         "tests/fixtures/fm-stations.bin is stale: rebuild it with fm_stations.py build (the Rust unit test reads it)")

    def test_layout(self):
        t = rb(self.build())
        self.assertEqual(len(t), HALF)
        self.assertEqual(t[:8], b"FMST\x01\xff\x01\x00")                      # magic, version, reserved, generation 1
        self.assertEqual(t[8:31], b"\xff" * 23)
        self.assertEqual(t[31], 0xA5)
        self.assertEqual(t[32:34], struct.pack("<H", 901))
        self.assertEqual(t[34:46], b"KERA" + b"\xff" * 8)
        self.assertEqual(t[46:62], b"Public Radio" + b"\xff" * 4)
        self.assertEqual(t[62:64], b"\xff\xa5")
        self.assertEqual(t[64:66], struct.pack("<H", 1011))
        self.assertEqual(t[96:], b"\xff" * (HALF - 96), "everything after the last record is erased flash")

    def test_the_dallas_list_and_other_lists_in_channels_fm_build(self):
        d = os.path.join(ROOT, "channels", "fm")
        files = [f for f in os.listdir(d) if f.endswith(".csv")]
        self.assertTrue(files)
        for f in files:
            st = fm.read_csv(os.path.join(d, f))
            self.assertTrue(0 < len(st) <= 127, f)
            self.assertEqual(fm.parse_half(fm.make_table(st))[1], st, f)

    def test_a_chirp_export_with_other_columns_and_blank_names_is_accepted(self):
        head = "Location,Name,Frequency,Duplex,Offset,Tone,Mode,Detail,Comment\n"
        st = fm.read_csv(self.csv("c.csv", head + '1,KERA,90.100000,off,0,,WFM,Public Radio,"a, comment"\n2,,98.100000,off,0,,WFM,,\n'))
        self.assertEqual(st, [(901, "KERA", "Public Radio"), (981, "", "")])

    def test_bad_input_is_refused_and_writes_nothing(self):
        head = "Name,Frequency,Detail\n"
        for why, text, msg in (
            ("below the band", head + "A,87.4,\n", "outside 87.5 to 108.0"),
            ("above the band", head + "A,108.1,\n", "outside 87.5 to 108.0"),
            ("not a multiple of 0.1", head + "A,98.15,\n", "multiple of 0.1"),
            ("not a number", head + "A,FM,\n", "not a frequency"),
            ("name too long", head + "THIRTEEN CHRS,98.1,\n", "13 characters"),
            ("name not ASCII", head + "Zürich,98.1,\n", "printable ASCII"),
            ("detail too long", head + "A,98.1,SEVENTEEN CHARS 1\n", "limit is 16"),
            ("same frequency twice", head + "A,98.1,\nB,98.1,\n", "already on line 2"),
            ("no frequency column", "Name,Detail\nA,B\n", "no 'Frequency' column"),
            ("no stations", head, "no stations"),
            ("too many", head + "".join(f"S{i},{(875 + i) / 10:.1f},\n" for i in range(128)), "holds 127"),
        ):
            out = os.path.join(self.box, "x.fmst")
            p = self.tool("build", self.csv("bad.csv", text), "-o", out, ok=False)
            self.assertEqual(p.returncode, 1, why)
            self.assertIn(msg, p.stderr, why)
            self.assertFalse(os.path.exists(out), why)

    def test_127_stations_fit_exactly_and_an_existing_file_is_not_overwritten(self):
        rows = "".join(f"S{i},{(875 + i) / 10:.1f},d\n" for i in range(127))
        out = self.build("Name,Frequency,Detail\n" + rows)
        t = rb(out)
        self.assertEqual(len(t), HALF)
        self.assertEqual(t[127 * REC + 31], 0xA5)
        self.assertEqual(len(fm.parse_half(t)[1]), 127)
        p = self.tool("build", self.csv("s.csv", "Name,Frequency\nA,98.1\n"), "-o", out, ok=False)
        self.assertIn("REFUSING TO OVERWRITE", p.stderr)
        self.tool("build", self.csv("s.csv", "Name,Frequency\nA,98.1\n"), "-o", out, "--force")
        self.assertEqual(len(fm.parse_half(rb(out))[1]), 1)


class Install(Box):
    def only_table_touched(self):
        now = self.nor_now()
        self.assertEqual(now[:TBL], self.nor0[:TBL], "something below the table changed")
        self.assertEqual(now[TBL + TBL_BYTES:], self.nor0[TBL + TBL_BYTES:], "something above the table changed")
        for line in self.log():
            kind, _, addr = line.partition(" ")
            self.assertIn(kind, ("D", "F"), f"sent something else: {line}")
            if kind == "D":
                self.assertTrue(TBL <= int(addr, 16) < TBL + TBL_BYTES, f"read outside the table: {line}")
            else:
                self.assertTrue(0 <= int(addr, 16) < HALF and int(addr, 16) % 512 == 0, f"write offset outside the table: {line}")

    def test_install_writes_reads_back_and_touches_nothing_else(self):
        table = rb(self.build())
        self.radio_with()
        p = self.tool("install", "FAKE", os.path.join(self.box, "s.fmst"), "--before", os.path.join(self.box, "old.bin"))
        self.assertIn("written and verified: 2 stations", p.stdout)
        self.assertEqual(self.nor_now()[TBL:TBL + TBL_BYTES], table + b"\xff" * HALF)
        self.assertEqual(rb(os.path.join(self.box, "old.bin")), self.nor0[TBL:TBL + TBL_BYTES], "the radio's old table was saved first")
        self.only_table_touched()
        self.assertEqual([l for l in self.log() if l.startswith("F")], [f"F {o:#x}" for o in range(0, HALF, 512)])
        # the same again changes nothing
        os.remove(os.path.join(self.radio, "log.txt"))
        p = self.tool("install", "FAKE", os.path.join(self.box, "s.fmst"))
        self.assertIn("already has this table", p.stdout)
        self.assertFalse([l for l in self.log() if l.startswith("F")])
        shown = self.tool("show", os.path.join(self.box, "old.bin"), ok=False)
        self.assertNotEqual(shown.returncode, 0, "the radio had no table before")

    def test_install_replaces_a_table_the_app_changed_and_clears_the_other_half(self):
        old = fm.make_table([(881, "KNTU", "Alt"), (901, "KERA", "")])
        newer = bytearray(fm.make_table([(1011, "WRR", "")]))
        newer[6:8] = struct.pack("<H", 2)
        self.radio_with({TBL: old, TBL + HALF: bytes(newer)})
        self.build()
        p = self.tool("install", "FAKE", os.path.join(self.box, "s.fmst"), "--before", os.path.join(self.box, "old.bin"))
        self.assertIn("the radio has 1 stations; this table has 2", p.stdout)
        self.assertEqual(self.nor_now()[TBL + HALF:TBL + TBL_BYTES], b"\xff" * HALF, "the app's other half is cleared")
        self.assertEqual(fm.decode(rb(os.path.join(self.box, "old.bin")))[:2], (1, 2))
        self.only_table_touched()
        # --before refuses to overwrite a different file
        p = self.tool("install", "FAKE", os.path.join(self.box, "s.fmst"), "--before", os.path.join(self.box, "old.bin"), ok=False)
        self.assertIn("is not what the radio has now", p.stderr)

    def test_files_that_are_not_a_table_are_refused_before_the_radio_is_touched(self):
        self.radio_with()
        good = rb(self.build())
        marked = bytearray(good)
        marked[32 + 31] = 0x00                                          # record 1 loses its mark
        bad = {"short": good[:100], "long": good + b"\xff" * HALF, "no header": b"\xff" * HALF,
               "wrong magic": b"XXXX" + good[4:], "damaged record": bytes(marked)}
        for why, blob in bad.items():
            p = os.path.join(self.box, "bad.fmst")
            with open(p, "wb") as f:
                f.write(blob)
            r = self.tool("install", "FAKE", p, ok=False)
            self.assertNotEqual(r.returncode, 0, why)
        self.assertFalse(self.log(), "the radio was never spoken to")
        self.assertEqual(self.nor_now(), self.nor0)

    def test_a_corrupt_write_is_caught_by_the_read_back(self):
        self.radio_with()
        self.build()
        # a fake that flips one byte of the second chunk as it stores it: patch the copy in the box
        src = rb(os.path.join(self.box, "serial.py")).decode()
        src = src.replace("self.nor[base + addr:base + addr + 512] = data\n            self._persist()\n            return self._reply(cmd, addr)\n        if cmd == READ_LOGO",
                          "self.nor[base + addr:base + addr + 512] = data\n            if addr == 512:\n                self.nor[base + addr] ^= 0xFF\n            self._persist()\n            return self._reply(cmd, addr)\n        if cmd == READ_LOGO")
        self.assertIn("addr == 512", src, "the fake changed: update this test")
        wt(os.path.join(self.box, "serial.py"), src)
        p = self.tool("install", "FAKE", os.path.join(self.box, "s.fmst"), ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("VERIFY FAILED", p.stderr)


class Port(Box):
    """What the fake radio does with the F command is what cps/mod.rs `handle_table_write` does."""

    def setUp(self):
        super().setUp()
        warnings.simplefilter("ignore", ResourceWarning)    # the fake leaves its state files to the garbage collector

    def frame(self, cmd, addr, data):
        sys.path.insert(0, os.path.join(HERE, "fakeserial"))
        import serial as fake  # noqa: E402
        body = struct.pack(">BHH", cmd, addr, len(data)) + data
        return bytes([0xA6]) + body + struct.pack(">H", fake.crc16(body)), fake

    def test_the_port_refuses_writes_outside_the_table_and_ones_that_are_not_whole_chunks(self):
        self.radio_with()
        os.environ["FAKE_RADIO_DIR"] = self.radio
        for addr, data in ((0x2000, b"\x00" * 512), (100, b"\x00" * 512), (0, b"\x00" * 100), (0, b"\x00" * 1024),
                           (0xFE00, b"\x00" * 512)):
            frame, fake = self.frame(0x46, addr, data)
            s = fake.Serial("FAKE")
            s.write(frame)
            reply = bytes(s._rx)
            self.assertEqual(reply[1], 0xEE, f"{addr:#x}+{len(data)} must be refused")
        self.assertEqual(self.nor_now(), self.nor0)

    def test_the_first_chunk_erases_both_halves_and_only_them(self):
        self.radio_with({TBL: b"\x00" * 64, TBL + HALF + 100: b"\x00" * 8})
        os.environ["FAKE_RADIO_DIR"] = self.radio
        frame, fake = self.frame(0x46, 0, b"\xab" * 512)
        s = fake.Serial("FAKE")
        s.write(frame)
        now = self.nor_now()
        self.assertEqual(now[TBL:TBL + 512], b"\xab" * 512)
        self.assertEqual(now[TBL + 512:TBL + TBL_BYTES], b"\xff" * (TBL_BYTES - 512))
        self.assertEqual(now[:TBL], self.nor0[:TBL])
        self.assertEqual(now[TBL + TBL_BYTES:], self.nor0[TBL + TBL_BYTES:])


class Constants(unittest.TestCase):
    """The table's address and shape are written in five places; they must agree."""

    def src(self, *parts):
        with open(os.path.join(FW, *parts), encoding="utf-8") as f:
            return f.read()

    def num(self, text, name):
        m = re.search(rf"const {name}: \w+ = (0x[0-9A-Fa-f_]+|\d+)", text)
        self.assertTrue(m, name)
        return int(m.group(1).replace("_", ""), 0)

    def test_app_core_tool_and_fake_agree(self):
        app = self.src("firmware-source", "apps", "fm", "src", "logic.rs")
        core = self.src("firmware-source", "src", "flash_map.rs")
        fake = self.src("tests", "fakeserial", "serial.py")
        frame = self.src("firmware-source", "src", "cps", "frame.rs")
        self.assertEqual(self.num(app, "TBL_ADDR"), fm.ADDR)
        self.assertEqual(self.num(app, "TBL_BYTES"), fm.SIZE)
        self.assertEqual(self.num(app, "SLOTS"), fm.SLOTS)
        self.assertEqual(self.num(app, "REC"), fm.REC)
        self.assertEqual(self.num(app, "LO"), fm.LO)
        self.assertEqual(self.num(app, "HI"), fm.HI)
        self.assertEqual(self.num(core, "FM_STATION_ADDR"), fm.ADDR)
        self.assertEqual(self.num(core, "FM_STATION_TABLE"), fm.SIZE)
        self.assertEqual(self.num(frame, "CMD_FM_WRITE"), fm.CMD_FM_WRITE)
        self.assertIn("base, size = 0x1D8000, 0x2000", fake)
        self.assertIn("cmd == 0x46", fake)
        self.assertEqual(fm.ADDR % 0x1000, 0)
        # clear of the neighbours listed in flash_map.rs's map: FSK PROBE capture to 0x1D0000, extras from 0x1E0000
        self.assertTrue(0x1D0000 <= fm.ADDR and fm.ADDR + fm.SIZE <= 0x1E0000)


if __name__ == "__main__":
    unittest.main()
