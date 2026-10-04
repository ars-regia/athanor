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

    def test_newer_packaged_version_moves_the_pin(self):
        got = bump.nvidia_pin("legacy", "580.190.01", "580.178.04", [], check=lambda b, v: True)
        self.assertEqual(got, "580.190.01")

    def test_unpackaged_candidate_keeps_the_pin(self):
        got = bump.nvidia_pin("open", "615.71.09", "610.57.04", [], check=lambda b, v: False)
        self.assertEqual(got, "610.57.04")

    def test_older_candidate_is_not_a_move(self):
        got = bump.nvidia_pin("legacy", "580.170.01", "580.178.04", [], check=never)
        self.assertEqual(got, "580.178.04")

    def test_current_lock_matching_the_repository_stays(self):
        self.assertFalse(bump.nvidia_relock("legacy", "580.178.04", [], verify=lambda b, v: "ok"))

    def test_republished_current_version_regenerates_the_lock(self):
        notes = []
        self.assertTrue(bump.nvidia_relock("legacy", "580.178.04", notes, verify=lambda b, v: "stale"))
        self.assertTrue(any("legacy" in n and "regenerated" in n for n in notes))

    def test_current_version_gone_is_a_note(self):
        notes = []
        self.assertFalse(bump.nvidia_relock("open", "610.57.04", notes, verify=lambda b, v: "gone"))
        self.assertTrue(any("610.57.04" in n and "mirror" in n for n in notes))

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

    def test_toolkit_candidate_comes_from_nvidia_repository(self):
        done = subprocess.CompletedProcess([], 0, stdout="1.20.2\n", stderr="")
        with mock.patch.object(bump.subprocess, "run", return_value=done) as run:
            self.assertEqual(bump.nvidia_toolkit("1.20.1"), "1.20.2")
        self.assertEqual(run.call_args.args[0][-4:], ["latest", "container-toolkit", "--major", "1"])

    def test_toolkit_version_is_read_from_its_lock(self):
        self.assertEqual(bump.TOOLKIT_LOCK.name, "container-toolkit.lock")
        with tempfile.TemporaryDirectory() as d:
            lock = pathlib.Path(d) / "container-toolkit.lock"
            lock.write_text("# branch container-toolkit\n# version 1.20.1\n# repository https://x/\n")
            self.assertEqual(bump.toolkit_version(lock), "1.20.1")
            lock.write_text("# branch container-toolkit\n")
            with self.assertRaises(SystemExit):
                bump.toolkit_version(lock)

    def test_system_containerfile_is_tracked_by_the_system_group(self):
        self.assertEqual(bump.GROUP_CONTAINERFILES["system"], [AZOTH.parents[2] / "system" / "Containerfile"])



class LockProblems(unittest.TestCase):
    PINS = {"NVIDIA_OPEN_VERSION": "615.71.09", "NVIDIA_LEGACY_VERSION": "580.178.04"}

    def test_matching_locks_have_no_problem(self):
        self.assertEqual(bump.lock_problems(self.PINS, "1.20.1", [], verify=lambda branch, version: "ok"), [])

    def test_a_stale_lock_is_named_with_its_version(self):
        states = {"open": "stale", "legacy": "ok", "container-toolkit": "ok"}
        got = bump.lock_problems(self.PINS, "1.20.1", [], verify=lambda branch, version: states[branch])
        self.assertEqual(len(got), 1)
        self.assertIn("NVIDIA open 615.71.09", got[0])
        self.assertIn("regenerated", got[0])

    def test_a_vanished_version_is_named_too(self):
        notes = []
        got = bump.lock_problems(self.PINS, "1.20.1", notes, verify=lambda branch, version: "gone" if branch == "legacy" else "ok")
        self.assertEqual(got, [])
        self.assertEqual(len(notes), 1)
        self.assertIn("NVIDIA legacy 580.178.04", notes[0])
        self.assertIn("no longer publishes", notes[0])

    def test_each_branch_is_verified_at_its_own_pin(self):
        seen = []
        bump.lock_problems(self.PINS, "1.20.1", [], verify=lambda branch, version: seen.append((branch, version)) or "ok")
        self.assertEqual(seen, [("open", "615.71.09"), ("legacy", "580.178.04"), ("container-toolkit", "1.20.1")])

class ToolkitInSystemGroup(unittest.TestCase):
    """The container toolkit moves with the system group, never with the kernel group."""

    def compute(self, candidate, state="ok"):
        pins = {"NVIDIA_OPEN_VERSION": "615.71.09", "NVIDIA_LEGACY_VERSION": "580.178.04"}
        codes = {"ok": 0, "stale": bump.LOCK_STALE}

        def lock(action, branch):
            # check: every candidate is packaged; verify: only the toolkit's state varies.
            code = codes[state] if action == "verify" and branch == "container-toolkit" else 0
            return subprocess.CompletedProcess([], code, stdout="", stderr="")

        with mock.patch.object(bump, "read_pins", return_value=pins), \
                mock.patch.object(bump, "toolkit_version", return_value="1.20.1"), \
                mock.patch.object(bump, "nvidia_toolkit", return_value=candidate), \
                mock.patch.object(bump, "lock_py", side_effect=lambda action, branch, *rest, allowed=(0,): lock(action, branch)), \
                mock.patch.object(bump, "base_images", return_value={}):
            return bump.compute("system")["locks"]

    def test_a_newer_packaged_version_moves_the_lock(self):
        self.assertEqual(self.compute("1.20.2"), {"container-toolkit": "1.20.2"})

    def test_a_republished_current_version_regenerates_the_lock(self):
        self.assertEqual(self.compute("1.20.1", state="stale"), {"container-toolkit": "1.20.1"})

    def test_a_matching_lock_stays(self):
        self.assertEqual(self.compute("1.20.1"), {})


class BaseImages(unittest.TestCase):
    def containerfiles(self, d, *digests):
        files = []
        for n, digest in enumerate(digests):
            cf = pathlib.Path(d) / f"Containerfile{n}"
            cf.write_text(f"FROM quay.io/fedora/base:43@sha256:{digest * 64}\n")
            files.append(cf)
        return files

    def test_same_ref_at_one_digest(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertEqual(bump.base_images(self.containerfiles(d, "a", "a")), {"quay.io/fedora/base:43": "sha256:" + "a" * 64})

    def test_same_ref_at_two_digests_names_the_files(self):
        with tempfile.TemporaryDirectory() as d:
            with self.assertRaises(SystemExit) as raised:
                bump.base_images(self.containerfiles(d, "a", "b"))
        self.assertIn("Containerfile0", str(raised.exception.code))
        self.assertIn("Containerfile1", str(raised.exception.code))


if __name__ == "__main__":
    unittest.main()
