"""Tests for channel_plan.py (CSV -> channel image) incl. an end-to-end restore into the FAKE radio.

  scripts/dev.sh tools-test        (or: python3 firmware/tests/test_channel_plan.py)

Ground-truth bytes are the first slots of the factory image as CHIRP decoded them from a real radio (see AGENTS/README notes).
Nothing here opens a real port; the end-to-end test uses firmware/tests/fakeserial.
"""
import importlib.util, os, subprocess, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from test_recovery_tools import Box, TOOLS, SLOT, SLOTS, rb, wb  # noqa: E402

spec = importlib.util.spec_from_file_location("channel_plan", os.path.join(TOOLS, "channel_plan.py"))
cp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cp)

HEAD = "Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,Mode,TStep,Skip,Comment\n"
FACTORY = os.path.join(os.path.dirname(os.path.dirname(HERE)), "my-backup", "factory_memory.img")


def row(name="", freq="146.520", duplex="", off="0", tone="", r="88.5", c="88.5", dcs="023", pol="NN", mode="FM", skip="", power=None):
    return {"Name": name, "Frequency": freq, "Duplex": duplex, "Offset": off, "Tone": tone, "rToneFreq": r, "cToneFreq": c,
            "DtcsCode": dcs, "DtcsPolarity": pol, "Mode": mode, "Skip": skip, **({"Power": power} if power else {})}


def csv_text(rows):
    cols = ["Location", "Name", "Frequency", "Duplex", "Offset", "Tone", "rToneFreq", "cToneFreq", "DtcsCode", "DtcsPolarity",
            "Mode", "Skip", "Power", "Detail"]
    return ",".join(cols) + "\n" + "".join(",".join(str(r.get(k, "")) for k in cols) + "\n" for r in rows)


class Encoding(unittest.TestCase):
    def enc(self, **kw):
        return cp.encode(row(**kw))[0]

    def test_ground_truth_ctcss_slots(self):
        for freq, tone, want in (("452.125", "69.3", "b502"), ("453.225", "91.5", "9303"), ("454.325", "136.5", "5505"),
                                 ("455.425", "151.4", "ea05"), ("456.525", "192.8", "8807"), ("457.625", "241.8", "7209")):
            rec = self.enc(freq=freq, tone="TSQL", c=tone, power="8W")
            self.assertEqual(rec[8:12].hex(), want + want, freq)
            self.assertEqual(rec[12:16].hex(), "00000004")
            self.assertEqual(rec[16:], b"\xff" * 16)
        self.assertEqual(self.enc(freq="452.125", tone="TSQL", c="69.3", power="8W")[:16].hex(), "0025214500252145b502b50200000004")

    def test_ground_truth_dcs_slots(self):
        for freq, code, pol, want in (("458.725", "025", "NN", "0200"), ("459.825", "134", "NN", "1800"), ("461.925", "274", "NN", "3300"),
                                      ("462.225", "346", "NN", "3b00"), ("463.325", "503", "NN", "5000"),
                                      ("464.425", "073", "RR", "7800"), ("465.525", "703", "RR", "cb00")):
            rec = self.enc(freq=freq, tone="DTCS", dcs=code, pol=pol)
            self.assertEqual(rec[8:12].hex(), want + want, code)

    def test_no_tone_and_tone_mode(self):
        self.assertEqual(self.enc()[8:12], bytes(4))
        t = self.enc(tone="Tone", r="100.0", c="123.0")                 # Tone: tx only
        self.assertEqual(t[8:12].hex(), "0000e803")

    def test_split_dcs_polarity(self):
        rec = cp.encode({**row(tone="DTCS", dcs="023", pol="NR"), "RxDtcsCode": "025"})[0]
        self.assertEqual(rec[10:12].hex(), "0100")                      # tx: 023 normal
        self.assertEqual(rec[8:10].hex(), f"{2 + 105:02x}00")           # rx: 025 reversed

    def test_factory_image_if_present(self):
        if not os.path.exists(FACTORY):
            self.skipTest("my-backup/factory_memory.img not present")
        img = rb(FACTORY)
        for slot, kw in ((0, dict(freq="452.125", tone="TSQL", c="69.3")), (11, dict(freq="464.425", tone="DTCS", dcs="073", pol="RR"))):
            self.assertEqual(self.enc(power="8W", **kw)[:12], img[slot * SLOT:slot * SLOT + 12])

    def test_frequency_power_flags_name(self):
        rec = self.enc(name="HELLO", freq="147.120", duplex="+", off="0.6", mode="NFM", skip="S", power="5.0W")
        self.assertEqual(rec[0:4].hex(), "00207114")                   # 147.120
        self.assertEqual(rec[4:8].hex(), "00207714")                   # 147.720
        self.assertEqual(rec[14], 2)                                    # 5 W -> mid
        self.assertEqual(rec[15], 0x40)                                 # narrow, skip
        self.assertEqual(rec[20:32], b"HELLO" + b"\xff" * 7)
        self.assertEqual(self.enc(freq="146.52", duplex="-", off="0.6", power="1W")[4:8].hex(), "00205914")   # 145.920
        for p, want in (("8W", 0), ("5W", 2), ("2W", 1), ("0.5W", 1), ("", 2), ("high", 2)):
            self.assertEqual(self.enc(power=p or None, freq="146.52")[14], want, p)

    def test_receive_only_and_simplex(self):
        self.assertEqual(self.enc(duplex="off")[4:8], b"\xff" * 4)
        rec = self.enc(duplex="")
        self.assertEqual(rec[4:8], rec[0:4])

    def test_decode_round_trip(self):
        cases = [row("A", "146.940", "-", "0.6", "TSQL", c="110.9"), row("B", "462.675", "", "0", "Tone", r="141.3", power="5W"),
                 row("C", "440.5125", "+", "5", "DTCS", dcs="754", pol="RN", mode="NFM", skip="S"), row("D", "162.4", "off")]
        for r in cases:
            rec, _ = cp.encode(r)
            d = cp.decode(rec)
            self.assertEqual(d["name"], r["Name"])
            self.assertAlmostEqual(d["rx"], float(r["Frequency"]), places=5)
            self.assertEqual(d["bw"], "narrow" if r["Mode"] == "NFM" else "wide")
            self.assertEqual(d["scan"], "skip" if r["Skip"] == "S" else "scan")
        self.assertEqual(cp.decode(cp.encode(cases[0])[0])["tones"], "TSQL 110.9")
        self.assertEqual(cp.decode(cp.encode(cases[1])[0])["tones"], "T141.3")
        self.assertEqual(cp.decode(cp.encode(cases[2])[0])["tones"], "D754R/754N")
        self.assertEqual(cp.decode(cp.encode(cases[0])[0])["shift"], "-0.6")
        self.assertEqual(cp.decode(cp.encode(cases[3])[0])["shift"], "RX only")
        self.assertIsNone(cp.decode(b"\xff" * 32))

    def test_decode_accepts_nul_padded_name(self):
        rec = bytearray(cp.encode(row("AB"))[0])
        rec[22:32] = b"\x00" * 10
        self.assertEqual(cp.decode(bytes(rec))["name"], "AB")

    def test_warnings(self):
        self.assertTrue(any("transmit plan" in w for w in cp.decode(self.enc(freq="162.4"))["warns"]))
        self.assertEqual(cp.decode(self.enc(freq="146.52"))["warns"], [])
        self.assertTrue(cp.encode(row(duplex="split"))[1])
        self.assertTrue(cp.encode(row(tone="Cross"))[1])

    def test_encode_errors(self):
        bad = [row(name="THIRTEEN CHARS"), row(name="café"), row(freq="abc"), row(freq=""), row(freq="5"), row(freq="600"),
               row(freq="1200"), row(freq="999", duplex="+", off="5"), row(duplex="+", off="0"), row(duplex="+", off="x"),
               row(tone="Tone", r="30"), row(tone="Tone", r=""), row(tone="DTCS", dcs="999")]
        for r in bad:
            with self.assertRaises(cp.PlanError, msg=str(r)):
                cp.encode(r)


def open_log(radio):
    p = os.path.join(radio, "log.txt")
    if not os.path.exists(p):
        return []
    with open(p) as f:
        return f.read().split("\n")


class Groups(unittest.TestCase):
    def test_parses_firmware_table(self):
        g = {x[0]: x for x in cp.load_groups()}
        self.assertEqual(g["WEATHER"][1:], ("WX", [(1, 7)]))
        self.assertEqual(g["REPEATERS"][2], [(234, 304), (327, 344)])
        self.assertEqual(g["ALL"][2], [(1, 1000)])
        self.assertEqual(g["CUSTOM RANGE"][2], [(1, 1000)])
        self.assertEqual(g["FED & GOV"][1], "FED")
        self.assertEqual(len(g), 13)

    def test_unparseable_table_fails_clearly(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "g.rs")
            wb(p, b"fn nothing() {}")
            with self.assertRaises(cp.PlanError) as e:
                cp.load_groups(p)
            self.assertIn("cannot parse", str(e.exception))
            with self.assertRaises(cp.PlanError):
                cp.load_groups(os.path.join(d, "missing.rs"))

    def test_name_matching(self):
        g = cp.load_groups()
        for stem, want in (("repeaters", "REPEATERS"), ("RPT", "REPEATERS"), ("weather", "WEATHER"), ("Police-Fire_EMS", "POLICE FIRE EMS"),
                           ("pd-fd", "POLICE FIRE EMS"), ("fed-gov", "FED & GOV"), ("frs-gmrs-murs", "FRS GMRS MURS"), ("all", "ALL")):
            self.assertEqual(cp.find_group(g, stem)[0], want, stem)
        with self.assertRaises(cp.PlanError):
            cp.find_group(g, "nonsense")


class Build(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.TemporaryDirectory(prefix="chplan-")
        self.dir = self.d.name

    def tearDown(self):
        self.d.cleanup()

    def put(self, name, rows):
        wb(os.path.join(self.dir, name), csv_text(rows).encode())

    def run_cli(self, *args):
        return subprocess.run([sys.executable, os.path.join(TOOLS, "channel_plan.py"), *args], capture_output=True, text=True, timeout=60)

    def test_group_placement_ignores_location(self):
        self.put("weather.csv", [dict(row("W1", "162.55"), Location=99), dict(row("W2", "162.4"), Location=3)])
        self.put("repeaters.csv", [row(f"R{i}", f"146.{i:03d}") for i in range(71)])
        table, entries, errors, summary = cp.build(self.dir)
        self.assertEqual(errors, [])
        slots = {e["slot"]: cp.decode(e["record"])["name"] for e in entries}
        self.assertEqual(slots[0], "W1")
        self.assertEqual(slots[1], "W2")
        self.assertEqual(slots[233], "R0")                  # printed 234
        self.assertEqual(slots[303], "R70")                  # printed 304 = last slot REPEATERS owns
        self.assertNotIn(326, slots)                         # printed 327-344 belong to the smaller GMRS group
        self.assertEqual(len(table), SLOT * SLOTS)
        self.assertEqual(table[2 * SLOT:3 * SLOT], b"\xff" * SLOT)
        self.assertIn(("WEATHER", 2, 7), summary)
        self.assertIn(("REPEATERS", 71, 71), summary)

    def test_all_csv_uses_location(self):
        self.put("all.csv", [dict(row("P", "146.52"), Location=500)])
        _, entries, errors, _ = cp.build(self.dir)
        self.assertEqual((errors, [e["slot"] for e in entries]), ([], [499]))
        self.put("all.csv", [dict(row("P", "146.52"), Location=0)])
        self.assertTrue(cp.build(self.dir)[2])
        self.put("all.csv", [dict(row("P", "146.52"), Location="")])
        self.assertTrue(cp.build(self.dir)[2])

    def test_too_many_channels(self):
        self.put("weather.csv", [row(f"W{i}", "162.4") for i in range(8)])
        errors = cp.build(self.dir)[2]
        self.assertTrue(any("8 channels" in e and "7 free slots" in e for e in errors), errors)

    def test_detail_column_is_checked_but_not_stored(self):
        self.put("weather.csv", [dict(row("W1", "162.55"), Detail="Richardson")])
        table, entries, errors, _ = cp.build(self.dir)
        self.assertEqual(errors, [])
        self.assertNotIn(b"Richardson", table)
        self.put("weather.csv", [dict(row("W1", "162.55"), Detail="x" * 17)])
        self.assertTrue(any("Detail" in e for e in cp.build(self.dir)[2]))

    def test_overlapping_groups_use_their_own_slots(self):
        """GMRS (305-344) contains FRS (305-326): a GMRS file starts at 327 even when FRS is not full."""
        self.put("frs.csv", [row("F1", "462.5625")])
        self.put("gmrs.csv", [row("G1", "462.55")])
        self.put("repeaters.csv", [row("R1", "146.94")])
        _, entries, errors, _ = cp.build(self.dir)
        self.assertEqual(errors, [])
        self.assertEqual(sorted(e["slot"] + 1 for e in entries), [234, 305, 327])
        # a group made only of smaller groups has nothing of its own
        self.put("f-g-m.csv", [row("X", "462.5625")])
        self.assertTrue(any("free slots of its own" in e for e in cp.build(self.dir)[2]))

    def test_all_csv_takes_slots_first_and_duplicates_are_refused(self):
        self.put("all.csv", [dict(row("A", "146.52"), Location=305)])
        self.put("frs.csv", [row("F1", "462.5625")])
        _, entries, errors, _ = cp.build(self.dir)
        self.assertEqual(errors, [])
        self.assertEqual(sorted(e["slot"] + 1 for e in entries), [305, 306])
        self.put("all.csv", [dict(row("A", "146.52"), Location=305), dict(row("B", "146.52"), Location=305)])
        self.assertTrue(any("already claimed" in e for e in cp.build(self.dir)[2]))

    def test_row_errors_reported_with_file_and_line(self):
        self.put("weather.csv", [row("THIS NAME IS TOO LONG", "162.4"), row("OK", "abc"), row("OK2", "162.4")])
        errors = cp.build(self.dir)[2]
        self.assertEqual(len(errors), 2)
        self.assertTrue(all(e.startswith("weather.csv line ") for e in errors), errors)

    def test_unknown_file_name_and_bad_header(self):
        self.put("bogus.csv", [row()])
        self.assertTrue(any("no group is named 'bogus'" in e for e in cp.build(self.dir)[2]))
        os.remove(os.path.join(self.dir, "bogus.csv"))
        wb(os.path.join(self.dir, "weather.csv"), b"a,b\n1,2\n")
        self.assertTrue(any("missing column" in e for e in cp.build(self.dir)[2]))
        os.remove(os.path.join(self.dir, "weather.csv"))
        with self.assertRaises(cp.PlanError):
            cp.build(self.dir)                              # empty directory
        self.put("custom-range.csv", [row()])
        self.assertTrue(any("CUSTOM RANGE" in e for e in cp.build(self.dir)[2]))

    def test_base_keeps_unassigned_slots(self):
        base = bytearray(b"\xff" * (SLOT * SLOTS))
        base[0:SLOT] = cp.encode(row("OLD0", "162.4"))[0]               # inside weather: replaced
        base[10 * SLOT:11 * SLOT] = cp.encode(row("OLD10", "146.52"))[0]  # outside: kept
        self.put("weather.csv", [row("NEW0", "162.55")])
        table, _, errors, _ = cp.build(self.dir, bytes(base))
        self.assertEqual(errors, [])
        self.assertEqual(cp.decode(table[0:SLOT])["name"], "NEW0")
        self.assertEqual(table[SLOT:2 * SLOT], b"\xff" * SLOT)          # weather slot 2 not assigned: base content (empty) stays
        self.assertEqual(table[10 * SLOT:11 * SLOT], bytes(base[10 * SLOT:11 * SLOT]))

    def test_cli_build_show_and_base_sizes(self):
        self.put("weather.csv", [row("WX1", "162.55")])
        out = os.path.join(self.dir, "o", "out.img")
        p = self.run_cli("build", self.dir, "-o", out)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertIn("Location column is ignored", p.stdout)
        self.assertIn("WEATHER: 1 / 7", p.stdout)
        img = rb(out)
        self.assertEqual(len(img), 0x8000)
        self.assertEqual(img[SLOT * SLOTS:], b"\xff" * (0x8000 - SLOT * SLOTS))
        s = self.run_cli("show", out)
        self.assertEqual(s.returncode, 0, s.stderr)
        self.assertIn("WX1", s.stdout)
        self.assertIn("162.55", s.stdout)
        for size in (0x7D00, 0x8380):                                    # --base accepts all three sizes; show reads backups
            b = os.path.join(self.dir, f"b{size}.img")
            wb(b, (img[:0x7D00] + b"\xff" * 0x300)[:0x8000].ljust(size, b"\x00") if size != 0x7D00 else img[:0x7D00])
            self.assertEqual(self.run_cli("build", self.dir, "-o", os.path.join(self.dir, f"o{size}.img"), "--base", b).returncode, 0)
            self.assertEqual(self.run_cli("show", b).returncode, 0)
        wb(os.path.join(self.dir, "bad.img"), b"123")
        self.assertNotEqual(self.run_cli("show", os.path.join(self.dir, "bad.img")).returncode, 0)

    def test_cli_error_exits_nonzero_and_writes_nothing(self):
        self.put("weather.csv", [row("TOO LONG A NAME", "162.4")])
        out = os.path.join(self.dir, "out.img")
        p = self.run_cli("build", self.dir, "-o", out)
        self.assertEqual(p.returncode, 1)
        self.assertIn("12", p.stderr)
        self.assertFalse(os.path.exists(out))

    def test_real_channel_lists_build(self):
        """Whatever is in channels/ must build without errors, and repeaters must start at printed channel 234."""
        real = os.path.join(os.path.dirname(os.path.dirname(HERE)), "channels")
        if not os.path.exists(os.path.join(real, "repeaters.csv")):
            self.skipTest("channels/repeaters.csv not present")
        table, entries, errors, _ = cp.build(real)
        self.assertEqual(errors, [])
        self.assertIn(233, [e["slot"] for e in entries])


class EndToEnd(Box):
    """CSV -> image -> restore_backup.py into the FAKE radio -> channel table matches; empty slots erase."""

    def test_restore_into_fake_radio(self):
        csvdir = os.path.join(self.box, "csv")
        os.mkdir(csvdir)
        rows = [dict(row("WX ONE", "162.55", mode="NFM"), Location=1), dict(row("RPT A", "146.94", "-", "0.6", "TSQL", c="110.9", power="5W"), Location=2)]
        wb(os.path.join(csvdir, "weather.csv"), csv_text(rows[:1]).encode())
        wb(os.path.join(csvdir, "repeaters.csv"), csv_text(rows[1:]).encode())
        img = os.path.join(self.box, "channels.img")
        p = subprocess.run([sys.executable, os.path.join(TOOLS, "channel_plan.py"), "build", csvdir, "-o", img], capture_output=True, text=True)
        self.assertEqual(p.returncode, 0, p.stderr)
        want = rb(img)
        self.assertEqual(len(want), 0x8000)
        before = self.nor_now()
        dry = self.run_tool("restore_backup.py", "fake", img, "--before", os.path.join(self.box, "undo.img"))
        self.assertIn("dry run", dry.stdout)
        self.assertEqual(self.nor_now(), before)                          # dry run wrote nothing
        self.assertFalse(any(l.startswith("W") for l in open_log(self.radio)))
        self.run_tool("restore_backup.py", "fake", img, "--before", os.path.join(self.box, "undo.img"), "--write")
        now = self.nor_now()
        self.assertEqual(now[:SLOT * SLOTS], want[:SLOT * SLOTS])         # the whole channel table is the image
        self.assertEqual(cp.decode(now[0:SLOT])["name"], "WX ONE")
        self.assertEqual(cp.decode(now[233 * SLOT:234 * SLOT])["tones"], "TSQL 110.9")
        self.assertEqual(now[5 * SLOT:6 * SLOT], b"\xff" * SLOT)         # slot that held other data is now empty
        self.assertEqual(now[0x8000:], before[0x8000:])                   # VFO, contacts, settings, calibration untouched
        self.assertEqual(rb(os.path.join(self.box, "undo.img"))[:SLOT * SLOTS], before[:SLOT * SLOTS])


if __name__ == "__main__":
    unittest.main()
