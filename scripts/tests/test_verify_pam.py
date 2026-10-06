"""Unit tests of the empty-password assertion of scripts/verify.py pam
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class Nullok(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.containerfile = self.root / verify.PAM_CONTAINERFILE
        self.containerfile.parent.mkdir(parents=True)
        self.containerfile.write_text((ROOT / verify.PAM_CONTAINERFILE).read_text())

    def tearDown(self):
        self.tmp.cleanup()

    def test_the_tree_drops_nullok(self):
        self.assertEqual(verify.nullok_problems(self.root), [])

    def test_a_build_without_the_feature_fails(self):
        text = self.containerfile.read_text()
        self.containerfile.write_text(text.replace("without-nullok", "with-mdns4"))
        self.assertEqual(len(verify.nullok_problems(self.root)), 1)

    def test_a_commented_out_step_fails(self):
        text = self.containerfile.read_text()
        self.containerfile.write_text(text.replace("RUN authselect enable-feature without-nullok", "# RUN authselect enable-feature without-nullok"))
        self.assertEqual(len(verify.nullok_problems(self.root)), 1)


if __name__ == "__main__":
    unittest.main()
