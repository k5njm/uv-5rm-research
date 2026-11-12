#!/usr/bin/env python3
"""Station names for the FM broadcast app: a CSV on the computer -> a table file -> the radio's data flash.

  fm_stations.py build STATIONS.csv -o OUT.fmst     check the CSV and write the table (4,096 bytes)
  fm_stations.py show OUT.fmst                      print what a table file (or a --before backup) holds
  fm_stations.py install PORT OUT.fmst [--before OLD.bin]
                                                    write the table to the radio through the programming port, read it
                                                    back and compare. --before saves what the radio had (8,192 bytes)

The CSV is CHIRP's format; only these columns are used (the rest are ignored, so a CHIRP export works as it is):
  Name       at most 12 printable ASCII characters (may be empty: the app then shows just the frequency)
  Frequency  MHz, 87.5 to 108.0, a multiple of 0.1 (the app tunes in 0.1 MHz steps)
  Detail     optional, at most 16 printable ASCII characters (a format or a city), shown under the name
At most 127 stations, no frequency twice. Nothing is truncated: a name that is too long fails the build.
Lists are not memory channels: keep them out of channels/ (channel_plan.py reads every file there); see channels/fm/.

The table, as the app reads it (apps/fm/src/logic.rs has the full description): the data flash at 0x1D8000, two 4 KB
halves. The tool writes half 0: a 32-byte header ("FMST", version 1, generation 1) and 32-byte records (frequency u16
in 0.1 MHz, name 12, detail 16, padded with 0xFF, 0xFF, 0xA5). Installing replaces the whole list, including stations
saved on the radio; keep a copy with --before. The programming port's `F` command erases both halves and accepts
chunks only inside them, so this tool cannot write anything else. Radio on, at its main frequency screen.
Needs a core with the `F` command (AUDIT 43); an older core answers with an error and nothing is written.
UNVERIFIED on real hardware until AUDIT says otherwise.
"""
import argparse, csv, os, struct, sys

ADDR, SIZE, HALF, REC, SLOTS = 0x1D8000, 0x2000, 0x1000, 32, 127
CMD_FM_WRITE, RAW = 0x46, 64
LO, HI = 875, 1080
NAME_LEN, DETAIL_LEN = 12, 16
MAGIC, VERSION, MARK = b"FMST", 1, 0xA5


class StationError(Exception):
    pass


def _ascii(text, limit, what, where):
    text = text.strip()
    if len(text) > limit:
        raise StationError(f"{where}: {what} '{text}' is {len(text)} characters; the limit is {limit}")
    if not all(0x20 <= ord(c) < 0x7F for c in text):
        raise StationError(f"{where}: {what} '{text}' has characters other than printable ASCII")
    return text


def _deci(text, where):
    try:
        mhz = float(text)
    except ValueError:
        raise StationError(f"{where}: '{text}' is not a frequency in MHz") from None
    deci = round(mhz * 10)
    if abs(mhz * 10 - deci) > 1e-6:
        raise StationError(f"{where}: {text} MHz is not a multiple of 0.1 MHz (the app tunes in 0.1 MHz steps)")
    if not LO <= deci <= HI:
        raise StationError(f"{where}: {text} MHz is outside 87.5 to 108.0")
    return deci


def read_csv(path):
    """[(deci, name, detail)] in file order; raises StationError naming the line."""
    with open(path, newline="", encoding="utf-8-sig") as f:
        rd = csv.DictReader(f)
        cols = {(c or "").strip().lower(): c for c in (rd.fieldnames or [])}
        for need in ("name", "frequency"):
            if need not in cols:
                raise StationError(f"{path}: no '{need.title()}' column (found: {', '.join(rd.fieldnames or []) or 'none'})")
        out, seen = [], {}
        for row in rd:
            where = f"{os.path.basename(path)} line {rd.line_num}"
            if not any((v or "").strip() for k, v in row.items() if k):
                continue
            deci = _deci((row[cols["frequency"]] or "").strip(), where)
            name = _ascii(row[cols["name"]] or "", NAME_LEN, "name", where)
            detail = _ascii(row[cols["detail"]] or "", DETAIL_LEN, "detail", where) if "detail" in cols else ""
            if deci in seen:
                raise StationError(f"{where}: {deci / 10:.1f} MHz is already on line {seen[deci]}")
            seen[deci] = rd.line_num
            out.append((deci, name, detail))
    if not out:
        raise StationError(f"{path}: no stations")
    if len(out) > SLOTS:
        raise StationError(f"{path}: {len(out)} stations; the table holds {SLOTS}")
    return out


def header(gen=1):
    return (MAGIC + bytes([VERSION, 0xFF]) + struct.pack("<H", gen)).ljust(REC - 1, b"\xff") + bytes([MARK])


def record(deci, name, detail):
    return (struct.pack("<H", deci) + name.encode().ljust(NAME_LEN, b"\xff") + detail.encode().ljust(DETAIL_LEN, b"\xff")
            + b"\xff" + bytes([MARK]))


def make_table(stations):
    """The 4,096-byte first half: header, records, then erased flash."""
    return (header() + b"".join(record(*s) for s in stations)).ljust(HALF, b"\xff")


def _text(field):
    end = next((i for i, b in enumerate(field) if b in (0x00, 0xFF)), len(field))
    return field[:end].decode("ascii", "replace").rstrip()


def parse_half(blob):
    """(generation, [(deci, name, detail)]) of one half; None if its header is not valid. Raises on a damaged record."""
    if len(blob) != HALF:
        raise StationError(f"a table half is {HALF} bytes, not {len(blob)}")
    if blob[:4] != MAGIC or blob[4] != VERSION or blob[REC - 1] != MARK:
        return None
    gen = struct.unpack_from("<H", blob, 6)[0]
    out = []
    for i in range(1, SLOTS + 1):
        r = blob[i * REC:(i + 1) * REC]
        deci = struct.unpack_from("<H", r)[0]
        if deci == 0xFFFF and r[REC - 1] == 0xFF:
            continue
        if r[REC - 1] != MARK or not LO <= deci <= HI:
            if deci == 0 and r[REC - 1] == MARK:
                continue            # deleted on the radio
            raise StationError(f"record {i} is damaged")
        out.append((deci, _text(r[2:14]), _text(r[14:30])))
    return gen, out


def decode(blob):
    """The active half of a 4,096- or 8,192-byte file: (half, generation, stations); None if there is no valid table."""
    halves = [blob[i:i + HALF] for i in range(0, len(blob), HALF)]
    if len(blob) not in (HALF, SIZE):
        raise StationError(f"a table is {HALF} or {SIZE} bytes, not {len(blob)}")
    found = []
    for n, h in enumerate(halves):
        try:
            p = parse_half(h)
        except StationError:
            p = None
        if p:
            found.append((n, p[0], p[1]))
    if not found:
        return None
    best = found[0]
    for f in found[1:]:
        if ((best[1] - f[1]) & 0xFFFF) >= 0x8000:   # the other generation is newer (wrap-safe, as the app compares)
            best = f
    return best


def describe(stations):
    return [f"  {d / 10:6.1f} MHz  {n or '(no name)':<12}  {t}".rstrip() for d, n, t in stations]


def cmd_build(a):
    stations = read_csv(a.csv)
    table = make_table(stations)
    if os.path.exists(a.output) and not a.force:
        raise SystemExit(f"REFUSING TO OVERWRITE {a.output} (use --force)")
    with open(a.output, "wb") as f:
        f.write(table)
    print("\n".join(describe(stations)))
    print(f"wrote {a.output}: {len(stations)} stations, {len(table)} bytes; install with: fm_stations.py install PORT {a.output}")


def cmd_show(a):
    with open(a.table, "rb") as f:
        blob = f.read()
    d = decode(blob)
    if d is None:
        raise SystemExit(f"{a.table}: no valid station table")
    print(f"half {d[0]}, generation {d[1]}, {len(d[2])} stations")
    print("\n".join(describe(d[2])))


def _connect(port):
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, "cps_tool.py"), encoding="utf-8") as f:
        source = f.read()
    src = source[:source.index("def do_logo")]
    assert "for _ in range(5):" in src
    env = {"__name__": "cps_tool"}
    exec(src.replace("for _ in range(5):", "for _ in range(1):", 1), env)     # ONE handshake
    return env["Cps"](port), env


def cmd_install(a):
    with open(a.table, "rb") as f:
        table = f.read()
    if len(table) != HALF:
        raise SystemExit(f"{a.table} is {len(table)} bytes; a table file is exactly {HALF}")
    try:
        parsed = parse_half(table)
    except StationError as e:
        raise SystemExit(f"{a.table}: {e}. Nothing written.")
    if parsed is None:
        raise SystemExit(f"{a.table} is not a station table (no FMST header). Nothing written.")
    cps, env = _connect(a.port)
    read_raw, frame_timeout, erase_timeout = env["READ_RAW"], env["FRAME_TIMEOUT"], env["ERASE_TIMEOUT"]

    def read_flash():
        out = bytearray()
        for i in range(SIZE // RAW):
            out += cps.send(read_raw, ADDR // RAW + i)[2]
        return bytes(out)

    want = table + b"\xff" * HALF
    current = read_flash()
    if a.before:
        if os.path.exists(a.before) and open(a.before, "rb").read() != current:
            cps.end()
            raise SystemExit(f"{a.before} exists and is not what the radio has now; choose a new file name. Nothing written.")
        with open(a.before, "wb") as f:
            f.write(current)
        print(f"saved the radio's current table to {a.before}")
    had = decode(current)
    print(f"the radio has {len(had[2]) if had else 'no'} stations; this table has {len(parsed[1])}")
    if current == want:
        print("the radio already has this table; nothing written")
    else:
        for off in range(0, HALF, 512):
            cps.send(CMD_FM_WRITE, off, table[off:off + 512], erase_timeout if off == 0 else frame_timeout)
        if read_flash() != want:
            cps.end()
            raise SystemExit("STATION TABLE VERIFY FAILED: the radio's table differs from the file. Run again.")
        print(f"station table written and verified: {len(parsed[1])} stations")
    cps.end()
    print("done; radio restarting")


def main(argv=None):
    ap = argparse.ArgumentParser(description="FM broadcast station names", add_help=True)
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("csv")
    b.add_argument("-o", "--output", required=True)
    b.add_argument("--force", action="store_true", help="overwrite an existing output file")
    s = sub.add_parser("show")
    s.add_argument("table")
    i = sub.add_parser("install")
    i.add_argument("port")
    i.add_argument("table")
    i.add_argument("--before", help="save the radio's current table here first (8,192 bytes)")
    a = ap.parse_args(argv)
    try:
        {"build": cmd_build, "show": cmd_show, "install": cmd_install}[a.cmd](a)
    except StationError as e:
        raise SystemExit(f"{e}\nnothing written")


if __name__ == "__main__":
    main()
