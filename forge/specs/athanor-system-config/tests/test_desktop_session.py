"""athanor-desktop publishes the session class to the user manager
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

The script runs up to its first import-environment; systemctl and loginctl are stand-ins
that record their arguments and the environment they were given.
"""

import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "SOURCES/usr/bin/athanor-desktop"


class DesktopSession(unittest.TestCase):
    def run_prologue(self, loginctl_body, session_id="3"):
        work = Path(tempfile.mkdtemp())
        lines = SCRIPT.read_text().splitlines()
        end = next(i for i, line in enumerate(lines) if "import-environment" in line)
        script = work / "athanor-desktop"
        script.write_text("\n".join(lines[: end + 1]) + "\n")
        stubs = work / "bin"
        stubs.mkdir()
        (stubs / "systemctl").write_text(
            f'#!/bin/sh\necho "$@" > "{work}/systemctl"\nenv > "{work}/env"\n'
        )
        (stubs / "loginctl").write_text(f"#!/bin/sh\n{loginctl_body}\n")
        for stub in stubs.iterdir():
            stub.chmod(0o755)
        env = {"PATH": f"{stubs}:/usr/bin:/bin"}
        if session_id is not None:
            env["XDG_SESSION_ID"] = session_id
        result = subprocess.run(
            ["/bin/sh", str(script)], env=env, capture_output=True, text=True
        )
        return result, work

    def test_the_class_is_read_from_logind_and_imported(self):
        result, work = self.run_prologue(
            '[ "$*" = "show-session 3 --property=Class --value" ] && echo user'
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("XDG_SESSION_CLASS", (work / "systemctl").read_text().split())
        self.assertIn("XDG_SESSION_CLASS=user", (work / "env").read_text().splitlines())

    def test_a_failing_loginctl_is_logged_and_the_session_goes_on(self):
        result, work = self.run_prologue("exit 1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cannot read the session class", result.stderr)
        self.assertTrue((work / "systemctl").exists())

    def test_no_session_id_is_logged_and_the_session_goes_on(self):
        result, work = self.run_prologue("echo user", session_id=None)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cannot read the session class", result.stderr)


if __name__ == "__main__":
    unittest.main()
