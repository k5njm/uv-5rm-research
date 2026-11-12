#!/usr/bin/env python3
"""Assemble the release bundle from a built release set (scripts/dev.sh bundle, .github/workflows/release.yml).

  bundle_release.py bundle --release build-out/release --out build-out/bundle --version v0.1.0 [--allow-unpinned]
  bundle_release.py notes  --version v0.1.0 --sums build-out/bundle/assets/SHA256SUMS --repo OWNER/NAME [--prev TAG] -o notes.md

`bundle` takes the directory `scripts/dev.sh release` writes (core .BF and .bin, the launcher apps and extras for the
current ABI, layout.json, SHA256SUMS) and produces, under --out:
  uv5rm-VERSION/        the staging tree: release set, the host tools a user needs, channels/, the runbook, LICENSE, INSTALL.md
  assets/               what gets published: the zip, its checksum, and the release set as flat files
It refuses (non-zero exit, nothing written to assets/) when the set is incomplete, SHA256SUMS does not match the files,
the built core's sha256 differs from firmware/expected-hashes.json (unless --allow-unpinned, for local rehearsals of a
changed core), or the staging tree contains anything personal (see FORBIDDEN). The zip is byte-reproducible.
Only the standard library is used. Layout inside the bundle mirrors the repository, so docs/FIRST-FLASH-RUNBOOK.md's
commands work as written (the host tools read ../variants.json and group_table.rs relative to themselves).
"""
import argparse
import fnmatch
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
ROOT = os.path.dirname(FW)

CORE_FILES = ["fw-default.BF", "fw-default.bin"]           # what `dev.sh release` writes for the default (YP07) variant
PINNED_CORE = "fw-default.bin"                              # its sha256 must equal expected-hashes.json

# Host tools a user needs to install the release (see docs/FIRST-FLASH-RUNBOOK.md), relative to firmware/source-changes/.
# radio_read.py is not in the brief's list but the runbook's read-only backup step (A5) is that script.
TOOLS = ["flash_bf.py", "cps_tool.py", "dump_nor.py", "install_apps.py", "restore_backup.py", "channel_extras.py",
         "channel_plan.py", "boot_logo_raw.py", "wrap_bf.py", "radio_read.py", "fm_stations.py", "probe_capture.py",
         "bootloader_probe.py"]          # the last one: install.py waits for the bootloader with it
# Files those tools read by relative path: flash_bf.py -> ../variants.json, channel_plan.py -> ../firmware-source/src/app/group_table.rs.
# Local imports and `cps_tool.py` (read as text by the others) are covered by TOOLS; test_bundle_release.py checks the closure.
DATA = ["firmware/variants.json", "firmware/firmware-source/src/app/group_table.rs", "docs/FIRST-FLASH-RUNBOOK.md", "docs/INSTALL.md", "LICENSE"]
CHANNELS_DIR = "channels"          # *.csv and README.md, and fm/*.csv (FM station lists)

# --- THE INSTALLER ---------------------------------------------------------------------------------------------------
# install.py is copied to the bundle root (it finds firmware/source-changes/, channels/ and release/ next to itself). It runs
# the tools above as programs, so every script it names must be in TOOLS (test_bundle_release.py checks the closure).
INSTALLER_SOURCE = "firmware/source-changes/install.py"
INSTALLER_DEST = "install.py"
# -------------------------------------------------------------------------------------------------------------------

# Nothing personal may enter a release. Matched against every path in the staging tree (relative, '/'-separated):
# a file or directory name, case-insensitive. `.env.sample` is fine in the repo but is not bundled either, so all `.env*` go.
FORBIDDEN_NAMES = [".env", ".env.*", "my-backup", "personal", "boot-logo.rgb565", "boot-logo.png", "*.img", "*.bak",
                   "factory_memory*", "*.extras", "build-out"]


class BundleError(Exception):
    pass


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def parse_sums(text):
    """`sha256sum` output -> {name: hex}. Binary-mode markers (`*name`) are accepted."""
    out = {}
    for line in text.splitlines():
        if not line.strip():
            continue
        m = re.fullmatch(r"([0-9a-f]{64})  ?\*?(.+)", line.strip())
        if not m:
            raise BundleError(f"bad line in SHA256SUMS: {line!r}")
        out[m.group(2)] = m.group(1)
    return out


def sums_text(files, base):
    return "".join(f"{sha256_file(os.path.join(base, n))}  {n}\n" for n in sorted(files))


def release_set(rel):
    """Names the release directory must hold, from its own layout.json (written by build_release.py)."""
    lp = os.path.join(rel, "layout.json")
    if not os.path.isfile(lp):
        raise BundleError(f"{rel}: no layout.json (is this the output of scripts/dev.sh release?)")
    with open(lp) as f:
        layout = json.load(f)
    apps = list(layout["slots"]) + list(layout.get("extras", []))
    if len(layout["slots"]) != 9:
        raise BundleError(f"layout.json lists {len(layout['slots'])} launcher slots, expected 9")
    return CORE_FILES + apps + ["layout.json"], apps


def check_release(rel):
    """Every file present, SHA256SUMS present and matching every file it covers; returns (files, sums)."""
    files, apps = release_set(rel)
    missing = [n for n in files + ["SHA256SUMS"] if not os.path.isfile(os.path.join(rel, n))]
    if missing:
        raise BundleError(f"{rel}: missing {', '.join(missing)}")
    with open(os.path.join(rel, "SHA256SUMS")) as f:
        sums = parse_sums(f.read())
    for n in CORE_FILES + apps:           # dev.sh release covers the core and every .app, not layout.json
        if n not in sums:
            raise BundleError(f"SHA256SUMS does not list {n}")
    for n, want in sums.items():
        p = os.path.join(rel, n)
        if not os.path.isfile(p):
            raise BundleError(f"SHA256SUMS lists {n}, which is not in {rel}")
        if sha256_file(p) != want:
            raise BundleError(f"{n}: sha256 does not match SHA256SUMS")
    return files + ["SHA256SUMS"], sums


def check_pin(rel, pins_path=None):
    """The built core must be byte-identical to the one the local gate pinned (firmware/expected-hashes.json)."""
    pins_path = pins_path or os.path.join(FW, "expected-hashes.json")
    with open(pins_path) as f:
        pins = json.load(f)
    want = pins.get(PINNED_CORE)
    got = sha256_file(os.path.join(rel, PINNED_CORE))
    if want is None:
        raise BundleError(f"{PINNED_CORE} is not pinned in {pins_path}")
    if got != want:
        raise BundleError(f"{PINNED_CORE} built here is {got}\n  but {os.path.basename(pins_path)} pins {want}\n"
                          "  the core is not the one the local check approved: refusing to bundle it "
                          "(scripts/dev.sh hashes --update records an intended change)")
    return got


def forbidden_in(stage):
    """Relative paths in the staging tree that match FORBIDDEN_NAMES (empty list = clean)."""
    bad = []
    for d, dirs, files in os.walk(stage):
        for n in dirs + files:
            if any(fnmatch.fnmatch(n.lower(), pat) for pat in FORBIDDEN_NAMES):
                bad.append(os.path.relpath(os.path.join(d, n), stage).replace(os.sep, "/"))
    return sorted(bad)


def install_md(version):
    return f"""# UV-5RM open firmware {version}: install

This is experimental firmware. You flash it at your own risk, on a radio you can afford to lose. It has run on one radio
(a 5RHNRFB-YP07); the installer below has only been tried against simulated radios. The first step is a backup of the
radio, including its calibration (which cannot be restored once lost): the installer does that before it writes anything.

**Fastest way: the installer.** You need Python 3.9 or newer and `pip install pyserial`, and the programming cable
(the two-pin K-plug, not USB-C; read `docs/FIRST-FLASH-RUNBOOK.md` > The cable first).

    python3 install.py --dry-run       # menus, a summary, and what it WOULD do; sends no write command to a radio
    python3 install.py                 # the same menus, then (after you type INSTALL) the real install
    python3 install.py --config setup.json   # repeat a saved setup

It identifies the radio (stock, this firmware, or flash mode), backs it up first, checks every file against
`release/SHA256SUMS`, flashes the core only if needed (and tells you which keys to hold), proves the calibration did not
change, then writes the apps, channels, channel details, FM stations and boot screen, reading each back. At the end it says
what was written, where the backups are, and for anything that failed, what state the radio is in and how to recover.
The guide, with the menus shown as text, the setup-file schema and recovery, is `docs/INSTALL.md` in this folder.

**Manual way (the fallback):**

1. Read `docs/FIRST-FLASH-RUNBOOK.md` first, all of it. It is the procedure: the cable, the read-only checks, what can
   and cannot be restored, the order of the steps. Do the read-only steps before anything is written.
2. Check the files: `cd release && sha256sum -c SHA256SUMS` (macOS: `shasum -a 256 -c SHA256SUMS`).
3. Needs Python 3 and `pip install pyserial` (and `pillow` for the boot-logo tools).
4. The commands in the runbook are written for this folder layout and work as written: `firmware/source-changes/` holds
   the tools, `release/` holds the core image (`fw-default.BF` for the 5RHNRFB-YP07 board) and the nine launcher apps
   (`layout.json` gives their slot order).
5. Channel lists are in `channels/` (CHIRP CSV, one file per group; `channels/README.md` explains):
   `python3 firmware/source-changes/channel_plan.py build channels -o channels.img`, then the runbook's restore step.
"""


def _copy(src, dst):
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    shutil.copyfile(src, dst)


def build_stage(rel, stage, version, root=ROOT):
    """Create the staging tree; returns the sorted list of its files (relative paths)."""
    if os.path.exists(stage):
        shutil.rmtree(stage)
    files, _ = check_release(rel)
    for n in files:
        _copy(os.path.join(rel, n), os.path.join(stage, "release", n))
    for t in TOOLS:
        _copy(os.path.join(root, "firmware", "source-changes", t), os.path.join(stage, "firmware", "source-changes", t))
    for d in DATA:
        _copy(os.path.join(root, d), os.path.join(stage, d))
    cdir = os.path.join(root, CHANNELS_DIR)
    for n in sorted(os.listdir(cdir)):
        if n.endswith(".csv") or n == "README.md":
            _copy(os.path.join(cdir, n), os.path.join(stage, CHANNELS_DIR, n))
    fdir = os.path.join(cdir, "fm")
    for n in sorted(os.listdir(fdir)) if os.path.isdir(fdir) else []:
        if n.endswith(".csv"):
            _copy(os.path.join(fdir, n), os.path.join(stage, CHANNELS_DIR, "fm", n))
    installer = os.path.join(root, INSTALLER_SOURCE)
    if not os.path.isfile(installer):
        raise BundleError(f"{INSTALLER_SOURCE} is missing: a release without the installer is refused")
    _copy(installer, os.path.join(stage, INSTALLER_DEST))
    with open(os.path.join(stage, "INSTALL.md"), "w") as f:
        f.write(install_md(version))
    bad = forbidden_in(stage)
    if bad:
        shutil.rmtree(stage)
        raise BundleError("personal or local files in the bundle: " + ", ".join(bad))
    return stage_files(stage)


def stage_files(stage):
    out = []
    for d, _, files in os.walk(stage):
        for n in files:
            out.append(os.path.relpath(os.path.join(d, n), stage).replace(os.sep, "/"))
    return sorted(out)


def make_zip(stage, zip_path, prefix):
    """Reproducible zip: sorted entries, fixed timestamps and modes."""
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as z:
        for rel in stage_files(stage):
            zi = zipfile.ZipInfo(f"{prefix}/{rel}", date_time=(2026, 1, 1, 0, 0, 0))
            zi.compress_type = zipfile.ZIP_DEFLATED
            zi.external_attr = (0o755 if rel.endswith(".py") else 0o644) << 16
            with open(os.path.join(stage, rel), "rb") as f:
                z.writestr(zi, f.read())


def bundle(rel, out, version, allow_unpinned=False, root=ROOT, pins_path=None):
    if not re.fullmatch(r"[A-Za-z0-9._-]+", version):
        raise BundleError(f"version {version!r}: letters, digits, '.', '_' and '-' only")
    files, _ = check_release(rel)
    if allow_unpinned:
        print("  WARNING: --allow-unpinned: the core is NOT checked against firmware/expected-hashes.json")
    else:
        print(f"  core {PINNED_CORE} matches the pin: {check_pin(rel, pins_path)[:16]}")
    name = f"uv5rm-{version}"
    stage = os.path.join(out, name)
    os.makedirs(out, exist_ok=True)
    listed = build_stage(rel, stage, version, root)
    assets = os.path.join(out, "assets")
    if os.path.exists(assets):
        shutil.rmtree(assets)
    os.makedirs(assets)
    zip_path = os.path.join(assets, name + ".zip")
    make_zip(stage, zip_path, name)
    with open(zip_path + ".sha256", "w") as f:
        f.write(f"{sha256_file(zip_path)}  {name}.zip\n")
    for n in files:                           # the release set again, flat, so single files can be downloaded and checked
        _copy(os.path.join(rel, n), os.path.join(assets, n))
    print(f"  {len(listed)} files in {stage}")
    print(f"  {zip_path}  {sha256_file(zip_path)}")
    return stage, zip_path, listed


def previous_tag(tag, runner=subprocess.run):
    r = runner(["git", "describe", "--tags", "--abbrev=0", f"{tag}^"], capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 and r.stdout.strip() else None


def commits_since(prev, tag, runner=subprocess.run):
    rng = f"{prev}..{tag}" if prev else tag
    r = runner(["git", "log", "--no-merges", "--format=- %s (%h)", rng], capture_output=True, text=True)
    return [l for l in r.stdout.splitlines() if l.strip()] if r.returncode == 0 else []


def release_notes(version, sums, commits, prev, repo, zip_sha=None):
    runbook = f"https://github.com/{repo}/blob/{version}/docs/FIRST-FLASH-RUNBOOK.md" if repo else "docs/FIRST-FLASH-RUNBOOK.md"
    since = f"since {prev}" if prev else "(first release: all commits)"
    lines = [
        f"# UV-5RM open firmware {version}",
        "",
        "**This firmware is experimental. You flash it entirely at your own risk.** It has run on one radio "
        "(a 5RHNRFB-YP07); nothing here is certified or supported, and flashing is effectively one way for that board. "
        "**Back up the radio's calibration first**: it cannot be restored once lost. Read the "
        f"[first-flash runbook]({runbook}) before connecting anything.",
        "",
        "Download the zip: it holds the core image, the nine launcher apps, the host tools, the channel lists and the runbook "
        "laid out so the runbook's commands work as written. The files are also attached individually.",
        "",
        "## Checksums",
        "",
        "SHA256SUMS (verify with `sha256sum -c SHA256SUMS` next to the files):",
        "",
        "```",
        sums.strip(),
        "```",
    ]
    if zip_sha:
        lines += ["", "Bundle zip:", "", "```", zip_sha.strip(), "```"]
    lines += ["", f"## Changes {since}", ""]
    lines += commits or ["- (no commits found)"]
    lines.append("")
    return "\n".join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("bundle")
    b.add_argument("--release", required=True)
    b.add_argument("--out", required=True)
    b.add_argument("--version", required=True)
    b.add_argument("--allow-unpinned", action="store_true")
    n = sub.add_parser("notes")
    n.add_argument("--version", required=True)
    n.add_argument("--sums", required=True, help="the SHA256SUMS file to include")
    n.add_argument("--repo", default="")
    n.add_argument("--head", default=None, help="what to list commits up to (default: --version, a tag; HEAD for a dry run)")
    n.add_argument("--prev", default=None, help="previous tag (default: the nearest tag before --head)")
    n.add_argument("--zip-sum", default=None, help="the bundle zip's .sha256 file")
    n.add_argument("-o", "--output", required=True)
    a = ap.parse_args(argv)
    try:
        if a.cmd == "bundle":
            bundle(a.release, a.out, a.version, a.allow_unpinned)
        else:
            head = a.head or a.version
            prev = a.prev or previous_tag(head)
            with open(a.sums) as f:
                sums = f.read()
            zs = open(a.zip_sum).read() if a.zip_sum else None
            with open(a.output, "w") as f:
                f.write(release_notes(a.version, sums, commits_since(prev, head), prev, a.repo, zs))
            print(f"  notes for {a.version} ({'since ' + prev if prev else 'first release'}) -> {a.output}")
    except BundleError as e:
        print(f"REFUSED: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
