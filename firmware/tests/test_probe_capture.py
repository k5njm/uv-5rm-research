"""Tests for probe_capture.py (download of FSK PROBE's flash capture) against the fake radio.

  scripts/dev.sh tools-test        (or: python3 firmware/tests/test_probe_capture.py)

The capture layout here is written by hand from the comment in apps/fskprobe/src/logic.rs, not by the app; the
simulator scenario fskprobe_capture and the FSK PROBE checks in the simulator suite check the app's side of the same
layout. Both header versions are read: "FSKCAP1" (the first probe) and "FSKCAP2" (options and registers recorded).
UNVERIFIED against real hardware.
"""
import os, shutil, struct, subprocess, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TOOLS = os.environ.get("TOOLS_DIR", os.path.join(os.path.dirname(HERE), "source-changes"))
NOR = 2 * 1024 * 1024
CAP, CAP_BYTES, PAGE, REC = 0x1B0000, 0x20000, 256, 8
MAX_RECORDS = (CAP_BYTES - PAGE) // REC


def rd(p, mode="r"):
    with open(p, mode) as f:
        return f.read()


def header(t0=1000):
    return (b"FSKCAP1\0" + struct.pack("<IHHHHIB", 404_800_000, 4800, 0x086D, 0x5388, 0xC195, t0, REC)).ljust(PAGE, b"\xff")


def header2(t0=1000, opts=0x10, per_irq=1, regs=(0x3002, 0x0309, 0x1008, 0x5625, 0x3B20, 0x0201, 0x00E0), frame_words=158):
    return (b"FSKCAP2\0" + struct.pack("<IHHHHIB", 404_800_000, 4800, 0x086D, 0x5388, 0xC195, t0, REC)
            + struct.pack("<BBB7HH", 2, opts, per_irq, *regs, frame_words)).ljust(PAGE, b"\xff")


def rec(t, word, flags, rssi=70):
    return struct.pack("<IHBB", t, word, flags, rssi)


def records(n, t0=1000):
    out = bytearray()
    for i in range(n):
        out += struct.pack("<IHBB", t0 + 30 + i * 33, (i * 0x9E37) & 0xFFFF, 3 if i % 160 == 0 else 2, 70 + i % 5)
    return bytes(out)


class Box(unittest.TestCase):
    def setUp(self):
        self.box = tempfile.mkdtemp(prefix="capbox-")
        self.radio = os.path.join(self.box, "radio")
        os.mkdir(self.radio)
        for f in ("cps_tool.py", "probe_capture.py"):
            shutil.copy(os.path.join(TOOLS, f), self.box)
        shutil.copy(os.path.join(HERE, "fakeserial", "serial.py"), self.box)
        self.out = os.path.join(self.box, "cap.bin")

    def tearDown(self):
        shutil.rmtree(self.box, ignore_errors=True)

    def radio_with(self, capture):
        nor = bytearray(b"\xff" * NOR)
        nor[CAP - 0x1000:CAP] = b"\x11" * 0x1000                       # neighbours the tool must never read
        nor[CAP + CAP_BYTES:CAP + CAP_BYTES + 0x1000] = b"\x22" * 0x1000
        nor[CAP:CAP + len(capture)] = capture
        with open(os.path.join(self.radio, "nor.bin"), "wb") as f:
            f.write(nor)
        self.nor0 = bytes(nor)

    def run_tool(self, *args, ok=True):
        env = dict(os.environ, FAKE_RADIO_DIR=self.radio)
        p = subprocess.run([sys.executable, os.path.join(self.box, "probe_capture.py"), *args],
                           cwd=self.box, env=env, capture_output=True, text=True, timeout=600)
        if ok and p.returncode != 0:
            self.fail(f"{args} -> {p.returncode}\n{p.stdout}\n{p.stderr}")
        return p

    def reads(self):
        p = os.path.join(self.radio, "log.txt")
        return [l for l in (rd(p).split("\n") if os.path.exists(p) else []) if l]

    def assert_only_read_the_capture(self):
        log = self.reads()
        self.assertTrue(log)
        for line in log:
            kind, _, addr = line.partition(" ")
            self.assertEqual(kind, "D", f"sent something other than a raw read: {line}")
            self.assertTrue(CAP <= int(addr, 16) < CAP + CAP_BYTES, f"read outside the capture range: {line}")
        self.assertEqual(rd(os.path.join(self.radio, "nor.bin"), "rb"), self.nor0, "the radio's flash changed")

    def test_downloads_a_capture_and_stops_at_the_first_erased_record(self):
        self.radio_with(header() + records(100))
        p = self.run_tool("fake", self.out)
        blob = rd(self.out, "rb")
        self.assertEqual(len(blob), PAGE * 5, "header page and the four pages holding 100 records")
        self.assertEqual(blob, self.nor0[CAP:CAP + PAGE * 5])
        self.assertIn("404.800000 MHz, 4800 baud", p.stdout)
        self.assertIn("100 records of 16352 (not full): 1 sync, 100 words", p.stdout)
        self.assertIn("header version 1: the configuration was not recorded", p.stdout)
        self.assertIn("median 3.3 ms", p.stdout)
        csv = rd(os.path.join(self.box, "cap.csv")).split("\n")
        self.assertEqual(csv[0], "time_100us,gap_100us,word,sync,done,rssi_dbm,pos,neg,frame,tail")
        self.assertEqual(csv[1], "1030,30,0000,1,0,-90,0,0,0,0")
        self.assertEqual(csv[2], "1063,33,9E37,0,0,-89,0,0,0,0")
        self.assertEqual(len([l for l in csv if l]), 101)
        self.assert_only_read_the_capture()
        self.assertEqual(len(self.reads()), 6 * 4, "five pages kept and the erased one that ended it, 4 reads each")

    def test_a_full_capture_ends_at_the_end_of_the_range(self):
        self.radio_with(header() + records(MAX_RECORDS))
        p = self.run_tool("fake", self.out)
        self.assertEqual(os.path.getsize(self.out), CAP_BYTES)
        self.assertIn("16352 records of 16352 (FULL)", p.stdout)
        self.assert_only_read_the_capture()

    def test_no_capture_on_the_radio_writes_nothing(self):
        self.radio_with(b"")
        p = self.run_tool("fake", self.out, ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("NO CAPTURE", p.stderr)
        self.assertFalse(os.path.exists(self.out))
        self.assert_only_read_the_capture()
        self.assertEqual(len(self.reads()), 4, "only the header page was read")

    def test_a_capture_with_no_records_is_reported_as_such(self):
        self.radio_with(header())
        p = self.run_tool("fake", self.out)
        self.assertIn("no records", p.stdout)
        self.assertEqual(os.path.getsize(self.out), PAGE)

    def test_refuses_to_overwrite_and_decodes_without_a_radio(self):
        self.radio_with(header() + records(40))
        self.run_tool("fake", self.out)
        again = self.run_tool("fake", self.out, ok=False)
        self.assertIn("REFUSING TO OVERWRITE", again.stderr)
        os.remove(os.path.join(self.radio, "log.txt"))
        p = self.run_tool("--decode", self.out)
        self.assertIn("40 records of 16352", p.stdout)
        self.assertFalse(os.path.exists(os.path.join(self.radio, "log.txt")), "decode must not touch the radio")
        with open(os.path.join(self.box, "junk.bin"), "wb") as f:
            f.write(b"\x00" * 600)
        bad = self.run_tool("--decode", os.path.join(self.box, "junk.bin"), ok=False)
        self.assertIn("not an FSK PROBE capture", bad.stderr)


    def test_a_version_2_capture_says_how_it_was_made_and_splits_into_frames(self):
        recs = rec(1030, 0, 1 | 8)                                           # sync, matched as sent
        recs += b"".join(rec(1063 + 33 * i, 0x1000 + i, 2 | (32 if i == 157 else 0)) for i in range(158))   # a whole frame
        recs += rec(9000, 0, 1 | 16)                                         # a sync matched inverted ...
        recs += b"".join(rec(9033 + 33 * i, 0x2000 + i, 2 | (4 if i == 127 else 0)) for i in range(128))   # ... and the chip's done
        recs += rec(20000, 0, 1) + rec(20033, 0x3000, 2)                    # a sync and one word, then nothing: cut off
        self.radio_with(header2(opts=0x01 | 1 << 2 | 1 << 3 | 1 << 5, per_irq=4) + recs)
        p = self.run_tool("fake", self.out)
        self.assertIn("options 0x2d: length 256 bytes (REG_5D 0x3b20), threshold 4 word(s) per interrupt, invert on, "
                      "RX bandwidth 1.2K, sync 2 bytes; frame = 158 words", p.stdout)
        self.assertIn("registers as written: REG_3F 3002 REG_58 0309 REG_59 1008 REG_5C 5625 REG_5D 3B20 REG_5E 0201 REG_70 00E0", p.stdout)
        self.assertIn("syncs: 3 (1 as sent, 1 inverted); frames ended by count 1, by the chip's done flag 1, by the next sync 0, cut off 1", p.stdout)
        self.assertIn("words per frame: min 1, median 128, max 158", p.stdout)
        csv = rd(os.path.join(self.box, "cap.csv")).split("\n")
        self.assertEqual(csv[1], "1030,30,,1,0,-90,1,0,0,0")
        self.assertEqual(csv[2 + 156], f"{1063 + 33 * 156},33,{0x1000 + 156:04X},0,0,-90,0,0,0,0")
        self.assertEqual(csv[2 + 157], f"{1063 + 33 * 157},33,{0x1000 + 157:04X},0,0,-90,0,0,1,0")
        self.assert_only_read_the_capture()

    def test_the_first_real_capture_still_decodes(self):
        # What the radio recorded on noise: a sync, and 119.7 ms later one word with the done flag.
        t = 28100
        self.radio_with(header(t0=1000) + rec(t, 0, 1, 46) + rec(t + 1197, 0xFCB7, 2 | 4, 46))
        p = self.run_tool("fake", self.out)
        self.assertIn("2 records of 16352 (not full): 1 sync, 1 words", p.stdout)
        self.assertIn("frames ended by count 0, by the chip's done flag 1", p.stdout)
        self.assertIn("sync to end: median 119.7 ms", p.stdout)
        self.assertIn("first words: FCB7", p.stdout)

    def test_an_unknown_header_version_is_refused(self):
        bad = bytearray(header2())
        bad[25] = 9
        self.radio_with(bytes(bad))
        p = self.run_tool("fake", self.out, ok=False)
        self.assertIn("header version 9", p.stderr + p.stdout)


if __name__ == "__main__":
    unittest.main()
