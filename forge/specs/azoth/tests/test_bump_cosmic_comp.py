"""Unit test of the cosmic-comp tracking of bump.py (python3 -B -m unittest discover -s forge/specs/azoth/tests -v)."""

import pathlib
import sys
import tempfile
import unittest
from unittest import mock

AZOTH = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AZOTH))
import bump  # noqa: E402

SPEC = """%global fedora_release 1.fc43
%global commit a55785993e8ef6aad38862cb1a9e1ccaad3c340d

Name:           cosmic-comp
Version:        1.8.0
Release:        %{fedora_release}.athanor3
"""


def updates(*nvrs):
    return {"pages": 1, "updates": [{"builds": [{"nvr": nvr}]} for nvr in nvrs]}


class CosmicCompTracking(unittest.TestCase):
    def test_the_newest_stable_build_wins(self):
        data = updates("cosmic-comp-1.8.0-1.fc43", "cosmic-comp-1.9.0-1.fc43", "cosmic-comp-1.8.0-2.fc43", "cosmic-panel-2.0.0-1.fc43")
        self.assertEqual(bump.cosmic_comp_newest(data), "1.9.0-1.fc43")

    def test_a_new_release_of_the_same_version_wins(self):
        data = updates("cosmic-comp-1.8.0-1.fc43", "cosmic-comp-1.8.0-2.fc43")
        self.assertEqual(bump.cosmic_comp_newest(data), "1.8.0-2.fc43")

    def test_no_build_aborts(self):
        with self.assertRaises(SystemExit):
            bump.cosmic_comp_newest(updates("cosmic-panel-2.0.0-1.fc43"))

    def test_the_pin_is_the_version_and_release_of_the_spec(self):
        self.assertEqual(bump.cosmic_comp_pin(SPEC), "1.8.0-1.fc43")

    def test_a_bump_rewrites_the_spec_and_resets_the_suffix(self):
        got = bump.cosmic_comp_spec(SPEC, "1.9.0-2.fc43", "b" * 40)
        self.assertEqual(bump.cosmic_comp_pin(got), "1.9.0-2.fc43")
        self.assertIn("Release:        %{fedora_release}.athanor1\n", got)
        self.assertIn("%global commit " + "b" * 40, got)

    def test_nothing_to_do_when_the_spec_already_has_the_newest(self):
        with mock.patch.object(bump, "cosmic_comp_nvrs", return_value=updates("cosmic-comp-1.8.0-1.fc43")):
            with tempfile.TemporaryDirectory() as d:
                spec = pathlib.Path(d) / "cosmic-comp.spec"
                spec.write_text(SPEC)
                with mock.patch.object(bump, "COSMIC_COMP_SPEC", spec):
                    self.assertIsNone(bump.cosmic_comp_move())

    def test_a_newer_build_is_a_move(self):
        with mock.patch.object(bump, "cosmic_comp_nvrs", return_value=updates("cosmic-comp-1.8.0-2.fc43")):
            with tempfile.TemporaryDirectory() as d:
                spec = pathlib.Path(d) / "cosmic-comp.spec"
                spec.write_text(SPEC)
                with mock.patch.object(bump, "COSMIC_COMP_SPEC", spec):
                    self.assertEqual(bump.cosmic_comp_move(), {"old": "1.8.0-1.fc43", "new": "1.8.0-2.fc43"})


def never(*_):
    raise AssertionError("not expected to be called")


class Independence(unittest.TestCase):
    def run_main(self, *argv):
        with mock.patch.object(sys, "argv", ["bump.py", *argv]):
            bump.main()

    def test_the_kernel_path_never_looks_up_cosmic_comp(self):
        stub = {"changed": False, "pins": {}, "new": {}, "images": {}, "locks": {}, "notes": []}
        with mock.patch.object(bump, "cosmic_comp_move", never), mock.patch.object(bump, "cosmic_comp_nvrs", never), mock.patch.object(bump, "compute", return_value=stub), mock.patch.object(sys, "stdout"):
            self.run_main("check")

    def test_a_cosmic_comp_failure_stays_in_its_own_run(self):
        with mock.patch.object(bump, "cosmic_comp_nvrs", side_effect=OSError("bodhi down")), mock.patch.object(bump, "compute", never):
            with self.assertRaises(OSError):
                self.run_main("cosmic-comp", "check")

    def test_the_cosmic_comp_path_never_runs_the_kernel_tracking(self):
        move = {"old": "1.8.0-1.fc43", "new": "1.8.0-2.fc43"}
        with mock.patch.object(bump, "cosmic_comp_move", return_value=move), mock.patch.object(bump, "compute", never), mock.patch.object(bump, "fedora_kernels", never):
            with mock.patch.object(sys, "stdout"):
                self.run_main("cosmic-comp", "check")

    def test_the_body_names_the_change_and_the_review(self):
        body = bump.cosmic_comp_body({"old": "1.8.0-1.fc43", "new": "1.8.0-2.fc43"})
        self.assertIn("1.8.0-2.fc43", body)
        self.assertIn("merged", body)


if __name__ == "__main__":
    unittest.main()
