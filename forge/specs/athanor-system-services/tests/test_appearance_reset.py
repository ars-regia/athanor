"""athanor-appearance-reset resets only the values cosmic-settings-daemon wrote
(python3 -B -m unittest discover -s forge/specs/athanor-system-services/tests -v).

dconf is a stand-in that keeps the user database as one file per key path and records each
call; a key without a file has no user value and reads empty, as dconf prints it.
"""

import subprocess
import tempfile
import unittest
from pathlib import Path

SOURCES = Path(__file__).resolve().parents[1] / "SOURCES"
SCRIPT = SOURCES / "usr/libexec/athanor-appearance-reset"
UNIT = SOURCES / "usr/lib/systemd/user/athanor-appearance-reset.service"
PRESET = SOURCES / "usr/lib/systemd/user-preset/80-athanor-system-services.preset"

INTERFACE = "/org/gnome/desktop/interface/"
COSMIC = {
    "icon-theme": "'Cosmic'",
    "font-name": "'Open Sans'",
    "monospace-font-name": "'Noto Sans Mono'",
}
BUTTONS = "/org/gnome/desktop/wm/preferences/button-layout"

FAKE_DCONF = """#!/bin/sh
echo "$*" >> "$WORK/calls"
[ "$1" = "$FAIL" ] && { echo "dconf: $1 failed" >&2; exit 1; }
file="$WORK/db$2"
case "$1" in
read) [ ! -f "$file" ] || cat "$file" ;;
reset) rm -f "$file" ;;
*) exit 2 ;;
esac
"""


class AppearanceReset(unittest.TestCase):
    def run_reset(self, values, fail=""):
        work = Path(tempfile.mkdtemp())
        for path, value in values.items():
            file = work / ("db" + path)
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(value + "\n")
        stubs = work / "bin"
        stubs.mkdir()
        (stubs / "dconf").write_text(FAKE_DCONF)
        (stubs / "dconf").chmod(0o755)
        env = {"PATH": f"{stubs}:/usr/bin:/bin", "WORK": str(work), "FAIL": fail}
        result = subprocess.run([str(SCRIPT)], env=env, capture_output=True, text=True)
        calls = (
            (work / "calls").read_text().splitlines()
            if (work / "calls").exists()
            else []
        )
        return result, work, calls

    def test_each_cosmic_value_is_reset(self):
        for key, value in COSMIC.items():
            with self.subTest(key=key):
                result, work, calls = self.run_reset({INTERFACE + key: value})
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(f"reset {INTERFACE}{key}", calls)
                self.assertFalse((work / ("db" + INTERFACE + key)).exists())
                self.assertIn(f"<5>{key} held", result.stdout)

    def test_every_other_value_is_kept(self):
        values = {
            INTERFACE + "icon-theme": "'Papirus'",
            INTERFACE + "font-name": "'Open Sans 11'",
            INTERFACE + "monospace-font-name": "'Noto Sans Mono 10'",
        }
        result, work, calls = self.run_reset(values)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([c for c in calls if c.startswith("reset")], [])
        for path, value in values.items():
            self.assertEqual((work / ("db" + path)).read_text(), value + "\n")

    def test_no_user_value_writes_nothing(self):
        result, _, calls = self.run_reset({})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, [f"read {INTERFACE}{key}" for key in COSMIC])
        self.assertEqual(result.stdout, "")

    def test_button_layout_is_untouched(self):
        values = {BUTTONS: "'close:'", **{INTERFACE + k: v for k, v in COSMIC.items()}}
        result, work, calls = self.run_reset(values)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any("button-layout" in call for call in calls))
        self.assertEqual((work / ("db" + BUTTONS)).read_text(), "'close:'\n")

    def test_a_failing_read_fails_the_run(self):
        result, _, calls = self.run_reset(
            {INTERFACE + "icon-theme": "'Cosmic'"}, fail="read"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("dconf: read failed", result.stderr)
        self.assertEqual(calls, [f"read {INTERFACE}icon-theme"])

    def test_a_failing_reset_fails_the_run(self):
        result, _, _ = self.run_reset(
            {INTERFACE + "icon-theme": "'Cosmic'"}, fail="reset"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("dconf: reset failed", result.stderr)

    def test_the_unit_runs_at_every_login_from_the_session_target(self):
        unit = "\n".join(l for l in UNIT.read_text().splitlines() if not l.startswith("#"))
        self.assertIn("Type=oneshot", unit)
        self.assertNotIn("RemainAfterExit", unit)
        self.assertIn("PartOf=graphical-session.target", unit)
        self.assertIn("WantedBy=athanor-session.target", unit)
        self.assertIn("ExecStart=/usr/libexec/athanor-appearance-reset", unit)
        self.assertIn(
            "enable athanor-appearance-reset.service", PRESET.read_text().splitlines()
        )


if __name__ == "__main__":
    unittest.main()
