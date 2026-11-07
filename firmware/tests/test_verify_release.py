"""verify_release.py on built artifacts: the size floors must actually fail when crossed. (scripts/dev.sh tools-test)"""
import os, struct, subprocess, sys, tempfile, unittest, zlib

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(os.path.dirname(HERE), "tools", "verify_release.py")
ABI = zlib.crc32(open(os.path.join(os.path.dirname(HERE), "firmware-source", "abi", "src", "lib.rs"), "rb").read()) & 0xFFFFFFFF


def image(n, sp=0x20004000, rst=0x080010D5):
    return (struct.pack("<II", sp, rst) + b"\x11" * (n - 8))


def app(image_bytes, bss, abi=ABI):
    img = b"\x07" * image_bytes
    hdr = struct.pack("<4sIB3x16s", b"OVL1", abi, 1, b"T".ljust(16, b"\0"))
    seg = struct.pack("<IIIII", 108, len(img), bss, 1, zlib.crc32(img) & 0xFFFFFFFF)
    return (hdr + seg).ljust(108, b"\0") + img


class Gate(unittest.TestCase):
    def run_gate(self, images=(), apps=()):
        d = tempfile.mkdtemp()
        args = [sys.executable, TOOL, "--no-committed", "--apps", d]
        for n, b in images:
            with open(os.path.join(d, n), "wb") as f:
                f.write(b)
            args += ["--image", os.path.join(d, n)]
        for n, b in apps:
            with open(os.path.join(d, n), "wb") as f:
                f.write(b)
        return subprocess.run(args, capture_output=True, text=True)

    def test_normal_artifacts_pass(self):
        p = self.run_gate([("fw.bin", image(61352))], [("a.app", app(4000, 8))])
        self.assertEqual(p.returncode, 0, p.stdout)

    def test_core_headroom_floor_fails_below_64_bytes(self):
        self.assertEqual(self.run_gate([("fw.bin", image(61440 - 64))], [("a.app", app(100, 4))]).returncode, 0)  # exactly at the floor
        p = self.run_gate([("fw.bin", image(61440 - 63))], [("a.app", app(100, 4))])
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("core headroom", p.stdout)

    def test_core_image_larger_than_the_area_fails(self):
        self.assertNotEqual(self.run_gate([("fw.bin", image(61441))], [("a.app", app(100, 4))]).returncode, 0)

    def test_bad_vectors_fail(self):
        self.assertNotEqual(self.run_gate([("fw.bin", image(2000, sp=0x10000000))], [("a.app", app(100, 4))]).returncode, 0)
        self.assertNotEqual(self.run_gate([("fw.bin", image(2000, rst=0x080010D4))], [("a.app", app(100, 4))]).returncode, 0)

    def test_arena_spare_floor_fails_below_64_bytes(self):
        self.assertEqual(self.run_gate([("fw.bin", image(2000))], [("a.app", app(8192 - 64 - 4, 4))]).returncode, 0)
        p = self.run_gate([("fw.bin", image(2000))], [("a.app", app(8192 - 63 - 4, 4))])
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("arena spare", p.stdout)

    def test_wrong_abi_hash_fails(self):
        self.assertNotEqual(self.run_gate([("fw.bin", image(2000))], [("a.app", app(100, 4, abi=ABI ^ 1))]).returncode, 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
