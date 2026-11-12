"""The release bundle (firmware/tools/bundle_release.py): what goes in, what must never go in, checksums, the pin check.
Runs with the standard library only. (scripts/dev.sh tools-test)"""
import ast, hashlib, importlib.util, json, os, re, shutil, subprocess, sys, tempfile, unittest, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
ROOT = os.path.dirname(FW)
spec = importlib.util.spec_from_file_location("bundle_release", os.path.join(FW, "tools", "bundle_release.py"))
br = importlib.util.module_from_spec(spec)
spec.loader.exec_module(br)

SLOTS = ["settings.app", "chanmgr.app", "searchqt.app", "spectrum.app", "bandsweep.app",
         "foxhunt.app", "rptfinder.app", "fskprobe.app", "fm.app"]
EXTRAS = ["hamtools.app", "text.app"]


def sha(b):
    return hashlib.sha256(b).hexdigest()


def make_release(d, extras=EXTRAS, mutate=None):
    """A fake `scripts/dev.sh release` output with a valid SHA256SUMS."""
    os.makedirs(d, exist_ok=True)
    blobs = {"fw-default.BF": b"BF" * 50, "fw-default.bin": b"CORE" * 40}
    for n in SLOTS + list(extras):
        blobs[n] = b"OVL1" + n.encode() * 3
    for n, b in blobs.items():
        with open(os.path.join(d, n), "wb") as f:
            f.write(b)
    with open(os.path.join(d, "layout.json"), "w") as f:
        json.dump({"slots": SLOTS, "extras": sorted(extras)}, f)
    names = ["fw-default.BF", "fw-default.bin"] + SLOTS + list(extras)
    with open(os.path.join(d, "SHA256SUMS"), "w") as f:
        f.write("".join(f"{sha(blobs[n])}  {n}\n" for n in names))
    return blobs


def pins_for(d, path, want=None):
    with open(os.path.join(d, "fw-default.bin"), "rb") as f:
        h = sha(f.read())
    with open(path, "w") as f:
        json.dump({"fw-default.bin": want or h, "fw-bk4819.bin": "0" * 64}, f)


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.rel = os.path.join(self.tmp, "release")
        self.blobs = make_release(self.rel)
        self.pins = os.path.join(self.tmp, "pins.json")
        pins_for(self.rel, self.pins)
        self.out = os.path.join(self.tmp, "out")

    def bundle(self, **kw):
        return br.bundle(self.rel, self.out, "v9.9.9", pins_path=self.pins, **kw)


class Contents(Base):
    def test_file_list(self):
        stage, zip_path, files = self.bundle()
        want = {"INSTALL.md", "LICENSE", "docs/FIRST-FLASH-RUNBOOK.md", "firmware/variants.json",
                "firmware/firmware-source/src/app/group_table.rs", "release/layout.json", "release/SHA256SUMS",
                "release/fw-default.BF", "release/fw-default.bin"}
        want |= {f"release/{n}" for n in SLOTS + EXTRAS}
        want |= {f"firmware/source-changes/{t}" for t in ["flash_bf.py", "cps_tool.py", "dump_nor.py", "install_apps.py",
                                                         "restore_backup.py", "channel_extras.py", "channel_plan.py",
                                                         "boot_logo_raw.py", "wrap_bf.py"]}
        want |= {f"channels/{n}" for n in os.listdir(os.path.join(ROOT, "channels")) if n.endswith(".csv")}
        self.assertTrue(want <= set(files), sorted(want - set(files)))
        extra = set(files) - want - {"channels/README.md", "firmware/source-changes/radio_read.py"}
        self.assertEqual(extra, set(), "unexpected files in the bundle")

    def test_installer_is_optional_and_picked_up_when_present(self):
        _, _, files = self.bundle()
        self.assertNotIn("install.py", files)                       # absent today: no failure
        fake = os.path.join(self.tmp, "root")                       # a root that does have one
        shutil.copytree(os.path.join(ROOT, "channels"), os.path.join(fake, "channels"))
        for rel in br.DATA + [f"firmware/source-changes/{t}" for t in br.TOOLS]:
            os.makedirs(os.path.dirname(os.path.join(fake, rel)), exist_ok=True)
            shutil.copy(os.path.join(ROOT, rel), os.path.join(fake, rel))
        os.makedirs(os.path.dirname(os.path.join(fake, br.INSTALLER_SOURCE)), exist_ok=True)
        with open(os.path.join(fake, br.INSTALLER_SOURCE), "w") as f:
            f.write("print('installer')\n")
        files = br.build_stage(self.rel, os.path.join(self.tmp, "stage2"), "v1", root=fake)
        self.assertIn("install.py", files)

    def test_runbook_commands_work_in_the_bundle_layout(self):
        """The bundle mirrors the repo so the tools find ../variants.json and group_table.rs; prove it by running one."""
        stage, _, _ = self.bundle()
        img = os.path.join(self.tmp, "channels.img")
        p = subprocess.run([sys.executable, "-I", os.path.join(stage, "firmware", "source-changes", "channel_plan.py"),
                            "build", os.path.join(stage, "channels"), "-o", img], capture_output=True, text=True, cwd=stage)
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        self.assertGreater(os.path.getsize(img), 1000)

    def test_tool_closure_is_in_the_bundle(self):
        """Every sibling script a bundled tool imports or reads, and every ../ data file it names, is bundled."""
        src = os.path.join(ROOT, "firmware", "source-changes")
        siblings = {n for n in os.listdir(src) if n.endswith(".py")}
        stage, _, files = self.bundle()
        have = {os.path.basename(f) for f in files if f.startswith("firmware/source-changes/")}
        for t in br.TOOLS:
            text = open(os.path.join(src, t), encoding="utf-8").read()
            for node in ast.walk(ast.parse(text)):
                mods = [a.name for a in node.names] if isinstance(node, ast.Import) else \
                       [node.module] if isinstance(node, ast.ImportFrom) and node.module else []
                for m in mods:
                    if m + ".py" in siblings:
                        self.assertIn(m + ".py", have, f"{t} imports {m}")
            for lit in re.findall(r'"(\w+\.py)"', text):
                if lit in siblings:
                    self.assertIn(lit, have, f"{t} reads {lit}")
            for call in re.findall(r'os\.path\.join\(([^()]*"\.\."[^()]*)\)', text):
                parts = re.findall(r'"([^"]+)"', call)
                i = parts.index("..")
                target = os.path.normpath(os.path.join(stage, "firmware", "source-changes", *parts[i:]))
                self.assertTrue(os.path.exists(target), f"{t}: ../ path {parts[i:]} is not in the bundle")


class Personal(Base):
    def test_forbidden_names(self):
        for bad in [".env", ".env.local", "my-backup/factory_memory.img", "build-out/personal/boot-logo.rgb565",
                    "x/personal/anything.bin", "boot-logo.png", "chan.img", "radio.bak", "My-Backup/x"]:
            d = os.path.join(self.tmp, "s", re.sub(r"\W", "_", bad))
            os.makedirs(os.path.join(d, os.path.dirname(bad)), exist_ok=True)
            with open(os.path.join(d, bad), "w") as f:
                f.write("x")
            self.assertTrue(br.forbidden_in(d), bad)
        clean = os.path.join(self.tmp, "clean")
        os.makedirs(os.path.join(clean, "channels"))
        for n in ["channels/frs.csv", "INSTALL.md", "release/fw-default.BF", "firmware/source-changes/flash_bf.py"]:
            os.makedirs(os.path.dirname(os.path.join(clean, n)), exist_ok=True)
            open(os.path.join(clean, n), "w").close()
        self.assertEqual(br.forbidden_in(clean), [])

    def test_a_personal_file_that_reaches_staging_fails_the_bundle_and_leaves_no_tree(self):
        shutil.rmtree(self.rel)
        make_release(self.rel, extras=EXTRAS + ["boot-logo.rgb565"])
        pins_for(self.rel, self.pins)
        with self.assertRaises(br.BundleError) as cm:
            self.bundle()
        self.assertIn("boot-logo.rgb565", str(cm.exception))
        self.assertFalse(os.path.exists(os.path.join(self.out, "uv5rm-v9.9.9")))
        self.assertFalse(os.path.exists(os.path.join(self.out, "assets", "uv5rm-v9.9.9.zip")))

    def test_the_real_repo_inputs_are_clean(self):
        stage, _, _ = self.bundle()
        self.assertEqual(br.forbidden_in(stage), [])


class Checksums(Base):
    def test_sums_match_bundle_and_zip(self):
        stage, zip_path, _ = self.bundle()
        sums = br.parse_sums(open(os.path.join(stage, "release", "SHA256SUMS")).read())
        self.assertEqual(set(sums), {"fw-default.BF", "fw-default.bin"} | set(SLOTS) | set(EXTRAS))
        for n, h in sums.items():                      # in the bundle, and the flat copies next to the zip
            self.assertEqual(br.sha256_file(os.path.join(stage, "release", n)), h, n)
            self.assertEqual(br.sha256_file(os.path.join(self.out, "assets", n)), h, n)
        self.assertEqual(open(os.path.join(self.out, "assets", "SHA256SUMS")).read(),
                         open(os.path.join(self.rel, "SHA256SUMS")).read())
        zsum = br.parse_sums(open(zip_path + ".sha256").read())
        self.assertEqual(zsum, {"uv5rm-v9.9.9.zip": br.sha256_file(zip_path)})
        with zipfile.ZipFile(zip_path) as z:
            self.assertIsNone(z.testzip())
            names = z.namelist()
            self.assertTrue(all(n.startswith("uv5rm-v9.9.9/") for n in names))
            self.assertEqual(sorted(n.split("/", 1)[1] for n in names), br.stage_files(stage))
            self.assertEqual(sha(z.read("uv5rm-v9.9.9/release/fw-default.BF")), sums["fw-default.BF"])

    def test_zip_is_reproducible(self):
        _, z1, _ = self.bundle()
        a = br.sha256_file(z1)
        shutil.rmtree(self.out)
        _, z2, _ = self.bundle()
        self.assertEqual(a, br.sha256_file(z2))

    def test_a_tampered_file_is_refused(self):
        with open(os.path.join(self.rel, "fm.app"), "ab") as f:
            f.write(b"x")
        with self.assertRaises(br.BundleError) as cm:
            self.bundle()
        self.assertIn("fm.app", str(cm.exception))

    def test_a_missing_file_or_unlisted_app_is_refused(self):
        os.remove(os.path.join(self.rel, "text.app"))
        with self.assertRaises(br.BundleError):
            self.bundle()
        make_release(self.rel)
        sums = open(os.path.join(self.rel, "SHA256SUMS")).read().replace("  hamtools.app\n", "  hamtools.app.x\n")
        open(os.path.join(self.rel, "SHA256SUMS"), "w").write(sums)
        with self.assertRaises(br.BundleError):
            self.bundle()


class Pin(Base):
    def test_matching_core_passes(self):
        self.bundle()

    def test_a_core_that_differs_from_the_pin_is_refused_and_nothing_is_published(self):
        pins_for(self.rel, self.pins, want="f" * 64)
        with self.assertRaises(br.BundleError) as cm:
            self.bundle()
        self.assertIn("pins", str(cm.exception))
        self.assertFalse(os.path.exists(os.path.join(self.out, "assets")))

    def test_allow_unpinned_is_explicit(self):
        pins_for(self.rel, self.pins, want="f" * 64)
        self.bundle(allow_unpinned=True)

    def test_cli_exit_status(self):
        pins_for(self.rel, self.pins, want="f" * 64)
        self.assertEqual(br.main(["bundle", "--release", self.rel, "--out", self.out, "--version", "v1"]), 1)
        self.assertEqual(br.main(["bundle", "--release", self.rel, "--out", self.out, "--version", "v1",
                                  "--allow-unpinned"]), 0)

    def test_real_pin_file_pins_the_core(self):
        with open(os.path.join(FW, "expected-hashes.json")) as f:
            self.assertIn("fw-default.bin", json.load(f))


class Notes(unittest.TestCase):
    def test_notes_contents(self):
        sums = "a" * 64 + "  fw-default.BF\n"
        t = br.release_notes("v1.2.3", sums, ["- one (abc1234)", "- two (def5678)"], "v1.2.2", "me/repo", "b" * 64 + "  z.zip\n")
        for s in ["v1.2.3", sums.strip(), "- one (abc1234)", "- two (def5678)", "since v1.2.2", "experimental",
                  "at your own risk", "calibration", "https://github.com/me/repo/blob/v1.2.3/docs/FIRST-FLASH-RUNBOOK.md",
                  "b" * 64]:
            self.assertIn(s, t)

    def test_first_release_and_git_helpers(self):
        self.assertIn("first release", br.release_notes("v1", "x", [], None, ""))

        class R:
            def __init__(self, out, rc=0):
                self.stdout, self.returncode = out, rc
        calls = []

        def runner(cmd, **kw):
            calls.append(cmd)
            return R("v0.9\n") if cmd[1] == "describe" else R("- c1 (1)\n- c2 (2)\n")
        self.assertEqual(br.previous_tag("v1", runner), "v0.9")
        self.assertEqual(br.commits_since("v0.9", "v1", runner), ["- c1 (1)", "- c2 (2)"])
        self.assertIn("v0.9..v1", calls[-1])
        self.assertIsNone(br.previous_tag("v1", lambda *a, **k: R("", 128)))


class Wiring(unittest.TestCase):
    def test_workflow_and_dev_sh_use_the_same_script(self):
        wf = open(os.path.join(ROOT, ".github", "workflows", "release.yml")).read()
        dev = open(os.path.join(ROOT, "scripts", "dev.sh")).read()
        self.assertIn("scripts/dev.sh bundle", wf)
        self.assertIn("bundle_release.py bundle", dev)
        self.assertIn("bundle) cmd_bundle", dev)
        self.assertNotIn("--allow-unpinned", wf)           # a tag build never skips the pin check
        self.assertNotIn("ALLOW_UNPINNED", wf)


if __name__ == "__main__":
    unittest.main(buffer=True)
