"""athanor-desktop publishes the session class and the import list to the user manager
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

The script runs up to its first import-environment, with the reader of the import list it
sources; systemctl and loginctl are stand-ins that record their arguments and the environment
they were given.
"""

import subprocess
import tempfile
import unittest
from pathlib import Path

SOURCES = Path(__file__).resolve().parents[1] / "SOURCES"
SCRIPT = SOURCES / "usr/bin/athanor-desktop"
IMPORT_LIST = "/usr/share/athanor-system-config/session-import"
READER = "/usr/libexec/athanor/session-import-names"


class DesktopSession(unittest.TestCase):
    def run_prologue(self, loginctl_body, session_id="3", listing=None):
        work = Path(tempfile.mkdtemp())
        lines = SCRIPT.read_text().splitlines()
        end = next(i for i, line in enumerate(lines) if "import-environment" in line)
        script = work / "athanor-desktop"
        prologue = "\n".join(lines[: end + 1]) + "\n"
        self.assertIn(IMPORT_LIST, prologue)
        self.assertIn(f". {READER}\n", prologue)
        prologue = prologue.replace(READER, str(SOURCES / READER.lstrip("/")))
        names = SOURCES / IMPORT_LIST.lstrip("/")
        if listing is not None:
            names = work / "session-import"
            names.write_bytes(listing)
        script.write_text(prologue.replace(IMPORT_LIST, str(names)))
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

    def test_the_names_of_the_import_list_are_imported(self):
        result, work = self.run_prologue("echo user")
        self.assertEqual(result.returncode, 0, result.stderr)
        names = [
            line
            for line in (SOURCES / IMPORT_LIST.lstrip("/")).read_text().splitlines()
            if not line.startswith("#")
        ]
        self.assertIn("WAYLAND_DISPLAY", names)
        self.assertEqual(
            (work / "systemctl").read_text().split(), ["--user", "import-environment", *names]
        )

    def refused(self, listing, message):
        result, work = self.run_prologue("echo user", listing=listing)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(message, result.stderr)
        self.assertFalse((work / "systemctl").exists(), "import-environment was called")

    def test_a_list_with_no_name_is_refused(self):
        for listing in (
            b"",
            b"# a comment\n\n# another\n",
            b"   \n\t\n",
            b"  # an indented comment\n\t# x\n",
            b"\r\n# c\r\n",
        ):
            with self.subTest(listing=listing):
                self.refused(listing, "names no variable")

    def test_an_invalid_name_is_refused(self):
        for listing in (b"WAYLAND_DISPLAY\n*\n", b"1BAD\n", b"A-B\n", b"A.B\n", b"$HOME\n"):
            with self.subTest(listing=listing):
                self.refused(listing, "not a variable name")

    def test_a_line_of_several_words_is_refused(self):
        self.refused(b"A B\n", "more than one word")

    def test_blank_lines_indented_comments_and_crlf_are_tolerated(self):
        result, work = self.run_prologue(
            "echo user", listing=b"# c\r\n  \r\n  # x y\r\n  WAYLAND_DISPLAY \r\n\tDISPLAY\r\nLAST"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            (work / "systemctl").read_text().split(),
            ["--user", "import-environment", "WAYLAND_DISPLAY", "DISPLAY", "LAST"],
        )

    def test_a_missing_list_is_refused(self):
        result, work = self.run_prologue("echo user", listing=b"")
        script = (work / "athanor-desktop").read_text().replace(str(work / "session-import"), str(work / "absent"))
        (work / "athanor-desktop").write_text(script)
        result = subprocess.run(
            ["/bin/sh", str(work / "athanor-desktop")],
            env={"PATH": f"{work / 'bin'}:/usr/bin:/bin"},
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("cannot read the import list", result.stderr)

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
