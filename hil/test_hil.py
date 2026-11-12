"""Offline tests for the HIL pipeline (no camera, no radio, no pyserial).

  python3 hil/test_hil.py        (needs Pillow + numpy; run `firmware/source-changes/simulator/run_scenarios.sh` first
                                  only if you regenerate goldens)
"""
import contextlib, glob, io, os, subprocess, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import injector as inj
import scenario as sc
import vision
from PIL import Image

SIM = os.path.join(os.path.dirname(HERE), "firmware", "source-changes", "simulator")
GOLDEN = os.path.join(SIM, "tests", "golden")
SCEN = os.path.join(SIM, "tests", "scenarios")


def _read(path):
    with open(path) as f:
        return f.read()


def _write(path, text):
    with open(path, "w") as f:
        f.write(text)


def goldens():
    return {os.path.relpath(p, GOLDEN)[:-4]: Image.open(p) for p in sorted(glob.glob(GOLDEN + "/*/*.png"))}


def camera_view(img):
    frame, corners = vision.synthetic_camera(img)
    return vision.rectify(frame, corners)


class ScenarioParsing(unittest.TestCase):
    def test_all_simulator_scenarios_parse(self):
        files = glob.glob(SCEN + "/*.txt")
        self.assertTrue(files)
        for f in files:
            self.assertTrue(sc.parse(_read(f)), f)

    def test_bad_steps_are_rejected(self):
        for bad in ("key banana", "key", "wait -1", "shot", "dance"):
            with self.assertRaises(ValueError, msg=bad):
                sc.parse(bad)

    def test_sim_only_steps_are_reported_not_skipped_silently(self):
        steps = sc.parse(_read(SCEN + "/hamtools_idtimer.txt"))
        ops = {(s.op, tuple(s.args[:1])) for s in sc.sim_only(steps)}
        self.assertIn(("step", ("1000",)), ops)
        self.assertIn(("expect", ("backlight",)), ops)

    def test_tx_scenarios_are_flagged(self):
        self.assertTrue(sc.uses_tx(sc.parse("allow tx\nkey menu")))
        self.assertFalse(sc.uses_tx(sc.parse("key menu")))


class Injector(unittest.TestCase):
    def test_ptt_is_never_pressable(self):
        with self.assertRaises(inj.InjectorError):
            inj.FakeInjector().press("ptt")

    def test_press_time_is_bounded(self):
        f = inj.FakeInjector()
        for ms in (0, inj.MAX_MS + 1):
            with self.assertRaises(inj.InjectorError):
                f.press("menu", ms)
        f.press("menu", inj.MAX_MS)

    def test_firmware_source_refuses_ptt(self):
        fw = _read(os.path.join(HERE, "injector_fw", "main.py"))
        self.assertIn('assert "ptt" not in keymap', fw)
        km = _read(os.path.join(HERE, "injector_fw", "keymap.example.json"))
        self.assertNotIn("ptt", km)


class Vision(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.g = goldens()
        cls.cam = {k: camera_view(v) for k, v in cls.g.items()}

    def test_rectify_recovers_every_golden_through_a_synthetic_webcam(self):
        for k, v in self.g.items():
            self.assertGreaterEqual(vision.similarity(self.cam[k], v), 0.80, k)

    # Apps whose screens share the house chrome (light title bar, dark hint strip at the bottom): the firmware's shared
    # list, and TEXT, SPECTRUM and BAND SWEEP, which were given the same bars (a quiet frame of any of them is mostly
    # that chrome, so the webcam check cannot tell them apart; the simulator compares pixel-exactly).
    LIST_APPS = {"chanmgr", "settings", "fm", "text", "spectrum", "bandsweep"}   # fm: its station list is the shared list too

    def test_wrong_tool_screen_is_rejected_with_margin(self):
        app = lambda k: k.split("/")[0].split("_")[0]          # hamtools, foxhunt, settings, ...
        raw = {k: v.tobytes() for k, v in self.g.items()}
        both_lists = lambda a, b: app(a) in self.LIST_APPS and app(b) in self.LIST_APPS
        worst_other = max(vision.similarity(self.cam[a], self.g[b]) for a in self.g for b in self.g
                          if app(a) != app(b) and raw[a] != raw[b] and not both_lists(a, b))
        # The correct screen scores >= 0.857, so the default 0.80 threshold separates different tools.
        self.assertLess(worst_other, 0.75)
        self.assertGreater(min(vision.similarity(self.cam[k], self.g[k]) for k in self.g), 0.80)

    def test_known_limit_short_lists_of_different_apps_look_alike(self):
        """Documented limitation (measured 0.956 after the list got a light title bar and a blue selection bar,
        0.645 before): a short CHAN MGR list and a short SETTINGS list are the same chrome with a few words of
        text, and the webcam check cannot tell them apart. The simulator checks list text pixel-exactly."""
        app = lambda k: k.split("/")[0].split("_")[0]
        worst = max(vision.similarity(self.cam[a], self.g[b]) for a in self.g for b in self.g
                    if app(a) != app(b) and app(a) in self.LIST_APPS and app(b) in self.LIST_APPS)
        self.assertGreaterEqual(worst, 0.80)

    def test_overdue_banner_vs_armed_is_detected(self):
        a = self.cam["hamtools_idtimer/overdue"]
        self.assertLess(vision.similarity(a, self.g["hamtools_idtimer/armed_10_00"]), 0.70)

    def test_known_limit_small_text_changes_are_not_detected(self):
        """Documented limitation: EVERY 10:00 vs EVERY 12:00 is invisible to the webcam check
        at the default threshold. Fine text is verified pixel-exactly by the simulator."""
        a = self.cam["hamtools_idtimer/idle"]
        self.assertGreaterEqual(vision.similarity(a, self.g["hamtools_idtimer_interval/interval_12m"]), 0.80)


class DryRun(unittest.TestCase):
    def run_hil(self, scenario, golden):
        p = subprocess.run([sys.executable, os.path.join(HERE, "hil_run.py"),
                            os.path.join(SCEN, scenario), "--golden", golden, "--dry-run",
                            "--out", tempfile.mkdtemp(prefix="hil-test-")],
                           capture_output=True, text=True, timeout=300)
        return p

    def test_dry_run_passes_for_each_scenario_with_screenshots(self):  # and refuses the ones that transmit
        for f in sorted(glob.glob(SCEN + "/*.txt")):
            p = self.run_hil(os.path.basename(f), GOLDEN)
            if sc.uses_tx(sc.parse(_read(f))):
                # a scenario that transmits must be refused by the rig, never run
                self.assertNotEqual(p.returncode, 0, f)
                self.assertIn("refusing without --allow-tx", p.stderr + p.stdout, f)
                continue
            self.assertEqual(p.returncode, 0, f"{f}\n{p.stdout}\n{p.stderr}")
            self.assertIn("0 did not", p.stdout)

    def test_dry_run_fails_when_goldens_are_for_the_wrong_screens(self):
        with tempfile.TemporaryDirectory() as d:
            os.makedirs(f"{d}/hamtools_wavelength")
            # serve the *grid* screens as if they were the wavelength goldens
            for shot in ("default_146_520", "typed_446_000", "typed_7_074"):
                Image.open(f"{GOLDEN}/hamtools_grid/greenwich.png").save(f"{d}/hamtools_wavelength/{shot}.png")
            # camera shows what the golden dir says, so the mismatch must come from comparing to the real ones
            p = subprocess.run([sys.executable, "-c", f"""
import sys; sys.path.insert(0, {HERE!r})
import vision
from PIL import Image
real = Image.open({GOLDEN!r} + '/hamtools_wavelength/default_146_520.png')
shown = Image.open({d!r} + '/hamtools_wavelength/default_146_520.png')
f, c = vision.synthetic_camera(shown)
print(vision.similarity(vision.rectify(f, c), real))
"""], capture_output=True, text=True)
            self.assertLess(float(p.stdout), 0.80)

    def test_tx_scenario_is_refused_without_flag(self):
        with tempfile.TemporaryDirectory() as d:
            s = os.path.join(d, "tx.txt")
            _write(s, "allow tx\nkey menu\nshot a\n")
            p = subprocess.run([sys.executable, os.path.join(HERE, "hil_run.py"), s, "--golden", d, "--dry-run"],
                               capture_output=True, text=True)
            self.assertNotEqual(p.returncode, 0)

    def test_dry_run_presses_the_expected_keys(self):
        import hil_run
        steps = sc.parse(_read(SCEN + "/hamtools_idtimer.txt"))
        fake = inj.FakeInjector()
        waits = []
        hil_run.run([s for s in steps if s.op != "shot"], GOLDEN, "hamtools_idtimer", fake,
                    None, None, 0.8, tempfile.mkdtemp(), sleep=waits.append)
        self.assertEqual(waits, [599.0, 2.0, 30.0, 0.2])   # the waits the scenario asks for, not slept
        self.assertEqual(fake.log, [("star", 120), ("menu", 120), ("down", 120), ("exit", 120), ("exit", 1300)])

    def test_hold_is_longer_than_the_radios_long_press_and_shorter_than_its_repeat(self):
        import hil_run, re
        kp = _read(os.path.join(os.path.dirname(HERE), "firmware", "firmware-source", "src", "device", "keypad.rs"))
        long_ms = int(re.search(r"const LONG_PRESS_TICKS: u32 = ([\d_]+);", kp).group(1).replace("_", "")) // 10
        rep_ms = int(re.search(r"const REPEAT_FIRE_TICKS: u32 = ([\d_]+);", kp).group(1).replace("_", "")) // 10
        self.assertGreater(hil_run.HOLD_MS, long_ms + 100)      # 900 ms (the first version) is a Single on the radio
        self.assertLess(hil_run.HOLD_MS, rep_ms)

    def test_hold_is_only_allowed_on_keys_the_keypad_can_long_press(self):
        import re
        kp = _read(os.path.join(os.path.dirname(HERE), "firmware", "firmware-source", "src", "device", "keypad.rs"))
        body = kp[kp.index("fn supports_long"):kp.index("fn supports_repeat")]
        names = set(re.findall(r"KeyId::(\w+)", body))
        ours = {"up": "Up", "down": "Down", "exit": "Exit", "star": "Asterisk", "pound": "Pound", "0": "Digit0", "ab": "Ab", "vm": "Vm", "band": "Band", "side1": "Side1", "side2": "Side2"}
        self.assertEqual({ours[k] for k in sc.LONG_KEYS}, names)
        with self.assertRaises(ValueError):
            sc.parse("hold menu\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)
