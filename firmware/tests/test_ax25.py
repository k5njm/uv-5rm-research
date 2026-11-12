"""Unit tests for the AX.25 / APRS (Bell 202) decoder in probe_capture.py.

  scripts/dev.sh tools-test        (or: python3 firmware/tests/test_ax25.py)

The vectors are built here from the AX.25 and HDLC rules (callsigns shifted left one bit, a UI frame, CRC-16/X.25, bit
stuffing after five 1s, flags, NRZI) and then packed into FIFO words the way a capture would hold them, in every word byte
order and bit order and in both polarities, so the decoder has to find the right reading by itself. Made-up callsigns only.
NOTHING HERE IS A RECORDING from the radio; the real modem's output is exactly what is unknown.
"""
import importlib.util, os, random, re, struct, sys, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TOOLS = os.environ.get("TOOLS_DIR", os.path.join(os.path.dirname(HERE), "source-changes"))
_spec = importlib.util.spec_from_file_location("probe_capture", os.path.join(TOOLS, "probe_capture.py"))
pc = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pc)


def address(call, ssid=0, last=False, repeated=False):
    name = call.ljust(6).encode("ascii")
    return bytes(c << 1 for c in name) + bytes([0x60 | (0x80 if repeated else 0) | ssid << 1 | int(last)])


def ui_frame(dest, src, path, payload, ssid_src=0):
    """An AX.25 UI frame with its FCS. `path` is a list of (call, ssid, repeated)."""
    out = address(dest) + address(src, ssid_src, last=not path)
    for i, (call, ssid, rep) in enumerate(path):
        out += address(call, ssid, last=i == len(path) - 1, repeated=rep)
    out += bytes([0x03, 0xF0]) + payload
    return out + pc.fcs_bytes(out)


def lsb_bits(data):
    return [(b >> k) & 1 for b in data for k in range(8)]


def stuff(bits):
    out, ones = [], 0
    for b in bits:
        out.append(b)
        ones = ones + 1 if b else 0
        if ones == 5:
            out.append(0)
            ones = 0
    return out


FLAG_BITS = lsb_bits(bytes([0x7E]))


def on_air_bits(frame, pre=40, post=2):
    return FLAG_BITS * pre + stuff(lsb_bits(frame)) + FLAG_BITS * post


def pack(bits, byte_first="lo", bit_first="msb"):
    """Time-ordered bits to FIFO words: the inverse of probe_capture.words_to_bits."""
    bits = list(bits) + [0] * (-len(bits) % 16)
    words = []
    for i in range(0, len(bits), 16):
        pair = []
        for j in (0, 8):
            chunk = bits[i + j:i + j + 8]
            pair.append(sum(b << (7 - k if bit_first == "msb" else k) for k, b in enumerate(chunk)))
        words.append(pair[0] | pair[1] << 8 if byte_first == "lo" else pair[1] | pair[0] << 8)
    return words


def capture(frame_bits, order=("lo", "msb"), level=0, drop=0, invert=False, nrzi=True, noise_words=0, seed=1):
    """FIFO words as the chip might deliver them: tone levels (NRZI), the first `drop` bits consumed by the sync match."""
    raw = pc.nrzi_encode(frame_bits, level) if nrzi else list(frame_bits)
    raw = raw[drop:]
    if invert:
        raw = [b ^ 1 for b in raw]
    rng = random.Random(seed)
    raw += [rng.getrandbits(1) for _ in range(16 * noise_words)]
    return pack(raw, *order)


def records_for(words, t0=1000, gap=133, sync_flags=1 | 8):
    """Capture records for one frame: a sync record, then one record per word, the last flagged as ended by count."""
    recs = [(t0 + 10, 0, sync_flags, 80)]
    for i, w in enumerate(words):
        recs.append((t0 + 10 + 133 * (i + 1), w, 2 | (32 if i == len(words) - 1 else 0), 80))
    return recs


HEAD = {"t0": 1000, "baud": 1200, "freq_hz": 144_390_000, "version": 3}
MESSAGE = ui_frame("APRS", "N0CALL", [("WIDE1", 1, True), ("WIDE2", 1, False)], b"!3200.00N/09700.00W-test packet", ssid_src=9)
MONITOR = "N0CALL-9>APRS,WIDE1-1*,WIDE2-1:!3200.00N/09700.00W-test packet"


class Basics(unittest.TestCase):
    def test_crc_matches_the_published_check_value(self):
        self.assertEqual(pc.crc_x25_register(b"123456789") ^ 0xFFFF, 0x906E)     # CRC-16/X.25 check value
        self.assertTrue(pc.fcs_ok(MESSAGE))
        self.assertFalse(pc.fcs_ok(MESSAGE[:-1] + bytes([MESSAGE[-1] ^ 1])))

    def test_monitor_format(self):
        self.assertEqual(pc.format_frame(MESSAGE), MONITOR)
        plain = ui_frame("APRS", "N0CALL", [], b"hi")
        self.assertEqual(pc.format_frame(plain), "N0CALL>APRS:hi")
        self.assertIn("<0D>", pc.format_frame(ui_frame("APRS", "N0CALL", [], b"a\rb")))

    def test_flags_nrzi_encode_to_the_sync_presets(self):
        # The derivation in apps/fskprobe/src/logic.rs: repeated flags from either starting level.
        for level, want_msb in ((0, 0xFE), (1, 0x01)):
            raw = pc.nrzi_encode(FLAG_BITS * 6, level)
            self.assertEqual(raw[:8], raw[8:16])
            self.assertEqual(raw[:8], [1, 1, 1, 1, 1, 1, 1, 0] if level == 0 else [0, 0, 0, 0, 0, 0, 0, 1])
            self.assertEqual(pack(raw[:16], "lo", "msb")[0], want_msb << 8 | want_msb)
            self.assertEqual(pack(raw[:16], "lo", "lsb")[0], {0xFE: 0x7F7F, 0x01: 0x8080}[want_msb])

    def test_pack_and_unpack_are_inverses(self):
        rng = random.Random(7)
        bits = [rng.getrandbits(1) for _ in range(160)]
        for order in pc.ORDERS:
            self.assertEqual(pc.words_to_bits(pack(bits, *order), *order), bits)

    def test_stuffing_and_flags(self):
        frame = ui_frame("APRS", "N0CALL", [], bytes([0x7E, 0xFF, 0x1F, 0x00, 0x7E]))   # runs of 1s that need stuffing
        bits = on_air_bits(frame, pre=3, post=2)
        segs, st = pc.hdlc_segments(bits)
        self.assertEqual(len(segs), 1)
        self.assertEqual(pc.bits_to_bytes(segs[0]), frame)
        self.assertEqual((st["flags"], st["aborts"]), (5, 0))
        self.assertEqual(st["longest_flag_run"], 3)
        # seven 1s in a row is an abort, and the frame it interrupts is dropped
        bad = FLAG_BITS * 2 + stuff(lsb_bits(frame))[:80] + [1] * 8 + FLAG_BITS * 2
        segs, st = pc.hdlc_segments(bad)
        self.assertEqual((segs, st["aborts"]), ([], 1))


class Readings(unittest.TestCase):
    def decode(self, words):
        res = pc.ax25_analyse([words])
        return {r: st for r, st in res.items() if st["passed"]}

    def test_every_word_order_bit_order_and_polarity_decodes(self):
        bits = on_air_bits(MESSAGE)
        for order in pc.ORDERS:
            for invert in (False, True):
                for level in (0, 1):
                    for drop in (0, 5, 29):
                        words = capture(bits, order, level=level, drop=drop, invert=invert, noise_words=6)
                        got = self.decode(words)
                        want = {(order[0], order[1], "nrzi")}
                        self.assertTrue(want <= set(got), f"{order} invert={invert} level={level} drop={drop}: {sorted(got)}")
                        for r, st in got.items():
                            self.assertEqual([f for _, f in st["passed"]], [MESSAGE])
                            self.assertEqual(pc.format_frame(st["passed"][0][1]), MONITOR)

    def test_a_demodulator_that_already_undid_nrzi_is_found_too(self):
        bits = on_air_bits(MESSAGE)
        for order in pc.ORDERS:
            self.assertIn((order[0], order[1], "direct"), self.decode(capture(bits, order, nrzi=False, noise_words=3)))
            self.assertIn((order[0], order[1], "direct-inverted"),
                          self.decode(capture(bits, order, nrzi=False, invert=True, noise_words=3)))

    def test_two_frames_in_one_capture_frame(self):
        other = ui_frame("APRS", "N0CALL", [], b">second")
        bits = FLAG_BITS * 30 + stuff(lsb_bits(MESSAGE)) + FLAG_BITS * 3 + stuff(lsb_bits(other)) + FLAG_BITS
        got = self.decode(capture(bits))
        self.assertEqual([f for _, f in got[("lo", "msb", "nrzi")]["passed"]], [MESSAGE, other])

    def test_noise_passes_nothing(self):
        rng = random.Random(3)
        for _ in range(20):
            words = [rng.getrandbits(16) for _ in range(128)]
            self.assertEqual(self.decode(words), {})

    def test_truncated_frame_passes_nothing(self):
        bits = FLAG_BITS * 30 + stuff(lsb_bits(MESSAGE))[:300]
        self.assertEqual(self.decode(capture(bits, noise_words=40)), {})

    def test_a_flipped_bit_is_a_bad_fcs_and_the_flip_is_found(self):
        bad = bytearray(MESSAGE)
        bad[20] ^= 0x04
        bits = on_air_bits(bytes(bad))
        words = capture(bits, noise_words=2)
        res = pc.ax25_analyse([words])[("lo", "msb", "nrzi")]
        self.assertEqual(res["passed"], [])
        self.assertEqual([f for _, f in res["bad"]], [bytes(bad)])
        self.assertEqual(pc.nearest_single_flip(bytes(bad)), 20 * 8 + 2)
        self.assertEqual(pc.plausible_address(bytes(bad)), 14)
        self.assertIsNone(pc.nearest_single_flip(bytes(bad[:30]) + b"\x00" * 3))

    def test_a_frame_with_a_dropped_bit_is_an_odd_length(self):
        stuffed = stuff(lsb_bits(MESSAGE))
        bits = FLAG_BITS * 30 + stuffed[:100] + stuffed[101:] + FLAG_BITS * 2
        res = pc.ax25_analyse([capture(bits)])[("lo", "msb", "nrzi")]
        self.assertEqual((res["passed"], res["odd"]), ([], 1))


class Report(unittest.TestCase):
    def test_pass_report_gives_time_reading_and_monitor_line(self):
        words = capture(on_air_bits(MESSAGE), ("hi", "lsb"), invert=True, drop=11, noise_words=5)
        lines = pc.ax25_report(HEAD, records_for(words, t0=21000))
        text = "\n".join(lines)
        self.assertIn("frames tried: 1 (best reading); FCS passed: 1", text)
        self.assertIn("PASS capture frame 1 at +2.00 s (high byte first, LSB first, NRZI undone): " + MONITOR, text)
        self.assertNotIn("why no frame passed", text)

    def test_noise_report_says_why(self):
        rng = random.Random(5)
        words = [rng.getrandbits(16) for _ in range(160)]
        text = "\n".join(pc.ax25_report(HEAD, records_for(words)))
        self.assertIn("FCS passed: 0", text)
        m = re.search(r"2560 raw bits delivered, (\d+\.\d)% are 1", text)
        self.assertTrue(m and 45 < float(m.group(1)) < 55, text)
        self.assertIn("run lengths of identical raw bits", text)
        self.assertIn("runs of 8 or more:", text)
        self.assertIn("readings (flags found", text)

    def test_flags_without_a_frame_are_seen_as_flags(self):
        words = capture(FLAG_BITS * 80)
        text = "\n".join(pc.ax25_report(HEAD, records_for(words)))
        self.assertIn("FCS passed: 0", text)
        self.assertIn("<- most flag-like", text)
        m = re.search(r"longest run (\d+)", text)
        self.assertTrue(m and int(m.group(1)) >= 70, text)
        self.assertIn("0 candidate frames of which 0 had a bad FCS", text)

    def test_clean_stream_has_no_long_runs(self):
        raw = [pc.words_to_bits(capture(on_air_bits(MESSAGE), noise_words=0))]
        hist = pc.run_histogram(raw)
        self.assertEqual(sum(hist[8:]), 0, hist)
        self.assertGreater(hist[7], 30, "the preamble's flags are runs of seven")
        self.assertEqual(hist[0], 0)

    def test_no_frames_is_said_plainly(self):
        lines = pc.ax25_report(HEAD, [(1010, 0, 1, 80)])
        self.assertIn("no frames", lines[0])

    def test_one_bad_frame_report_names_the_flip(self):
        bad = bytearray(MESSAGE)
        bad[30] ^= 0x10
        words = capture(on_air_bits(bytes(bad)), noise_words=2)
        text = "\n".join(pc.ax25_report(HEAD, records_for(words)))
        self.assertIn("one flipped bit (bit 244) would make the FCS pass", text)
        self.assertIn("14/14 address bytes plausible", text)


if __name__ == "__main__":
    unittest.main()
