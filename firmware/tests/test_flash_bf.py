"""flash_bf.py against a fake KDH bootloader: what is refused, what is sent, what survives interruption.

  scripts/dev.sh tools-test        (or: python3 firmware/tests/test_flash_bf.py)

UNVERIFIED against a real bootloader: the fake reproduces what flash_bf.py expects.
The .BF files are made here with an independent implementation of the documented wrap
(1 KiB packages; the first two and last two plain; the rest XOR KDHT/RBGI by index % 3).
"""
import hashlib, json, os, shutil, struct, subprocess, sys, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
PKG = 1024


def wb(p, d):
    with open(p, "wb") as f:
        f.write(d)


def rb(p):
    with open(p, "rb") as f:
        return f.read()


def xor(block, key):
    out = bytearray()
    for i, x in enumerate(block):
        k = key[i % 4]
        out.append(x if x in (0, 0xFF, k, k ^ 0xFF) else x ^ k)
    return bytes(out)


def crypt(data):
    n = -(-len(data) // PKG)
    out = bytearray()
    for i in range(n):
        blk = data[i * PKG:(i + 1) * PKG]
        if 2 <= i < n - 2 and i % 3 == 1:
            blk = xor(blk, b"KDHT")
        elif 2 <= i < n - 2 and i % 3 == 2:
            blk = xor(blk, b"RBGI")
        out += blk
    return bytes(out)


def image(n=9 * PKG + 300):
    body = bytes((i * 7 + 3) & 0xFF for i in range(n))
    return struct.pack("<II", 0x20004000, 0x080010D5) + body[8:]


def wrap(img, regions=1, l2=0):
    enc = crypt(img)
    return struct.pack(">BLL", regions, len(enc), l2) + b"\x00" * 7 + enc + b"\x00" * l2


class Box(unittest.TestCase):
    def setUp(self):
        self.box = tempfile.mkdtemp(prefix="flashbox-")
        self.bl = os.path.join(self.box, "bl")
        os.mkdir(self.bl)
        os.makedirs(os.path.join(self.box, "source-changes"))
        shutil.copy(os.path.join(FW, "source-changes", "flash_bf.py"), os.path.join(self.box, "source-changes"))
        shutil.copy(os.path.join(FW, "variants.json"), self.box)
        shutil.copy(os.path.join(HERE, "fakebootloader", "serial.py"), os.path.join(self.box, "source-changes"))
        self.img = image()
        self.bf = os.path.join(self.box, "fw.BF")
        wb(self.bf, wrap(self.img))
        self.sha = hashlib.sha256(rb(self.bf)).hexdigest()

    def tearDown(self):
        shutil.rmtree(self.box, ignore_errors=True)

    def flash(self, *extra, bf=None, sha=None, board="5RHNRFB-YP07", variant="default", gate=True):
        bf = bf or self.bf
        args = [bf, "fakeport", sha or self.sha[:16]]
        if gate:
            args += ["--board", board, "--variant", variant]
        env = dict(os.environ, FAKE_BL_DIR=self.bl)
        return subprocess.run([sys.executable, os.path.join(self.box, "source-changes", "flash_bf.py"), *args, *extra],
                              cwd=self.box, env=env, capture_output=True, text=True, timeout=300)

    def state(self):
        p = os.path.join(self.bl, "state.json")
        return json.loads(rb(p)) if os.path.exists(p) else None

    def flash_bytes(self):
        p = os.path.join(self.bl, "flash.bin")
        return rb(p) if os.path.exists(p) else b""

    def untouched(self):
        self.assertIsNone(self.state(), "the serial port was opened")

    def fault(self, **f):
        with open(os.path.join(self.bl, "faults.json"), "w") as fh:
            json.dump(f, fh)


class Refusals(Box):
    def test_wrong_hash_is_refused_before_opening_the_port(self):
        p = self.flash(sha="0" * 16)
        self.assertNotEqual(p.returncode, 0)
        self.untouched()

    def test_bad_vector_tables_are_refused(self):
        bad = bytearray(self.img)
        bad[3] = 0x10                                    # initial SP not in SRAM
        for i, mod in enumerate((bad, bytearray(self.img[:4] + b"\x00\x00\x00\x00" + self.img[8:]))):
            f = os.path.join(self.box, f"bad{i}.BF")
            wb(f, wrap(bytes(mod)))
            p = self.flash(bf=f, sha=hashlib.sha256(rb(f)).hexdigest()[:16])
            self.assertNotEqual(p.returncode, 0, i)
        self.untouched()

    def test_two_region_files_are_refused(self):
        f = os.path.join(self.box, "two.BF")
        wb(f, wrap(self.img, regions=2, l2=16))
        p = self.flash(bf=f, sha=hashlib.sha256(rb(f)).hexdigest()[:16])
        self.assertNotEqual(p.returncode, 0)
        self.untouched()

    def test_board_gate_requires_the_flags(self):
        p = self.flash(gate=False)
        self.assertNotEqual(p.returncode, 0)
        self.untouched()

    def test_board_gate_rejects_wrong_board_for_variant(self):
        for board, variant in (("5RHBK", "default"), ("5RHNRFB-YP07", "bk4819"), ("5RH", "default"),
                               ("5rhnrfb-yp07", "default"), ("", "default")):
            p = self.flash(board=board, variant=variant)
            self.assertNotEqual(p.returncode, 0, (board, variant))
            self.assertTrue("BOARD GATE" in p.stderr + p.stdout or "usage" in (p.stderr + p.stdout).lower()
                            or "flash_bf.py FILE" in p.stderr + p.stdout, (board, variant, p.stderr))
        self.untouched()

    def test_unknown_variant_is_refused(self):
        self.assertNotEqual(self.flash(variant="nope").returncode, 0)
        self.untouched()

    def test_check_only_never_opens_the_port(self):
        p = self.flash("--check-only")
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertIn("serial port not opened", p.stdout)
        self.untouched()

    def test_check_only_still_enforces_the_gate(self):
        p = self.flash("--check-only", board="5RHBK")
        self.assertNotEqual(p.returncode, 0)


class Transfer(Box):
    def test_matching_board_and_variant_flashes_the_exact_image(self):
        for board, variant in (("5RHNRFB-YP07", "default"), ("5RHBK", "bk4819")):
            shutil.rmtree(self.bl); os.mkdir(self.bl)
            p = self.flash(board=board, variant=variant)
            self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
            self.assertIn("FLASH COMPLETE", p.stdout)
            self.assertTrue(self.state()["ended"])
            enc = rb(self.bf)[16:]
            self.assertEqual(self.flash_bytes()[:len(enc)], enc)                 # what the bootloader holds
            self.assertEqual(self.flash_bytes()[len(enc):], b"\x00" * (len(self.flash_bytes()) - len(enc)))
            self.assertEqual(crypt(self.flash_bytes()[:len(enc)]), self.img)     # and it decrypts to the image

    def test_interrupted_transfer_leaves_a_partial_flash_and_a_rerun_completes(self):
        self.fault(die_after_packages=3)
        p = self.flash()
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("FLASH FAILED", p.stdout)
        st = self.state()
        self.assertEqual(st["got"], 3)
        self.assertFalse(st["ended"])                                           # END never acknowledged
        os.remove(os.path.join(self.bl, "faults.json"))                         # radio back in flash mode
        p = self.flash()
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        self.assertTrue(self.state()["ended"])
        self.assertEqual(crypt(self.flash_bytes()[:len(rb(self.bf)) - 16]), self.img)

    def test_flash_write_failure_stops_without_end(self):
        self.fault(flash_fail_at=4)
        p = self.flash()
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("flash write failed", p.stdout)
        self.assertFalse(self.state()["ended"])
        self.assertEqual(self.state()["got"], 4)

    # Unknown about the real bootloader (docs/TEST-DOUBLES.md): what a repeated package gets. flash_bf.py repeats the same
    # frame when an ACK does not arrive in 2 s, so a lost ACK is survivable only if the bootloader acknowledges the repeat.
    def test_a_lost_ack_is_survived_if_the_bootloader_acknowledges_a_repeat(self):
        self.fault(lose_ack_at=2, duplicate_package="ack")
        p = self.flash()
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        self.assertEqual(crypt(self.flash_bytes()[:len(rb(self.bf)) - 16]), self.img)

    def test_a_lost_ack_stops_the_flash_if_the_bootloader_answers_a_repeat_with_a_sequence_error(self):
        self.fault(lose_ack_at=2, duplicate_package="e2")
        p = self.flash()
        self.assertNotEqual(p.returncode, 0)                    # the host gives up after five sequence errors ...
        self.assertIn("sequence or length", p.stdout)
        self.assertEqual(self.state()["got"], 3)                # ... with the package it could not confirm in fact stored
        os.remove(os.path.join(self.bl, "faults.json"))
        p = self.flash()                                        # and a rerun (the handshake restarts the transfer) completes
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)

    def test_end_that_is_never_answered_still_counts_as_flashed(self):
        self.fault(no_end_reply=True)                           # the radio may reboot before it answers END
        p = self.flash()
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        self.assertIn("radio likely rebooted", p.stdout)

    def test_silent_bootloader_fails_at_handshake(self):
        self.fault(silent=True)
        p = self.flash()
        self.assertNotEqual(p.returncode, 0)
        self.assertEqual(self.state()["got"], 0)


class WrapTool(unittest.TestCase):
    """wrap_bf.py (what `scripts/dev.sh bf` runs) must agree with the independent wrap above."""

    def test_wrap_bf_matches_independent_wrap_and_passes_check_only(self):
        box = tempfile.mkdtemp(prefix="wrapbox-")
        self.addCleanup(shutil.rmtree, box, ignore_errors=True)
        img, src, out = image(), os.path.join(box, "fw.bin"), os.path.join(box, "fw.BF")
        wb(src, img)
        r = subprocess.run([sys.executable, os.path.join(FW, "source-changes", "wrap_bf.py"), src, out],
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("round trip only", r.stdout)
        self.assertEqual(rb(out), wrap(img))
        r = subprocess.run([sys.executable, os.path.join(FW, "source-changes", "flash_bf.py"), out, "/dev/null",
                            hashlib.sha256(rb(out)).hexdigest()[:16], "--board", "5RHNRFB-YP07", "--variant", "default",
                            "--check-only"], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("serial port not opened", r.stdout)


if __name__ == "__main__":
    unittest.main(verbosity=2)
