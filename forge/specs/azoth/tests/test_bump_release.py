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

    def test_the_repository_is_at_43(self):
        self.assertEqual(bump.fedora_version(), 43)

    def test_every_derived_value_follows_45(self):
        self.containerfile("FROM scratch\nARG FEDORA_VERSION=45\nENV FEDORA_VERSION=$FEDORA_VERSION\n")
        self.assertEqual(bump.fedora_version(), 45)
        self.assertEqual(bump.fedora_releases(), ("F45", "F46"))
        self.assertTrue(bump.cosmic_comp_re().match("cosmic-comp-1.9.0-2.fc45"))
        self.assertIsNone(bump.cosmic_comp_re().match("cosmic-comp-1.9.0-2.fc43"))
        with mock.patch.object(bump, "http", return_value=(200, b'{"updates": []}')) as http:
            bump.cosmic_comp_nvrs()
        self.assertIn("releases=F45", http.call_args.args[0])
        self.assertEqual(bump.cosmic_comp_newest(updates("cosmic-comp-1.8.0-1.fc45", "cosmic-comp-1.9.0-1.fc43")), "1.8.0-1.fc45")
        with self.assertRaisesRegex(SystemExit, "F45"):
            bump.cosmic_comp_newest(updates("cosmic-comp-1.9.0-1.fc43"))
        spec = bump.cosmic_comp_spec(SPEC, "1.9.0-2.fc45", "b" * 40, datetime.date(2026, 10, 8))
        self.assertIn("%global fedora_release 2.fc45\n", spec)
        self.assertEqual(bump.cosmic_comp_pin(spec), "1.9.0-2.fc45")
        self.assertIn("stable F45 build moved", bump.cosmic_comp_body({"old": "1.8.0-1.fc45", "new": "1.9.0-2.fc45"}))

    def test_a_missing_or_malformed_line_is_an_error(self):
        for text in ("FROM scratch\n", "ARG FEDORA_VERSION=\n", "ARG FEDORA_VERSION=rawhide\n"):
            with self.subTest(text=text):
                self.containerfile(text)
                with self.assertRaisesRegex(SystemExit, "ARG FEDORA_VERSION"):
                    bump.fedora_version()


if __name__ == "__main__":
    unittest.main()
