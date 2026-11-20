"""bump.py follows ARG FEDORA_VERSION of system/Containerfile (python3 -B -m unittest discover -s forge/specs/azoth/tests -v)."""

import datetime
import pathlib
import sys
import tempfile
import unittest
from unittest import mock

AZOTH = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AZOTH))
import bump  # noqa: E402

SPEC = "%global fedora_release 1.fc43\nVersion:        1.8.0\nRelease:        %{fedora_release}.athanor3\n%global commit a\n%changelog\n"


def updates(*nvrs):
    return {"updates": [{"builds": [{"nvr": nvr}]} for nvr in nvrs]}


class ReleaseTest(unittest.TestCase):
    def containerfile(self, text):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        path = pathlib.Path(tmp.name) / "Containerfile"
        path.write_text(text)
        patcher = mock.patch.object(bump, "SYSTEM_CONTAINERFILE", path)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_the_repository_is_at_45(self):
        self.assertEqual(bump.fedora_version(), 45)

    def test_every_derived_value_follows_the_containerfile(self):
        # 99, not the repository's own release: the values must come from the patched file.
        self.containerfile("FROM scratch\nARG FEDORA_VERSION=99\nENV FEDORA_VERSION=$FEDORA_VERSION\n")
        self.assertEqual(bump.fedora_version(), 99)
        self.assertEqual(bump.fedora_releases("7.2.11-300.fc45"), ("F99", "F100"))
        self.assertTrue(bump.cosmic_comp_re().match("cosmic-comp-1.9.0-2.fc99"))
        self.assertIsNone(bump.cosmic_comp_re().match("cosmic-comp-1.9.0-2.fc45"))
        with mock.patch.object(bump, "http", return_value=(200, b'{"updates": []}')) as http:
            bump.cosmic_comp_nvrs()
        self.assertIn("releases=F99", http.call_args.args[0])
        self.assertEqual(bump.cosmic_comp_newest(updates("cosmic-comp-1.8.0-1.fc99", "cosmic-comp-1.9.0-1.fc45")), "1.8.0-1.fc99")
        with self.assertRaisesRegex(SystemExit, "F99"):
            bump.cosmic_comp_newest(updates("cosmic-comp-1.9.0-1.fc45"))
        spec = bump.cosmic_comp_spec(SPEC, "1.9.0-2.fc99", "b" * 40, datetime.date(2026, 10, 8))
        self.assertIn("%global fedora_release 2.fc99\n", spec)
        self.assertEqual(bump.cosmic_comp_pin(spec), "1.9.0-2.fc99")
        self.assertIn("stable F99 build moved", bump.cosmic_comp_body({"old": "1.8.0-1.fc45", "new": "1.9.0-2.fc99"}))

    def test_the_kernel_never_moves_back_from_its_pinned_release(self):
        # The kernel source runs ahead of the image: FEDORA_VERSION 43, pin fc45.
        self.containerfile("FROM scratch\nARG FEDORA_VERSION=43\n")
        self.assertEqual(bump.fedora_releases("7.2.11-300.fc45"), ("F45", "F46"))
        with mock.patch.object(bump, "http", return_value=(200, b'{"pages": 1, "updates": []}')) as http:
            self.assertEqual(bump.fedora_kernels("7.2.11-300.fc45"), {})
        self.assertEqual([c.args[0].count("releases=F45") for c in http.call_args_list], [1, 0])
        self.assertEqual([c.args[0].count("releases=F46") for c in http.call_args_list], [0, 1])
        with mock.patch.object(bump, "http", return_value=(200, b'{"updates": []}')) as http:
            bump.cosmic_comp_nvrs()
        self.assertIn("releases=F43", http.call_args.args[0])
        self.assertTrue(bump.cosmic_comp_re().match("cosmic-comp-1.9.0-2.fc43"))
        with self.assertRaisesRegex(SystemExit, "no Fedora release"):
            bump.fedora_releases("7.2.11-300.el9")

    def test_a_missing_or_malformed_line_is_an_error(self):
        for text in ("FROM scratch\n", "ARG FEDORA_VERSION=\n", "ARG FEDORA_VERSION=rawhide\n"):
            with self.subTest(text=text):
                self.containerfile(text)
                with self.assertRaisesRegex(SystemExit, "ARG FEDORA_VERSION"):
                    bump.fedora_version()


if __name__ == "__main__":
    unittest.main()
