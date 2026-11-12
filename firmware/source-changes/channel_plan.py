"""Turn CHIRP-format CSV channel lists into a 0x8000-byte channel image for restore_backup.py, or decode one.

  channel_plan.py build CHANNELS_DIR -o OUT.img [--base EXISTING.img] [--group-table group_table.rs]
  channel_plan.py show IMAGE            (any channel image: 0x8000 / 0x7D00 bytes, or a radio_read.py backup of 0x8380)

`build` also writes OUT.extras: the channel extras table (the Detail column: a location or description of up to 16
characters per channel, which the main screen shows beside the channel number). Install it with channel_extras.py.

Nothing here talks to a radio. Follow-up (preview, then write; UNVERIFIED on real hardware, see channels/README.md):
  restore_backup.py PORT OUT.img --before undo.img            dry run: says how many records differ, writes nothing
  restore_backup.py PORT OUT.img --before undo.img --write    write; undo.img gets the radio's current records first
restore_backup.py accepts a 0x8000-byte file as "channels only" (IMAGE_SIZES). It then handles exactly the 1000 channel
slots: every slot is compared with the radio and each DIFFERING slot is written, including slots that are all 0xFF in the
image, so an empty slot in OUT.img ERASES whatever the radio has there (the undo file restores it). Without --base, a build
therefore wipes every channel the CSVs do not define; use --base with a radio_read.py backup to keep them.
VFO, contacts and settings are not touched for a 0x8000-byte image.

Placement: each CHANNELS_DIR/<name>.csv is one group. <name> (case-insensitive, '-', '_' and '/' count as spaces, '&' ignored)
must equal a group name or short name in firmware/firmware-source/src/app/group_table.rs (parsed at run time, so the table is
not duplicated). Channels land in that group's slots in file order; the CSV Location column is IGNORED for placement. Groups overlap (GMRS
contains the FRS channels, REPEATERS the GMRS repeater slots): a file uses only its group's OWN slots, the ones inside no
smaller group (GMRS: 327-344, REPEATERS: 234-304), minus any all.csv has taken. A group with no slots of its own
(FRS GMRS MURS) cannot be a file. all.csv
is the exception: channels go to their own Location (printed number 1-1000). More channels than slots, two files claiming a
slot, a name over 12 characters or non-printable-ASCII, an unparseable row or an out-of-range frequency fail the build with
a message and write nothing. (CUSTOM RANGE has no slots of its own and cannot be a file.)

Record rules are mirrored from firmware/UV-5RM_Channel_Uploader.html (the upstream uploader); [U:x] = its function x.
  layout        32 bytes, 0xFF filled; RX freq BCD LE in 10 Hz units at 0; TX at 4                       [U:encode, bcdBytes, to10Hz]
  receive-only  Duplex "off": TX bytes stay 0xFF. Duplex "+"/"-": TX = RX +/- Offset. Anything else ("" or CHIRP "split"):
                TX = RX (simplex; split is NOT honoured, a warning says so)                              [U:encode]
  tones         Tone=Tone: rx code 0, tx code = round(rToneFreq*10). TSQL: both = round(cToneFreq*10) (tenths of Hz).
                DTCS: code = index in the 105-entry DCS_CODES list (1-based) + 105 when polarity is R; DtcsPolarity "ab":
                a applies to TX (DtcsCode), b to RX (RxDtcsCode, default DtcsCode). Any other Tone value (Cross, DTCS-R,
                empty) = no tone; CHIRP "Cross" gets a warning.                                           [U:encode, dcsCode]
  power         Power text parsed as watts: >6 -> 0 (High), >2 -> 2 (Mid), else 1 (Low); missing/non-numeric -> 2 (Mid)
                                                                                                         [U:powerRawFromText, POWER_NAMES]
  bandwidth     Mode NFM -> flag 0x40 (narrow); any other mode wide. AM is not stored: decode infers AM from 108-136 MHz
                RX.                                                                                       [U:encode, decode]
  scan          flag 0x04 set unless Skip is "S". dtmf group and ptt id 0, decoder bytes left 0xFF.                [U:encode]
  name          bytes 20-31, 0xFF padded (stock layout); decode ends at 0x00 or 0xFF. The uploader silently truncates to 12
                characters; this tool refuses instead.                                                    [U:encode]
  limits        RX 16-560 or 740-1120 MHz, +/- offset >0 and <=100 MHz, tone 60-260 Hz, standard DCS code    [U:validate]
                Extra here: BCD in 8 digits holds under 1000 MHz, so RX or TX >= 1000 MHz is refused (the uploader would
                garble it). Extra here: only Name and Frequency columns are required (the uploader also wants Power, RxDtcsCode,
                Location...); a missing optional column takes the default shown above.
  TX plan       The uploader does NOT make channels receive-only outside TX_PLAN (144-148, 222-225, 420-450, 462.550-462.725,
                467.550-467.725, 151.82-151.94, 154.57-154.60 MHz): it writes the TX frequency anyway and only tags the row
                "TX blocked" ("the radio shows TX PROHIBITED") [U:txAllowed]. Same here: a warning, record unchanged.
"""
import argparse, csv, io, math, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
GROUP_TABLE = os.path.join(HERE, "..", "firmware-source", "src", "app", "group_table.rs")
SLOT, SLOTS, IMAGE_LEN = 32, 1000, 0x8000
DETAIL_MAX = 16     # optional Detail column: goes into the channel extras table (OUT.extras), not the channel record
# Channel extras table, as the core reads it (flash_map.rs CHAN_EXTRA_*): one 20-byte record per slot:
# 16 bytes of detail text padded with 0xFF, 2 bytes reserved for group tags, 1 reserved, 0xA5 = "this record is set".
EXTRA_REC, EXTRA_TABLE, EXTRA_MARK = 20, 20480, 0xA5


def extras_table(entries):
    """The extras table for these entries: a record for every placed channel (so a channel without a Detail shows
    nothing rather than whatever was in the flash before), 0xFF everywhere else."""
    out = bytearray(b"\xff" * EXTRA_TABLE)
    for e in entries:
        rec = e.get("detail", "").encode("ascii").ljust(DETAIL_MAX, b"\xff") + b"\xff\xff\xff" + bytes([EXTRA_MARK])
        assert len(rec) == EXTRA_REC
        out[e["slot"] * EXTRA_REC:(e["slot"] + 1) * EXTRA_REC] = rec
    return bytes(out)
# [U:DCS_CODES]
DCS_CODES = ("023 025 026 031 032 036 043 047 051 053 054 065 071 072 073 074 114 115 116 122 125 131 132 134 143 145 152 155 "
             "156 162 165 172 174 205 212 223 225 226 243 244 245 246 251 252 255 261 263 265 266 271 274 306 311 315 325 331 "
             "332 343 346 351 356 364 365 371 411 412 413 423 431 432 445 446 452 454 455 462 464 465 466 503 506 516 523 526 "
             "532 546 565 606 612 624 627 631 632 645 654 662 664 703 712 723 731 732 734 743 754").split()
# [U:TX_PLAN], MHz
TX_PLAN = [(144, 148), (222, 225), (420, 450), (462.55, 462.725), (467.55, 467.725), (151.82, 151.94), (154.57, 154.6)]
POWER_NAMES = {0: "High", 1: "Low", 2: "Mid"}                       # [U:POWER_NAMES]


class PlanError(Exception):
    pass


# ---------- groups, parsed from the firmware's table ----------
def load_groups(path=GROUP_TABLE):
    """[(name, short, [(lo, hi), ...])] from group_table.rs MODES; PlanError if the file's shape is not recognised."""
    try:
        with open(path, encoding="utf-8") as f:
            src = f.read()
    except OSError as e:
        raise PlanError(f"cannot read the group table {path}: {e}")
    m = re.search(r"const ALL[^=]*=\s*&\[(.*?)\];", src)
    allr = parse_ranges(m.group(1)) if m else None
    groups = []
    for g in re.finditer(r'ChanMode\s*\{\s*name:\s*"([^"]+)",\s*short:\s*"([^"]+)",\s*ranges:\s*(ALL|&\[(.*?)\])\s*,?\s*\}', src):
        name, short, ref, body = g.groups()
        if ref == "ALL":
            if allr is None:
                raise PlanError(f"{path}: group {name} uses ALL but `const ALL` was not found")
            groups.append((name, short, allr))
        else:
            groups.append((name, short, parse_ranges(body)))
    if len(groups) < 2 or not any(g[0] == "ALL" for g in groups):
        raise PlanError(f"cannot parse the MODES table in {path} (found {len(groups)} groups, expected ChanMode {{ name, short, ranges }} entries)")
    return groups


def parse_ranges(body):
    r = [(int(a), int(b)) for a, b in re.findall(r"\(\s*(\d+)\s*,\s*(\d+)\s*\)", body)]
    if not r:
        raise PlanError(f"cannot parse channel ranges from: {body.strip()[:60]}")
    return r


def norm(s):
    return " ".join(re.sub(r"[-_/]", " ", s.lower().replace("&", " ")).split())


def slots_of(ranges):
    """Slot numbers (0-based) of PRINTED-number ranges, in order."""
    return [n - 1 for lo, hi in ranges for n in range(lo, hi + 1)]


def find_group(groups, stem):
    for g in groups:
        if norm(stem) in (norm(g[0]), norm(g[1])):
            return g
    raise PlanError(f"{stem}.csv: no group is named '{stem}'. Groups: " + ", ".join(f"{g[0]} ({g[1]})" for g in groups))


# ---------- encode / decode ----------
def js_round(x):
    return math.floor(x + 0.5)


def to10hz(text):
    return js_round(float(text) * 1e5)                              # [U:to10Hz]


def bcd(v):
    s = f"{v:08d}"
    return bytes(int(s[i:i + 2], 16) for i in (6, 4, 2, 0))        # [U:bcdBytes]


def from_bcd(b):
    v = 0
    for x in reversed(b):
        v = v * 100 + (x >> 4) * 10 + (x & 15)
    return v


def power_raw(text):
    m = re.match(r"\s*[-+]?(\d+\.?\d*|\.\d+)", text or "")          # parseFloat
    if not m:
        return 2
    w = float(m.group(0))
    return 0 if w > 6 else 2 if w > 2 else 1                        # [U:powerRawFromText]


def num(text, what):
    try:
        v = float(text)
    except ValueError:
        raise PlanError(f"{what} '{text}' is not a number")
    if not math.isfinite(v):
        raise PlanError(f"{what} '{text}' is not a number")
    return v


def dcs_code(code, pol):
    c = str(code).strip().zfill(3)
    if c not in DCS_CODES:
        raise PlanError(f"DCS code {code} is not a standard code")
    return DCS_CODES.index(c) + 1 + (105 if pol == "R" else 0)     # [U:dcsCode]


def encode(row):
    """(32-byte record, [warnings]) from a CSV row dict (stripped values). Raises PlanError."""
    g = lambda k, d="": (row.get(k) or d).strip()
    warns = []
    name = g("Name")
    if len(name) > 12:
        raise PlanError(f"name '{name}' is {len(name)} characters; the limit is 12")
    if not re.fullmatch(r"[\x20-\x7e]*", name):
        raise PlanError(f"name '{name}' has characters other than printable ASCII")
    detail = g("Detail")            # location / description; checked now, stored on the radio only once it has a place
    if len(detail) > DETAIL_MAX or not re.fullmatch(r"[\x20-\x7e]*", detail):
        raise PlanError(f"Detail '{detail}' must be at most {DETAIL_MAX} printable ASCII characters")
    freq = g("Frequency")
    if not freq:
        raise PlanError("no Frequency")
    mhz = num(freq, "frequency")
    if not ((16 <= mhz <= 560) or (740 <= mhz <= 1120)):           # [U:validate]
        raise PlanError(f"frequency {freq} MHz is outside 16-560 and 740-1120 MHz")
    duplex, tone = g("Duplex"), g("Tone")
    rx = to10hz(freq)
    tx = None
    if duplex in ("+", "-"):
        off = num(g("Offset", "0"), "offset")
        if not (0 < off <= 100):                                    # [U:validate]
            raise PlanError(f"offset '{g('Offset')}' must be above 0 and at most 100 MHz")
        tx = rx + to10hz(g("Offset", "0")) if duplex == "+" else rx - to10hz(g("Offset", "0"))
    elif duplex != "off":
        tx = rx
        if duplex == "split":
            warns.append("split duplex not supported: simplex")
    if max(rx, tx or 0) > 99999999 or (tx is not None and tx <= 0):
        raise PlanError("a frequency of 1000 MHz or more (or a TX below 0) does not fit the 8-digit BCD field")
    rxc = txc = 0
    if tone == "Tone":
        txc = tone_code(g("rToneFreq"), "rToneFreq")
    elif tone == "TSQL":
        rxc = txc = tone_code(g("cToneFreq"), "cToneFreq")
    elif tone == "DTCS":
        pol = g("DtcsPolarity", "NN").ljust(2, "N")
        txc = dcs_code(g("DtcsCode", "023"), pol[0])
        rxc = dcs_code(g("RxDtcsCode") or g("DtcsCode", "023"), pol[1])
    elif tone:
        warns.append(f"Tone '{tone}' not supported: no tone")
    b = bytearray(b"\xff" * SLOT)
    b[0:4] = bcd(rx)
    if tx is not None:
        b[4:8] = bcd(tx)
    b[8:12] = rxc.to_bytes(2, "little") + txc.to_bytes(2, "little")
    b[12], b[13], b[14] = 0, 0, power_raw(g("Power"))
    b[15] = (0 if g("Skip") == "S" else 0x04) | (0x40 if g("Mode") == "NFM" else 0)
    b[20:20 + len(name)] = name.encode("ascii")
    if g("Mode") == "AM":
        warns.append("AM is not stored; the radio decides by band")
    return bytes(b), warns


def tone_code(text, what):
    v = num(text, what)
    if not (60 <= v <= 260):                                         # [U:validate]
        raise PlanError(f"{what} {text} must be 60-260 Hz")
    return js_round(v * 10)                                          # [U:toneCode]


def decode(rec):
    """Dict of display fields from a 32-byte record, or None for an empty slot. Mirrors [U:decode]."""
    if rec == b"\xff" * SLOT:
        return None
    rx = from_bcd(rec[0:4])
    rxonly = rec[4:8] == b"\xff" * 4
    tx = rx if rxonly else from_bcd(rec[4:8])
    rxc, txc = int.from_bytes(rec[8:10], "little"), int.from_bytes(rec[10:12], "little")
    dcs = lambda v: (DCS_CODES[(v - 105 if v > 105 else v) - 1], "R" if v > 105 else "N")
    isdcs, istone = (lambda v: 1 <= v <= 210), (lambda v: v > 250)
    warns = []
    if rxc == 0 and istone(txc):
        tones = f"T{txc / 10:.1f}"
    elif rxc == txc and istone(txc):
        tones = f"TSQL {txc / 10:.1f}"
    elif isdcs(rxc) and isdcs(txc):
        (a, ap), (c, cp) = dcs(txc), dcs(rxc)
        tones = f"D{a}{ap}" if (a, ap) == (c, cp) else f"D{a}{ap}/{c}{cp}"
    elif rxc or txc:
        tones = f"raw {rxc:#x}/{txc:#x}"
        warns.append("unrecognised tone codes")
    else:
        tones = "-"
    name = bytearray()
    for x in rec[20:32]:
        if x in (0, 0xFF):
            break
        name.append(x)
    if not rxonly and not any(a * 1e6 <= tx * 10 <= c * 1e6 for a, c in TX_PLAN):
        warns.append("TX outside transmit plan")
    shift = "RX only" if rxonly else "simplex" if tx == rx else f"{'+' if tx > rx else '-'}{abs(tx - rx) / 1e5:g}"
    return dict(name=name.decode("latin-1"), rx=rx / 1e5, shift=shift, tones=tones, power=POWER_NAMES.get(rec[14], f"?{rec[14]}"),
                bw="narrow" if rec[15] & 0x40 else "wide", scan="scan" if rec[15] & 0x04 else "skip",
                am=108 <= rx / 1e5 <= 136, warns=warns)


# ---------- images ----------
def load_image(path):
    with open(path, "rb") as f:
        data = f.read()
    if len(data) not in (0x8380, 0x8000, SLOT * SLOTS):
        raise PlanError(f"{path} is {len(data)} bytes; expected 0x8380, 0x8000 or 0x7D00 (channel image or radio_read.py backup)")
    return data[:SLOT * SLOTS]


def read_rows(path):
    with open(path, encoding="utf-8-sig", newline="") as f:
        text = f.read()
    rd = csv.DictReader(io.StringIO(text))
    cols = rd.fieldnames or []
    missing = [c for c in ("Name", "Frequency") if c not in cols]
    if missing:
        raise PlanError(f"{os.path.basename(path)}: not a CHIRP channel CSV, missing column(s): {', '.join(missing)}")
    rows = []
    for r in rd:
        if not any((v or "").strip() for k, v in r.items() if k):
            continue
        rows.append((rd.line_num, {k: (v or "").strip() if isinstance(v, str) or v is None else "" for k, v in r.items() if k}))
    return rows


def build(directory, base=None, groups=None):
    """(table bytes (32000), [entries], errors). entries: slot, group, record, warnings. Errors are strings."""
    groups = groups or load_groups()
    errors, claims, entries, summary = [], {}, [], []
    files = sorted(f for f in os.listdir(directory) if f.lower().endswith(".csv"))
    if not files:
        raise PlanError(f"no .csv files in {directory}")

    def order(fn):          # all.csv (fixed Locations) first, then the smallest group first, then by name
        try:
            g = find_group(groups, fn[:-4])
        except PlanError:
            return (1, 0, fn)
        return (0, 0, fn) if g[0] == "ALL" else (1, len(slots_of(g[2])), fn)

    for fn in sorted(files, key=order):
        stem = fn[:-4]
        try:
            grp = find_group(groups, stem)
            if grp[0] == "CUSTOM RANGE":
                raise PlanError(f"{fn}: CUSTOM RANGE has no slots of its own; use a real group or all.csv")
            rows = read_rows(os.path.join(directory, fn))
        except (PlanError, csv.Error, UnicodeDecodeError) as e:
            errors.append(str(e))
            continue
        is_all = grp[0] == "ALL"
        # Groups overlap (GMRS contains the FRS channels, REPEATERS contains the GMRS repeater slots). A file uses only
        # the slots that are its group's OWN: not inside any smaller group, and not already taken (all.csv).
        mine = slots_of(grp[2])
        inner = {n for g in groups if g[0] not in ("ALL", "CUSTOM RANGE", grp[0]) and len(slots_of(g[2])) < len(mine)
                 for n in slots_of(g[2])}
        avail = [n for n in mine if n not in claims and (is_all or n not in inner)]
        if not is_all and len(rows) > len(avail):
            errors.append(f"{fn}: {len(rows)} channels but group {grp[0]} has only {len(avail)} free slots of its own")
        used = 0
        for k, (line, row) in enumerate(rows):
            where = f"{fn} line {line}" + (f" ({row.get('Name', '')})" if row.get("Name") else "")
            try:
                if is_all:
                    loc = row.get("Location", "")
                    if not loc.isdigit() or not 1 <= int(loc) <= SLOTS:
                        raise PlanError(f"all.csv needs a Location of 1-{SLOTS}, got '{loc}'")
                    slot = int(loc) - 1
                elif k < len(avail):
                    slot = avail[k]
                else:
                    continue
                rec, warns = encode(row)
            except PlanError as e:
                errors.append(f"{where}: {e}")
                continue
            if slot in claims:
                errors.append(f"{where}: slot {slot + 1} is already claimed by {claims[slot]}")
                continue
            claims[slot] = where
            entries.append(dict(slot=slot, group=grp[1], record=rec, warns=warns,
                                detail=(row.get("Detail") or "").strip()))
            used += 1
        summary.append((grp[0], used, SLOTS if is_all else len(avail)))
    table = bytearray(base if base is not None else b"\xff" * (SLOT * SLOTS))
    for e in entries:
        table[e["slot"] * SLOT:(e["slot"] + 1) * SLOT] = e["record"]
    entries.sort(key=lambda e: e["slot"])
    return bytes(table), entries, errors, summary


def group_of(groups, slot):
    for name, short, rng in groups:
        if name not in ("ALL", "CUSTOM RANGE") and (slot + 1) in {n for lo, hi in rng for n in range(lo, hi + 1)}:
            return short
    return "-"


def print_table(table, groups, labels=None, extra=None):
    hdr = ("CH", "GROUP", "NAME", "RX MHz", "SHIFT", "TONES", "PWR", "BW", "SCAN", "WARNINGS")
    rows = []
    for slot in range(SLOTS):
        d = decode(table[slot * SLOT:(slot + 1) * SLOT])
        if d is None:
            continue
        w = d["warns"] + ((extra or {}).get(slot, []))
        w = list(dict.fromkeys(w))
        rows.append((str(slot + 1), (labels or {}).get(slot) or group_of(groups, slot), d["name"], f"{d['rx']:.5f}".rstrip("0").rstrip("."),
                     d["shift"], d["tones"], d["power"], d["bw"] + (" AM?" if d["am"] else ""), d["scan"], "; ".join(w)))
    widths = [max(len(r[i]) for r in rows + [hdr]) for i in range(len(hdr))]
    for r in [hdr] + rows:
        print("  ".join(c.ljust(w) for c, w in zip(r, widths)).rstrip())
    return len(rows)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0], formatter_class=argparse.RawDescriptionHelpFormatter,
                                 epilog="See the module docstring (head -60 channel_plan.py) for the rules and the follow-up commands.")
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("dir")
    b.add_argument("-o", "--output", required=True)
    b.add_argument("--base")
    b.add_argument("--group-table", default=GROUP_TABLE)
    s = sub.add_parser("show")
    s.add_argument("image")
    s.add_argument("--group-table", default=GROUP_TABLE)
    a = ap.parse_args(argv)
    try:
        groups = load_groups(a.group_table)
        if a.cmd == "show":
            n = print_table(load_image(a.image), groups)
            print(f"{n} channels in {a.image}")
            return 0
        base = load_image(a.base) if a.base else None
        table, entries, errors, summary = build(a.dir, base, groups)
    except PlanError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    if errors:
        print(f"{len(errors)} problem(s); nothing written:", file=sys.stderr)
        for e in errors:
            print(f"  {e}", file=sys.stderr)
        return 1
    print("The CSV Location column is ignored for placement (channels fill each group's slots in file order); only all.csv uses it.")
    print_table(table, groups, {e["slot"]: e["group"] for e in entries}, {e["slot"]: e["warns"] for e in entries})
    for name, used, cap in summary:
        print(f"  {name}: {used} / {cap} slots used")
    os.makedirs(os.path.dirname(os.path.abspath(a.output)), exist_ok=True)
    with open(a.output, "wb") as f:
        f.write(table.ljust(IMAGE_LEN, b"\xff"))
    print(f"wrote {a.output} ({IMAGE_LEN:#x} bytes, {len(entries)} channels placed" +
          (f", other slots from {a.base}" if base else ", every other slot empty") + ")")
    extras = os.path.splitext(a.output)[0] + ".extras"
    with open(extras, "wb") as f:
        f.write(extras_table(entries))
    n = sum(1 for e in entries if e["detail"])
    print(f"wrote {extras} ({EXTRA_TABLE} bytes: the Detail text of {n} channels; install with channel_extras.py)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
