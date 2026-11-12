#!/usr/bin/env python3
"""One installer for a UV-5RM: a stock or already-converted radio -> this firmware, apps, channels, FM stations.

  install.py                          menus (apps per slot, channel lists, FM list, boot logo), a summary, then the install
  install.py --config setup.json      repeat a saved setup: no questions except the final confirmation (--yes skips that)
  install.py --dry-run [...]          go through everything and print what would be written; no write or erase command
                                      is sent to the radio (reads are; it also works with no radio at all)

  --port PORT          serial port of the programming cable (or "port" in the setup file)
  --release DIR        the release files: core .BF, .app files, layout.json, SHA256SUMS (default: ./release, in a bundle)
  --save FILE          save the chosen setup as JSON (the menus also offer this)
  --backup-dir DIR     where backups go (default ./uv5rm-backups); each run makes a NEW timestamped folder in it
  --resume DIR         continue after a failed core flash: use the backup folder of the earlier run (hashes re-checked)
  --yes                do not ask for the final confirmation (scripts)
  --settle S --wait S --retries N --probe-timeout S    timing of the waits after the radio restarts (defaults 4 / 180 / 8 / 2)

The fixed order, each step refusing to go on if the one before did not verify:
  a identify the radio: stock firmware, this firmware, or the bootloader (a half-seated cable is recognised: re-seat it)
  b back up first: stock -> radio_read.py image and the calibration page, each read twice and compared; this firmware ->
    dump_nor.py --config twice and compared (channels, calibration). A new folder, never overwriting anything
  c check every file against SHA256SUMS, and the core with flash_bf.py's own board/variant gate (--check-only)
  d flash the core, only if needed: guides you into bootloader mode, waits for it, then waits for the radio to come back
  e read the calibration page again; it must be byte-identical to the copy from before
  f apps, channels (whole 4 KB sectors, restore_backup.py), channel details, FM stations, boot logo: each read back
    and verified by its own tool
  g a report: what was written, where the backups are, and for a failed step what state the radio is in and how to recover
Every radio operation is done by the existing tool (flash_bf.py, radio_read.py, dump_nor.py, install_apps.py,
restore_backup.py, channel_extras.py, fm_stations.py, boot_logo_raw.py, channel_plan.py), run as a program; none of their
protocol code is copied here. Settings (TX INHIBIT, TX BANDS) and calibration are never written, and nothing keys the
transmitter. Personal boot screens are NOT made here (that needs the Rust simulator); give a finished 40,960-byte
.rgb565 file if you have one. UNVERIFIED on a real radio: tested only against simulated ones (firmware/tests/test_install.py).

Exit status: 0 done, 1 a step failed (the report says what state the radio is in), 2 refused before anything was written.
Setup file: schema 1, see docs/INSTALL.md.
"""
import argparse
import datetime
import glob
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

SCHEMA = 1
HERE = os.path.dirname(os.path.abspath(__file__))
# A release bundle has install.py at its root next to firmware/; in the repository it is firmware/source-changes/install.py.
ROOT = HERE if os.path.isdir(os.path.join(HERE, "firmware", "source-changes")) else os.path.dirname(os.path.dirname(HERE))
TOOLS = os.path.join(ROOT, "firmware", "source-changes")
SLOTS = 9
SLOT_SIZE = 32 * 1024
LOGO_SIZE = 160 * 128 * 2
CHAN_IMAGE = 0x8000
CAL = (0xF000, 0x10000)
SETUP_KEYS = {"schema", "port", "board", "release", "core", "apps", "channels", "keep_existing_channels", "fm_stations", "boot_logo"}
BOOTLOADER_KEYS = ("Radio off. Hold the two small side buttons below PTT (not PTT itself) and switch it on while holding them.\n"
                   "The screen stays dark or shows a flash-mode screen. Keep the cable plugged in.")
STAGES = ["identify", "backup", "files", "core", "calibration", "apps", "channels", "details", "fm", "logo", "final calibration"]


class Refused(Exception):
    """Something is wrong with the request or the files; nothing has been written to the radio."""


class StepFailed(Exception):
    def __init__(self, stage, message, state="", recover=""):
        Exception.__init__(self, message)
        self.stage, self.message, self.state, self.recover = stage, message, state, recover


# ------------------------------------------------------------------------------------------------ files and tools
def load_module(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(TOOLS, name + ".py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def const_from(script, name):
    """A bytes constant (NAME = b"...") read out of a tool's source, so the handshake strings exist once."""
    with open(os.path.join(TOOLS, script), encoding="utf-8") as f:
        m = re.search(r"^" + name + r" = b\"([^\"]*)\"", f.read(), re.M)
    if not m:
        raise Refused(f"{script}: cannot find {name}; the installer and the tools are from different versions")
    return m.group(1).encode("ascii")


def parse_sums(text):
    out = {}
    for line in text.splitlines():
        if line.strip():
            m = re.fullmatch(r"([0-9a-f]{64})  ?\*?(.+)", line.strip())
            if not m:
                raise Refused(f"SHA256SUMS: bad line {line!r}")
            out[m.group(2)] = m.group(1)
    return out


def default_release():
    for d in (os.path.join(ROOT, "release"), os.path.join(ROOT, "build-out", "release")):
        if os.path.isfile(os.path.join(d, "layout.json")):
            return d
    return os.path.join(ROOT, "release")


def app_info(path):
    with open(path, "rb") as f:
        d = f.read()
    if d[:4] != b"OVL1":
        raise Refused(f"{os.path.basename(path)} is not an app package (no OVL1 header)")
    if len(d) > SLOT_SIZE:
        raise Refused(f"{os.path.basename(path)} is {len(d)} bytes; a slot holds {SLOT_SIZE}")
    return {"file": os.path.basename(path), "title": d[12:28].split(b"\0")[0].decode("ascii", "replace"),
            "size": len(d), "abi": d[4:8].hex()}


class Release:
    """The release directory: apps with their header data, the default layout, the checksums."""

    def __init__(self, path):
        self.path = path
        lp = os.path.join(path, "layout.json")
        if not os.path.isfile(lp):
            raise Refused(f"{path}: no layout.json. Point --release at the folder with the .BF and .app files.")
        with open(lp) as f:
            self.layout = json.load(f)
        sp = os.path.join(path, "SHA256SUMS")
        if not os.path.isfile(sp):
            raise Refused(f"{path}: no SHA256SUMS; nothing can be checked, so nothing will be written")
        with open(sp) as f:
            self.sums = parse_sums(f.read())
        self.apps = {}
        for p in sorted(glob.glob(os.path.join(path, "*.app"))):
            self.apps[os.path.basename(p)] = app_info(p)

    def default_slots(self):
        return [n if n in self.apps else None for n in self.layout["slots"]][:SLOTS]

    def check(self, name, where=None):
        """Refuse unless `name` is listed in SHA256SUMS and its bytes match (where = a copy to check instead)."""
        p = where or os.path.join(self.path, name)
        if name not in self.sums:
            raise Refused(f"{name} is not listed in {os.path.join(self.path, 'SHA256SUMS')}; refusing to write a file that cannot be checked")
        if not os.path.isfile(p):
            raise Refused(f"{p} is missing")
        got = sha256_file(p)
        if got != self.sums[name]:
            raise Refused(f"CHECKSUM MISMATCH: {name} is {got[:16]}..., SHA256SUMS says {self.sums[name][:16]}... "
                          "The download is damaged or has been changed. Nothing was written; download the release again.")
        return got


def variants():
    with open(os.path.join(ROOT, "firmware", "variants.json")) as f:
        return json.load(f)


def variant_for(board):
    for v, info in variants().items():
        if board in info["boards"]:
            return v
    boards = [b for i in variants().values() for b in i["boards"]]
    raise Refused(f"BOARD GATE: {board!r} is not a supported board ({', '.join(boards)}). Hold 8 while powering on and type exactly "
                  "what the screen shows. Nothing was written.")


# ------------------------------------------------------------------------------------------------ setup file
def empty_setup():
    return {"schema": SCHEMA, "port": None, "board": None, "release": None, "core": "auto", "apps": [None] * SLOTS,
            "channels": {}, "keep_existing_channels": True, "fm_stations": None, "boot_logo": None}


def check_slots(apps):
    """The rules for the nine launcher slots: exactly nine, each an .app file name or None, no app twice."""
    if not isinstance(apps, list) or len(apps) != SLOTS:
        raise Refused(f"'apps' must list exactly {SLOTS} slots (use null for an empty slot)")
    seen = {}
    for i, a in enumerate(apps):
        if a is None:
            continue
        if not isinstance(a, str) or not a.endswith(".app"):
            raise Refused(f"slot {i + 1}: {a!r} is not an .app file name")
        if a in seen:
            raise Refused(f"{a} is in slot {seen[a] + 1} and slot {i + 1}: an app may be placed only once")
        seen[a] = i
    return apps


def load_setup(path):
    """The saved setup, with relative paths resolved against the file's folder. Refuses an unknown schema or key."""
    try:
        with open(path) as f:
            raw = json.load(f)
    except (OSError, ValueError) as e:
        raise Refused(f"{path}: cannot read the setup file ({e})")
    if not isinstance(raw, dict) or raw.get("schema") != SCHEMA:
        raise Refused(f"{path}: schema {raw.get('schema') if isinstance(raw, dict) else None!r}; this installer reads schema {SCHEMA} only. "
                      "Use the installer that wrote the file, or make the setup again from the menu.")
    extra = sorted(set(raw) - SETUP_KEYS)
    if extra:
        raise Refused(f"{path}: unknown setting(s) {', '.join(extra)}; check the spelling against docs/INSTALL.md")
    s = empty_setup()
    s.update(raw)
    base = os.path.dirname(os.path.abspath(path))
    rel = lambda p: None if p is None else (p if os.path.isabs(p) else os.path.normpath(os.path.join(base, p)))
    s["release"], s["fm_stations"], s["boot_logo"] = rel(s["release"]), rel(s["fm_stations"]), rel(s["boot_logo"])
    if not isinstance(s["channels"], dict):
        raise Refused(f"{path}: 'channels' must map a group name to a CSV file")
    s["channels"] = {g: rel(p) for g, p in s["channels"].items()}
    if s["core"] not in ("auto", "flash", "skip"):
        raise Refused(f"{path}: 'core' must be auto, flash or skip")
    check_slots(s["apps"])
    return s


def save_setup(setup, path):
    """Write the setup as JSON; paths inside the file's folder are written relative to it. Never overwrites."""
    if os.path.exists(path):
        raise Refused(f"{path} exists; choose another name (an existing setup is never overwritten)")
    base = os.path.dirname(os.path.abspath(path))

    def rel(p):
        if p is None:
            return None
        try:
            r = os.path.relpath(p, base)
        except ValueError:
            return p
        return r

    out = dict(setup)
    out["release"], out["fm_stations"], out["boot_logo"] = rel(setup["release"]), rel(setup["fm_stations"]), rel(setup["boot_logo"])
    out["channels"] = {g: rel(p) for g, p in setup["channels"].items()}
    with open(path, "w") as f:
        json.dump(out, f, indent=2)
        f.write("\n")


# ------------------------------------------------------------------------------------------------ channel and FM checks
def group_slug(name):
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")


def check_channel_csv(path, group, cp):
    """(count, [problems]) by channel_plan.py's own rules for ONE file used as `group`'s list."""
    with tempfile.TemporaryDirectory() as d:
        shutil.copyfile(path, os.path.join(d, group_slug(group) + ".csv"))
        try:
            groups = cp.load_groups()
            _, entries, errors, _ = cp.build(d, None, groups)
        except cp.PlanError as e:
            return 0, [str(e)]
        except (OSError, UnicodeDecodeError) as e:
            return 0, [f"{path}: {e}"]
    return len(entries), errors


def resolve_group(text, cp):
    """A group name as the firmware's table has it, or a Refused with channel_plan's own message."""
    try:
        return cp.find_group(cp.load_groups(), text.strip())[0]
    except cp.PlanError as e:
        raise Refused(str(e).replace(f"{text.strip()}.csv: ", ""))


def check_fm_csv(path, fm):
    try:
        return len(fm.read_csv(path)), []
    except (fm.StationError, OSError, UnicodeDecodeError) as e:
        return 0, [str(e)]


# ------------------------------------------------------------------------------------------------ menus (prompts)
class Ui:
    """Numbered prompts over stdin/stdout: works in a terminal, in a pipe and in the tests. Selection rules live in
    functions that do not print or read (check_slots, check_channel_csv, ...), so the same rules apply from a setup file."""

    def __init__(self, stdin=None, stdout=None):
        self.stdin, self.out = stdin or sys.stdin, stdout or sys.stdout

    def say(self, text=""):
        print(text, file=self.out, flush=True)

    def ask(self, prompt):
        print(prompt, end="", file=self.out, flush=True)
        line = self.stdin.readline()
        if line == "":
            raise Refused("the input ended before the questions were answered (use --config to run without questions)")
        return line.strip()

    def tty(self):
        try:
            return self.stdin.isatty()
        except (AttributeError, ValueError):
            return False


def candidate_ports():
    return sorted(set(glob.glob("/dev/cu.usbserial*") + glob.glob("/dev/cu.wchusbserial*") + glob.glob("/dev/ttyUSB*")
                      + glob.glob("/dev/ttyACM*")))


def menu_port(ui, setup):
    ports = candidate_ports()
    ui.say("\nStep 1 of 7: the serial port of the programming cable")
    for i, p in enumerate(ports, 1):
        ui.say(f"  {i}. {p}")
    ui.say("  Type a number, a port name, or 'none' to only plan (no radio connected; implies --dry-run).")
    while True:
        a = ui.ask("port> ")
        if a.lower() == "none":
            setup["port"] = None
            return
        if a.isdigit() and 1 <= int(a) <= len(ports):
            setup["port"] = ports[int(a) - 1]
            return
        if a and not a.isdigit():
            setup["port"] = a
            return
        ui.say("  not a choice")


def menu_board(ui, setup):
    ok = [b for v in variants().values() for b in v["boards"]]
    ui.say("\nStep 2 of 7: the board. Hold 8 while powering the radio on and type what the screen shows.")
    ui.say("  Supported: " + ", ".join(ok))
    while True:
        b = ui.ask("board> ")
        try:
            variant_for(b)
        except Refused as e:
            ui.say("  " + str(e))
            continue
        setup["board"] = b
        return


def menu_apps(ui, setup, rel):
    ui.say("\nStep 3 of 7: apps in the nine launcher slots")
    apps = sorted(rel.apps.values(), key=lambda a: a["file"])
    ui.say("  Available:")
    for i, a in enumerate(apps, 1):
        ui.say(f"   {i:2}. {a['title']:<16} {a['file']:<16} {a['size']:>6} bytes")
    ui.say("  Commands: SLOT NUMBER (put app NUMBER in slot SLOT)   SLOT 0 (empty the slot)   d (release defaults)   x (all empty)   ok")
    slots = list(setup["apps"])
    while True:
        for i, n in enumerate(slots, 1):
            ui.say(f"   slot {i}: {rel.apps[n]['title'] if n else '(empty)'}")
        cmd = ui.ask("apps> ").lower()
        if cmd in ("ok", ""):
            setup["apps"] = slots
            return
        if cmd == "d":
            slots = rel.default_slots()
        elif cmd == "x":
            slots = [None] * SLOTS
        else:
            m = re.fullmatch(r"(\d+)\s+(\d+)", cmd)
            if not m or not 1 <= int(m.group(1)) <= SLOTS or int(m.group(2)) > len(apps):
                ui.say("  not a command")
                continue
            slot, n = int(m.group(1)) - 1, int(m.group(2))
            trial = list(slots)
            trial[slot] = apps[n - 1]["file"] if n else None
            try:
                check_slots(trial)
            except Refused as e:
                ui.say("  " + str(e))
                continue
            slots = trial


def bundled_lists(kind):
    d = os.path.join(ROOT, "channels", "fm") if kind == "fm" else os.path.join(ROOT, "channels")
    return sorted(glob.glob(os.path.join(d, "*.csv")))


def menu_channels(ui, setup, cp):
    ui.say("\nStep 4 of 7: channel lists (one list per group; a list replaces that group's channels)")
    files = bundled_lists("channels")
    chosen = dict(setup["channels"])
    info = {}
    for p in files:
        stem = os.path.basename(p)[:-4]
        try:
            g = resolve_group(stem, cp)
        except Refused:
            continue
        n, errs = check_channel_csv(p, g, cp)
        info[p] = (g, n, errs)
    while True:
        ui.say("  Lists (x = chosen):")
        for i, (p, (g, n, errs)) in enumerate(info.items(), 1):
            ui.say(f"   {i:2}. [{'x' if chosen.get(g) == p else ' '}] {g:<14} {n:>4} channels  {os.path.relpath(p, ROOT)}" + ("  (PROBLEMS)" if errs else ""))
        for g, p in chosen.items():
            if p not in info:
                ui.say(f"       [x] {g:<14} {'':>4}           {p}")
        ui.say("  Commands: NUMBER (choose or drop a list)   a PATH (your own CHIRP CSV)   n (none)   ok")
        cmd = ui.ask("channels> ")
        if cmd.lower() in ("ok", ""):
            setup["channels"] = chosen
            return
        if cmd.lower() == "n":
            chosen = {}
        elif cmd.isdigit() and 1 <= int(cmd) <= len(info):
            p = list(info)[int(cmd) - 1]
            g, n, errs = info[p]
            if chosen.get(g) == p:
                del chosen[g]
            elif errs:
                ui.say(f"  {len(errs)} problem(s) in {p}:")
                for e in errs:
                    ui.say("    " + e)
            else:
                chosen[g] = p
        elif cmd.lower().startswith("a "):
            p = os.path.abspath(os.path.expanduser(cmd[2:].strip()))
            if not os.path.isfile(p):
                ui.say(f"  {p}: no such file")
                continue
            try:
                g = resolve_group(ui.ask("  which group is this list for? (a name from the list above, e.g. REPEATERS) "), cp)
            except Refused as e:
                ui.say("  " + str(e))
                continue
            n, errs = check_channel_csv(p, g, cp)
            if errs:
                ui.say(f"  {len(errs)} problem(s); the list was not added:")
                for e in errs:
                    ui.say("    " + e)
            else:
                chosen[g] = p
                ui.say(f"  added: {n} channels for {g}")
        else:
            ui.say("  not a command")


def menu_fm(ui, setup, fm):
    ui.say("\nStep 5 of 7: FM broadcast station list for the FM app (optional)")
    files = bundled_lists("fm")
    for i, p in enumerate(files, 1):
        n, errs = check_fm_csv(p, fm)
        ui.say(f"   {i}. {n:>3} stations  {os.path.relpath(p, ROOT)}")
    ui.say("  Commands: NUMBER   p PATH (your own CSV)   n (none, the default)")
    while True:
        cmd = ui.ask("fm> ")
        if cmd.lower() in ("n", ""):
            setup["fm_stations"] = None
            return
        p = files[int(cmd) - 1] if cmd.isdigit() and 1 <= int(cmd) <= len(files) else (
            os.path.abspath(os.path.expanduser(cmd[2:].strip())) if cmd.lower().startswith("p ") else None)
        if p is None or not os.path.isfile(p):
            ui.say("  not a choice or no such file")
            continue
        n, errs = check_fm_csv(p, fm)
        if errs:
            ui.say("  " + "\n  ".join(errs))
            continue
        setup["fm_stations"] = p
        return


def menu_logo(ui, setup):
    ui.say(f"\nStep 6 of 7: boot screen (optional). A finished {LOGO_SIZE}-byte .rgb565 file, or none.")
    ui.say("  The installer cannot draw a personal boot screen (that needs the Rust simulator: docs/INSTALL.md).")
    while True:
        a = ui.ask("logo file (Enter = leave the boot screen alone)> ")
        if a == "":
            setup["boot_logo"] = None
            return
        p = os.path.abspath(os.path.expanduser(a))
        if os.path.isfile(p) and os.path.getsize(p) == LOGO_SIZE:
            setup["boot_logo"] = p
            return
        ui.say(f"  {p}: not a file of exactly {LOGO_SIZE} bytes")


def menu_core(ui, setup):
    ui.say("\nStep 7 of 7: core firmware")
    ui.say("  1. automatic: flash it on a stock radio; on a radio already running this firmware, ask you then")
    ui.say("  2. always flash the core")
    ui.say("  3. never flash it (the radio already runs exactly this build)")
    while True:
        a = ui.ask("core [1]> ") or "1"
        if a in ("1", "2", "3"):
            setup["core"] = {"1": "auto", "2": "flash", "3": "skip"}[a]
            return
        ui.say("  not a choice")


def run_menus(ui, setup, rel, cp, fm, port_given):
    ui.say("UV-5RM installer. Nothing is written to the radio until you type INSTALL at the end.")
    if not port_given:
        menu_port(ui, setup)
    if not setup.get("board"):
        menu_board(ui, setup)
    if not any(setup["apps"]):
        setup["apps"] = rel.default_slots()
    menu_apps(ui, setup, rel)
    menu_channels(ui, setup, cp)
    menu_fm(ui, setup, fm)
    menu_logo(ui, setup)
    menu_core(ui, setup)
    return setup


# ------------------------------------------------------------------------------------------------ the plan
class Plan:
    """Everything the install will write, validated, built with the tools' own rules. No radio is involved."""

    def __init__(self, setup, rel, cp, fm):
        self.setup, self.rel, self.cp, self.fm = setup, rel, cp, fm
        self.variant = variant_for(setup["board"] or "")
        self.core = f"fw-{self.variant}.BF"
        check_slots(setup["apps"])
        for n in setup["apps"]:
            if n is not None and n not in rel.apps:
                raise Refused(f"{n} is not in {rel.path}")
        abis = {rel.apps[n]["abi"] for n in setup["apps"] if n}
        if len(abis) > 1:
            raise Refused(f"the chosen apps were built for different ABIs ({', '.join(sorted(abis))}); they cannot all load. Use one release.")
        rel.check(self.core)
        for n in setup["apps"]:
            if n:
                rel.check(n)
        self.channel_counts = {}
        for g, p in setup["channels"].items():
            if not os.path.isfile(p):
                raise Refused(f"{p}: no such file (list for {g})")
            n, errs = check_channel_csv(p, resolve_group(g, cp), cp)
            if errs:
                raise Refused(f"{p} (list for {g}) cannot be used:\n  " + "\n  ".join(errs))
            self.channel_counts[g] = n
        if setup["fm_stations"]:
            n, errs = check_fm_csv(setup["fm_stations"], fm)
            if errs:
                raise Refused(f"{setup['fm_stations']}: " + "\n  ".join(errs))
            self.fm_count = n
        if setup["boot_logo"] and (not os.path.isfile(setup["boot_logo"]) or os.path.getsize(setup["boot_logo"]) != LOGO_SIZE):
            raise Refused(f"{setup['boot_logo']} is not a {LOGO_SIZE}-byte boot logo file")

    def summary(self, state_note="decided when the radio has been identified"):
        s, r = self.setup, self.rel
        lines = ["", "WILL BE WRITTEN TO THE RADIO:"]
        lines.append(f"  core firmware {self.core} ({self.variant}, board {s['board']}): {state_note}; setting '{s['core']}'")
        for i, n in enumerate(s["apps"], 1):
            lines.append(f"  app slot {i}: " + (f"{r.apps[n]['title']} ({n}, {r.apps[n]['size']} bytes)" if n else "empty (the slot is erased if it holds something)"))
        if s["channels"]:
            for g, p in s["channels"].items():
                lines.append(f"  channels, group {g}: {self.channel_counts[g]} from {p}")
            lines.append("  channel details (the text shown beside the channel number): replaced for the channels above"
                         + ("; other channels on the radio are kept" if s["keep_existing_channels"] else "; EVERY OTHER CHANNEL IS ERASED"))
        if s["fm_stations"]:
            lines.append(f"  FM station list: {self.fm_count} stations from {s['fm_stations']} (replaces the list on the radio)")
        if s["boot_logo"]:
            lines.append(f"  boot screen: {s['boot_logo']} (the old one is saved first)")
        lines += ["", "WILL NOT BE TOUCHED:",
                  "  settings, including TX INHIBIT and TX BANDS (nothing here writes them; the radio keeps refusing to transmit)",
                  "  calibration (read before and after to prove it)", "  VFO, DTMF contacts, ANI"]
        if s["channels"]:
            lines.append("  channels outside the lists above" if s["keep_existing_channels"] else "  (nothing else: every other channel is erased, as you chose)")
        else:
            lines.append("  channels and channel details (no list chosen)")
        if not s["fm_stations"]:
            lines.append("  the FM station list")
        if not s["boot_logo"]:
            lines.append("  the boot screen")
        lines += ["", "BACKUPS: made first, in a new folder (nothing is overwritten); the installer stops if they cannot be verified.", ""]
        return "\n".join(lines)


# ------------------------------------------------------------------------------------------------ talking to the tools
class Opts:
    def __init__(self, **kw):
        self.port = None
        self.dry_run = False
        self.yes = False
        self.settle, self.wait, self.retries, self.probe_timeout = 4.0, 180.0, 8, 2.0
        self.can_ask = True
        self.backup_dir, self.resume = "uv5rm-backups", None
        self.__dict__.update(kw)


class Radio:
    """Runs the tools. All radio traffic is in them; this only starts them, reads their output and decides."""

    def __init__(self, port, opts, ui, log):
        self.port, self.opts, self.ui, self.log = port, opts, ui, log
        self.first = True

    def run(self, argv, retry=True, label=None):
        """Run a tool; (returncode, output). A tool that could not get its handshake (radio still restarting from the
        previous tool) wrote nothing, so it is run again after a wait: up to opts.retries times."""
        script = os.path.join(TOOLS, argv[0])
        label = label or " ".join(argv[:1])
        for attempt in range(1, self.opts.retries + 2):
            if not self.first and attempt == 1 and retry:
                time.sleep(self.opts.settle)
            self.first = False
            self.log(f"$ {label}: {' '.join(argv)}  (attempt {attempt})")
            p = subprocess.Popen([sys.executable, script] + argv[1:], stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                 universal_newlines=True)
            lines = []
            for line in p.stdout:
                line = line.rstrip("\n")
                if "\r" in line:
                    line = line.split("\r")[-1]
                lines.append(line)
                self.log("  " + line)
                if line.strip():
                    self.ui.say("    " + line)
            rc = p.wait()
            out = "\n".join(lines)
            if "ident ack: 50" in out:
                raise StepFailed(label, "the programming cable echoes the computer's bytes back (ident ack: 50): it is not fully seated. "
                                 "Re-seat the cable and run again", "the radio was not written", "")
            no_hs = rc != 0 and ("NO RESPONSE: is the radio" in out or "Radio did not acknowledge" in out) and "connected to custom firmware" not in out
            if no_hs and retry and attempt <= self.opts.retries:
                self.ui.say(f"    the radio did not answer ({label}); it may still be restarting. Waiting {self.opts.settle:g} s and trying again "
                            f"({attempt}/{self.opts.retries})")
                time.sleep(self.opts.settle)
                continue
            return rc, out
        return 1, ""

    # --- identification (reads only) ---
    def probe_handshake(self, handshake, tries=3):
        """b'ack' / b'echo' / b'' for one handshake string. The only serial traffic outside the tools: the same strings the
        tools send (read from their source), nothing else."""
        try:
            import serial
        except ImportError:
            raise Refused("pyserial is missing: pip install pyserial")
        for _ in range(tries):
            try:
                s = serial.Serial(self.port, 115200, timeout=self.opts.probe_timeout / 4 + 0.05)
            except (OSError, ValueError) as e:
                raise Refused(f"cannot open {self.port}: {e}")
            try:
                s.reset_input_buffer()
                s.write(handshake)
                r = s.read(len(handshake) + 2)
            finally:
                s.close()
            if r[:1] == b"\x06":
                self.first = False          # the radio is now in a programming session and will restart: the next tool waits
                return "ack"
            if r and handshake.startswith(r[:1]):
                return "echo"
        return ""

    def identify(self):
        """'custom', 'stock' or 'bootloader'; StepFailed for an echo (cable) or silence."""
        recov = "Check the cable is pushed in all the way, the radio is on at its main screen (or in flash mode), and the right port is chosen."
        hs_custom, hs_stock = const_from("cps_tool.py", "HANDSHAKE"), const_from("radio_read.py", "IDENT")
        # Stock first: it is the one thing radio_read.py has been seen to send to a stock radio (AUDIT 25). The open firmware's
        # detector ignores it (cps/mod.rs HandshakeDetector resets on a mismatch), so a radio running this firmware is not
        # disturbed. Only when the stock firmware does not answer is anything else sent. Whether a stock radio or the
        # bootloader tolerates the later probes is UNVERIFIED.
        gone = "the cable echoes what the computer sends: it is not fully seated. Re-seat the cable and run again."
        r = self.probe_handshake(hs_stock)
        if r == "ack":
            return "stock"
        if r == "echo":
            raise StepFailed("identify", gone, "the radio was not written", recov)
        r = self.probe_handshake(hs_custom)
        if r == "ack":
            return "custom"
        if r == "echo":
            raise StepFailed("identify", gone, "the radio was not written", recov)
        rc, out = self.run(["bootloader_probe.py", self.port, "--no-idle", "--attempts", "2", "--timeout", f"{self.opts.probe_timeout:g}"],
                           retry=False, label="bootloader probe")
        if "BOOTLOADER ANSWERED" in out:
            return "bootloader"
        if "424f4f544c4f41444552" in out.replace(" ", ""):
            raise StepFailed("identify", "the cable echoes what the computer sends: it is not fully seated. Re-seat the cable and run again.",
                             "the radio was not written", recov)
        raise StepFailed("identify", f"nothing answered on {self.port} (not as stock firmware, as this firmware, or as the bootloader).",
                         "the radio was not written", recov)


def read_file(path):
    with open(path, "rb") as f:
        return f.read()


def write_new(path, data):
    with open(path, "xb") as f:
        f.write(data)


# ------------------------------------------------------------------------------------------------ the install
class Report:
    def __init__(self):
        self.stages = {s: ("not run", "") for s in STAGES}
        self.notes = []
        self.failure = None
        self.backup_dir = None

    def ok(self, stage, detail=""):
        self.stages[stage] = ("done", detail)

    def skip(self, stage, why):
        self.stages[stage] = ("skipped", why)

    def would(self, stage, what):
        self.stages[stage] = ("dry run", what)

    def text(self, dry):
        lines = ["", "=" * 70, "REPORT" + (" (dry run: nothing was written to the radio)" if dry else ""), "=" * 70]
        for s in STAGES:
            st, d = self.stages[s]
            lines.append(f"  {s:<18} {st}" + (f": {d}" if d else ""))
        if self.backup_dir:
            lines.append(f"\nBackups and logs: {self.backup_dir}")
        if self.failure:
            f = self.failure
            lines += ["", f"FAILED at step '{f.stage}': {f.message}", f"State of the radio: {f.state or 'see the step above'}"]
            if f.recover:
                lines.append(f"To recover: {f.recover}")
        lines += self.notes
        return "\n".join(lines)


def new_run_dir(root, resume):
    if resume:
        if not os.path.isdir(resume):
            raise Refused(f"{resume}: no such backup folder")
        return os.path.abspath(resume)
    os.makedirs(root, exist_ok=True)
    base = os.path.join(root, "uv5rm-" + datetime.datetime.now().strftime("%Y%m%d-%H%M%S"))
    d, n = base, 1
    while True:
        try:
            os.mkdir(d)
            return os.path.abspath(d)
        except FileExistsError:
            n += 1
            d = f"{base}-{n}"


def cmd_failed(stage, out, state, recover):
    tail = [l.strip() for l in out.splitlines() if l.strip()][-2:]
    return StepFailed(stage, " / ".join(tail) or "the tool failed without output", state, recover)


def run_install(setup, opts, ui):
    rep = Report()
    dry = opts.dry_run
    try:
        cp, fm = load_module("channel_plan"), load_module("fm_stations")
        rel = Release(setup["release"] or default_release())
        plan = Plan(setup, rel, cp, fm)
    except Refused as e:
        ui.say("\nREFUSED, nothing was written: " + str(e))
        return 2
    ui.say(plan.summary())
    if not dry and not opts.yes:
        ui.say("Type INSTALL (capitals) to write all of this to the radio, anything else to stop.")
        if ui.ask("confirm> ") != "INSTALL":
            ui.say("Stopped. Nothing was written.")
            return 2
    if not opts.port and not dry:
        ui.say("\nREFUSED, nothing was written: no serial port (--port, or \"port\" in the setup file).")
        return 2
    run_dir = None
    try:
        run_dir = new_run_dir(opts.backup_dir, opts.resume) if (opts.resume or not dry) else None
        rep.backup_dir = run_dir
        stamp = datetime.datetime.now().strftime("%H%M%S")
        logf = open(os.path.join(run_dir, f"install-{stamp}.log"), "a") if run_dir else None
        log = (lambda s: (logf.write(s + "\n"), logf.flush())) if logf else (lambda s: None)
        try:
            execute(plan, setup, opts, ui, rep, run_dir, stamp, log, dry)
        finally:
            if logf:
                logf.close()
    except StepFailed as e:
        rep.failure = e
        if e.stage in rep.stages:
            rep.stages[e.stage] = ("FAILED", e.message)
    except Refused as e:
        rep.failure = StepFailed("setup", str(e), "the radio was not written")
    except KeyboardInterrupt:
        rep.failure = StepFailed("interrupted", "stopped from the keyboard",
                                 "unknown: the step that was running may have been cut off part-way; its tool verifies by reading back, so run the installer again",
                                 "run the same command again; every step is safe to repeat")
    ui.say(rep.text(dry))
    if run_dir and not opts.resume and all(n.startswith("install-") and n.endswith(".log") for n in os.listdir(run_dir)):
        shutil.rmtree(run_dir)          # nothing was backed up or written (the run stopped before the first backup file)
        run_dir = None
    if run_dir:
        with open(os.path.join(run_dir, f"report-{datetime.datetime.now().strftime('%H%M%S')}.txt"), "w") as f:
            f.write(rep.text(dry) + "\n")
    return 1 if rep.failure else 0


def execute(plan, setup, opts, ui, rep, run_dir, stamp, log, dry):
    s, rel = setup, plan.rel
    port = opts.port
    radio = Radio(port, opts, ui, log)
    A = lambda name: os.path.join(run_dir, name) if run_dir else None

    # ---------------- a. identify
    state = None
    if port:
        ui.say("\n[a] identifying the radio (reads only)")
        state = radio.identify()
        ui.say(f"    the radio is: {dict(stock='running the STOCK firmware', custom='running THIS (open) firmware', bootloader='in BOOTLOADER (flash) mode')[state]}")
        rep.ok("identify", state)
    else:
        rep.skip("identify", "no radio attached")
        ui.say("\n[a] no serial port given: planning only")
    if state == "bootloader" and not opts.resume:
        if dry:
            rep.notes.append("The radio is in bootloader mode: a real run needs --resume with the backup folder of the earlier run.")
        else:
            raise StepFailed("identify", "the radio is in bootloader mode, where it cannot be backed up.", "in the bootloader (a core is probably missing or half written); calibration and channels cannot be read from here",
                             "if an earlier run of this installer failed in the core flash, run again with --resume FOLDER (its backup folder). "
                             "Otherwise power-cycle the radio normally and run again.")

    # ---------------- b. backup
    cal_before = None
    base_image = None
    if opts.resume:
        manifest = os.path.join(run_dir, "backup.json")
        if not os.path.isfile(manifest):
            raise Refused(f"{opts.resume}: no backup.json; this is not a backup folder made by the installer")
        with open(manifest) as f:
            bj = json.load(f)
        for name, h in bj["files"].items():
            if not os.path.isfile(os.path.join(run_dir, name)) or sha256_file(os.path.join(run_dir, name)) != h:
                raise StepFailed("backup", f"{name} in {run_dir} is missing or changed since it was made", "the radio was not written", "use the backup folder as the earlier run left it")
        cal_before = read_file(os.path.join(run_dir, bj["calibration"]))
        base_image = os.path.join(run_dir, bj["channels"]) if bj.get("channels") else None
        rep.ok("backup", f"using the verified backup in {run_dir}")
    elif dry:
        rep.would("backup", "stock: radio_read.py twice; this firmware: dump_nor.py --config twice; both compared (not done in a dry run)")
    else:
        ui.say(f"\n[b] backing up first, into {run_dir}")
        files = {}
        if state == "stock":
            img, cal = [A(f"stock-memory-{i}.img") for i in (1, 2)], [A(f"stock-calibration-{i}.bin") for i in (1, 2)]
            for i in (0, 1):
                rc, out = radio.run(["radio_read.py", port, img[i], "--cal", cal[i]], label=f"stock backup read {i + 1} of 2")
                if rc != 0 or not os.path.isfile(img[i]) or not os.path.isfile(cal[i]):
                    raise cmd_failed("backup", out, "stock firmware, nothing written", "check the cable and run the installer again; nothing was changed on the radio")
            if read_file(img[0]) != read_file(img[1]) or read_file(cal[0]) != read_file(cal[1]):
                raise StepFailed("backup", "two reads of the stock radio were NOT identical, so the backup cannot be trusted",
                                 "stock firmware, nothing written", "re-seat the cable and run the installer again (new backup folder); stop if it keeps differing")
            os.rename(img[0], A("stock-memory.img"))
            os.rename(cal[0], A("stock-calibration.bin"))
            os.remove(img[1])
            os.remove(cal[1])
            files = {"stock-memory.img": None, "stock-calibration.bin": None}
            cal_name, chan_name = "stock-calibration.bin", "stock-memory.img"
            bj = {"kind": "stock", "calibration": cal_name, "channels": chan_name}
        elif state == "custom":
            cfg = [A(f"config-{i}.bin") for i in (1, 2)]
            for i in (0, 1):
                rc, out = radio.run(["dump_nor.py", port, cfg[i], "--config"], label=f"configuration dump {i + 1} of 2")
                if rc != 0 or not os.path.isfile(cfg[i]):
                    raise cmd_failed("backup", out, "this firmware, nothing written", "check the cable and run the installer again; nothing was changed on the radio")
            a, b = read_file(cfg[0]), read_file(cfg[1])
            if a != b or len(a) != 0x10000:
                raise StepFailed("backup", "two reads of the radio's flash were NOT identical, so the backup cannot be trusted",
                                 "this firmware, nothing written", "re-seat the cable and run the installer again (new backup folder); stop if it keeps differing")
            os.rename(cfg[0], A("config-0x0000-0xFFFF.bin"))
            os.remove(cfg[1])
            write_new(A("calibration-0xF000.bin"), a[CAL[0]:CAL[1]])
            write_new(A("channels-before.img"), a[:CHAN_IMAGE])
            files = {"config-0x0000-0xFFFF.bin": None, "calibration-0xF000.bin": None, "channels-before.img": None}
            cal_name, chan_name = "calibration-0xF000.bin", "channels-before.img"
            bj = {"kind": "custom", "calibration": cal_name, "channels": chan_name}
        else:
            raise StepFailed("backup", "a radio in bootloader mode cannot be backed up", "in the bootloader", "power-cycle normally and run again")
        bj["files"] = {n: sha256_file(A(n)) for n in files}
        bj["created"] = datetime.datetime.now().isoformat(timespec="seconds")
        with open(A("backup.json"), "w") as f:
            json.dump(bj, f, indent=2)
        cal_before = read_file(A(cal_name))
        base_image = A(chan_name)
        ui.say(f"    backup verified (read twice, identical). calibration sha256 {hashlib.sha256(cal_before).hexdigest()[:16]}...")
        rep.ok("backup", f"{', '.join(sorted(files))} in {run_dir}")

    # ---------------- c. files
    ui.say("\n[c] checking every file against SHA256SUMS, and the core against the board gate")
    core_path = os.path.join(rel.path, plan.core)
    rc, out = radio.run(["flash_bf.py", core_path, port or "none", rel.sums[plan.core][:16], "--board", s["board"], "--variant", plan.variant, "--check-only"],
                        retry=False, label="core check")
    if rc != 0 or "serial port not opened" not in out:
        raise cmd_failed("files", out, "the radio was not written", "")
    stage_dir = os.path.join(run_dir, "stage-" + stamp) if run_dir else tempfile.mkdtemp(prefix="uv5rm-stage-")
    os.makedirs(stage_dir, exist_ok=True)
    try:
        for n in [x for x in s["apps"] if x]:
            shutil.copyfile(os.path.join(rel.path, n), os.path.join(stage_dir, n))
            rel.check(n, os.path.join(stage_dir, n))
        with open(os.path.join(stage_dir, "layout.json"), "w") as f:
            json.dump({"slots": s["apps"]}, f)
        # channels: the CSV files go in one folder named by group, the tool builds the image and the extras table
        chan_img = None
        if s["channels"]:
            cdir = os.path.join(stage_dir, "channels")
            os.makedirs(cdir)
            for g, p in s["channels"].items():
                shutil.copyfile(p, os.path.join(cdir, group_slug(resolve_group(g, plan.cp)) + ".csv"))
            chan_img = os.path.join(stage_dir, "channels.img")
            argv = ["channel_plan.py", "build", cdir, "-o", chan_img]
            if s["keep_existing_channels"] and base_image:
                argv += ["--base", base_image]
            rc, out = radio.run(argv, retry=False, label="channel plan")
            if rc != 0:
                raise cmd_failed("files", out, "the radio was not written", "")
        fm_table = None
        if s["fm_stations"]:
            fm_table = os.path.join(stage_dir, "fm.fmst")
            rc, out = radio.run(["fm_stations.py", "build", s["fm_stations"], "-o", fm_table], retry=False, label="FM station table")
            if rc != 0:
                raise cmd_failed("files", out, "the radio was not written", "")
        rep.ok("files", "apps, core and generated tables checked; core board gate: " + s["board"] + " -> " + plan.variant)

        # ---------------- d. core
        want_flash = decide_core(s, state, opts, ui)
        if want_flash is None and dry:
            rep.would("core", "the radio already runs this firmware: you would be asked whether it is this exact build")
            want_flash = False
        elif want_flash is None:
            raise StepFailed("core", "the radio already runs this firmware, and 'core' is 'auto': the installer cannot tell whether it is this exact build "
                             "(the port cannot read the core). Set core to \"flash\" or \"skip\".", "this firmware, nothing written", "run again with a setup that says core flash or skip")
        if rep.stages["core"][0] == "dry run":
            pass
        elif not want_flash:
            rep.skip("core", "not flashed (" + ("setting skip" if s["core"] == "skip" else "already this build, as you said") + ")")
        elif dry:
            rep.would("core", f"flash {plan.core} through the bootloader (you would be guided into flash mode)")
        else:
            state = flash_core(radio, plan, core_path, state, opts, ui, port, run_dir, stamp)
            rep.ok("core", f"{plan.core} flashed and acknowledged; the radio restarted into it")
        # ---------------- e. calibration
        if want_flash and not dry:
            ui.say("\n[e] reading the calibration page again; it must be identical to the copy from before")
            check_calibration(radio, cal_before, port, A(f"calibration-after-flash-{stamp}.bin"), "calibration")
            rep.ok("calibration", "identical to the copy from before the flash")
        elif want_flash and dry:
            rep.would("calibration", "compare with the copy from before the flash")
        else:
            rep.skip("calibration", "no core flash")
        if state == "stock" and not want_flash:
            raise Refused("the radio runs the stock firmware; apps and channels need this firmware's core")

        # ---------------- f. writes
        if dry and state != "custom":
            for st, what in (("apps", "install_apps.py"), ("channels", "restore_backup.py"), ("details", "channel_extras.py"),
                             ("fm", "fm_stations.py install"), ("logo", "boot_logo_raw.py")):
                if st == "apps" or (st == "channels" and chan_img) or (st == "details" and chan_img) or (st == "fm" and fm_table) or (st == "logo" and s["boot_logo"]):
                    rep.would(st, f"{what} once the radio runs this firmware ({'no radio attached' if state is None else 'it does not now'})")
                else:
                    rep.skip(st, "not chosen")
            rep.would("final calibration", "dump_nor.py --cal compared with the copy from before")
            return
        d = ["--dry-run"] if dry else []
        ui.say("\n[f] writing" if not dry else "\n[f] dry run: reading what each tool would change")
        rc, out = radio.run(["install_apps.py", port, stage_dir] + d, label="apps")
        if rc != 0 or ("read back in full" not in out and not dry) or "done; radio restarting" not in out:
            raise cmd_failed("apps", out, "apps: some slots may be empty or old; nothing else was changed; the core and settings are intact",
                             "run the installer again (slots that already match are skipped)")
        rep.would("apps", "see the lines above") if dry else rep.ok("apps", "9 slots read back in full")
        if chan_img:
            undo = A(f"undo-channels-{stamp}.img") or os.path.join(stage_dir, "undo.img")
            rc, out = radio.run(["restore_backup.py", port, chan_img, "--before", undo] + ([] if dry else ["--write"]), label="channels")
            if rc != 0 or (not dry and "verified through the port" not in out):
                raise cmd_failed("channels", out, "channels: whole 4 KB sectors are rewritten, so a sector may be partly written until this is run again; "
                                 "apps are done; the core and settings are intact",
                                 f"run the installer again (it is safe to repeat). The channels from before are in {undo} and the backup folder")
            rep.would("channels", "see the lines above") if dry else rep.ok("channels", f"{sum(plan.channel_counts.values())} channels placed; all 1000 slots verified through the port")
            extras = os.path.splitext(chan_img)[0] + ".extras"
            rc, out = radio.run(["channel_extras.py", port, extras] + d + ([] if dry else ["--before", A(f"extras-before-{stamp}.bin")]), label="channel details")
            if rc != 0:
                raise cmd_failed("details", out, "channel details table: may be the old table or erased (channels and apps are done); "
                                 "the details only show text beside the channel number", "run the installer again")
            rep.would("details", "see the lines above") if dry else rep.ok("details", "table written and read back")
        else:
            rep.skip("channels", "no list chosen")
            rep.skip("details", "no list chosen")
        if fm_table:
            rc, out = radio.run(["fm_stations.py", "install", port, fm_table] + d + ([] if dry else ["--before", A(f"fm-before-{stamp}.bin")]), label="FM stations")
            if rc != 0:
                raise cmd_failed("fm", out, "FM station table: may be the old list or erased (everything before it is done)",
                                 "run the installer again; it needs a core with the F port command (this release's core has it)")
            rep.would("fm", "see the lines above") if dry else rep.ok("fm", f"{plan.fm_count} stations written and read back")
        else:
            rep.skip("fm", "no list chosen")
        if s["boot_logo"]:
            rc, out = radio.run(["boot_logo_raw.py", port, s["boot_logo"], A(f"logo-before-{stamp}.rgb565") or os.path.join(stage_dir, "logo-before")] + d, label="boot logo")
            if rc != 0:
                raise cmd_failed("logo", out, "boot screen: the tool puts the old logo back when the read-back differs; if it says it did not, the saved copy is in the backup folder",
                                 f"run boot_logo_raw.py with the saved logo-before-{stamp}.rgb565 as the logo to undo")
            rep.would("logo", "see the lines above") if dry else rep.ok("logo", "written and read back; the old logo is saved")
        else:
            rep.skip("logo", "not chosen")
        if dry:
            rep.would("final calibration", "dump_nor.py --cal compared with the copy from before")
        else:
            ui.say("\n[g] final check: calibration must still be identical")
            check_calibration(radio, cal_before, port, A(f"calibration-final-{stamp}.bin"), "final calibration")
            rep.ok("final calibration", "identical to the copy from before")
            rep.notes.append("\nDone. The radio restarts after the last tool; TX INHIBIT and TX BANDS were not touched. Nothing was transmitted.")
    finally:
        if not run_dir:
            shutil.rmtree(stage_dir, ignore_errors=True)


def decide_core(setup, state, opts, ui):
    """True = flash, False = do not, None = cannot decide without being told."""
    c = setup["core"]
    if c == "flash":
        return True
    if state in ("stock", "bootloader"):
        if c == "skip":
            raise StepFailed("core", f"'core' is skip but the radio is {'on the stock firmware' if state == 'stock' else 'in the bootloader'}: it needs the core",
                             "unchanged", "set core to auto or flash")
        return True
    if state is None:            # no radio: plan for the full path
        return c != "skip"
    if c == "skip":
        return False
    if opts.yes or not opts.can_ask:
        return None
    a = ui.ask("The radio already runs this firmware. Is it exactly THIS build (same release)? The port cannot tell. [y = skip the flash / n = flash]> ")
    return a.lower() not in ("y", "yes")


def flash_core(radio, plan, core_path, state, opts, ui, port, run_dir, stamp):
    ui.say("\n[d] flashing the core firmware")
    if state != "bootloader":
        ui.say("    Put the radio into flash (bootloader) mode now:\n    " + BOOTLOADER_KEYS.replace("\n", "\n    "))
        deadline = time.monotonic() + opts.wait
        while True:
            if ui.tty():
                a = ui.ask("    Press Enter when the radio is in flash mode (q = give up)> ")
                if a.lower() == "q":
                    raise StepFailed("core", "you stopped before the radio was in flash mode", f"unchanged: still running {'this firmware' if state == 'custom' else 'the stock firmware'}",
                                     "run the installer again when ready")
            rc, out = radio.run(["bootloader_probe.py", port, "--no-idle", "--attempts", "2", "--timeout", f"{opts.probe_timeout:g}"], retry=False, label="bootloader probe")
            if "BOOTLOADER ANSWERED" in out:
                break
            if "424f4f544c4f41444552" in out.replace(" ", ""):
                raise StepFailed("core", "the cable echoes what the computer sends: re-seat the cable", "unchanged", "re-seat the cable and run again")
            if time.monotonic() > deadline:
                raise StepFailed("core", f"the bootloader did not answer within {opts.wait:g} s", f"unchanged: still running {'this firmware' if state == 'custom' else 'the stock firmware'}",
                                 "run the installer again; hold the two small side buttons below PTT (not PTT) while switching on")
            ui.say("    no answer yet from the bootloader; trying again (hold the two small side buttons below PTT while switching on)")
            time.sleep(opts.settle)
    ui.say("    bootloader answered; flashing (do not touch the radio or the cable)")
    rc, out = radio.run(["flash_bf.py", core_path, port, plan.rel.sums[plan.core][:16], "--board", plan.setup["board"], "--variant", plan.variant],
                        retry=False, label="core flash")
    if rc != 0 or "FLASH COMPLETE" not in out:
        started = "handshake ok" in out
        raise cmd_failed("core", out,
                         ("INTERRUPTED: the bootloader has a partly written core. The radio will not start normally but is still in flash mode (the bootloader is not overwritten). "
                          "Do not power it off if you can avoid it.") if started else
                         "the flash did not start: nothing was written; the radio is in flash mode",
                         f"run: python3 install.py --config SETUP.json --resume {run_dir}   (the radio stays in flash mode and the backup is reused). "
                         f"Or directly: python3 firmware/source-changes/flash_bf.py {core_path} {port} {plan.rel.sums[plan.core][:16]} --board {plan.setup['board']} --variant {plan.variant}")
    ui.say("    flash complete; waiting for the radio to restart into the new core")
    deadline = time.monotonic() + opts.wait
    time.sleep(opts.settle)
    while True:
        try:
            if radio.identify() == "custom":
                return "custom"
        except StepFailed:
            pass
        if time.monotonic() > deadline:
            raise StepFailed("core", f"the core was flashed and acknowledged, but the radio did not answer as this firmware within {opts.wait:g} s",
                             "the new core is written; it may be starting slowly, or not starting. Calibration has NOT been checked",
                             f"switch the radio off and on at its normal screen, then run: python3 firmware/source-changes/dump_nor.py {port} cal-now.bin --cal "
                             f"and compare with {os.path.join(run_dir, 'calibration-0xF000.bin') if run_dir else 'the backup'}; if the radio will not start, flash again with --resume {run_dir}")
        ui.say("    not answering yet; waiting")
        time.sleep(opts.settle)


def check_calibration(radio, before, port, out_path, stage):
    rc, out = radio.run(["dump_nor.py", port, out_path, "--cal"], label="calibration dump")
    if rc != 0 or not os.path.isfile(out_path):
        raise cmd_failed(stage, out, "unknown calibration state: it could not be read", f"read it by hand: python3 firmware/source-changes/dump_nor.py {port} cal.bin --cal")
    after = read_file(out_path)
    if after != before:
        diff = sum(1 for a, b in zip(after, before) if a != b)
        raise StepFailed(stage, f"CALIBRATION DIFFERS: {diff} bytes changed (before sha256 {hashlib.sha256(before).hexdigest()[:16]}..., "
                         f"after {hashlib.sha256(after).hexdigest()[:16]}...). Something wrote it. Stopped before anything else was written.",
                         "the core is flashed; apps and channels were NOT written; calibration differs from the backup",
                         f"do not use the radio for transmit. Keep {out_path} and the backup folder; ask for help before writing anything else")


# ------------------------------------------------------------------------------------------------ main
def build_parser():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0], add_help=True)
    ap.add_argument("--config")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--yes", action="store_true")
    ap.add_argument("--port")
    ap.add_argument("--release")
    ap.add_argument("--save")
    ap.add_argument("--backup-dir", default="uv5rm-backups")
    ap.add_argument("--resume")
    ap.add_argument("--settle", type=float, default=4.0)
    ap.add_argument("--wait", type=float, default=180.0)
    ap.add_argument("--retries", type=int, default=8)
    ap.add_argument("--probe-timeout", type=float, default=2.0)
    return ap


def main(argv=None, stdin=None, stdout=None):
    a = build_parser().parse_args(argv)
    ui = Ui(stdin, stdout)
    try:
        cp, fm = load_module("channel_plan"), load_module("fm_stations")
        if a.config:
            setup = load_setup(a.config)
        else:
            setup = empty_setup()
        if a.release:
            setup["release"] = os.path.abspath(a.release)
        if a.port:
            setup["port"] = a.port
        rel = Release(setup["release"] or default_release())
        if not a.config:
            run_menus(ui, setup, rel, cp, fm, bool(a.port))
            target = a.save or ui.ask("\nSave this setup so it can be repeated? File name (Enter = do not save)> ")
            if target:
                try:
                    save_setup(setup, target)
                    ui.say(f"saved {target}: repeat it with  install.py --config {target}")
                except Refused as e:
                    ui.say("  not saved: " + str(e))
        elif a.save:
            save_setup(setup, a.save)
    except Refused as e:
        ui.say("REFUSED, nothing was written: " + str(e))
        return 2
    opts = Opts(port=setup["port"], dry_run=a.dry_run or (setup["port"] is None), yes=a.yes, settle=a.settle, wait=a.wait,
                retries=a.retries, probe_timeout=a.probe_timeout, backup_dir=a.backup_dir, resume=a.resume,
                can_ask=(not a.config) or ui.tty())
    if setup["port"] is None and not a.dry_run:
        ui.say("No serial port: running as a dry run (nothing can be written without a radio).")
    return run_install(setup, opts, ui)


if __name__ == "__main__":
    sys.exit(main())
