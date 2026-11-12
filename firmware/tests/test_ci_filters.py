"""CI path filtering: scripts/ci_changed.py (the table) against .github/workflows/ci.yml (the gates).

For example sets of changed files this asserts which jobs the workflow would run, without GitHub: the script's
`job=true|false` outputs are fed through the `if:` / `needs:` of each job as parsed from the workflow text. PyYAML is
not in the dev image, so the workflow is read with a small line parser (it only needs job names, `needs` and `if`).
Standard library only. (scripts/dev.sh tools-test)"""
import importlib.util, os, re, shutil, subprocess, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
SCRIPT = os.path.join(ROOT, "scripts", "ci_changed.py")
WORKFLOW = os.path.join(ROOT, ".github", "workflows", "ci.yml")
spec = importlib.util.spec_from_file_location("ci_changed", SCRIPT)
cc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cc)


def parse_workflow(path=WORKFLOW):
    """{job: {"needs": [...], "if": str|None, "outputs": {name: expr}}} from the `jobs:` block, in file order."""
    jobs, cur, in_jobs, in_outputs = {}, None, False, False
    with open(path) as f:
        for raw in f:
            line = raw.rstrip("\n")
            if re.match(r"^jobs:\s*$", line):
                in_jobs = True
                continue
            if not in_jobs or not line.strip() or line.lstrip().startswith("#"):
                continue
            if re.match(r"^\S", line):
                in_jobs = False
                continue
            m = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
            if m:
                cur = jobs.setdefault(m.group(1), {"needs": [], "if": None, "outputs": {}})
                in_outputs = False
                continue
            if cur is None:
                continue
            m = re.match(r"^    (needs|if|outputs):\s*(.*?)\s*$", line)
            if m:
                key, val = m.groups()
                in_outputs = key == "outputs"
                if key == "needs":
                    cur["needs"] = [x.strip() for x in val.strip("[]").split(",") if x.strip()]
                elif key == "if":
                    cur["if"] = val
                continue
            if in_outputs:
                m = re.match(r"^      (\w+):\s*(.+?)\s*$", line)
                if m:
                    cur["outputs"][m.group(1)] = m.group(2)
                    continue
            if re.match(r"^    \S", line):
                in_outputs = False
    return jobs


def evaluate(cond, outputs):
    """Evaluate an `if:` made of `needs.changes.outputs.X == 'true'`, always(), &&, ||, parentheses and !."""
    expr = re.sub(r"needs\.changes\.outputs\.(\w+)\s*==\s*'true'", lambda m: str(bool(outputs[m.group(1)])), cond)
    expr = expr.replace("always()", "True").replace("&&", " and ").replace("||", " or ").replace("!", " not ")
    if re.search(r"[^TrueFalsandot() ]", expr):
        raise ValueError(f"cannot evaluate {cond!r} (got {expr!r})")
    return bool(eval(expr, {"__builtins__": {}}, {}))


def would_run(files=None, all_jobs=False, jobs=None):
    """Names of workflow jobs that run for this change (what GitHub would do with the script's outputs)."""
    jobs = jobs or parse_workflow()
    out = cc.jobs_for(files) if not all_jobs else set(cc.ALL)
    outputs = {j: j in out for j in cc.JOBS}
    ran = {"changes"}
    for name, j in jobs.items():
        if name == "changes":
            continue
        needs_ok = all(n in ran for n in j["needs"])
        if j["if"] and "always()" in j["if"]:
            run = evaluate(j["if"], outputs)
        else:
            run = needs_ok and (j["if"] is None or evaluate(j["if"], outputs))
        if run:
            ran.add(name)
    return ran - {"changes", "ci-ok"}


ALL_JOBS = {"gate", "sim", "tools", "hil", "images", "web"}

CASES = [
    # (description, changed files, jobs that must run)
    ("docs only", ["README.md", "AUDIT.md", "docs/USER-GUIDE.md", "channels/README.md", "LICENSE", ".env.sample",
                   "firmware/preview_menus.png", "firmware/firmware-source/README.md"], set()),
    ("one host tool", ["firmware/source-changes/set_boot_logo.py"], {"tools"}),
    ("one host tool and its test", ["firmware/source-changes/flash_bf.py", "firmware/tests/test_flash_bf.py"], {"tools"}),
    ("a channel list", ["channels/frs.csv"], {"tools"}),
    ("docs plus one tool", ["README.md", "firmware/source-changes/cps_tool.py"], {"tools"}),
    # an app the simulator includes by #[path] (simulator/main.rs) and mkapp builds: unit tests, scenarios, overlay build,
    # size budgets and pinned hashes (the `images` job), and the browser test, which serves the images that job built.
    ("simulated app source (hamtools logic)", ["firmware/firmware-source/apps/hamtools/src/logic.rs"],
     {"sim", "images", "web"}),
    ("simulated app source (fm)", ["firmware/firmware-source/apps/fm/src/main.rs"], {"sim", "images", "web"}),
    ("app the simulator does not include today (rptfind): sim still runs", ["firmware/firmware-source/apps/rptfind/src/main.rs"], {"sim", "images", "web"}),
    ("app logic only the simulator uses", ["firmware/source-changes/apps_new/foxhunt/src/logic.rs"], {"sim"}),
    ("overlay builder", ["firmware/firmware-source/tools/mkapp.py"], {"images", "web"}),
    ("core firmware source", ["firmware/firmware-source/src/main.rs"], {"sim", "images", "web"}),
    ("core UI file the simulator includes", ["firmware/firmware-source/src/ui/list.rs"], {"sim", "images", "web"}),
    ("core build script", ["firmware/tools/build_core.sh"], {"images", "web"}),
    ("memory layout", ["firmware/firmware-source/memory.x"], {"images", "web"}),
    ("group table (also read by channel_plan.py)", ["firmware/firmware-source/src/app/group_table.rs"],
     {"sim", "tools", "images", "web"}),
    ("simulator scenario (golden frames are also the HIL's reference)",
     ["firmware/source-changes/simulator/tests/scenarios/fm_basic.txt"], {"sim", "hil"}),
    ("simulator style goldens", ["firmware/source-changes/simulator/tests/golden-d74/x.png"], {"sim"}),
    ("simulator code", ["firmware/source-changes/simulator/main.rs"], {"sim"}),
    ("HIL tool", ["hil/vision.py"], {"hil"}),
    ("HIL injector firmware", ["hil/injector_fw/main.py"], {"hil"}),
    ("web page", ["web/builder.js"], {"tools", "images", "web"}),
    ("web browser test", ["web/test/run.cjs"], {"images", "web"}),
    ("manifest tool", ["firmware/tools/make_manifest.py"], {"tools", "images", "web"}),
    ("release gate", ["firmware/tools/verify_release.py"], {"gate", "tools", "images"}),
    ("a committed .app", ["firmware/apps/fm.app"], {"gate"}),
    ("the committed .BF", ["firmware/UV-5RM_Custom_Firmware.BF"], {"gate"}),
    ("pinned hashes", ["firmware/expected-hashes.json"], {"images"}),
    ("pin tool", ["firmware/tools/pin_hashes.py"], {"images"}),
    ("release bundle script", ["firmware/tools/bundle_release.py", "firmware/tests/test_bundle_release.py"], {"tools"}),
    ("release workflow", [".github/workflows/release.yml"], ALL_JOBS),
    ("ABI", ["firmware/firmware-source/abi/src/lib.rs"], ALL_JOBS),
    ("ABI with a doc", ["README.md", "firmware/firmware-source/abi/src/lib.rs"], ALL_JOBS),
    ("ci workflow", [".github/workflows/ci.yml"], ALL_JOBS),
    ("repro workflow", [".github/workflows/repro.yml"], ALL_JOBS),
    ("toolchain pin", ["firmware/toolchain.env"], ALL_JOBS),
    ("nightly pin file", ["firmware/firmware-source/rust-toolchain.toml"], ALL_JOBS),
    ("dev.sh", ["scripts/dev.sh"], ALL_JOBS),
    ("Dockerfile", ["dev/Dockerfile"], ALL_JOBS),
    ("the filter table itself", ["scripts/ci_changed.py"], ALL_JOBS),
    ("a file nobody classified", ["somewhere/new/file.xyz"], ALL_JOBS),
    ("not a local replay script", ["scripts/ci-replay.sh"], set()),
]


class Workflow(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.jobs = parse_workflow()

    def test_structure(self):
        j = self.jobs
        self.assertEqual(list(j), ["changes", "gate", "sim", "tools", "hil", "images", "web", "ci-ok"])
        self.assertEqual(set(j["changes"]["outputs"]), set(cc.JOBS), "changes must export one output per filter job")
        for n in cc.JOBS:
            self.assertEqual(j[n]["if"], f"needs.changes.outputs.{n} == 'true'", n)
            self.assertIn("changes", j[n]["needs"], n)
        self.assertEqual(j["web"]["needs"], ["changes", "images"])
        self.assertEqual(j["ci-ok"]["if"], "always()")
        self.assertEqual(set(j["ci-ok"]["needs"]), set(j) - {"ci-ok"}, "ci-ok must need every other job")

    def test_pull_requests_are_not_path_filtered_by_github_itself(self):
        """paths-ignore would leave a required check pending forever on a docs-only PR; the changes job does the filtering."""
        with open(WORKFLOW) as f:
            text = f.read()
        self.assertNotIn("paths-ignore", text)
        self.assertNotIn("\n    paths:", text)

    def test_changes_job_diffs_prs_and_runs_everything_otherwise(self):
        with open(WORKFLOW) as f:
            text = f.read()
        self.assertIn("fetch-depth: 0", text)
        self.assertIn("--base $BASE_SHA", text)
        self.assertIn('--all', text)

    def test_rust_jobs_pin_the_toolchain_and_the_core_build_has_rust_nm(self):
        with open(WORKFLOW) as f:
            text = f.read()
        sections = re.split(r"\n  ([a-z-]+):\n", text)
        by = dict(zip(sections[1::2], sections[2::2]))
        for job in ("sim", "images"):
            self.assertIn("rustup default", by[job], job)
            self.assertIn("firmware/toolchain.env", by[job], job)
        self.assertIn("rustup toolchain install \"$SMALL_TOOLCHAIN\"", by["images"])
        self.assertIn("rust-nm", by["images"])
        self.assertIn("/tmp/shim:$PATH", by["images"].split("Core firmware builds")[1])
        for job in ("gate", "tools", "hil", "web", "changes"):
            self.assertNotIn("cargo", by[job], f"{job} does not need Rust")


class Filters(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.jobs = parse_workflow()

    def test_table_of_changes(self):
        for desc, files, want in CASES:
            with self.subTest(desc):
                self.assertEqual(would_run(files, jobs=self.jobs), want, f"{desc}: {files}")

    def test_push_to_main_runs_everything(self):
        self.assertEqual(would_run(all_jobs=True, jobs=self.jobs), ALL_JOBS)

    def test_web_never_runs_without_the_images_it_serves(self):
        for desc, files, _ in CASES:
            out = cc.jobs_for(files)
            if "web" in out:
                self.assertIn("images", out, desc)

    def test_every_rule_pattern_matches_some_file_or_is_a_known_future_path(self):
        """Stale rules (a renamed directory) would silently stop filtering: each pattern must still match a tracked file."""
        files = tracked_files()
        if files is None:
            self.skipTest("no git history here")
        future = {"firmware/source-changes/install.py"}
        for rx, pat, _ in cc._COMPILED:
            if pat in future:
                continue
            self.assertTrue(any(rx.match(f) for f in files), f"rule {pat!r} matches no tracked file")

    def test_no_tracked_file_is_unclassified(self):
        files = tracked_files()
        if files is None:
            self.skipTest("no git history here")
        loose = [f for f in files if not cc.classify(f)[1]]
        self.assertEqual(loose, [], "add these to RULES in scripts/ci_changed.py (an unknown file runs everything)")


class Script(unittest.TestCase):
    def run_script(self, *args, cwd=ROOT):
        p = subprocess.run([sys.executable, "-I", SCRIPT, *args], capture_output=True, text=True, cwd=cwd)
        out = dict(l.split("=") for l in p.stdout.splitlines())
        return p.returncode, out, p.stderr

    def test_outputs_are_github_output_lines(self):
        rc, out, _ = self.run_script("--files", "firmware/source-changes/dump_nor.py")
        self.assertEqual(rc, 0)
        self.assertEqual(out, {"gate": "false", "sim": "false", "tools": "true", "hil": "false", "images": "false", "web": "false"})
        rc, out, _ = self.run_script("--all")
        self.assertEqual(set(out.values()), {"true"})
        rc, out, _ = self.run_script("--files")
        self.assertEqual(set(out.values()), {"false"})

    def test_pull_request_diff_against_a_base(self):
        if not shutil.which("git"):
            self.skipTest("no git")
        d = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, d, True)

        def g(*a):
            return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false",
                                   "-c", "safe.directory=*", *a], cwd=d, capture_output=True, text=True, check=True).stdout.strip()
        g("init", "-q", "-b", "main")
        for f in ("README.md", "firmware/source-changes/a.py", "hil/x.py"):
            os.makedirs(os.path.dirname(os.path.join(d, f)) or d, exist_ok=True)
            with open(os.path.join(d, f), "w") as fh:
                fh.write("1")
        g("add", "-A"); g("commit", "-qm", "base")
        base = g("rev-parse", "HEAD")
        g("checkout", "-qb", "pr")
        with open(os.path.join(d, "hil/x.py"), "w") as fh:
            fh.write("2")
        g("commit", "-qam", "change hil")
        g("checkout", "-q", "main")                     # base moves on: its own change must not count for the PR
        with open(os.path.join(d, "firmware/source-changes/a.py"), "w") as fh:
            fh.write("2")
        g("commit", "-qam", "base moves")
        g("checkout", "-q", "pr")
        rc, out, err = self.run_script("--base", base, cwd=d)
        self.assertEqual(rc, 0, err)
        self.assertEqual({k for k, v in out.items() if v == "true"}, {"hil"}, err)
        rc, out, err = self.run_script("--base", "0" * 40, cwd=d)     # unreadable base: run everything, never nothing
        self.assertEqual(rc, 0)
        self.assertEqual(set(out.values()), {"true"})
        self.assertIn("running everything", err)


def tracked_files():
    if not shutil.which("git"):
        return None
    p = subprocess.run(["git", "-c", "safe.directory=*", "ls-files"], cwd=ROOT, capture_output=True, text=True)
    return p.stdout.splitlines() if p.returncode == 0 and p.stdout.strip() else None


if __name__ == "__main__":
    unittest.main()
