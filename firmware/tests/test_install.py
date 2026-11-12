"""install.py end to end against a simulated radio that can be the stock firmware, this firmware or the KDH bootloader.

  scripts/dev.sh tools-test        (or: python3 firmware/tests/test_install.py)

A test box mirrors the release bundle (install.py at its root, firmware/source-changes/, channels/, release/) and puts the
dispatching fake `serial` (fakeradio/) in front of every script. The radio's flash is the directory radio/ (nor.bin, logs);
radio/mode says what it is running; radio/bl is the bootloader fake. Everything here is UNVERIFIED against a real radio.
"""
import hashlib, json, os, re, shutil, subprocess, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
ROOT = os.path.dirname(FW)
SRC = os.path.join(FW, "source-changes")
sys.path.insert(0, HERE)
import test_flash_bf as tfb  # noqa: E402  (its image()/wrap() build a valid .BF)

TOOLS = ["flash_bf.py", "cps_tool.py", "dump_nor.py", "install_apps.py", "restore_backup.py", "channel_extras.py",
         "channel_plan.py", "boot_logo_raw.py", "wrap_bf.py", "radio_read.py", "fm_stations.py", "bootloader_probe.py", "install.py"]
NOR = 2 * 1024 * 1024
SLOT_BASE, SLOT_SIZE = 0x14C000, 0x8000
APPS = ["settings.app", "chanmgr.app", "searchqt.app", "spectrum.app", "bandsweep.app", "foxhunt.app", "rptfinder.app",
        "fskprobe.app", "fm.app"]
EXTRA_APPS = ["hamtools.app", "text.app"]
LOGO = 160 * 128 * 2


def rb(p):
    with open(p, "rb") as f:
        return f.read()


def wb(p, d):
    with open(p, "wb") as f:
        f.write(d)


def sha(b):
    return hashlib.sha256(b).hexdigest()


def pattern(n, seed):
    return bytes(((i * 31 + seed * 17) ^ (i >> 3)) & 0xFF for i in range(n))


def app_bytes(name, abi=b"\x11\x22\x33\x44", size=1300):
    title = name[:-4].upper().encode().ljust(16, b"\0")
    return (b"OVL1" + abi + b"\0\0\0\0" + title + pattern(size, len(name)))[:size]


class Box(unittest.TestCase):
    maxDiff = None

    def setUp(self):
        self.box = tempfile.mkdtemp(prefix="installbox-")
        self.addCleanup(shutil.rmtree, self.box, True)
        self.radio = os.path.join(self.box, "radio")
        os.makedirs(os.path.join(self.radio, "bl"))
        sc = os.path.join(self.box, "firmware", "source-changes")
        os.makedirs(sc)
        for t in TOOLS:
            shutil.copy(os.path.join(SRC, t), sc if t != "install.py" else self.box)
        for d in (self.box, sc):               # the fake `serial` in front of every script (sys.path[0])
            shutil.copy(os.path.join(HERE, "fakeradio", "serial.py"), d)
            shutil.copy(os.path.join(HERE, "fakeserial", "serial.py"), os.path.join(d, "fake_cps.py"))
            shutil.copy(os.path.join(HERE, "fakebootloader", "serial.py"), os.path.join(d, "fake_bl.py"))
        shutil.copy(os.path.join(FW, "variants.json"), os.path.join(self.box, "firmware"))
        gt = os.path.join(self.box, "firmware", "firmware-source", "src", "app")
        os.makedirs(gt)
        shutil.copy(os.path.join(FW, "firmware-source", "src", "app", "group_table.rs"), gt)
        os.makedirs(os.path.join(self.box, "channels", "fm"))
        for n in ("weather.csv", "frs.csv", "simplex.csv"):
            shutil.copy(os.path.join(ROOT, "channels", n), os.path.join(self.box, "channels", n))
        shutil.copy(os.path.join(HERE, "fixtures", "fm-stations.csv"), os.path.join(self.box, "channels", "fm", "test.csv"))
        self.make_release()
        wb(os.path.join(self.box, "logo.rgb565"), pattern(LOGO, 9))
        self.nor0 = self.make_nor()
        self.set_mode("custom")
        self.dispatch()

    # ---- the world
    def make_release(self, mutate=None):
        rel = os.path.join(self.box, "release")
        os.makedirs(rel, exist_ok=True)
        self.img = tfb.image()
        self.bf = tfb.wrap(self.img)
        blobs = {"fw-default.BF": self.bf}
        for n in APPS + EXTRA_APPS:
            blobs[n] = app_bytes(n)
        for n, b in blobs.items():
            wb(os.path.join(rel, n), b)
        with open(os.path.join(rel, "layout.json"), "w") as f:
            json.dump({"slots": APPS, "extras": EXTRA_APPS}, f)
        with open(os.path.join(rel, "SHA256SUMS"), "w") as f:
            f.write("".join(f"{sha(b)}  {n}\n" for n, b in sorted(blobs.items())))

    def make_nor(self):
        nor = bytearray(b"\xff" * NOR)
        nor[:32000] = pattern(32000, 1)                                  # somebody's channels
        nor[0x8000:0x8040] = pattern(0x40, 3)
        nor[0x9000:0x9000 + 168] = pattern(168, 6)
        nor[0xA000:0xA2C0] = pattern(0x2C0, 4)
        nor[0xF000:0x10000] = pattern(0x1000, 2)                         # calibration
        # an older app set from an earlier install: slot 1 holds something, the rest are erased
        old = app_bytes("old.app")
        nor[SLOT_BASE:SLOT_BASE + len(old)] = old
        wb(os.path.join(self.radio, "nor.bin"), bytes(nor))
        wb(os.path.join(self.radio, "settings.bin"), bytes(nor[0x9000:0x9000 + 168]))
        wb(os.path.join(self.radio, "logo.bin"), pattern(LOGO, 5))
        return bytes(nor)

    def set_mode(self, mode):
        with open(os.path.join(self.radio, "mode"), "w") as f:
            f.write(mode)

    def dispatch(self, **cfg):
        with open(os.path.join(self.radio, "dispatch.json"), "w") as f:
            json.dump(cfg, f)

    def fault(self, **f):
        with open(os.path.join(self.radio, "faults.json"), "w") as fh:
            json.dump(f, fh)

    def bl_fault(self, **f):
        with open(os.path.join(self.radio, "bl", "faults.json"), "w") as fh:
            json.dump(f, fh)

    def nor_now(self):
        return rb(os.path.join(self.radio, "nor.bin"))

    def log(self, name="log.txt", sub=""):
        p = os.path.join(self.radio, sub, name)
        if not os.path.exists(p):
            return []
        with open(p) as f:
            return [l for l in f.read().split("\n") if l]

    def writes(self):
        """Every write/erase command the custom-firmware fake logged."""
        return [l for l in self.log() if re.match(r"(A|a|X|F|l|W) ", l) or l.startswith("settings write") or l.startswith("logo erase")]

    def read_json(self, p):
        with open(p) as f:
            return json.load(f)

    def mode_now(self):
        with open(os.path.join(self.radio, "mode")) as f:
            return f.read()

    def log_clear(self):
        for sub in ("", "bl"):
            p = os.path.join(self.radio, sub, "log.txt")
            if os.path.exists(p):
                os.remove(p)

    def bl_writes(self):
        return [l for l in self.log(sub="bl") if not l.startswith("cmd 0x01")]

    # ---- the run
    def setup(self, **kw):
        s = {"schema": 1, "port": "fake", "board": "5RHNRFB-YP07", "release": "release", "core": "auto",
             "apps": [APPS[0], APPS[1], APPS[2], None, APPS[4], APPS[5], APPS[6], APPS[7], APPS[8]],
             "channels": {"WEATHER": "channels/weather.csv", "FRS": "channels/frs.csv"},
             "keep_existing_channels": True, "fm_stations": "channels/fm/test.csv", "boot_logo": "logo.rgb565"}
        s.update(kw)
        return s

    def write_setup(self, name="setup.json", **kw):
        p = os.path.join(self.box, name)
        with open(p, "w") as f:
            json.dump(self.setup(**kw), f)
        return p

    def install(self, *args, stdin="", timeout=600):
        env = dict(os.environ, FAKE_RADIO_DIR=self.radio)
        cmd = [sys.executable, os.path.join(self.box, "install.py"), "--settle", "0", "--probe-timeout", "0.2", "--wait", "6",
               "--backup-dir", os.path.join(self.box, "backups"), *args]
        return subprocess.run(cmd, cwd=self.box, env=env, input=stdin, capture_output=True, text=True, timeout=timeout)

    def run_config(self, cfg=None, *extra, **kw):
        path = cfg or self.write_setup(**kw)
        return self.install("--config", path, "--yes", *extra)

    def backups(self):
        d = os.path.join(self.box, "backups")
        return sorted(os.listdir(d)) if os.path.isdir(d) else []

    def run_dir(self):
        b = self.backups()
        self.assertEqual(len(b), 1, b)
        return os.path.join(self.box, "backups", b[0])

    def stages(self, out):
        """{stage: status} from the report block."""
        block = out.split("REPORT", 1)[1]
        res = {}
        for line in block.splitlines():
            m = re.match(r"  (identify|backup|files|core|calibration|apps|channels|details|fm|logo|final calibration)\s+(done|skipped|FAILED|not run|dry run)", line)
            if m:
                res[m.group(1)] = m.group(2)
        return res

    def ok(self, p):
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        return p.stdout


class ConfigRun(Box):
    def check_everything_written(self, run):
        nor = self.nor_now()
        stage = os.path.join(run, [d for d in os.listdir(run) if d.startswith("stage-")][0])
        for i, n in enumerate(self.setup()["apps"]):                   # the nine slots
            slot = nor[SLOT_BASE + i * SLOT_SIZE:SLOT_BASE + (i + 1) * SLOT_SIZE]
            if n is None:
                self.assertEqual(slot, b"\xff" * SLOT_SIZE, f"slot {i + 1} should be empty")
            else:
                self.assertEqual(slot[:1300], app_bytes(n), n)
                self.assertEqual(slot[1300:], b"\xff" * (SLOT_SIZE - 1300))
        img = rb(os.path.join(stage, "channels.img"))
        self.assertEqual(nor[:32000], img[:32000])                                      # channels, all 1000 slots
        self.assertEqual(nor[0x1E0000:0x1E0000 + 20480], rb(os.path.join(stage, "channels.extras")))
        self.assertEqual(nor[0x1D8000:0x1D9000], rb(os.path.join(stage, "fm.fmst")))
        self.assertEqual(nor[0x1D9000:0x1DA000], b"\xff" * 0x1000)
        self.assertEqual(rb(os.path.join(self.radio, "logo.bin")), rb(os.path.join(self.box, "logo.rgb565")))
        # what must NOT have changed: calibration, settings, VFO, DTMF
        self.assertEqual(nor[0xF000:0x10000], self.nor0[0xF000:0x10000])
        self.assertEqual(nor[0x9000:0x9000 + 168], self.nor0[0x9000:0x9000 + 168])
        self.assertEqual(nor[0x8000:0x8040], self.nor0[0x8000:0x8040])
        self.assertEqual(nor[0xA000:0xA2C0], self.nor0[0xA000:0xA2C0])
        self.assertFalse([l for l in self.log() if re.match(r"W 0x(9|a)[0-9a-f]{3}$", l)], "wrote into settings or DTMF")
        self.assertFalse([l for l in self.log() if l.startswith("settings write")])
        # channels outside the two groups were kept: weather 1-7, FRS 305-326 are the only slots that changed
        for slot in range(1000):
            if not (slot < 7 or 304 <= slot < 326):
                self.assertEqual(nor[slot * 32:(slot + 1) * 32], self.nor0[slot * 32:(slot + 1) * 32], f"channel {slot + 1} changed")

    def test_config_from_stock_flashes_the_core_then_everything_else(self):
        self.set_mode("stock")
        self.dispatch(user_enters_bootloader_after=1, boot_silent=1, reboot_silent=1)
        p = self.run_config()
        out = self.ok(p)
        self.assertEqual(self.stages(out), {"identify": "done", "backup": "done", "files": "done", "core": "done", "calibration": "done",
                                            "apps": "done", "channels": "done", "details": "done", "fm": "done", "logo": "done",
                                            "final calibration": "done"})
        # the core: what the bootloader received is the .BF's one region, and the radio now answers as this firmware
        region = self.bf[16:]
        self.assertEqual(rb(os.path.join(self.radio, "running_core.txt")).decode(), sha(rb(os.path.join(self.radio, "bl", "flash.bin"))))
        self.assertEqual(rb(os.path.join(self.radio, "bl", "flash.bin")).rstrip(b"\x00")[:len(region.rstrip(b"\x00"))], region.rstrip(b"\x00"))
        self.assertEqual(self.mode_now(), "custom")
        # backups first: a new folder, the stock image and calibration taken before the first write
        run = self.run_dir()
        self.assertEqual(len(rb(os.path.join(run, "stock-memory.img"))), 0x8380)
        self.assertEqual(rb(os.path.join(run, "stock-calibration.bin")), self.nor0[0xF000:0x10000])
        self.assertEqual(rb(os.path.join(run, "stock-memory.img"))[:32000], self.nor0[:32000])
        self.assertEqual(sorted(self.read_json(os.path.join(run, "backup.json"))["files"]), ["stock-calibration.bin", "stock-memory.img"])
        # nothing but reads reached the stock firmware; the only other bytes it saw are KDH handshake frames (aa01...) while the
        # installer waited for the bootloader: whether a real stock radio ignores those is UNVERIFIED
        self.assertFalse([l for l in self.log() if l.startswith("S ??") and not l.startswith("S ?? aa01")],
                         "something other than reads reached the stock firmware")
        stock_reads = [i for i, l in enumerate(self.log()) if l.startswith("S R ")]
        first_write = min(i for i, l in enumerate(self.log()) if re.match(r"(A|a|W|X|F|l) ", l))
        self.assertTrue(stock_reads and max(stock_reads) < first_write)
        self.check_everything_written(run)

    def test_config_on_this_firmware_skips_the_core_and_backs_up_with_dump_nor(self):
        self.dispatch(reboot_silent=1)
        out = self.ok(self.run_config(core="skip"))
        st = self.stages(out)
        self.assertEqual(st["core"], "skipped")
        self.assertEqual(st["calibration"], "skipped")
        self.assertEqual(st["final calibration"], "done")
        self.assertEqual(self.log("log.txt", "bl"), [])                       # the bootloader was never involved
        run = self.run_dir()
        self.assertEqual(rb(os.path.join(run, "calibration-0xF000.bin")), self.nor0[0xF000:0x10000])
        self.assertEqual(rb(os.path.join(run, "channels-before.img")), self.nor0[:0x8000])
        self.assertEqual(len(rb(os.path.join(run, "config-0x0000-0xFFFF.bin"))), 0x10000)
        self.check_everything_written(run)

    def test_core_flash_on_this_firmware_when_asked(self):
        self.dispatch(user_enters_bootloader_after=2)
        out = self.ok(self.run_config(core="flash"))
        self.assertEqual(self.stages(out)["core"], "done")
        self.assertTrue(os.path.exists(os.path.join(self.radio, "running_core.txt")))
        self.assertGreater(len([l for l in self.log(sub="bl") if l.startswith("cmd 0x03")]), 5)

    def test_auto_on_this_firmware_needs_to_be_told(self):
        p = self.run_config(core="auto")                                     # --yes and a config: cannot ask
        self.assertEqual(p.returncode, 1, p.stdout)
        self.assertIn("cannot tell whether it is this exact build", p.stdout)
        self.assertEqual(self.writes(), [])

    def test_wrong_board_is_refused_before_the_radio_is_touched(self):
        p = self.run_config(board="5RHXX")
        self.assertEqual(p.returncode, 2, p.stdout)
        self.assertIn("BOARD GATE", p.stdout)
        self.assertEqual(self.log(), [])
        self.assertEqual(self.backups(), [])

    def test_confirmation_is_required_without_yes(self):
        cfg = self.write_setup(core="skip")
        p = self.install("--config", cfg, stdin="no\n")
        self.assertEqual(p.returncode, 2, p.stdout)
        self.assertEqual(self.nor_now(), self.nor0)
        self.assertEqual(self.log(), [])
        self.assertIn("WILL BE WRITTEN", p.stdout)
        self.assertIn("WILL NOT BE TOUCHED", p.stdout)
        p = self.install("--config", cfg, stdin="INSTALL\n")
        self.ok(p)
        self.assertNotEqual(self.nor_now(), self.nor0)


class DryRun(Box):
    def assert_nothing_sent(self):
        self.assertEqual(self.writes(), [], self.log())
        self.assertEqual(self.nor_now(), self.nor0)
        self.assertEqual(self.bl_writes(), [])
        self.assertEqual(self.backups(), [])
        self.assertFalse(os.path.exists(os.path.join(self.radio, "running_core.txt")))

    def test_dry_run_on_this_firmware_reads_only(self):
        p = self.run_config(None, "--dry-run", core="skip")
        out = self.ok(p)
        self.assertIn("dry run", out)
        self.assertEqual(self.stages(out)["apps"], "dry run")
        reads = [l for l in self.log() if l]
        self.assertTrue(reads and all(re.match(r"[RD] ", l) for l in reads), [l for l in reads if not re.match(r"[RD] ", l)][:5])
        self.assert_nothing_sent()

    def test_dry_run_on_stock_and_in_bootloader_mode(self):
        for mode in ("stock", "bootloader"):
            with self.subTest(mode=mode):
                self.set_mode(mode)
                self.log_clear()
                p = self.run_config(None, "--dry-run")
                self.ok(p)
                self.assertIn("dry run", p.stdout)
                self.assert_nothing_sent()
                self.assertFalse([l for l in self.log() if l.startswith("S ??")])

    def test_dry_run_with_no_radio_at_all(self):
        s = self.setup(port=None)
        cfg = os.path.join(self.box, "noradio.json")
        with open(cfg, "w") as f:
            json.dump(s, f)
        p = self.install("--config", cfg, "--dry-run")
        out = self.ok(p)
        self.assertIn("WILL BE WRITTEN", out)
        self.assertIn("dry run", out)
        self.assertEqual(self.log(), [])
        # a setup with no port runs as a dry run even without the flag
        p = self.install("--config", cfg, "--yes")
        self.assertIn("dry run", self.ok(p))

    def test_dry_run_never_writes_even_with_a_changed_radio(self):
        self.dispatch(cal_changes_on_flash=True)
        p = self.run_config(None, "--dry-run", core="flash")
        self.ok(p)
        self.assert_nothing_sent()


class Safety(Box):
    def test_backup_that_cannot_be_verified_stops_everything_before_any_write(self):
        self.fault(corrupt_read_once=True)
        p = self.run_config(core="skip")
        self.assertEqual(p.returncode, 1, p.stdout)
        self.assertIn("NOT identical", p.stdout)
        self.assertEqual(self.stages(p.stdout)["backup"], "FAILED")
        self.assertEqual(self.stages(p.stdout)["apps"], "not run")
        self.assertEqual(self.writes(), [])
        self.assertEqual(self.nor_now(), self.nor0)

    def test_stock_backup_that_does_not_repeat_stops_before_the_bootloader(self):
        self.set_mode("stock")
        self.dispatch(stock_noise_on_session=2, user_enters_bootloader_after=0)
        p = self.run_config()
        self.assertEqual(p.returncode, 1, p.stdout)
        self.assertIn("NOT identical", p.stdout)
        self.assertEqual(self.log("log.txt", "bl"), [])
        self.assertFalse(os.path.exists(os.path.join(self.radio, "bl", "flash.bin")))

    def test_checksum_mismatch_in_the_release_stops_before_the_radio_is_touched(self):
        with open(os.path.join(self.box, "release", "chanmgr.app"), "ab") as f:
            f.write(b"x")
        p = self.run_config(core="skip")
        self.assertEqual(p.returncode, 2, p.stdout)
        self.assertIn("CHECKSUM MISMATCH: chanmgr.app", p.stdout)
        self.assertEqual(self.log(), [])
        self.assertEqual(self.backups(), [])

    def test_core_checksum_mismatch_and_unlisted_file(self):
        with open(os.path.join(self.box, "release", "fw-default.BF"), "ab") as f:
            f.write(b"x")
        p = self.run_config(core="skip")
        self.assertEqual(p.returncode, 2)
        self.assertIn("CHECKSUM MISMATCH: fw-default.BF", p.stdout)
        self.make_release()
        sums = rb(os.path.join(self.box, "release", "SHA256SUMS")).decode()
        wb(os.path.join(self.box, "release", "SHA256SUMS"), "".join(l + "\n" for l in sums.splitlines() if "foxhunt" not in l).encode())
        p = self.run_config(core="skip")
        self.assertEqual(p.returncode, 2)
        self.assertIn("foxhunt.app is not listed", p.stdout)
        self.assertEqual(self.log(), [])

    def test_calibration_that_differs_after_the_flash_stops_and_reports(self):
        self.dispatch(user_enters_bootloader_after=0, cal_changes_on_flash=True)
        p = self.run_config(core="flash")
        self.assertEqual(p.returncode, 1, p.stdout)
        self.assertIn("CALIBRATION DIFFERS: 1 bytes changed", p.stdout)
        st = self.stages(p.stdout)
        self.assertEqual((st["core"], st["calibration"], st["apps"], st["channels"], st["logo"]), ("done", "FAILED", "not run", "not run", "not run"))
        self.assertIn("apps and channels were NOT written", p.stdout)
        self.assertEqual(self.writes(), [])                                   # the radio was never written after the flash

    def test_half_seated_cable_is_recognised(self):
        for mode in ("custom", "stock", "bootloader"):
            with self.subTest(mode=mode):
                self.set_mode(mode)
                self.dispatch(echo=True)
                p = self.run_config(core="skip")
                self.assertEqual(p.returncode, 1, p.stdout)
                self.assertRegex(p.stdout, r"(?i)re-seat the cable")
                self.assertIn("echoes", p.stdout)
                self.assertEqual(self.writes(), [])
                self.assertEqual(self.backups(), [])

    def test_a_silent_radio_is_reported(self):
        self.fault(silent=True)
        self.set_mode("custom")
        p = self.run_config(core="skip")
        self.assertEqual(p.returncode, 1)
        self.assertIn("nothing answered", p.stdout)

    def test_bootloader_start_needs_a_backup_folder(self):
        self.set_mode("bootloader")
        p = self.run_config()
        self.assertEqual(p.returncode, 1, p.stdout)
        self.assertIn("--resume", p.stdout)
        self.assertEqual(self.bl_writes(), [])

    def test_never_keys_the_transmitter_or_writes_settings(self):
        src = rb(os.path.join(SRC, "install.py")).decode()
        for bad in ("tone_on", "TXINH", "0x9000", "keydown", "ptt_on"):
            self.assertNotIn(bad, src, bad)
        self.dispatch(reboot_silent=1)
        self.ok(self.run_config(core="skip"))
        bad = [l for l in self.log() if l.startswith("settings") or (l.startswith("W ") and int(l.split()[1], 16) >= 0x8000)]
        self.assertEqual(bad, [])                                          # no VFO, settings, DTMF or contact write


class Failures(Box):
    """A fault injected at each stage in turn: the report says which stage failed, earlier ones are done, later ones not run."""
    ORDER = ["identify", "backup", "files", "core", "calibration", "apps", "channels", "details", "fm", "logo", "final calibration"]

    def expect(self, p, failed, state_words):
        self.assertEqual(p.returncode, 1, p.stdout)
        st = self.stages(p.stdout)
        i = self.ORDER.index(failed)
        for k, s in enumerate(self.ORDER):
            if k < i:
                self.assertIn(st[s], ("done", "skipped"), (s, st))
            elif k == i:
                self.assertEqual(st[s], "FAILED", st)
            else:
                self.assertEqual(st[s], "not run", (s, st))
        self.assertIn(f"FAILED at step '{failed}'", p.stdout)
        self.assertIn("State of the radio:", p.stdout)
        self.assertIn(state_words, p.stdout)
        self.assertIn("To recover:", p.stdout)

    def test_fault_in_apps(self):
        self.fault(corrupt_app_write=True)
        p = self.run_config(core="skip")
        self.expect(p, "apps", "some slots may be empty or old")
        self.assertIn("VERIFY FAILED", p.stdout)
        self.assertFalse([l for l in self.log() if l.startswith("W ")], "channels were written after a failed app install")

    def test_fault_in_channels(self):
        self.fault(corrupt_channel_write=True)
        p = self.run_config(core="skip")
        self.expect(p, "channels", "whole 4 KB sectors")
        self.assertIn("READ-BACK DIFFERS", p.stdout)
        self.assertFalse([l for l in self.log() if l.startswith("X ") or l.startswith("F ") or l.startswith("l ")])

    def test_cable_pulled_during_channels_then_run_again_completes(self):
        self.fault(die_after_writes=3)
        p = self.run_config(core="skip")
        self.expect(p, "channels", "whole 4 KB sectors")
        os.remove(os.path.join(self.radio, "faults.json"))
        self.ok(self.run_config(core="skip"))
        self.assertEqual(len(self.backups()), 2)                              # the second run made its own, new backup folder

    def test_fault_in_details(self):
        self.fault(corrupt_extra_write=True)
        p = self.run_config(core="skip")
        self.expect(p, "details", "channel details table")
        self.assertIn("EXTRAS VERIFY FAILED", p.stdout)

    def test_fault_in_fm(self):
        self.fault(corrupt_fm_write=True)
        p = self.run_config(core="skip")
        self.expect(p, "fm", "FM station table")
        self.assertIn("STATION TABLE VERIFY FAILED", p.stdout)

    def test_fault_in_logo_puts_the_old_one_back(self):
        self.fault(corrupt_logo_write=True)
        p = self.run_config(core="skip")
        self.expect(p, "logo", "boot screen")
        self.assertIn("LOGO VERIFY FAILED", p.stdout)
        self.assertEqual(rb(os.path.join(self.radio, "logo.bin")), pattern(LOGO, 5))      # the old logo is back

    def test_fault_in_the_flash_then_resume(self):
        self.set_mode("stock")
        self.dispatch(user_enters_bootloader_after=0)
        self.bl_fault(die_after_packages=3)
        p = self.run_config(core="flash")
        self.expect(p, "core", "partly written core")
        self.assertIn("--resume", p.stdout)
        self.assertEqual(self.mode_now(), "bootloader")
        self.assertFalse(os.path.exists(os.path.join(self.radio, "running_core.txt")))
        self.assertEqual(self.writes(), [])
        run = self.run_dir()
        # the radio is in the bootloader; the first run's backup is the way back in
        os.remove(os.path.join(self.radio, "bl", "faults.json"))
        p = self.run_config(None, "--resume", run, core="flash")
        out = self.ok(p)
        self.assertEqual(self.stages(out)["core"], "done")
        self.assertEqual(self.stages(out)["final calibration"], "done")
        self.assertEqual(len(self.backups()), 1)                              # resumed in place, no second backup
        self.assertTrue(os.path.exists(os.path.join(self.radio, "running_core.txt")))

    def test_flash_that_never_starts_leaves_the_radio_in_the_bootloader_untouched(self):
        self.set_mode("stock")
        self.dispatch(user_enters_bootloader_after=0)
        self.bl_fault(flash_fail_at=0)
        p = self.run_config(core="flash")
        self.expect(p, "core", "INTERRUPTED")                               # a refused first package: handshake had succeeded

    def test_bootloader_that_never_answers_gives_up_with_the_radio_unchanged(self):
        self.set_mode("stock")
        self.dispatch(user_enters_bootloader_after=10 ** 6)
        p = self.run_config(None, "--wait", "1", core="flash")
        self.expect(p, "core", "unchanged: still running the stock firmware")
        self.assertIn("two small side buttons", p.stdout)
        self.assertEqual(self.bl_writes(), [])


class Menu(Box):
    def test_interactive_menu_then_install(self):
        self.dispatch(reboot_silent=1)
        saved = os.path.join(self.box, "mine.json")
        mine = os.path.join(self.box, "my-repeaters.csv")
        shutil.copy(os.path.join(self.box, "channels", "simplex.csv"), mine)
        script = "\n".join([
            "5RHXX",                    # board: not a board -> refused, asks again
            "5RHNRFB-YP07",
            "x",                        # all slots empty
            "1 9", "2 9", "2 2",        # settings twice -> refused; then chanmgr in slot 2
            "3 8",                      # a third app (searchqt)
            "9 0", "ok",
            "n",                        # no channel lists yet
            "a /nonexistent.csv",
            "a " + mine, "SIMPLEX",     # own list, for the simplex group
            "1",                        # the bundled list 1 (frs, sorted by name)
            "ok",
            "1",                        # FM: the bundled list
            "",                         # no boot logo
            "3",                        # core: never flash
            saved,                      # save the setup
            "INSTALL",
        ]) + "\n"
        p = self.install("--port", "fake", stdin=script)
        out = self.ok(p)
        self.assertIn("BOARD GATE", out)
        self.assertIn("an app may be placed only once", out)
        self.assertIn("no such file", out)
        self.assertIn("WILL BE WRITTEN", out)
        self.assertIn("WILL NOT BE TOUCHED", out)
        self.assertLess(out.index("WILL BE WRITTEN"), out.index("Type INSTALL"))
        with open(saved) as fh:
            s = json.load(fh)
        self.assertEqual(s["schema"], 1)
        self.assertEqual(s["core"], "skip")
        self.assertEqual(sorted(a for a in s["apps"] if a), ["chanmgr.app", "searchqt.app", "settings.app"])
        self.assertEqual(s["apps"][0], "settings.app")
        self.assertEqual(s["apps"][1], "chanmgr.app")
        self.assertIsNone(s["apps"][8])
        self.assertEqual(sorted(s["channels"]), ["FRS", "SIMPLEX"])
        self.assertEqual(s["fm_stations"], "channels/fm/test.csv")
        # the install really happened, from the menu's choices
        nor = self.nor_now()
        self.assertEqual(nor[SLOT_BASE:SLOT_BASE + 1300], app_bytes("settings.app"))
        self.assertEqual(nor[SLOT_BASE + 8 * SLOT_SIZE:SLOT_BASE + 9 * SLOT_SIZE], b"\xff" * SLOT_SIZE)
        self.assertEqual(self.stages(out)["logo"], "skipped")
        # the saved file repeats it
        before = self.nor_now()
        self.ok(self.install("--config", saved, "--yes"))
        self.assertEqual(self.nor_now(), before)

    def test_nothing_is_written_without_the_typed_confirmation(self):
        script = "\n".join(["5RHNRFB-YP07", "d", "ok", "n", "ok", "n", "", "3", "", "install"]) + "\n"   # lower case: not accepted
        p = self.install("--port", "fake", stdin=script)
        self.assertEqual(p.returncode, 2, p.stdout)
        self.assertIn("Stopped. Nothing was written.", p.stdout)
        self.assertEqual(self.log(), [])
        self.assertEqual(self.nor_now(), self.nor0)

    def test_menu_prompts_without_a_tty_are_plain_numbered_prompts(self):
        script = "\n".join(["5RHNRFB-YP07", "d", "ok", "n", "ok", "n", "", "3", ""]) + "\n"
        p = self.install("--port", "fake", "--dry-run", stdin=script)
        self.ok(p)
        self.assertIn("Step 3 of 7", p.stdout)
        self.assertNotIn("\x1b", p.stdout)

    def test_input_that_ends_early_is_refused(self):
        p = self.install("--port", "fake", stdin="5RHNRFB-YP07\n")
        self.assertEqual(p.returncode, 2)
        self.assertIn("the input ended", p.stdout)
        self.assertEqual(self.log(), [])


class Refusals(Box):
    def refused(self, text, **kw):
        p = self.run_config(core="skip", **kw)
        self.assertEqual(p.returncode, 2, p.stdout)
        self.assertIn(text, p.stdout)
        self.assertEqual(self.log(), [])
        self.assertEqual(self.backups(), [])
        return p.stdout

    def test_app_twice(self):
        apps = [APPS[0]] * 2 + [None] * 7
        self.refused("an app may be placed only once", apps=apps)

    def test_wrong_number_of_slots_and_unknown_app(self):
        self.refused("exactly 9 slots", apps=[APPS[0]])
        self.refused("nonesuch.app is not in", apps=["nonesuch.app"] + [None] * 8)

    def test_apps_with_different_abis(self):
        rel = os.path.join(self.box, "release")
        wb(os.path.join(rel, "fm.app"), app_bytes("fm.app", abi=b"\x99\x99\x99\x99"))
        sums = rb(os.path.join(rel, "SHA256SUMS")).decode().replace(sha(app_bytes("fm.app")), sha(app_bytes("fm.app", abi=b"\x99\x99\x99\x99")))
        wb(os.path.join(rel, "SHA256SUMS"), sums.encode())
        self.refused("different ABIs")

    def test_bad_channel_csv_shows_channel_plans_own_message(self):
        bad = os.path.join(self.box, "bad.csv")
        wb(bad, b"Name,Frequency\nTHIS NAME IS FAR TOO LONG,146.52\n")
        out = self.refused("the limit is 12", channels={"WEATHER": "bad.csv"})
        self.assertIn("bad.csv", out)
        wb(bad, b"Foo,Bar\n1,2\n")
        self.refused("not a CHIRP channel CSV", channels={"WEATHER": "bad.csv"})
        self.refused("no group is named", channels={"NOT A GROUP": "bad.csv"})
        wb(bad, b"Name,Frequency\n" + b"".join(b"CH%d,146.%03d\n" % (i, i) for i in range(1, 12)))
        self.refused("free slots of its own", channels={"WEATHER": "bad.csv"})        # 11 channels, weather has 7

    def test_fm_list_out_of_range(self):
        bad = os.path.join(self.box, "fm-bad.csv")
        wb(bad, b"Name,Frequency\nHIGH,120.0\n")
        self.refused("outside 87.5 to 108.0", fm_stations="fm-bad.csv")
        wb(bad, b"Name,Frequency\nODD,99.95\n")
        self.refused("not a multiple of 0.1 MHz", fm_stations="fm-bad.csv")

    def test_wrong_size_logo(self):
        wb(os.path.join(self.box, "small.rgb565"), b"\x00" * 10)
        self.refused("boot logo file", boot_logo="small.rgb565")

    def test_menu_rejects_bad_lists_and_fm_with_the_tools_messages(self):
        bad = os.path.join(self.box, "bad.csv")
        wb(bad, b"Name,Frequency\nTHIS NAME IS FAR TOO LONG,146.52\n")
        fmbad = os.path.join(self.box, "fm-bad.csv")
        wb(fmbad, b"Name,Frequency\nHIGH,120.0\n")
        script = "\n".join(["5RHNRFB-YP07", "d", "ok", "a " + bad, "SIMPLEX", "ok", "p " + fmbad, "n", "", "3", "", "install"]) + "\n"
        p = self.install("--port", "fake", stdin=script)
        self.assertIn("the limit is 12", p.stdout)
        self.assertIn("the list was not added", p.stdout)
        self.assertIn("outside 87.5 to 108.0", p.stdout)
        self.assertEqual(self.log(), [])


class SetupFile(Box):
    def load(self):
        spec_path = os.path.join(SRC, "install.py")
        import importlib.util
        spec = importlib.util.spec_from_file_location("install_under_test", spec_path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        return mod

    def test_round_trip_and_relative_paths(self):
        m = self.load()
        os.makedirs(os.path.join(self.box, "sub"))
        cfg = os.path.join(self.box, "sub", "s.json")
        s = self.setup(release="../release", channels={"FRS": "../channels/frs.csv"}, fm_stations="../channels/fm/test.csv", boot_logo="../logo.rgb565")
        with open(cfg, "w") as f:
            json.dump(s, f)
        loaded = m.load_setup(cfg)
        self.assertEqual(loaded["release"], os.path.join(self.box, "release"))               # relative to the FILE, not the cwd
        self.assertEqual(loaded["channels"]["FRS"], os.path.join(self.box, "channels", "frs.csv"))
        again = os.path.join(self.box, "sub", "again.json")
        m.save_setup(loaded, again)
        self.assertEqual(m.load_setup(again), loaded)
        with open(again) as fh:
            saved = json.load(fh)
        self.assertEqual(saved["release"], "../release")                  # written relative to the file again
        self.assertEqual(saved["channels"], {"FRS": "../channels/frs.csv"})
        self.assertEqual(saved["schema"], 1)
        with self.assertRaises(m.Refused):
            m.save_setup(loaded, again)                                                         # never overwrites
        # run from another working directory: the same result
        self.dispatch(reboot_silent=0)
        self.ok(self.install("--config", cfg, "--yes", "--dry-run"))

    def test_unknown_schema_and_unknown_keys_are_refused(self):
        for raw, text in (({"schema": 2}, "schema 2"), ({}, "schema None"), ({"schema": 1, "prot": "x"}, "unknown setting"),
                          ({"schema": 1, "core": "maybe"}, "'core' must be")):
            with self.subTest(raw=raw):
                cfg = os.path.join(self.box, "bad.json")
                with open(cfg, "w") as f:
                    json.dump(raw, f)
                p = self.install("--config", cfg, "--yes")
                self.assertEqual(p.returncode, 2, p.stdout)
                self.assertIn(text, p.stdout)
                self.assertEqual(self.log(), [])
        cfg = os.path.join(self.box, "notjson.json")
        wb(cfg, b"{")
        self.assertEqual(self.install("--config", cfg).returncode, 2)

    def test_handshake_strings_come_from_the_tools(self):
        m = self.load()
        self.assertEqual(m.const_from("cps_tool.py", "HANDSHAKE"), b"PROGRAMBF-5RH")
        self.assertEqual(m.const_from("radio_read.py", "IDENT"), b"PROGRAMBFNORMALU")


if __name__ == "__main__":
    unittest.main(verbosity=2)
