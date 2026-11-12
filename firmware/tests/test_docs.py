"""The documentation cannot rot silently. (scripts/dev.sh tools-test)

* every relative link and image in README.md, docs/*.md and channels/README.md resolves, and a link to a heading
  (`file.md#some-heading`) names a heading that exists;
* every image is a screenshot made from a simulator golden frame: it lives in docs/img, is named in the table in
  scripts/docs-screens.sh, that table's frame and scenario shot exist, and nothing else is in docs/img;
* every picture is used by a document and has alt text that says what it shows;
* the guide's launcher-slot table, channel-group table and settings reference agree with the sources they describe
  (firmware/tools/build_release.py, src/app/group_table.rs, apps/settings/common.rs), so a change there that forgets the
  guide fails here;
* README.md links every other document in docs/.
Standard library only.
"""
import glob, os, re, struct, unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
SIM = os.path.join(ROOT, "firmware", "source-changes", "simulator", "tests")
IMG_DIR = os.path.join(ROOT, "docs", "img")
SCRIPT = os.path.join(ROOT, "scripts", "docs-screens.sh")
GUIDE = os.path.join(ROOT, "docs", "USER-GUIDE.md")
SRC = os.path.join(ROOT, "firmware", "firmware-source")
MIN_ALT = 20     # characters: "SPECTRUM" is a name, not a description


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def doc_files():
    files = [os.path.join(ROOT, "README.md"), os.path.join(ROOT, "channels", "README.md")]
    files += sorted(glob.glob(os.path.join(ROOT, "docs", "*.md")))
    return files


def rel(path):
    return os.path.relpath(path, ROOT)


def strip_code(text):
    """Markdown with fenced blocks and inline code removed (an example link in code is not a link)."""
    text = re.sub(r"^(```|~~~).*?^\1[^\n]*$", "", text, flags=re.S | re.M)
    return re.sub(r"`[^`\n]*`", "", text)


LINK = re.compile(r"(!?)\[((?:[^\[\]]|\[[^\]]*\])*)\]\(\s*<?([^)\s>]+)>?(?:\s+\"[^\"]*\")?\s*\)")
HTML_IMG = re.compile(r"<img\b[^>]*\bsrc=[\"']([^\"']+)[\"']", re.I)


def links(path):
    """[(is_image, alt_or_text, target)] of one document."""
    text = strip_code(read(path))
    out = [(bool(m.group(1)), m.group(2), m.group(3)) for m in LINK.finditer(text)]
    out += [(True, "", m.group(1)) for m in HTML_IMG.finditer(text)]
    return out


def slug(heading, seen):
    """GitHub's anchor for a heading: lower case, punctuation dropped, spaces to hyphens, -1 -2 for repeats."""
    h = re.sub(r"[^\w\- ]", "", heading.strip().lower())
    h = h.replace(" ", "-")
    n = seen.get(h, 0)
    seen[h] = n + 1
    return h if n == 0 else f"{h}-{n}"


def anchors(path):
    seen, out = {}, set()
    in_fence = False
    for line in read(path).splitlines():
        if re.match(r"^(```|~~~)", line):
            in_fence = not in_fence
            continue
        m = None if in_fence else re.match(r"^#{1,6}\s+(.*?)\s*#*\s*$", line)
        if m:
            out.add(slug(m.group(1), seen))
    return out


def png_size(path):
    with open(path, "rb") as f:
        head = f.read(24)
    if head[:8] != b"\x89PNG\r\n\x1a\n" or head[12:16] != b"IHDR":
        raise ValueError(f"{rel(path)} is not a PNG")
    return struct.unpack(">II", head[16:24])


def frame_map():
    """[(scenario, shot, output_name)] from FRAMES=( ... ) in scripts/docs-screens.sh."""
    text = read(SCRIPT)
    body = text[text.index("FRAMES=("):]
    body = body[:body.index("\n)")]
    out = []
    for line in body.splitlines()[1:]:
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        m = re.fullmatch(r'"(\S+)/(\S+)\s+(\S+)"', line)
        assert m, f"cannot read this line of the FRAMES table: {line!r}"
        out.append(m.groups())
    return out


class Links(unittest.TestCase):
    def test_there_are_documents(self):
        self.assertGreaterEqual(len(doc_files()), 8)

    def test_every_relative_link_and_image_resolves(self):
        problems = []
        for doc in doc_files():
            own = anchors(doc)
            for is_image, _, target in links(doc):
                if re.match(r"^[a-z][a-z0-9+.-]*:", target, re.I):      # https:, mailto:
                    continue
                path, _, frag = target.partition("#")
                if not path:
                    if frag and frag not in own:
                        problems.append(f"{rel(doc)}: #{frag} is not a heading of this file")
                    continue
                dest = os.path.normpath(os.path.join(os.path.dirname(doc), path))
                if not os.path.exists(dest):
                    problems.append(f"{rel(doc)}: {target} does not exist")
                elif frag and dest.endswith(".md") and frag not in anchors(dest):
                    problems.append(f"{rel(doc)}: {target}: no such heading in {rel(dest)}")
        self.assertEqual(problems, [])

    def test_readme_links_every_document_in_docs(self):
        linked = set()
        for _, _, target in links(os.path.join(ROOT, "README.md")):
            linked.add(os.path.normpath(target.partition("#")[0]))
        missing = [rel(p) for p in sorted(glob.glob(os.path.join(ROOT, "docs", "*.md"))) if rel(p) not in linked]
        self.assertEqual(missing, [], "README.md is the map of the documents: link these")


class Images(unittest.TestCase):
    def test_every_picture_is_a_golden_frame_listed_in_the_screens_script(self):
        frames = frame_map()
        names = [n for _, _, n in frames]
        self.assertEqual(len(names), len(set(names)), "an output name appears twice in docs-screens.sh")
        for scenario, shot, name in frames:
            golden = os.path.join(SIM, "golden", scenario, shot + ".png")
            self.assertTrue(os.path.isfile(golden), f"docs-screens.sh names a golden frame that is not there: {scenario}/{shot}")
            self.assertEqual(png_size(golden), (160, 128), f"{scenario}/{shot} is not a 160x128 screen")
            script = os.path.join(SIM, "scenarios", scenario + ".txt")
            self.assertTrue(os.path.isfile(script), f"no scenario script for {scenario}")
            self.assertTrue(re.search(rf"^\s*(?:tx)?shot\s+{re.escape(shot)}\b", read(script), re.M),
                            f"scenario {scenario} takes no shot named {shot}")
            image = os.path.join(IMG_DIR, name + ".png")
            self.assertTrue(os.path.isfile(image), f"docs/img/{name}.png is missing: run scripts/docs-screens.sh")
            self.assertEqual(png_size(image), (480, 384), f"docs/img/{name}.png is not the 3x enlargement")

    def test_nothing_else_is_in_docs_img(self):
        mapped = {n + ".png" for _, _, n in frame_map()}
        present = set(os.listdir(IMG_DIR))
        self.assertEqual(sorted(present - mapped), [], "an image that is not made by docs-screens.sh (never draw one by hand)")

    def test_every_image_is_used_and_every_use_is_described(self):
        used = {}
        for doc in doc_files():
            for is_image, alt, target in links(doc):
                if not is_image or re.match(r"^[a-z][a-z0-9+.-]*:", target, re.I):
                    continue
                dest = os.path.normpath(os.path.join(os.path.dirname(doc), target))
                self.assertEqual(os.path.dirname(dest), IMG_DIR, f"{rel(doc)}: {target}: pictures come from docs/img only")
                self.assertGreaterEqual(len(alt.strip()), MIN_ALT, f"{rel(doc)}: {target}: alt text must say what the picture shows")
                used.setdefault(os.path.basename(dest), []).append(rel(doc))
        orphans = sorted(set(os.listdir(IMG_DIR)) - set(used))
        self.assertEqual(orphans, [], "images no document uses: use them or take them out of docs-screens.sh")


class GuideAgreesWithTheSource(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.guide = read(GUIDE)

    def test_launcher_slots(self):
        layout = re.search(r"LAYOUT\s*=\s*\[(.*?)\]", read(os.path.join(ROOT, "firmware", "tools", "build_release.py")), re.S).group(1)
        files = re.findall(r'"(\w+\.app)"', layout)
        names = {}
        for tool in ("provenance.py", "build_release.py"):       # build_release.py adds FSK PROBE to provenance's table
            names.update(re.findall(r'"(\w+\.app)":\s*\("([^"]+)"', read(os.path.join(ROOT, "firmware", "tools", tool))))
        want = [(i + 1, names[f]) for i, f in enumerate(files)]
        self.assertEqual(len(want), 9)
        have = [(int(n), name.strip()) for n, name in re.findall(r"^\|\s*(\d)\s*\|\s*([A-Z][A-Z /]*?)\s*\|\s*$", self.guide, re.M)]
        self.assertEqual(have, want, "the launcher table in the guide differs from LAYOUT in firmware/tools/build_release.py")
        extras = re.search(r"EXTRAS\s*=\s*\{(.*?)\}\n", read(os.path.join(ROOT, "firmware", "tools", "build_release.py")), re.S).group(1)
        for title in re.findall(r'\("([A-Z ]+)"', extras) + ["TEXT"]:
            self.assertIn(title, self.guide)

    def test_channel_groups(self):
        text = read(os.path.join(SRC, "src", "app", "group_table.rs"))
        modes = re.findall(r'ChanMode\s*\{\s*name:\s*"([^"]+)",\s*short:\s*"([^"]+)",\s*ranges:\s*([^}]+?)\s*\}', text)
        self.assertEqual(len(modes), 16)
        for i, (name, short, ranges) in enumerate(modes):
            row = next((l for l in self.guide.splitlines() if re.match(rf"^\|\s*{i}\s*\|\s*{re.escape(name)}\s*\|", l)), None)
            self.assertIsNotNone(row, f"the guide's group table has no row for group {i}, {name}")
            self.assertIn(f"| {short} |", row, f"group {i} {name}: short name {short}")
            if ranges != "ALL":
                for a, b in re.findall(r"\((\d+),\s*(\d+)\)", ranges):
                    self.assertTrue(a in row and b in row, f"group {i} {name}: slots {a}-{b} missing from {row!r}")

    def test_every_setting_is_in_the_settings_reference(self):
        common = read(os.path.join(SRC, "apps", "settings", "common.rs"))
        body = common[common.index("fn label(self) -> &'static str {\n        use SettingItem::*;"):]
        body = body[:body.index("fn is_scalar")]
        labels = re.findall(r'=>\s*"([A-Z][A-Z0-9 /+\-]+)",', body)
        labels = [l for l in labels if l != "BAND SHORT"]       # has a label but is not offered: BAND's short press is fixed
        self.assertGreaterEqual(len(labels), 45)
        ref = self.guide[self.guide.index("## Settings reference"):]
        for label in labels:
            self.assertTrue(re.search(rf"^\|\s*`?{re.escape(label)}`?\s*\|", ref, re.M), f"{label} has no row in the settings reference")
        for group in re.findall(r'SettingsGroup::\w+\s*=>\s*"([A-Z/ ]+)"', common):
            self.assertIn(group, ref, group)


if __name__ == "__main__":
    unittest.main()
