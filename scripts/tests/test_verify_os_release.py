"""Unit tests of the os-release assertion of scripts/verify.py os-release
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class OsRelease(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.path = self.root / verify.OS_RELEASE
        self.path.parent.mkdir(parents=True)

    def tearDown(self):
        self.tmp.cleanup()

    def problems(self, text):
        self.path.write_text(text)
        return verify.os_release_problems(self.root)

    def test_the_shipped_file_passes(self):
        self.assertEqual(verify.os_release_problems(), [])

    def test_key_value_lines_and_blank_lines_pass(self):
        self.assertEqual(self.problems('NAME="Athanor OS"\n\nID=athanor\n'), [])

    def test_a_comment_line_fails(self):
        # bootc-image-builder rejects it with "readOSRelease: invalid input".
        self.assertEqual(
            self.problems("ID=athanor\n# support end\nVERSION_ID=43\n"),
            [
                f"{verify.OS_RELEASE}:2: not KEY=VALUE, bootc-image-builder rejects it: # support end"
            ],
        )

    def test_a_missing_file_fails(self):
        self.path.parent.rmdir()
        self.assertEqual(len(verify.os_release_problems(self.root)), 1)


if __name__ == "__main__":
    unittest.main()
