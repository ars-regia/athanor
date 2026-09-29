"""Unit test of the NVIDIA availability rule of bump.py (python3 -B -m unittest discover -s forge/specs/azoth/tests -v)."""

import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

AZOTH = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AZOTH))
import bump  # noqa: E402


def never(*_):
    raise AssertionError("not expected to be called")


class NvidiaAvailability(unittest.TestCase):
    def test_version_not_packaged_keeps_the_pin_and_notes_why(self):
        notes = []
        got = bump.packaged_or_current("open", "615.71.09", "610.57.04", notes, check=lambda branch, version: False)
        self.assertEqual(got, "610.57.04")
        self.assertTrue(any("615.71.09" in n and "open" in n for n in notes))

    def test_packaged_version_moves(self):
        notes = []
        got = bump.packaged_or_current("legacy", "580.190.01", "580.178.04", notes, check=lambda branch, version: True)
        self.assertEqual(got, "580.190.01")
        self.assertEqual(notes, [])

    def lock_check_exit(self, code, stderr=""):
        done = subprocess.CompletedProcess([], code, stdout="", stderr=stderr)
        with mock.patch.object(bump.subprocess, "run", return_value=done):
            return bump.lock_check("legacy", "580.190.01")

    def test_lock_check_published(self):
        self.assertTrue(self.lock_check_exit(0))

    def test_lock_check_not_published(self):
        self.assertFalse(self.lock_check_exit(3))

    def test_lock_check_error_aborts_with_stderr(self):
        with self.assertRaises(SystemExit) as raised:
            self.lock_check_exit(1, "lock.py: connection reset")
        self.assertIn("connection reset", str(raised.exception.code))

    def test_newer_packaged_version_moves_and_regenerates(self):
        got = bump.nvidia_version("legacy", "580.190.01", "580.178.04", [], check=lambda b, v: True, verify=never)
        self.assertEqual(got, ("580.190.01", True))

    def test_current_lock_matching_the_repository_stays(self):
        got = bump.nvidia_version("legacy", "580.178.04", "580.178.04", [], check=never, verify=lambda b, v: "ok")
        self.assertEqual(got, ("580.178.04", False))

    def test_republished_current_version_regenerates_the_lock(self):
        notes = []
        got = bump.nvidia_version("legacy", "580.178.04", "580.178.04", notes, check=never, verify=lambda b, v: "stale")
        self.assertEqual(got, ("580.178.04", True))
        self.assertTrue(any("legacy" in n and "regenerated" in n for n in notes))

    def test_unpackaged_candidate_still_verifies_the_current_lock(self):
        got = bump.nvidia_version("open", "615.71.09", "610.57.04", [], check=lambda b, v: False, verify=lambda b, v: "stale")
        self.assertEqual(got, ("610.57.04", True))

    def test_current_version_gone_with_nothing_newer_aborts(self):
        with self.assertRaises(SystemExit) as raised:
            bump.nvidia_version("open", "615.71.09", "610.57.04", [], check=lambda b, v: False, verify=lambda b, v: "gone")
        self.assertIn("610.57.04", str(raised.exception.code))

    def test_older_candidate_is_not_a_move(self):
        got = bump.nvidia_version("legacy", "580.170.01", "580.178.04", [], check=never, verify=lambda b, v: "ok")
        self.assertEqual(got, ("580.178.04", False))

    def test_lock_verify_maps_exit_codes(self):
        for code, state in ((0, "ok"), (3, "gone"), (4, "stale")):
            done = subprocess.CompletedProcess([], code, stdout="", stderr="")
            with self.subTest(code=code), mock.patch.object(bump.subprocess, "run", return_value=done):
                self.assertEqual(bump.lock_verify("legacy", "580.178.04"), state)

    def test_legacy_candidate_comes_from_the_repository(self):
        done = subprocess.CompletedProcess([], 0, stdout="580.190.01\n", stderr="")
        with mock.patch.object(bump.subprocess, "run", return_value=done) as run:
            self.assertEqual(bump.nvidia_legacy("580.178.04"), "580.190.01")
        self.assertEqual(run.call_args.args[0][-4:], ["latest", "legacy", "--major", "580"])

    def test_system_containerfile_is_tracked(self):
        self.assertIn(AZOTH.parents[2] / "system" / "Containerfile", bump.CONTAINERFILES)


class BaseImages(unittest.TestCase):
    def containerfiles(self, d, *digests):
        files = []
        for n, digest in enumerate(digests):
            cf = pathlib.Path(d) / f"Containerfile{n}"
            cf.write_text(f"FROM quay.io/fedora/base:43@sha256:{digest * 64}\n")
            files.append(cf)
        return files

    def test_same_ref_at_one_digest(self):
        with tempfile.TemporaryDirectory() as d, mock.patch.object(bump, "CONTAINERFILES", self.containerfiles(d, "a", "a")):
            self.assertEqual(bump.base_images(), {"quay.io/fedora/base:43": "sha256:" + "a" * 64})

    def test_same_ref_at_two_digests_names_the_files(self):
        with tempfile.TemporaryDirectory() as d, mock.patch.object(bump, "CONTAINERFILES", self.containerfiles(d, "a", "b")):
            with self.assertRaises(SystemExit) as raised:
                bump.base_images()
        self.assertIn("Containerfile0", str(raised.exception.code))
        self.assertIn("Containerfile1", str(raised.exception.code))


if __name__ == "__main__":
    unittest.main()
