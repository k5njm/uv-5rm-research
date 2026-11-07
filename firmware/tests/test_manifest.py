"""make_manifest.py: contents come from the files, bad inputs are refused. (scripts/dev.sh tools-test)"""
import json, os, struct, subprocess, sys, tempfile, unittest, zlib

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(os.path.dirname(HERE), "tools", "make_manifest.py")
ABI = zlib.crc32(open(os.path.join(os.path.dirname(HERE), "firmware-source", "abi", "src", "lib.rs"), "rb").read()) & 0xFFFFFFFF


def app(name, image=b"\x01\x02\x03\x04" * 50, bss=8, abi=ABI, crc_bad=False):
    hdr = struct.pack("<4sIB3x16s", b"OVL1", abi, 1, name.encode().ljust(16, b"\0"))
    crc = zlib.crc32(image) & 0xFFFFFFFF ^ (1 if crc_bad else 0)
    seg = struct.pack("<IIIII", 108, len(image), bss, 1, crc)
    return (hdr + seg + b"\0" * 60).ljust(108, b"\0") + image


class Manifest(unittest.TestCase):
    def build(self, files):
        d = tempfile.mkdtemp()
        for n, b in files.items():
            with open(os.path.join(d, n), "wb") as f:
                f.write(b)
        out = os.path.join(d, "m.json")
        p = subprocess.run([sys.executable, TOOL, d, out], capture_output=True, text=True)
        return p, out

    def test_reads_everything_from_the_files(self):
        fw = struct.pack("<II", 0x20004000, 0x080010D5) + b"\xAA" * 1000
        p, out = self.build({"fw-default.bin": fw, "fw-bk4819.bin": fw, "demo-classic.app": app("DEMO"),
                             "demo-d74.app": app("DEMO", bss=16)})
        self.assertEqual(p.returncode, 0, p.stderr)
        m = json.load(open(out))
        self.assertEqual(m["abi_hash"], f"{ABI:08x}")
        self.assertEqual({c["id"] for c in m["core"]}, {"default", "bk4819"})
        self.assertEqual(m["core"][0]["headroom_bytes"], 61440 - len(fw))
        self.assertTrue(all(c["vectors_ok"] for c in m["core"]))
        self.assertIn("5RHBK", next(c for c in m["core"] if c["id"] == "bk4819")["boards"])
        demo = m["apps"][0]
        self.assertEqual(set(demo["styles"]), {"classic", "d74"})
        self.assertEqual(demo["styles"]["d74"]["segments"][0]["arena_use"], 200 + 16)

    def test_corrupt_segment_crc_is_refused(self):
        p, _ = self.build({"demo-classic.app": app("DEMO", crc_bad=True)})
        self.assertNotEqual(p.returncode, 0)

    def test_unknown_variant_is_refused(self):
        p, _ = self.build({"fw-mystery.bin": b"\0" * 64})
        self.assertNotEqual(p.returncode, 0)

    def test_non_overlay_file_is_refused(self):
        p, _ = self.build({"demo-classic.app": b"nope" * 100})
        self.assertNotEqual(p.returncode, 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
