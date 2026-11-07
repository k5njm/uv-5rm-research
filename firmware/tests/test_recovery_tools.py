"""Tests for dump_nor.py (read-only calibration/flash dump) and restore_backup.py against a fake radio.

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
        for f in ("cps_tool.py", "dump_nor.py", "restore_backup.py"):
            shutil.copy(os.path.join(TOOLS, f), self.box)
        shutil.copy(os.path.join(HERE, "fakeserial", "serial.py"), self.box)
        nor = bytearray(b"\xff" * NOR)
        nor[:SLOT * SLOTS] = pattern(SLOT * SLOTS, 1)               # a channel table
        nor[CAL_LO:CAL_HI] = pattern(CAL_HI - CAL_LO, 2)            # calibration page
        nor[0x8000:0x8040] = pattern(0x40, 3)                       # VFO record
        nor[0xA000:0xA2C0] = pattern(0x2C0, 4)                      # ANI header + 20 DTMF contacts + rest
        nor[0xD000:0xD040] = pattern(0x40, 5)                       # firmware scratch
        self.nor0 = bytes(nor)
        wb(os.path.join(self.radio, "nor.bin"), self.nor0)
        # the fake overlays settings (0x9000) and the logo (0xC0000) onto raw reads: make them identity
        nor[0x9000:0x9000 + 168] = pattern(168, 6)                 # settings record (the radio's own)
        self.nor0 = bytes(nor)
        wb(os.path.join(self.radio, "nor.bin"), self.nor0)
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


class RestoreBackup(Box):
    """restore_backup.py: every record the port can write is restored and verified; everything else is left alone."""
    CONTACTS = (0xA020, 0xA020 + 20 * 16)

    def image(self, name="backup.img"):
        """A radio_read.py-format image (0x8380 bytes) of the GOOD state (nor0) for the restorable regions."""
        img = (self.nor0[:SLOT * SLOTS].ljust(0x8000, b"\xff") + self.nor0[0x8000:0x8040] + pattern(0x40, 7)
               + self.nor0[0xA000:0xA2C0] + self.nor0[0xD000:0xD040])
        assert len(img) == 0x8380
        p = os.path.join(self.box, name)
        wb(p, img)
        return p

    def wipe(self):
        """The radio now holds someone else's channels, VFO and contacts."""
        nor = bytearray(self.nor0)
        nor[:SLOT * SLOTS] = pattern(SLOT * SLOTS, 9)
        nor[0x8000:0x8040] = pattern(0x40, 10)
        nor[self.CONTACTS[0]:self.CONTACTS[1]] = pattern(self.CONTACTS[1] - self.CONTACTS[0], 11)
        wb(os.path.join(self.radio, "nor.bin"), bytes(nor))
        return bytes(nor)

    def restore(self, *extra, name="undo.img", ok=True):
        return self.run_tool("restore_backup.py", "fake", self.image(), "--before", os.path.join(self.box, name), *extra, ok=ok)

    def writes(self):
        return [int(l.split()[1], 16) for l in self.log() if l.startswith("W ")]

    def test_dry_run_writes_nothing_and_lists_what_it_skips(self):
        wiped = self.wipe()
        p = self.restore()
        self.assertIn("records differ", p.stdout)
        for must in ("settings (0x9000)", "DTMF/ANI header", "0xD000 scratch", "calibration"):
            self.assertIn(f"skipped: {must}", p.stdout)
        self.assertEqual(self.nor_now(), wiped)
        self.assertEqual(self.writes(), [])
        self.assertFalse(os.path.exists(os.path.join(self.box, "undo.img")))

    def test_restores_channels_vfo_and_contacts_and_leaves_everything_else_alone(self):
        wiped = self.wipe()
        self.restore("--write")
        now = self.nor_now()
        self.assertEqual(now[:SLOT * SLOTS], self.nor0[:SLOT * SLOTS])                    # 1000 channels
        self.assertEqual(now[0x8000:0x8040], self.nor0[0x8000:0x8040])                    # VFO
        lo, hi = self.CONTACTS
        self.assertEqual(now[lo:hi], self.nor0[lo:hi])                                    # 20 contacts
        restored = [(0, SLOT * SLOTS), (0x8000, 0x8040), (lo, hi)]
        for i in range(len(now)):                                                        # everything else untouched
            pass
        mask = bytearray(len(now))
        for a, b in restored:
            mask[a:b] = b"\x01" * (b - a)
        others_same = all(now[i] == wiped[i] for i in range(len(now)) if not mask[i])
        self.assertTrue(others_same, "a region the tool must not touch changed")
        self.assertEqual(now[CAL_LO:CAL_HI], self.nor0[CAL_LO:CAL_HI])                    # calibration
        self.assertEqual(now[0x9000:0x9000 + 168], wiped[0x9000:0x9000 + 168])            # settings: refused by design
        self.assertEqual(now[0xD000:0xD040], wiped[0xD000:0xD040])                         # scratch
        self.assertEqual(now[0xA000:0xA020], wiped[0xA000:0xA020])                         # ANI header
        self.assertEqual(now[0xA160:0xA2C0], wiped[0xA160:0xA2C0])                         # DTMF tail
        for a in self.writes():                                                           # only ever those addresses
            self.assertTrue((a < SLOT * SLOTS and a % SLOT == 0) or a == 0x8000 or (lo <= a < hi and (a - lo) % 16 == 0), hex(a))

    def test_settings_are_never_written_even_when_the_image_differs(self):
        self.wipe()
        self.restore("--write")
        self.assertFalse([a for a in self.writes() if 0x9000 <= a < 0xA000])
        self.assertEqual(rb(os.path.join(self.radio, "settings.bin")), self.nor0[0x9000:0x9000 + 168])

    def test_second_run_is_a_no_op(self):
        self.wipe()
        self.restore("--write")
        wb(os.path.join(self.radio, "log.txt"), b"")
        p = self.restore("--write", name="undo2.img")
        self.assertIn("0 of 1021", p.stdout)
        self.assertEqual(self.writes(), [])

    def test_undo_image_puts_the_previous_state_back(self):
        wiped = self.wipe()
        self.restore("--write")
        undo = os.path.join(self.box, "undo.img")
        self.assertEqual(os.path.getsize(undo), 0x8380)
        self.run_tool("restore_backup.py", "fake", undo, "--before", os.path.join(self.box, "undo2.img"), "--write")
        now = self.nor_now()
        lo, hi = self.CONTACTS
        self.assertEqual(now[:SLOT * SLOTS], wiped[:SLOT * SLOTS])
        self.assertEqual(now[0x8000:0x8040], wiped[0x8000:0x8040])
        self.assertEqual(now[lo:hi], wiped[lo:hi])

    def test_wrong_size_image_is_refused_before_connecting(self):
        bad = os.path.join(self.box, "bad.img")
        wb(bad, b"\x00" * 1234)
        p = self.run_tool("restore_backup.py", "fake", bad, "--before", os.path.join(self.box, "b.img"), "--write", ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(self.log(), [])
        self.assertEqual(self.nor_now(), self.nor0)

    def test_does_not_overwrite_a_different_undo_file(self):
        self.wipe()
        undo = os.path.join(self.box, "undo.img")
        wb(undo, b"old undo")
        p = self.run_tool("restore_backup.py", "fake", self.image(), "--before", undo, "--write", ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(rb(undo), b"old undo")
        self.assertEqual(self.writes(), [])

    def test_a_corrupt_write_is_detected_and_that_record_put_back_for_each_record_type(self):
        for fault, label in (("corrupt_channel_write", "channel"), ("corrupt_vfo_write", "vfo"), ("corrupt_contact_write", "contact")):
            with self.subTest(fault=fault):
                wiped = self.wipe()
                if label != "channel":                       # make sure only the faulted record type differs first
                    nor = bytearray(self.nor_now())
                    nor[:SLOT * SLOTS] = self.nor0[:SLOT * SLOTS]
                    if label == "vfo":
                        nor[self.CONTACTS[0]:self.CONTACTS[1]] = self.nor0[self.CONTACTS[0]:self.CONTACTS[1]]
                    else:
                        nor[0x8000:0x8040] = self.nor0[0x8000:0x8040]
                    wb(os.path.join(self.radio, "nor.bin"), bytes(nor))
                    wiped = bytes(nor)
                self.fault(**{fault: True})
                p = self.restore("--write", ok=False)
                self.assertNotEqual(p.returncode, 0)
                self.assertIn("READ-BACK DIFFERS", p.stdout)
                self.assertIn(label, p.stdout)
                now = self.nor_now()
                bad = next(int(l.split()[1], 16) for l in self.log() if l.startswith("W "))
                size = 32 if label == "channel" else 64 if label == "vfo" else 16
                self.assertEqual(now[bad:bad + size], wiped[bad:bad + size])               # that record was put back
                os.remove(os.path.join(self.box, "undo.img"))
                wb(os.path.join(self.radio, "log.txt"), b"")

    def test_cable_pulled_mid_restore_then_rerun_completes_everything(self):
        wiped = self.wipe()
        self.fault(die_after_writes=5)
        undo = os.path.join(self.box, "undo.img")
        p = self.restore("--write", ok=False)
        self.assertNotEqual(p.returncode, 0)
        partial = self.nor_now()
        self.assertNotEqual(partial[:SLOT * SLOTS], self.nor0[:SLOT * SLOTS])               # genuinely incomplete
        self.assertEqual(partial[CAL_LO:CAL_HI], self.nor0[CAL_LO:CAL_HI])
        os.remove(os.path.join(self.radio, "faults.json"))                                  # "reconnect"
        self.restore("--write", name="undo2.img")
        lo, hi = self.CONTACTS
        now = self.nor_now()
        self.assertEqual(now[:SLOT * SLOTS], self.nor0[:SLOT * SLOTS])
        self.assertEqual(now[0x8000:0x8040], self.nor0[0x8000:0x8040])
        self.assertEqual(now[lo:hi], self.nor0[lo:hi])
        self.assertEqual(rb(undo)[:SLOT * SLOTS], wiped[:SLOT * SLOTS])                      # first undo file kept intact


if __name__ == "__main__":
    unittest.main(verbosity=2)
