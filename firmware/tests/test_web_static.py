"""Static guards on web/ (phase 1: planning + fake radio only). No browser needed; the real
browser test is waiting on approval to add Node/Playwright. (scripts/dev.sh tools-test)"""
import os, re, unittest

WEB = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), "web")


def read(n):
    with open(os.path.join(WEB, n)) as f:
        return f.read()


class WebStatic(unittest.TestCase):
    def test_never_touches_a_real_serial_port_or_the_network_outside_its_own_files(self):
        for n in ("builder.js", "index.html"):
            s = read(n)
            self.assertNotIn("navigator.serial", s, n)
            self.assertNotIn("requestPort", s, n)
            self.assertNotIn("WebSocket", s, n)
            self.assertIsNone(re.search(r"https?://", s), f"{n} references an external URL")

    def test_page_says_it_is_planning_only_and_unverified(self):
        s = read("index.html")
        self.assertIn("cannot flash a real radio", s)
        self.assertIn("UNVERIFIED", s)

    def test_page_loads_the_script_and_the_ids_the_script_uses(self):
        html, js = read("index.html"), read("builder.js")
        self.assertIn('src="builder.js"', html)
        for i in set(re.findall(r"\$\(\"(\w+)\"\)", js)):
            self.assertIn(f'id="{i}"', html + js, i)

    def test_write_button_starts_disabled_and_needs_a_board_string(self):
        self.assertIn('id="write" disabled', read("index.html"))
        self.assertIn("selection().board", read("builder.js"))

    def test_rules_in_the_page_match_the_release_gate(self):
        js = read("builder.js")
        for key in ("min_core_headroom", "min_arena_spare", "slot_bytes", "arena_bytes", "abi_hash", "BOARD GATE"):
            self.assertIn(key, js, key)


if __name__ == "__main__":
    unittest.main(verbosity=2)
