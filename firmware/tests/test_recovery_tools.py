"""Tests for dump_nor.py (read-only calibration/flash dump) and restore_channels.py against a fake radio.

  scripts/dev.sh tools-test        (or: python3 firmware/tests/test_recovery_tools.py)

The fake radio logs every command ("R/W/D <addr>"), so tests assert exactly what a tool touched.
All of this is UNVERIFIED against real hardware.
"""
import hashlib, json, os, shutil, subprocess, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TOOLS = os.environ.get("TOOLS_DIR", os.path.join(os.path.dirname(HERE), "source-changes"))
NOR = 2 * 1024 * 1024
CAL_LO, CAL_HI = 0xF000, 0x10000
SLOT, SLOTS = 32, 1000


def wb(p, d):
    with open(p, "wb") as f:
        f.write(d)


def rb(p):
    with open(p, "rb") as f:
        return f.read()


def pattern(n, seed):
    return bytes(((i * 31 + seed * 17) ^ (i >> 3)) & 0xFF for i in range(n))


class Box(unittest.TestCase):
    def setUp(self):
        self.box = tempfile.mkdtemp(prefix="recbox-")
        self.radio = os.path.join(self.box, "radio")
        os.mkdir(self.radio)
        for f in ("cps_tool.py", "dump_nor.py", "restore_channels.py"):
            shutil.copy(os.path.join(TOOLS, f), self.box)
        shutil.copy(os.path.join(HERE, "fakeserial", "serial.py"), self.box)
        nor = bytearray(b"\xff" * NOR)
        nor[:SLOT * SLOTS] = pattern(SLOT * SLOTS, 1)               # a channel table
        nor[CAL_LO:CAL_HI] = pattern(CAL_HI - CAL_LO, 2)            # calibration page
        self.nor0 = bytes(nor)
        wb(os.path.join(self.radio, "nor.bin"), self.nor0)
        # the fake overlays settings (0x9000) and the logo (0xC0000) onto raw reads: make them identity
        wb(os.path.join(self.radio, "settings.bin"), self.nor0[0x9000:0x9000 + 168])
        wb(os.path.join(self.radio, "logo.bin"), self.nor0[0xC0000:0xC0000 + 160 * 128 * 2])

    def tearDown(self):
        shutil.rmtree(self.box, ignore_errors=True)

    def run_tool(self, script, *args, ok=True):
        env = dict(os.environ, FAKE_RADIO_DIR=self.radio)
        p = subprocess.run([sys.executable, os.path.join(self.box, script), *args],
                           cwd=self.box, env=env, capture_output=True, text=True, timeout=300)
        if ok and p.returncode != 0:
            self.fail(f"{script} {args} -> {p.returncode}\n{p.stdout}\n{p.stderr}")
        return p

    def log(self):
        p = os.path.join(self.radio, "log.txt")
        return open(p).read().split("\n") if os.path.exists(p) else []

    def fault(self, **f):
        with open(os.path.join(self.radio, "faults.json"), "w") as fh:
            json.dump(f, fh)

    def nor_now(self):
        return rb(os.path.join(self.radio, "nor.bin"))

    def image(self, name="backup.img", table=None):
        """A radio_read.py-format image (0x8380 bytes) with the given 32000-byte channel table."""
        table = table or self.nor0[:SLOT * SLOTS]
        img = table.ljust(0x8000, b"\xff") + b"\x00" * (0x8380 - 0x8000)
        p = os.path.join(self.box, name)
        wb(p, img)
        return p


class DumpNor(Box):
    def test_cal_dump_is_exact_and_read_only(self):
        out = os.path.join(self.box, "cal.bin")
        p = self.run_tool("dump_nor.py", "fake", out, "--cal")
        self.assertEqual(rb(out), self.nor0[CAL_LO:CAL_HI])
        self.assertIn(hashlib.sha256(rb(out)).hexdigest(), p.stdout)
        cmds = [l for l in self.log() if l]
        self.assertTrue(cmds and all(l.startswith("D ") for l in cmds), cmds[:3])
        self.assertEqual(self.nor_now(), self.nor0)                    # radio unchanged

    def test_default_is_calibration_and_config_range_works(self):
        out = os.path.join(self.box, "cfg.bin")
        self.run_tool("dump_nor.py", "fake", out, "--config")
        self.assertEqual(rb(out), self.nor0[:0x10000])

    def test_refuses_to_overwrite_existing_file(self):
        out = os.path.join(self.box, "keep.bin")
        wb(out, b"precious")
        p = self.run_tool("dump_nor.py", "fake", out, ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(rb(out), b"precious")
        self.assertEqual(self.log(), [])                               # never even connected for reads

    def test_tool_can_only_send_reads(self):
        import re
        src = open(os.path.join(TOOLS, "dump_nor.py")).read()
        body = src.split('"""', 2)[2]
        sent = set(re.findall(r"cps\.send\((\w+)", body))
        self.assertEqual(sent, {"READ_RAW"}, sent)                 # the only call site sends raw reads
        self.assertIn("cmd not in (READ_RAW, END)", body)         # and the guard rejects anything else
        self.assertNotIn("Cps.send", body)

    def test_silent_radio_fails_cleanly(self):
        self.fault(silent=True)
        p = self.run_tool("dump_nor.py", "fake", os.path.join(self.box, "x.bin"), ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertFalse(os.path.exists(os.path.join(self.box, "x.bin")))


class RestoreChannels(Box):
    def wipe_channels(self):
        nor = bytearray(self.nor0)
        nor[:SLOT * SLOTS] = pattern(SLOT * SLOTS, 9)                  # someone else's channel table
        wb(os.path.join(self.radio, "nor.bin"), bytes(nor))
        return bytes(nor)

    def test_dry_run_writes_nothing(self):
        wiped = self.wipe_channels()
        p = self.run_tool("restore_channels.py", "fake", self.image(), "--before", os.path.join(self.box, "b.bin"))
        self.assertIn("slots differ", p.stdout)
        self.assertEqual(self.nor_now(), wiped)
        self.assertFalse([l for l in self.log() if l.startswith("W ")])
        self.assertFalse(os.path.exists(os.path.join(self.box, "b.bin")))

    def test_restore_is_byte_identical_and_leaves_everything_else_alone(self):
        wiped = self.wipe_channels()
        before = os.path.join(self.box, "undo.bin")
        self.run_tool("restore_channels.py", "fake", self.image(), "--before", before, "--write")
        now = self.nor_now()
        self.assertEqual(now[:SLOT * SLOTS], self.nor0[:SLOT * SLOTS])          # table restored
        self.assertEqual(now[SLOT * SLOTS:], wiped[SLOT * SLOTS:])              # nothing else changed
        self.assertEqual(now[CAL_LO:CAL_HI], self.nor0[CAL_LO:CAL_HI])          # calibration untouched
        self.assertEqual(rb(before), wiped[:SLOT * SLOTS])                      # undo file = previous state
        for l in (x for x in self.log() if x.startswith("W ")):
            a = int(l.split()[1], 16)
            self.assertTrue(a < SLOT * SLOTS and a % SLOT == 0, l)             # only channel slots

    def test_second_run_is_a_no_op(self):
        self.wipe_channels()
        before = os.path.join(self.box, "undo.bin")
        self.run_tool("restore_channels.py", "fake", self.image(), "--before", before, "--write")
        open(os.path.join(self.radio, "log.txt"), "w").close()
        p = self.run_tool("restore_channels.py", "fake", self.image(), "--before",
                          os.path.join(self.box, "undo2.bin"), "--write")
        self.assertIn("0 of 1000", p.stdout)
        self.assertFalse([l for l in self.log() if l.startswith("W ")])

    def test_wrong_size_image_is_refused_before_connecting(self):
        bad = os.path.join(self.box, "bad.img")
        wb(bad, b"\x00" * 1234)
        p = self.run_tool("restore_channels.py", "fake", bad, "--before", os.path.join(self.box, "b.bin"),
                          "--write", ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(self.log(), [])
        self.assertEqual(self.nor_now(), self.nor0)

    def test_does_not_overwrite_a_different_undo_file(self):
        self.wipe_channels()
        undo = os.path.join(self.box, "undo.bin")
        wb(undo, b"old undo")
        p = self.run_tool("restore_channels.py", "fake", self.image(), "--before", undo, "--write", ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(rb(undo), b"old undo")
        self.assertFalse([l for l in self.log() if l.startswith("W ")])

    def test_corrupt_write_is_detected_and_that_slot_put_back(self):
        wiped = self.wipe_channels()
        self.fault(corrupt_channel_write=True)
        p = self.run_tool("restore_channels.py", "fake", self.image(), "--before",
                          os.path.join(self.box, "u.bin"), "--write", ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("READ-BACK DIFFERS", p.stdout)
        now = self.nor_now()
        first = next(i for i in range(SLOTS) if wiped[i * SLOT:(i + 1) * SLOT] != self.nor0[i * SLOT:(i + 1) * SLOT])
        self.assertEqual(now[first * SLOT:(first + 1) * SLOT], wiped[first * SLOT:(first + 1) * SLOT])

    def test_cable_pulled_mid_restore_then_rerun_completes(self):
        wiped = self.wipe_channels()
        self.fault(die_after_channel_writes=5)
        undo = os.path.join(self.box, "undo.bin")
        p = self.run_tool("restore_channels.py", "fake", self.image(), "--before", undo, "--write", ok=False)
        self.assertNotEqual(p.returncode, 0)
        partial = self.nor_now()
        self.assertNotEqual(partial[:SLOT * SLOTS], self.nor0[:SLOT * SLOTS])   # genuinely incomplete
        self.assertEqual(partial[CAL_LO:CAL_HI], self.nor0[CAL_LO:CAL_HI])
        os.remove(os.path.join(self.radio, "faults.json"))                     # "reconnect"
        self.run_tool("restore_channels.py", "fake", self.image(), "--before",
                      os.path.join(self.box, "undo2.bin"), "--write")
        self.assertEqual(self.nor_now()[:SLOT * SLOTS], self.nor0[:SLOT * SLOTS])
        self.assertEqual(rb(undo), wiped[:SLOT * SLOTS])                        # first undo file kept intact


if __name__ == "__main__":
    unittest.main(verbosity=2)
