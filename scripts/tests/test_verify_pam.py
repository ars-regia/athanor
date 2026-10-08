"""Unit tests of the PAM assertions of scripts/verify.py pam
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class Tree(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.containerfile = self.root / verify.PAM_CONTAINERFILE
        self.containerfile.parent.mkdir(parents=True)
        self.containerfile.write_text((ROOT / verify.PAM_CONTAINERFILE).read_text())

    def tearDown(self):
        self.tmp.cleanup()


class Nullok(Tree):
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


class Pwquality(Tree):
    def test_the_tree_asks_twelve_characters(self):
        self.assertEqual(verify.pwquality_problems(self.root), [])

    def test_a_shorter_minimum_fails(self):
        text = self.containerfile.read_text()
        self.containerfile.write_text(text.replace("'minlen = 12'", "'minlen = 8'"))
        self.assertEqual(len(verify.pwquality_problems(self.root)), 1)

    def test_the_minimum_in_a_comment_only_fails(self):
        lines = self.containerfile.read_text().splitlines(keepends=True)
        kept = ["# " + line if "'minlen = 12'" in line else line for line in lines]
        self.containerfile.write_text("".join(kept))
        self.assertEqual(len(verify.pwquality_problems(self.root)), 1)


class Faillock(Tree):
    def problems(self, mutate):
        self.containerfile.write_text(mutate((ROOT / verify.PAM_CONTAINERFILE).read_text()))
        return verify.faillock_problems(self.root)

    def test_the_tree_locks_accounts_out(self):
        self.assertEqual(verify.faillock_problems(self.root), [])

    def test_a_build_without_the_feature_fails(self):
        self.assertEqual(len(self.problems(lambda t: t.replace("enable-feature with-faillock", "enable-feature with-mdns4"))), 1)

    def test_a_commented_out_step_fails(self):
        self.assertTrue(self.problems(lambda t: t.replace("RUN authselect enable-feature with-faillock", "# RUN authselect enable-feature with-faillock")))

    def test_a_later_disable_fails(self):
        self.assertEqual(len(self.problems(lambda t: t + "\nRUN authselect disable-feature with-faillock\n")), 1)

    def test_a_looser_threshold_fails(self):
        for old, new in (("deny = 5", "deny = 50"), ("fail_interval = 900", "fail_interval = 90"), ("unlock_time = 600", "unlock_time = 6")):
            with self.subTest(old=old):
                self.assertEqual(len(self.problems(lambda t: t.replace(old, new))), 1)

    def test_a_later_assignment_wins_and_fails(self):
        self.assertEqual(len(self.problems(lambda t: t.replace("'unlock_time = 600'", "'unlock_time = 600' 'deny = 50'"))), 1)

    def test_a_later_assignment_with_the_right_value_passes(self):
        self.assertEqual(self.problems(lambda t: t.replace("'deny = 5'", "'deny = 50' 'deny = 5'")), [])

    def test_thresholds_outside_the_appending_step_do_not_count(self):
        def move(t):
            t = t.replace("'deny = 5' ", "")
            return t + "\nRUN echo 'deny = 5' > /tmp/x\n"
        self.assertEqual(len(self.problems(move)), 1)

    def test_locking_root_out_fails(self):
        for extra in ("even_deny_root", "  even_deny_root", "admin_group = wheel", " admin_group=wheel"):
            with self.subTest(extra=extra):
                self.assertEqual(len(self.problems(lambda t: t.replace("'unlock_time = 600'", f"'unlock_time = 600' '{extra}'"))), 1)

    def test_a_step_without_the_stack_check_fails(self):
        self.assertEqual(len(self.problems(lambda t: t.replace("pam_faillock.so preauth", "pam_faillock.so"))), 1)

    def test_a_missing_appending_step_fails(self):
        self.assertEqual(len(self.problems(lambda t: t.replace(">> /etc/security/faillock.conf", ">> /etc/security/other.conf"))), 1)


if __name__ == "__main__":
    unittest.main()
