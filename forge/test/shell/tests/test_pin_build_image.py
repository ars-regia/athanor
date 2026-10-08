import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import pin_build_image  # noqa: E402

DIGEST = "sha256:" + "ab12" * 16
GOOD = "ghcr.io/ars-regia/athanor-shell-rig-build@" + DIGEST


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

    def test_accepts_a_registry_with_a_port(self):
        self.run_pin("localhost:5000/o/athanor-shell-rig-build@" + DIGEST)

    def test_rejects_what_is_not_a_full_lowercase_reference(self):
        bad = (
            "",
            DIGEST,  # the old digest-only form is rejected, not migrated: it was never committed
            "ghcr.io/Ars-Regia/athanor-shell-rig-build@" + DIGEST,
            "ghcr.io/o/athanor-shell-rig-build:latest",
            "ghcr.io/o/other@" + DIGEST,
            "ghcr.io/o/athanor-shell-rig-build@sha256:abc",
            GOOD + "\nextra",
        )
        for b in bad:
            with self.assertRaises(SystemExit, msg=b):
                self.run_pin(b)


if __name__ == "__main__":
    unittest.main()
