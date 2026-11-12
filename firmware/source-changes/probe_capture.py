"""Download and decode what FSK PROBE recorded (its `#` key) from the radio's flash. Read-only.

  probe_capture.py PORT OUT.bin      download the capture, write OUT.bin and OUT.csv, print a summary
  probe_capture.py --decode IN.bin   decode a file downloaded earlier (no radio), write IN.csv
  add --ax25 to either to force the AX.25 / APRS decode (it runs by itself for 1200 baud captures and APRS presets):
  every frame is read in every plausible word and bit order, flags found, NRZI undone, bit stuffing removed, the FCS
  checked; frames that pass are printed as SOURCE>DEST,PATH:payload, and when none pass, diagnostics say why

The capture lives at 0x1B0000-0x1D0000 in the radio's data flash (apps/fskprobe/src/logic.rs):
  page 0 (256 bytes)  "FSKCAP3\\0" (version 2, "FSKCAP2\\0", stops after the frame words; version 1, "FSKCAP1\\0", after the record size):
                      frequency Hz u32, baud u16, sync halves 2 x u16, baud register u16, start time u32 (100 us
                      units), record size u8; then header version u8, options u8, words per interrupt u8, the values
                      written to REG_3F, 58, 59 (receive running), 5C, 5D, 5E, 70 (7 x u16), frame words u16;
                      version 3 adds: bandwidth setting u8 (0 auto, 1 1.2K, 2 2.4K, 3 FFSK 1200/1800), mode u8 (0 FSK,
                      1 FFSK 1200/2400, 2 FFSK 1200/1800), preset u8 (0 RS41, 1 APRS, 2 custom), frame kind u8 (0 RS41, 1 APRS)
  then records        8 bytes each, little-endian: time u32 (100 us units), FIFO word u16, flags u8, RSSI raw u8
                      flags: 1 = sync seen, 2 = word read, 4 = chip's done flag, 8 / 16 = chip matched the sync as sent /
                      inverted (REG_0B), 32 = last word of a frame the app ended by count, 64 = word read after the done
                      flag to empty the FIFO; 0xFF = erased, the end of the capture
At most 16,352 records. OUT.bin is the header page and the record pages that hold data, exactly as read.
Only the handshake, raw-read ('D') and END frames can leave this script, and only for addresses inside that range.
Radio on, at its main frequency screen (not inside an app). An existing OUT.bin is never overwritten.
"""
import os, struct, sys

CAP_ADDR, CAP_BYTES, PAGE, REC = 0x1B0000, 0x20000, 256, 8
MAGIC1 = b"FSKCAP1\0"
MAGIC2 = b"FSKCAP2\0"
MAGIC = b"FSKCAP3\0"
MAGICS = {MAGIC1: 1, MAGIC2: 2, MAGIC: 3}
MAX_RECORDS = (CAP_BYTES - PAGE) // REC
RAW_CHUNK = 64
FLAG_SYNC, FLAG_WORD, FLAG_DONE, FLAG_POS, FLAG_NEG, FLAG_FRAME, FLAG_TAIL = 1, 2, 4, 8, 16, 32, 64
REG_NAMES = ("REG_3F", "REG_58", "REG_59", "REG_5C", "REG_5D", "REG_5E", "REG_70")
LEN_NAMES = ("frame", "256 bytes", "2048 bytes")
MODE_NAMES = {0: "FSK", 4: "FFSK 1200/2400", 7: "FFSK 1200/1800"}
BW_NAMES = {0: "FSK 1.2K", 4: "FSK 2.4K / FFSK 1200/2400", 1: "FFSK 1200/1800", 2: "NOAA SAME"}
PRESET_NAMES = ("RS41", "APRS", "custom")


def parse(blob):
    """(header dict, [(time, word, flags, rssi_raw)]) from a downloaded capture; raises ValueError if it is not one.
    Reads version 1, 2 and 3 headers; version 1 has no `version` key, only version 3 has `extra`."""
    if len(blob) < PAGE or blob[:8] not in MAGICS:
        raise ValueError("not an FSK PROBE capture (no FSKCAP1, FSKCAP2 or FSKCAP3 header)")
    freq, baud, sync1, sync2, baudreg, t0, rec = struct.unpack_from("<IHHHHIB", blob, 8)
    if rec != REC:
        raise ValueError(f"record size {rec} is not the {REC} this tool reads")
    head = {"freq_hz": freq, "baud": baud, "sync": (sync1, sync2), "baud_reg": baudreg, "t0": t0, "version": 1}
    if MAGICS[blob[:8]] >= 2:
        version, opts, per_irq = struct.unpack_from("<BBB", blob, 25)
        if version != MAGICS[blob[:8]]:
            raise ValueError(f"header version {version} does not match the {MAGICS[blob[:8]]} its magic says")
        regs = struct.unpack_from("<7H", blob, 28)
        (frame_words,) = struct.unpack_from("<H", blob, 42)
        head.update(version=version, opts=opts, per_irq=per_irq, regs=dict(zip(REG_NAMES, regs)),
                    frame_words=frame_words)
        if version >= 3:
            bw, mode, preset, kind = struct.unpack_from("<4B", blob, 44)
            head["extra"] = {"bw": bw, "mode": mode, "preset": preset, "kind": kind}
    records = []
    for off in range(PAGE, min(len(blob), CAP_BYTES) - REC + 1, REC):
        t, word, flags, rssi = struct.unpack_from("<IHBB", blob, off)
        if flags == 0xFF:
            break
        records.append((t, word, flags, rssi))
    return head, records


def describe(head):
    """Lines saying which configuration made the capture."""
    if head["version"] == 1:
        return ["header version 1: the configuration was not recorded. Every version 1 capture came from the first probe, "
                "whose REG_59 had no 4-byte-sync bit (a 16-bit match) and held the RX-FIFO clear bit, and which asked "
                "for 65 bytes (HARDWARE.md, FSK modem registers)"]
    o, r = head["opts"], head["regs"]
    sync2 = bool(o >> 5 & 1)
    out = []
    if "extra" in head:
        e, r58, r5d = head["extra"], r["REG_58"], r["REG_5D"]
        out.append(f"preset {PRESET_NAMES[min(e['preset'], 2)]}; RX mode {MODE_NAMES.get(r58 >> 10 & 7, 'unknown')} "
                   f"(REG_58 bits 12:10 = {r58 >> 10 & 7:03b}), bandwidth {BW_NAMES.get(r58 >> 1 & 7, 'unknown')} "
                   f"(bits 3:1 = {r58 >> 1 & 7:03b}, setting {('auto', '1.2K', '2.4K', 'FFSK 1200/1800')[e['bw'] & 3]}); "
                   f"chip told {((r5d >> 8) & 0xFF | ((r5d >> 5) & 7) << 8) + 1} bytes; "
                   f"frame kind {('RS41', 'APRS')[e['kind'] & 1]}")
    out += [f"options {o:#04x}: length {LEN_NAMES[o & 3]} (REG_5D {r['REG_5D']:#06x}), threshold {head['per_irq']} "
            f"word(s) per interrupt, invert {'on' if o >> 3 & 1 else 'off'}, RX bandwidth {'2.4K' if o >> 4 & 1 else '1.2K'}, "
            f"sync {2 if sync2 else 4} bytes; frame = {head['frame_words']} words",
            "registers as written: " + " ".join(f"{k} {v:04X}" for k, v in r.items())]
    return out


def frames(records):
    """Split the records at each sync. Each frame: start time, end time, words, how it ended: 'count' (the app's
    word count, flag 32), 'done' (the chip's done flag), 'next' (another sync came first) or 'cut' (the capture or
    the words ended)."""
    out, cur = [], None
    for t, word, flags, _ in records:
        if flags & FLAG_SYNC:
            if cur:
                cur["end"] = "next"
                out.append(cur)
            cur = {"t0": t, "t1": t, "words": 0, "end": "cut", "pos": bool(flags & FLAG_POS), "neg": bool(flags & FLAG_NEG),
                   "data": []}
        if cur is None:
            continue
        if flags & FLAG_WORD:
            cur["words"] += 1
            cur["data"].append(word)
        cur["t1"] = t
        if flags & (FLAG_FRAME | FLAG_DONE):
            cur["end"] = "count" if flags & FLAG_FRAME else "done"
            out.append(cur)
            cur = None
    if cur:
        out.append(cur)
    return out


def summary(head, records):
    out = [f"frequency {head['freq_hz'] / 1e6:.6f} MHz, {head['baud']} baud (register {head['baud_reg']:#06x}), "
           f"sync {head['sync'][0]:04X} {head['sync'][1]:04X}"]
    out += describe(head)
    if not records:
        return out + ["no records: the modem delivered nothing while the capture ran"]
    words = [r for r in records if r[2] & FLAG_WORD]
    syncs = sum(1 for r in records if r[2] & FLAG_SYNC)
    span = (records[-1][0] - head["t0"]) & 0xFFFFFFFF
    out.append(f"{len(records)} records of {MAX_RECORDS} ({'FULL' if len(records) >= MAX_RECORDS else 'not full'}): "
               f"{syncs} sync, {len(words)} words, over {span / 10000:.2f} s")
    if len(words) > 1:
        gaps = sorted((b[0] - a[0]) & 0xFFFFFFFF for a, b in zip(words, words[1:]))
        times = {}
        for r in words:
            times[r[0]] = times.get(r[0], 0) + 1
        out.append(f"time between words: median {gaps[len(gaps) // 2] / 10:.1f} ms, longest {gaps[-1] / 10:.1f} ms; "
                   f"most words read in one interrupt: {max(times.values())} (8 means the FIFO may have overflowed)")
        if span:
            out.append(f"average {len(words) * 16 * 10000 / span:.0f} bits/s delivered, against {head['baud']} on the air")
    if syncs:
        pos = sum(1 for r in records if r[2] & FLAG_POS)
        neg = sum(1 for r in records if r[2] & FLAG_NEG)
        fr = frames(records)
        by = {k: sum(1 for f in fr if f["end"] == k) for k in ("count", "done", "next", "cut")}
        out.append(f"syncs: {syncs} ({pos} as sent, {neg} inverted); frames ended by count {by['count']}, "
                   f"by the chip's done flag {by['done']}, by the next sync {by['next']}, cut off {by['cut']}")
        if fr:
            ws = sorted(f["words"] for f in fr)
            ds = sorted(((f["t1"] - f["t0"]) & 0xFFFFFFFF) for f in fr)
            out.append(f"words per frame: min {ws[0]}, median {ws[len(ws) // 2]}, max {ws[-1]}; "
                       f"sync to end: median {ds[len(ds) // 2] / 10:.1f} ms, longest {ds[-1] / 10:.1f} ms")
    rssi = [r[3] for r in records]
    out.append(f"RSSI {min(rssi) - 160} to {max(rssi) - 160} dBm")
    out.append("first words: " + " ".join(f"{r[1]:04X}" for r in words[:16]))
    return out


# ---- AX.25 / APRS (Bell 202) decoding of what the modem delivered -----------------------------------------------------
# The chip hands over the demodulator's output bits, 16 to a FIFO word. For APRS those are the raw tone decisions: NRZI
# coded (a 0 data bit is a tone change, a 1 is no change), HDLC framed (flag 0x7E, a 0 stuffed after five 1s, bytes
# least-significant bit first, then a CRC-16/X.25 FCS, low byte first). Nothing says which byte of a word came first on
# the air, which bit of a byte came first, or whether the chip already undid the NRZI, so every reading is tried.
# NRZI is polarity-blind (inverting every raw bit gives the same data), so polarity only matters for the "direct"
# readings, which treat the raw bits as data without undoing NRZI.

FLAG = 0x7E
FCS_RESIDUE = 0xF0B8       # the CRC register after the frame INCLUDING its FCS, with no final inversion
ORDERS = [(b, o) for b in ("lo", "hi") for o in ("msb", "lsb")]       # byte of the word first, bit of the byte first
READINGS = [(b, o, m) for (b, o) in ORDERS for m in ("nrzi", "direct", "direct-inverted")]


def crc_x25_register(data, reg=0xFFFF):
    for byte in data:
        reg ^= byte
        for _ in range(8):
            reg = (reg >> 1) ^ 0x8408 if reg & 1 else reg >> 1
    return reg


def fcs_bytes(data):
    """The two FCS bytes for `data`, low byte first."""
    crc = crc_x25_register(data) ^ 0xFFFF
    return bytes([crc & 0xFF, crc >> 8])


def fcs_ok(frame):
    return len(frame) >= 3 and crc_x25_register(frame) == FCS_RESIDUE


def words_to_bits(words, byte_first="lo", bit_first="msb"):
    """The bits of FIFO words in time order, for a chosen byte order within a word and bit order within a byte."""
    bits = []
    for w in words:
        pair = (w & 0xFF, w >> 8) if byte_first == "lo" else (w >> 8, w & 0xFF)
        for byte in pair:
            bits.extend((byte >> k) & 1 for k in (range(7, -1, -1) if bit_first == "msb" else range(8)))
    return bits


def nrzi_decode(raw):
    """Data bits from tone levels: 1 when the level did not change, 0 when it did. One bit shorter than `raw`."""
    return [1 if a == b else 0 for a, b in zip(raw, raw[1:])]


def nrzi_encode(data, level=0):
    out = []
    for b in data:
        if b == 0:
            level ^= 1
        out.append(level)
    return out


def hdlc_segments(bits):
    """Splits a data-bit stream at HDLC flags. Returns (segments, stats): each segment is the unstuffed bits between two
    flags (empty ones, back-to-back flags, are not segments); stats counts flags, the longest run of consecutive flags,
    and aborts (seven or more 1s in a row, which no frame contains)."""
    segs, cur, ones = [], [], 0
    flags = run = longest = aborts = 0
    discard = False
    had_flag = False
    for b in bits:
        if b:
            ones += 1
            cur.append(1)
            if ones == 7 and not discard:
                aborts += 1
                discard = True
            continue
        if ones == 6:                               # 0 111111 0: a flag; its opening 0 and six 1s are the last 7 bits
            flags += 1
            run = run + 1 if had_flag and len(cur) <= 7 and not discard else 1
            if not discard and len(cur) > 7:
                segs.append(cur[:-7])
            longest = max(longest, run)
            had_flag = True
            cur, discard = [], False
        elif ones != 5:                             # (after five 1s the 0 is a stuffed one, and is dropped)
            cur.append(0)
        ones = 0
    return segs, {"flags": flags, "longest_flag_run": longest, "aborts": aborts}


def bits_to_bytes(bits):
    return bytes(sum(b << k for k, b in enumerate(bits[i:i + 8])) for i in range(0, len(bits) - 7, 8))


def addr_text(field):
    """One 7-byte AX.25 address as (call-ssid, last-address bit, has-been-repeated bit); None if it is not one."""
    call = bytes(b >> 1 for b in field[:6])
    if not all(0x20 <= c < 0x7F for c in call):
        return None
    ssid = field[6] >> 1 & 15
    name = call.decode("ascii").rstrip() + (f"-{ssid}" if ssid else "")
    return name, field[6] & 1, field[6] >> 7 & 1


def format_frame(frame):
    """`SOURCE>DEST,PATH:payload` for a UI frame, or a description of a frame that is not one; the FCS is not shown."""
    body = frame[:-2]
    addrs, i = [], 0
    while i + 7 <= len(body) and len(addrs) < 10:
        a = addr_text(body[i:i + 7])
        if a is None:
            break
        addrs.append(a)
        i += 7
        if a[1]:
            break
    if len(addrs) < 2 or not addrs[-1][1]:
        return "not an AX.25 address field: " + body[:24].hex()
    dest, src, via = addrs[0][0], addrs[1][0], addrs[2:]
    path = "".join("," + n + ("*" if h else "") for n, _, h in via)
    if i + 2 > len(body):
        return f"{src}>{dest}{path}: (no control byte)"
    ctl, pid = body[i], body[i + 1]
    info = body[i + 2:]
    text = "".join(chr(c) if 0x20 <= c < 0x7F else f"<{c:02X}>" for c in info)
    if ctl == 0x03:
        return f"{src}>{dest}{path}:{text}" + ("" if pid == 0xF0 else f"  [pid {pid:02X}]")
    return f"{src}>{dest}{path}: control {ctl:02X} pid {pid:02X} {text}"


def plausible_address(frame):
    """How many of the first 14 bytes look like shifted ASCII callsign characters or SSID bytes (0 to 14)."""
    return sum(1 for k, b in enumerate(frame[:14]) if (k % 7 == 6) or 0x20 <= b >> 1 < 0x7F)


def nearest_single_flip(frame):
    """Bit index (from the frame's first bit) whose flip makes the FCS pass, or None. Brute force; only used on a few frames."""
    if len(frame) > 400:
        return None
    buf = bytearray(frame)
    for k in range(len(buf) * 8):
        buf[k // 8] ^= 1 << (k % 8)
        ok = fcs_ok(bytes(buf))
        buf[k // 8] ^= 1 << (k % 8)
        if ok:
            return k
    return None


def reading_bits(words, reading):
    byte_first, bit_first, mode = reading
    raw = words_to_bits(words, byte_first, bit_first)
    if mode == "nrzi":
        return nrzi_decode(raw)
    return raw if mode == "direct" else [b ^ 1 for b in raw]


def run_histogram(raw_frames, top=10):
    """Counts of runs of identical raw bits by length (1..top-1, and `top` meaning top or more), over all frames."""
    hist = [0] * (top + 1)
    for raw in raw_frames:
        n = 0
        for i, b in enumerate(raw):
            n += 1
            if i + 1 == len(raw) or raw[i + 1] != b:
                hist[min(n, top)] += 1
                n = 0
    return hist


def ax25_analyse(frame_words):
    """frame_words: a list of word lists (one per captured frame). For every reading, find and check frames.
    Returns {reading: stats} where stats has flags, longest_flag_run, aborts, tried (segments of at least 17 bytes),
    short, odd (not a whole number of bytes), passed [(frame index, bytes)], bad [(frame index, bytes)]."""
    out = {}
    for reading in READINGS:
        st = {"flags": 0, "longest_flag_run": 0, "aborts": 0, "tried": 0, "short": 0, "odd": 0, "passed": [], "bad": []}
        for n, words in enumerate(frame_words):
            segs, hs = hdlc_segments(reading_bits(words, reading))
            st["flags"] += hs["flags"]
            st["aborts"] += hs["aborts"]
            st["longest_flag_run"] = max(st["longest_flag_run"], hs["longest_flag_run"])
            for seg in segs:
                if len(seg) < 17 * 8:
                    st["short"] += 1
                elif len(seg) % 8:
                    st["odd"] += 1
                else:
                    st["tried"] += 1
                    frame = bits_to_bytes(seg)
                    (st["passed"] if fcs_ok(frame) else st["bad"]).append((n, frame))
        out[reading] = st
    return out


def reading_name(r):
    return f"{'low' if r[0] == 'lo' else 'high'} byte first, {r[1].upper()} first, " + {
        "nrzi": "NRZI undone", "direct": "no NRZI (as data)", "direct-inverted": "no NRZI, inverted"}[r[2]]


def ax25_report(head, records):
    """Lines: frames decoded, or why none were."""
    fr = [f for f in frames(records) if f["data"]]
    out = ["AX.25 decode (Bell 202 / HDLC): " + (f"{len(fr)} captured frame(s), {sum(len(f['data']) for f in fr)} words, "
           f"{len(READINGS)} readings tried (word byte order x bit order x NRZI undone / not / inverted)" if fr else
           "no frames (no sync was seen, or no words followed one)")]
    if not fr:
        return out
    res = ax25_analyse([f["data"] for f in fr])
    tried = max(st["tried"] for st in res.values())
    passed = {}
    for reading in READINGS:
        for n, frame in res[reading]["passed"]:
            passed.setdefault((n, frame), reading)
    out.append(f"frames tried: {tried} (best reading); FCS passed: {len(passed)}")
    for (n, frame), reading in sorted(passed.items(), key=lambda kv: kv[0][0]):
        t = ((fr[n]["t0"] - head["t0"]) & 0xFFFFFFFF) / 10000
        out.append(f"  PASS capture frame {n + 1} at +{t:.2f} s ({reading_name(reading)}): {format_frame(frame)}")
    if passed:
        return out
    out += ax25_why(fr, res)
    return out


def ax25_why(fr, res):
    """Diagnostics for a capture in which no frame passed its FCS: what the bits look like, reading by reading."""
    raw = [words_to_bits(f["data"]) for f in fr]
    total = sum(len(r) for r in raw)
    ones = sum(sum(r) for r in raw)
    out = [f"why no frame passed: {total} raw bits delivered, {100.0 * ones / max(total, 1):.1f}% are 1 "
           f"(a flag preamble is 87.5% one level or the other; random noise is 50%)"]
    # A preamble reads as flags in several readings (the pattern is the same bit-reversed); the frame body tells them apart.
    best = max(READINGS, key=lambda r: (res[r]["longest_flag_run"], res[r]["tried"], res[r]["flags"]))
    out.append("readings (flags found / longest run of flags / frames tried / bad FCS / odd bit count / short / aborts):")
    for r in READINGS:
        st = res[r]
        mark = "  <- most flag-like" if r == best and st["flags"] else ""
        out.append(f"  {reading_name(r):52s} {st['flags']:4d} / {st['longest_flag_run']:3d} / {st['tried']:3d} / "
                   f"{len(st['bad']):3d} / {st['odd']:3d} / {st['short']:3d} / {st['aborts']:3d}{mark}")
    hist = run_histogram(raw)
    runs = sum(hist)
    out.append("run lengths of identical raw bits (clean NRZI/HDLC: 1 to 6 from data, 7 from flags, never more; noise: halves each step): "
               + "  ".join(f"{k}{'+' if k == len(hist) - 1 else ''}:{100.0 * hist[k] / max(runs, 1):.1f}%" for k in range(1, len(hist))))
    out.append(f"runs of 8 or more: {100.0 * sum(hist[8:]) / max(runs, 1):.1f}% of {runs} (noise: 0.8%; a clean stream: 0, "
               f"or a few where the clock slipped)")
    st = res[best]
    if st["flags"] == 0:
        out.append("no flag in any reading: the modem delivered no flag structure (noise, the wrong tone pair, or the "
                   "words are not bits of this signal)")
        return out
    out.append(f"best reading ({reading_name(best)}): {st['flags']} flags, longest run {st['longest_flag_run']}, "
               f"{st['tried']} candidate frames of which {len(st['bad'])} had a bad FCS")
    near = 0
    for n, frame in st["bad"][:8]:
        pa = plausible_address(frame)
        k = nearest_single_flip(frame) if pa >= 10 else None
        near += k is not None
        out.append(f"  frame {n + 1}: {len(frame)} bytes, {pa}/14 address bytes plausible"
                   + (f", one flipped bit (bit {k}) would make the FCS pass" if k is not None else
                      (", no single bit flip fixes it" if pa >= 10 else "")))
    if st["bad"] and not near:
        out.append("  (a bad FCS on a frame whose address field looks right means bit errors or a slipped clock; "
                   "one with a garbage address field means the bits are not this signal's)")
    return out


def write_csv(path, head, records):
    last = head["t0"]
    with open(path, "w") as f:
        f.write("time_100us,gap_100us,word,sync,done,rssi_dbm,pos,neg,frame,tail\n")
        for t, word, flags, rssi in records:
            f.write(f"{t},{(t - last) & 0xFFFFFFFF},{f'{word:04X}' if flags & FLAG_WORD else ''},"
                    f"{int(bool(flags & FLAG_SYNC))},{int(bool(flags & FLAG_DONE))},{rssi - 160},"
                    f"{int(bool(flags & FLAG_POS))},{int(bool(flags & FLAG_NEG))},{int(bool(flags & FLAG_FRAME))},"
                    f"{int(bool(flags & FLAG_TAIL))}\n")
            last = t


def wants_ax25(head, force):
    """APRS is 1200 baud: decode as AX.25 when asked to, for the APRS preset, and for any 1200 baud capture."""
    return force or head["baud"] == 1200 or head.get("extra", {}).get("kind") == 1


def report(blob, base, ax25=False):
    head, records = parse(blob)
    for line in summary(head, records):
        print(line)
    if wants_ax25(head, ax25):
        for line in ax25_report(head, records):
            print(line)
    write_csv(base + ".csv", head, records)
    print(f"records -> {base}.csv")


def download(port):
    here = os.path.dirname(os.path.abspath(__file__))
    source = open(os.path.join(here, "cps_tool.py"), encoding="utf-8").read()
    ns = {"__name__": "cps_tool", "__file__": os.path.join(here, "cps_tool.py")}
    exec(source[:source.index("def do_logo")], ns)
    READ_RAW, END = ns["READ_RAW"], ns["END"]
    cps = ns["Cps"](port)
    _send = cps.send

    def guarded_send(cmd, addr, data=b"", timeout=ns["FRAME_TIMEOUT"]):
        inside = CAP_ADDR <= addr * RAW_CHUNK and addr * RAW_CHUNK + RAW_CHUNK <= CAP_ADDR + CAP_BYTES
        if cmd not in (READ_RAW, END) or (cmd == READ_RAW and (data or not inside)):
            raise SystemExit(f"REFUSED to send command {cmd:#x} at {addr * RAW_CHUNK:#x}: this tool only reads the capture")
        return _send(cmd, addr, data, timeout)

    cps.send = guarded_send

    def page(index):
        out = bytearray()
        for a in range(CAP_ADDR + index * PAGE, CAP_ADDR + (index + 1) * PAGE, RAW_CHUNK):
            chunk = cps.send(READ_RAW, a // RAW_CHUNK)[2]
            if len(chunk) != RAW_CHUNK:
                raise SystemExit(f"SHORT READ at {a:#x}: got {len(chunk)} bytes")
            out += chunk
        return bytes(out)

    blob = bytearray(page(0))
    if bytes(blob[:8]) in MAGICS:
        for index in range(1, CAP_BYTES // PAGE):
            p = page(index)
            if p[6] == 0xFF:       # the first record of this page is erased: nothing was recorded from here on
                break
            blob += p
            if index % 16 == 0:
                print(f"\r  {index * PAGE // REC} records", end="", flush=True)
        print()
    cps.end()
    return bytes(blob)


def main(argv):
    ax25 = "--ax25" in argv
    argv = [a for a in argv if a != "--ax25"]
    if len(argv) == 2 and argv[0] == "--decode":
        with open(argv[1], "rb") as f:
            blob = f.read()
        try:
            report(blob, os.path.splitext(argv[1])[0], ax25)
        except ValueError as e:
            raise SystemExit(f"{argv[1]}: {e}")
        return
    if len(argv) != 2 or argv[0].startswith("--"):
        raise SystemExit(__doc__)
    port, out = argv
    if os.path.exists(out):
        raise SystemExit(f"REFUSING TO OVERWRITE {out}: choose a new file name")
    blob = download(port)
    if bytes(blob[:8]) not in MAGICS:
        print("done; radio restarting")
        raise SystemExit("NO CAPTURE on the radio: in FSK PROBE press # to record, # again to stop, then run this")
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    with open(out, "xb") as f:
        f.write(blob)
    print(f"read {len(blob)} bytes -> {out}")
    report(blob, os.path.splitext(out)[0], ax25)
    print("done; radio restarting")


if __name__ == "__main__":
    main(sys.argv[1:])
