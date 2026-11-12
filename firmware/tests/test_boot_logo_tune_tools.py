"""End-to-end tests of source-changes/set_boot_logo.py and set_boot_tune.py against a fake radio.

  python3 firmware/tests/test_boot_logo_tune_tools.py      (needs Pillow; no radio, no pyserial)

Each test runs the real scripts in a throwaway sandbox that contains copies of the scripts
plus tests/fakeserial/serial.py in place of pyserial. The scripts exec() cps_tool.py, so the
whole Cps protocol path is exercised too.
"""
import json, os, shutil, struct, subprocess, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TOOLS = os.environ.get("TOOLS_DIR", os.path.join(os.path.dirname(HERE), "source-changes"))
LOGO = 160 * 128 * 2
SET_ADDR, SET_LEN, TUNE_OFF, TUNE_PAIRS = 0x9000, 168, 72, 48


def wb(path, data):
    with open(path, "wb") as f:
        f.write(data)


def rb(path):
    with open(path, "rb") as f:
        return f.read()


def rgb565_le(r, g, b):
    v = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
    return struct.pack("<H", v)


class Sandbox(unittest.TestCase):
    def setUp(self):
        self.box = tempfile.mkdtemp(prefix="cpsbox-")
        self.radio = os.path.join(self.box, "radio")
        os.mkdir(self.radio)
        for f in ("cps_tool.py", "set_boot_logo.py", "set_boot_tune.py"):
            shutil.copy(os.path.join(TOOLS, f), self.box)
        shutil.copy(os.path.join(HERE, "fakeserial", "serial.py"), self.box)
        self.logo_a = bytes((i * 7) & 0xFF for i in range(LOGO))
        wb(os.path.join(self.radio, "logo.bin"), self.logo_a)
        self.settings = bytes((i * 3 + 1) & 0xFF for i in range(SET_LEN))
        wb(os.path.join(self.radio, "settings.bin"), self.settings)

    def tearDown(self):
        shutil.rmtree(self.box, ignore_errors=True)

    def run_tool(self, script, *args, expect_ok=True):
        env = dict(os.environ, FAKE_RADIO_DIR=self.radio)
        p = subprocess.run([sys.executable, os.path.join(self.box, script), *args],
                           cwd=self.box, env=env, capture_output=True, text=True, timeout=120)
        if expect_ok and p.returncode != 0:
            self.fail(f"{script} {args} exited {p.returncode}\n{p.stdout}\n{p.stderr}")
        return p

    def fault(self, **f):
        with open(os.path.join(self.radio, "faults.json"), "w") as fh:
            json.dump(f, fh)

    def radio_file(self, name):
        return rb(os.path.join(self.radio, name))

    def make_png(self, name, colour):
        from PIL import Image
        p = os.path.join(self.box, name)
        Image.new("RGB", (160, 128), colour).save(p)
        return p


class LogoTests(Sandbox):
    def test_preview_converts_exactly_to_rgb565(self):
        png = self.make_png("red.png", (255, 0, 0))
        raw = os.path.join(self.box, "out.rgb565")
        self.run_tool("set_boot_logo.py", "preview", png, "--raw", raw)
        self.assertEqual(rb(raw), rgb565_le(255, 0, 0) * (160 * 128))

    def test_preview_letterboxes_with_background(self):
        from PIL import Image
        p = os.path.join(self.box, "wide.png")
        Image.new("RGB", (320, 64), (0, 255, 0)).save(p)
        raw = os.path.join(self.box, "out.rgb565")
        self.run_tool("set_boot_logo.py", "preview", p, "--raw", raw, "--fit", "contain")
        data = rb(raw)
        self.assertEqual(len(data), LOGO)
        self.assertEqual(data[:2], rgb565_le(0, 0, 0))                       # top row is background
        mid = (64 * 160 + 80) * 2
        self.assertEqual(data[mid:mid + 2], rgb565_le(0, 255, 0))            # centre is the image

    def test_upload_backs_up_writes_and_verifies(self):
        png = self.make_png("blue.png", (0, 0, 255))
        bk = os.path.join(self.box, "bk", "logo-backup.rgb565")
        p = self.run_tool("set_boot_logo.py", "upload", "fake", png, bk)
        self.assertIn("logo written and verified", p.stdout)
        self.assertEqual(rb(bk), self.logo_a)                 # backup = old logo
        self.assertEqual(self.radio_file("logo.bin"), rgb565_le(0, 0, 255) * (160 * 128))

    def test_upload_skips_identical_logo(self):
        png = self.make_png("same.png", (10, 20, 30))
        bk = os.path.join(self.box, "b1.rgb565")
        self.run_tool("set_boot_logo.py", "upload", "fake", png, bk)
        wb(os.path.join(self.radio, "log.txt"), b"")
        p = self.run_tool("set_boot_logo.py", "upload", "fake", png, os.path.join(self.box, "b2.rgb565"))
        self.assertIn("already installed", p.stdout)
        self.assertNotIn("logo erase", rb(os.path.join(self.radio, "log.txt")).decode())

    def test_failed_verify_restores_original(self):
        png = self.make_png("g.png", (1, 200, 3))
        bk = os.path.join(self.box, "b.rgb565")
        self.fault(corrupt_logo_write=True)
        p = self.run_tool("set_boot_logo.py", "upload", "fake", png, bk, expect_ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("VERIFY FAILED", p.stdout)
        self.assertEqual(self.radio_file("logo.bin"), self.logo_a)           # original is back

    def test_refuses_to_overwrite_a_different_backup(self):
        png = self.make_png("x.png", (9, 9, 9))
        bk = os.path.join(self.box, "keep.rgb565")
        wb(bk, b"\x01" * LOGO)
        p = self.run_tool("set_boot_logo.py", "upload", "fake", png, bk, expect_ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("REFUSING TO OVERWRITE", p.stdout + p.stderr)
        self.assertEqual(rb(bk), b"\x01" * LOGO)
        self.assertEqual(self.radio_file("logo.bin"), self.logo_a)           # radio untouched

    def test_restore_roundtrip(self):
        png = self.make_png("r.png", (255, 255, 0))
        bk = os.path.join(self.box, "orig.rgb565")
        self.run_tool("set_boot_logo.py", "upload", "fake", png, bk)
        self.run_tool("set_boot_logo.py", "restore", "fake", bk)
        self.assertEqual(self.radio_file("logo.bin"), self.logo_a)

    def test_restore_rejects_wrong_size_without_writing(self):
        bad = os.path.join(self.box, "bad.rgb565")
        wb(bad, b"\x00" * 100)
        p = self.run_tool("set_boot_logo.py", "restore", "fake", bad, expect_ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(self.radio_file("logo.bin"), self.logo_a)


class TuneTests(Sandbox):
    def run_tune(self, *args, **kw):
        return self.run_tool("set_boot_tune.py", "fake", *args, **kw)

    def test_default_preset_changes_only_the_tune(self):
        bk = os.path.join(self.box, "tune-bk.bin")
        p = self.run_tune(bk)
        self.assertIn("tune written and verified", p.stdout)
        self.assertEqual(rb(bk), self.settings)               # backup = before
        now = self.radio_file("settings.bin")
        end = TUNE_OFF + TUNE_PAIRS * 2
        self.assertEqual(now[:TUNE_OFF], self.settings[:TUNE_OFF])
        self.assertEqual(now[end:], self.settings[end:])
        self.assertNotEqual(now[TUNE_OFF:end], self.settings[TUNE_OFF:end])

    def test_presets_are_valid_and_distinct(self):
        seen = set()
        for name in ("doom", "mario"):
            self.setUp_radio_reset()
            self.run_tune(os.path.join(self.box, f"{name}.bin"), name)
            t = self.radio_file("settings.bin")[TUNE_OFF:TUNE_OFF + TUNE_PAIRS * 2]
            pairs = [(t[i], t[i + 1]) for i in range(0, len(t), 2)]
            used = [p for p in pairs if p != (0, 0)]
            self.assertTrue(used and len(used) <= TUNE_PAIRS)
            self.assertTrue(all(0 <= n <= 45 and d > 0 for n, d in used), name)
            seen.add(bytes(t))
        self.assertEqual(len(seen), 2)

    def setUp_radio_reset(self):
        wb(os.path.join(self.radio, "settings.bin"), self.settings)

    def test_unknown_preset_exits_before_touching_the_radio(self):
        p = self.run_tune(os.path.join(self.box, "u.bin"), "nope", expect_ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(self.radio_file("settings.bin"), self.settings)
        self.assertFalse(os.path.exists(os.path.join(self.box, "u.bin")))

    def test_readback_mismatch_restores_settings(self):
        self.fault(corrupt_settings_write=True)
        p = self.run_tune(os.path.join(self.box, "f.bin"), expect_ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("READ-BACK DIFFERS", p.stdout)
        self.assertEqual(self.radio_file("settings.bin"), self.settings)


if __name__ == "__main__":
    unittest.main(verbosity=2)
