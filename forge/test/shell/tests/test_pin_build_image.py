import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import pin_build_image  # noqa: E402

GOOD = "sha256:" + "ab12" * 16


class PinBuildImage(unittest.TestCase):
    def run_pin(self, text):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "in").write_text(text)
            pinfile = d / "pin"
            pin_build_image.pin(d / "in", d / "out", pinfile)
            return pinfile.read_text(), (d / "out/title").read_text(), (d / "out/body.md").read_text()

    def test_writes_pin_title_and_body(self):
        pinned, title, body = self.run_pin(GOOD + "\n")
        self.assertEqual(pinned, GOOD + "\n")
        self.assertEqual(title.split()[-1], "ab12ab12ab12")
        self.assertIn(GOOD, body)

    def test_rejects_anything_but_a_digest(self):
        for bad in ("", "latest", "sha256:abc", GOOD.upper(), GOOD + "\nextra"):
            with self.assertRaises(SystemExit, msg=bad):
                self.run_pin(bad)


if __name__ == "__main__":
    unittest.main()
