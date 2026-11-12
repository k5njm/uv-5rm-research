#!/usr/bin/env python3
"""Decide which CI jobs a change needs (used by the `changes` job of .github/workflows/ci.yml; no third-party code).

  ci_changed.py --all                      everything runs (a push to main, or when in doubt)
  ci_changed.py --base SHA                 files changed by HEAD relative to the merge base with SHA (a pull request)
  ci_changed.py --files a b c              an explicit list (tests, rehearsals)
Prints one `job=true|false` line per job, ready to append to $GITHUB_OUTPUT; the reasoning goes to stderr.

How the table below was made: every rule names files that a job's steps actually read, found by reading the code, not
by directory name. The notes say where each came from. When a step starts reading a new path, add it here;
firmware/tests/test_ci_filters.py fails for any tracked file that no rule mentions (an unknown file runs everything,
the safe default), and checks the example change sets against the real workflow.

Jobs (outputs):
  gate    verify_release.py on the committed .BF and .app files
  sim     simulator build, app unit tests (logic.rs), scenario + golden-frame tests for every style
  tools   host tool tests (firmware/tests/test_*.py: fake radio, fake bootloader, recovery tools, manifest, bundle, filters,
          and test_docs.py: README.md, docs/*.md, docs/img and what the guide says about the launcher, groups and settings)
  hil     hil/test_hil.py (synthetic camera, fake injector) against the simulator's committed goldens and scenarios
  images  overlay builds (HAM TOOLS styles, FSK PROBE), core firmware (both variants), size budgets, pinned hashes
  web     headless-Chromium test of web/ against the images the `images` job built (so web implies images)
"""
import argparse
import os
import re
import subprocess
import sys

JOBS = ["gate", "sim", "tools", "hil", "images", "web"]
ALL = set(JOBS)

# (glob, jobs). `**` crosses directories, `*` does not. A file's jobs are the union over every rule it matches;
# a file that matches no rule is "unclassified" and runs everything. An empty job list means "no CI job reads this".
RULES = [
    # --- anything that changes how every job runs ---------------------------------------------------------------
    (".github/workflows/*", ALL),                      # the workflows themselves
    ("scripts/ci_changed.py", ALL),                    # this table
    ("scripts/dev.sh", ALL),                           # the local gate CI repeats
    ("dev/Dockerfile", ALL),
    ("firmware/toolchain.env", ALL),                   # the toolchain pin
    ("**/rust-toolchain.toml", ALL),
    # The ABI file: abi/src/lib.rs is hashed into every .app header and the core (build.rs), include!()d by the
    # simulator, and read by verify_release.py, make_manifest.py and tests as the expected ABI hash.
    ("firmware/firmware-source/abi/**", ALL),

    # --- core firmware: build_core.sh builds firmware-source (src, abi, at32f421-pac via path deps, memory.x via build.rs)
    # The simulator includes some of these by #[path] (fmtlite, ui/list, ui/toast, app/group_table, fault_text, ...)
    # and that list grows; naming them one by one went stale within a day, so any core source also runs `sim`.
    ("firmware/firmware-source/src/**", {"sim", "images", "web"}),
    ("firmware/firmware-source/build.rs", {"images", "web"}),
    ("firmware/firmware-source/memory.x", {"images", "web"}),
    ("firmware/firmware-source/Cargo.toml", {"images", "web"}),
    ("firmware/firmware-source/Cargo.lock", {"images", "web"}),
    ("firmware/firmware-source/.cargo/**", {"images", "web"}),
    ("firmware/firmware-source/at32f421-pac/**", {"images", "web"}),
    ("firmware/firmware-source/sgp4mini/**", {"images", "web"}),
    ("firmware/tools/build_core.sh", {"images", "web"}),
    ("firmware/source-changes/elf2bin.py", {"images", "web"}),   # flattens the ELF (also a host tool: see below)
    # Core files the simulator pulls in by #[path] (simulator/main.rs) or that a tool test reads.
    ("firmware/firmware-source/src/fmtlite.rs", {"sim"}),
    ("firmware/firmware-source/src/ui/list.rs", {"sim"}),
    ("firmware/firmware-source/src/ui/toast.rs", {"sim"}),
    ("firmware/firmware-source/src/app/group_table.rs", {"sim", "tools"}),   # channel_plan.py reads it too
    # The fakes are checked against the core source they copy (docs/TEST-DOUBLES.md): test_fake_fidelity.py reads the
    # settings clamps and defaults, the simulator include_str!s them and the keypad's long-press table, and test_hil.py
    # reads the keypad's long-press and repeat timing.
    ("firmware/firmware-source/src/flash_map.rs", {"sim", "tools"}),
    ("firmware/firmware-source/src/app/keyfn.rs", {"sim", "tools"}),
    ("firmware/firmware-source/src/device/keypad.rs", {"sim", "hil"}),

    # --- overlay apps: mkapp.py builds apps/<name> (+ abi); the images job builds hamtools x4 styles and fskprobe --
    # Every app directory counts, including ones the CI step does not build today, so adding one to CI cannot be missed.
    # Same for apps: which ones the simulator includes changes as apps are added or split.
    ("firmware/firmware-source/apps/**", {"sim", "images", "web"}),
    ("firmware/firmware-source/tools/mkapp.py", {"images", "web"}),
    ("firmware/firmware-source/tools/*.py", set()),                           # the other push/flash helpers: no CI step
    # The simulator #[path]-includes these app sources (the other apps are not simulated).
    ("firmware/firmware-source/apps/hamtools/**", {"sim"}),
    ("firmware/firmware-source/apps/fskprobe/**", {"sim"}),
    ("firmware/firmware-source/apps/spectrum/**", {"sim"}),
    ("firmware/firmware-source/apps/fm/**", {"sim"}),
    ("firmware/firmware-source/apps/chanmgr/**", {"sim"}),
    ("firmware/firmware-source/apps/settings/common.rs", {"sim", "tools"}),   # test_docs.py: the guide lists every setting
    ("firmware/firmware-source/*", set()),                                    # README, LICENSE, flake.*, make.sh, rustfmt.toml

    # --- simulator, scenarios, golden frames -------------------------------------------------------------------
    ("firmware/source-changes/simulator/**", {"sim"}),
    ("firmware/source-changes/apps_new/**", {"sim"}),                         # logic.rs of rptfind, bandsweep, foxhunt, ...
    ("firmware/skull_logo.rgb565", {"sim"}),                                  # argument of the simulator's own run
    # hil/test_hil.py reads tests/golden (not the style dirs) and tests/scenarios.
    # test_docs.py checks that every golden frame and scenario shot named in scripts/docs-screens.sh still exists.
    ("firmware/source-changes/simulator/tests/golden/**", {"hil", "tools"}),
    ("firmware/source-changes/simulator/tests/scenarios/**", {"hil", "tools"}),

    # --- host tools and their tests ----------------------------------------------------------------------------
    # The tests copy tools from source-changes/ into a sandbox next to a fake `serial`; channel_plan.py builds the
    # real channels/*.csv; test_manifest / test_verify_release run firmware/tools scripts against the ABI file.
    ("firmware/source-changes/*.py", {"tools"}),
    ("firmware/tests/**", {"tools"}),
    ("channels/*.csv", {"tools"}),
    ("channels/fm/*.csv", {"tools"}),                 # FM station lists: fm_stations.py and its tests
    ("firmware/variants.json", {"tools", "web", "images"}),                             # flash_bf.py and make_manifest.py read it
    ("firmware/tools/make_manifest.py", {"tools", "web", "images"}),
    ("firmware/tools/verify_release.py", {"tools", "gate", "images"}),        # gate, its tests, the size-budget step
    ("firmware/tools/bundle_release.py", {"tools"}),                          # tested by test_bundle_release.py
    ("web/index.html", {"tools", "web", "images"}),                           # test_web_static.py reads web/ too
    ("web/builder.js", {"tools", "web", "images"}),

    # --- release gate and pins ---------------------------------------------------------------------------------
    ("firmware/UV-5RM_Custom_Firmware.BF", {"gate"}),
    ("firmware/apps/*.app", {"gate"}),
    ("firmware/legacy-abi-hash.txt", {"gate"}),
    ("firmware/tools/pin_hashes.py", {"images"}),
    ("firmware/expected-hashes.json", {"images"}),

    # --- web builder: the browser test serves web/ with a manifest of the freshly built images ------------------
    ("web/test/**", {"web", "images"}),

    # --- HIL -----------------------------------------------------------------------------------------------------
    ("hil/*.py", {"hil"}),
    ("hil/*.json", {"hil"}),
    ("hil/injector_fw/**", {"hil"}),                                          # test_hil.py checks the injector firmware's PTT guard

    # --- read by no CI job -------------------------------------------------------------------------------------
    ("**/*.md", set()),
    ("docs/**", set()),
    # test_docs.py (the `tools` job) checks every link and picture of these, so they are not "read by no job":
    ("README.md", {"tools"}),
    ("channels/README.md", {"tools"}),
    ("docs/*.md", {"tools"}),
    ("docs/img/*.png", {"tools"}),
    ("docs/INSTALL.md", {"tools"}),                   # bundled; test_bundle_release.py reads it
    ("docs/FIRST-FLASH-RUNBOOK.md", {"tools"}),       # bundled the same way
    ("LICENSE", set()),
    ("firmware/source-changes/LICENSE-GPL3", set()),
    (".env.sample", set()),
    (".gitignore", set()),
    ("firmware/*.png", set()),
    ("firmware/*.html", set()),                                               # the standalone uploader pages
    ("firmware/source-changes/*.rs", set()),                                  # backdrop.rs, modes_*.rs: included by nothing built
    ("firmware/source-changes/*.html", set()),
    ("firmware/source-changes/*.js", set()),
    ("firmware/tools/build_release.py", {"tools"}),                           # `dev.sh release` / release.yml; test_docs.py reads LAYOUT
    ("firmware/tools/provenance.py", {"tools"}),                              # `dev.sh provenance`; test_docs.py reads the app names
    ("firmware/tools/upstream_status.py", set()),
    ("firmware/tools/size/**", set()),
    ("firmware/tools/experiments/**", set()),
    ("scripts/ci-replay.sh", set()),
    ("scripts/chirp-backup.sh", set()),
    ("scripts/docs-screens.sh", {"tools"}),                                   # test_docs.py reads its table of screenshots
    ("scripts/serial_bridge.py", set()),
]


def _glob_re(pat):
    out, i = [], 0
    while i < len(pat):
        if pat.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif pat.startswith("**", i):
            out.append(".*")
            i += 2
        elif pat[i] == "*":
            out.append("[^/]*")
            i += 1
        else:
            out.append(re.escape(pat[i]))
            i += 1
    return re.compile("^" + "".join(out) + "$")


_COMPILED = [(_glob_re(p), p, set(j)) for p, j in RULES]


def classify(path):
    """(jobs, matched_patterns) for one repo-relative path; no match at all -> every job, matched == []."""
    jobs, hit = set(), []
    for rx, pat, j in _COMPILED:
        if rx.match(path):
            jobs |= j
            hit.append(pat)
    return (set(ALL) if not hit else jobs), hit


def jobs_for(files):
    jobs = set()
    for f in files:
        j, hit = classify(f)
        if not hit:
            print(f"  {f}: not in any rule, running everything", file=sys.stderr)
        jobs |= j
    return jobs


def changed_files(base):
    # Three dots: what this branch changed since it left `base`, not what base gained since. --no-renames lists both
    # sides of a move, so a file moved out of a watched directory still counts.
    r = subprocess.run(["git", "diff", "--name-only", "--no-renames", f"{base}...HEAD"], capture_output=True, text=True)
    if r.returncode != 0:
        return None, r.stderr.strip()
    return [l for l in r.stdout.splitlines() if l], ""


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--all", action="store_true")
    g.add_argument("--base")
    g.add_argument("--files", nargs="*")
    a = ap.parse_args(argv)
    if a.all:
        jobs = set(ALL)
        print("everything runs (--all)", file=sys.stderr)
    else:
        if a.files is not None:
            files = a.files
        else:
            files, err = changed_files(a.base)
            if files is None:
                print(f"git diff failed ({err}); running everything", file=sys.stderr)
                files = ["?unknown?"]
        print(f"{len(files)} changed file(s)", file=sys.stderr)
        for f in files[:50]:
            print(f"  {f}", file=sys.stderr)
        jobs = jobs_for(files)
    print("jobs: " + (", ".join(j for j in JOBS if j in jobs) or "none"), file=sys.stderr)
    for j in JOBS:
        print(f"{j}={'true' if j in jobs else 'false'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
