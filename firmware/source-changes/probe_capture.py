"""Download and decode what FSK PROBE recorded (its `#` key) from the radio's flash. Read-only.

  probe_capture.py PORT OUT.bin      download the capture, write OUT.bin and OUT.csv, print a summary
  probe_capture.py --decode IN.bin   decode a file downloaded earlier (no radio), write IN.csv

The capture lives at 0x1B0000-0x1D0000 in the radio's data flash (apps/fskprobe/src/logic.rs):
  page 0 (256 bytes)  "FSKCAP1\\0", frequency Hz u32, baud u16, sync halves 2 x u16, baud register u16,
                      start time u32 (100 us units), record size u8
  then records        8 bytes each, little-endian: time u32 (100 us units), FIFO word u16, flags u8, RSSI raw u8
                      flags: 1 = sync seen, 2 = word read, 4 = packet done; 0xFF = erased, the end of the capture
At most 16,352 records. OUT.bin is the header page and the record pages that hold data, exactly as read.
Only the handshake, raw-read ('D') and END frames can leave this script, and only for addresses inside that range.
Radio on, at its main frequency screen (not inside an app). An existing OUT.bin is never overwritten.
"""
import os, struct, sys

CAP_ADDR, CAP_BYTES, PAGE, REC = 0x1B0000, 0x20000, 256, 8
MAGIC = b"FSKCAP1\0"
MAX_RECORDS = (CAP_BYTES - PAGE) // REC
RAW_CHUNK = 64
FLAG_SYNC, FLAG_WORD, FLAG_DONE = 1, 2, 4


def parse(blob):
    """(header dict, [(time, word, flags, rssi_raw)]) from a downloaded capture; raises ValueError if it is not one."""
    if len(blob) < PAGE or blob[:8] != MAGIC:
        raise ValueError("not an FSK PROBE capture (no FSKCAP1 header)")
    freq, baud, sync1, sync2, baudreg, t0, rec = struct.unpack_from("<IHHHHIB", blob, 8)
    if rec != REC:
        raise ValueError(f"record size {rec} is not the {REC} this tool reads")
    head = {"freq_hz": freq, "baud": baud, "sync": (sync1, sync2), "baud_reg": baudreg, "t0": t0}
    records = []
    for off in range(PAGE, min(len(blob), CAP_BYTES) - REC + 1, REC):
        t, word, flags, rssi = struct.unpack_from("<IHBB", blob, off)
        if flags == 0xFF:
            break
        records.append((t, word, flags, rssi))
    return head, records


def summary(head, records):
    out = [f"frequency {head['freq_hz'] / 1e6:.6f} MHz, {head['baud']} baud (register {head['baud_reg']:#06x}), "
           f"sync {head['sync'][0]:04X} {head['sync'][1]:04X}"]
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
                   f"most words taken in one tick: {max(times.values())} (8 means the FIFO may have overflowed)")
        if span:
            out.append(f"average {len(words) * 16 * 10000 / span:.0f} bits/s delivered, against {head['baud']} on the air")
    rssi = [r[3] for r in records]
    out.append(f"RSSI {min(rssi) - 160} to {max(rssi) - 160} dBm")
    out.append("first words: " + " ".join(f"{r[1]:04X}" for r in words[:16]))
    return out


def write_csv(path, head, records):
    last = head["t0"]
    with open(path, "w") as f:
        f.write("time_100us,gap_100us,word,sync,done,rssi_dbm\n")
        for t, word, flags, rssi in records:
            f.write(f"{t},{(t - last) & 0xFFFFFFFF},{f'{word:04X}' if flags & FLAG_WORD else ''},"
                    f"{int(bool(flags & FLAG_SYNC))},{int(bool(flags & FLAG_DONE))},{rssi - 160}\n")
            last = t


def report(blob, base):
    head, records = parse(blob)
    for line in summary(head, records):
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
    if blob[:8] == MAGIC:
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
    if len(argv) == 2 and argv[0] == "--decode":
        with open(argv[1], "rb") as f:
            blob = f.read()
        try:
            report(blob, os.path.splitext(argv[1])[0])
        except ValueError as e:
            raise SystemExit(f"{argv[1]}: {e}")
        return
    if len(argv) != 2 or argv[0].startswith("--"):
        raise SystemExit(__doc__)
    port, out = argv
    if os.path.exists(out):
        raise SystemExit(f"REFUSING TO OVERWRITE {out}: choose a new file name")
    blob = download(port)
    if blob[:8] != MAGIC:
        print("done; radio restarting")
        raise SystemExit("NO CAPTURE on the radio: in FSK PROBE press # to record, # again to stop, then run this")
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    with open(out, "xb") as f:
        f.write(blob)
    print(f"read {len(blob)} bytes -> {out}")
    report(blob, os.path.splitext(out)[0])
    print("done; radio restarting")


if __name__ == "__main__":
    main(sys.argv[1:])
