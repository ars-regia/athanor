"""Unit tests of system/secureboot/athanor-secureboot-enroll with a mokutil stub
(python3 -B -m unittest discover -s system/tests -v). The script is run from a copy whose
certificate and EFI paths point into a temporary directory."""

import os
import pathlib
import subprocess
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
ENROLL = ROOT / "system" / "secureboot" / "athanor-secureboot-enroll"

# A mokutil that answers --test-key with STUB_TEST_KEY and logs every call.
STUB = textwrap.dedent("""\
    #!/usr/bin/env bash
    echo "$*" >> "$STUB_LOG"
    case $1 in
    --sb-state) echo "SecureBoot disabled" ;;
    --test-key) echo "$2 $STUB_TEST_KEY"; exit "${STUB_TEST_RC:-0}" ;;
    --import) exit 0 ;;
    esac
    """)


class Enroll(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        (self.dir / "bin").mkdir()
        (self.dir / "efi").mkdir()
        self.cert = self.dir / "cert.der"
        self.cert.write_bytes(b"der")
        stub = self.dir / "bin" / "mokutil"
        stub.write_text(STUB)
        stub.chmod(0o755)
        script = ENROLL.read_text()
        script = script.replace(
            "/usr/share/athanor/secureboot/athanor-secureboot.der", str(self.cert)
        )
        script = script.replace("/sys/firmware/efi", str(self.dir / "efi"))
        self.script = self.dir / "enroll"
        self.script.write_text(script)
        self.log = self.dir / "mokutil.log"

    def tearDown(self):
        self.tmp.cleanup()

    def run_enroll(self, answer, *args, rc=0):
        env = dict(
            os.environ,
            PATH=f"{self.dir / 'bin'}:{os.environ['PATH']}",
            STUB_LOG=str(self.log),
            STUB_TEST_KEY=answer,
            STUB_TEST_RC=str(rc),
        )
        return subprocess.run(
            ["bash", str(self.script), *args], env=env, capture_output=True, text=True
        )

    def calls(self):
        return self.log.read_text().splitlines() if self.log.exists() else []

    def test_status_reports_and_changes_nothing(self):
        result = self.run_enroll("is not enrolled", "--status")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Athanor certificate: not enrolled", result.stdout)
        self.assertFalse(any(c.startswith("--import") for c in self.calls()))

    def test_an_enrolled_certificate_is_not_requested_again(self):
        # mokutil exits 1 for an enrolled key on some versions: the text decides, not the status.
        result = self.run_enroll("is already enrolled", rc=1)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("enrolled as a Machine Owner Key", result.stdout)
        self.assertFalse(any(c.startswith("--import") for c in self.calls()))

    def test_a_pending_request_is_not_filed_twice(self):
        result = self.run_enroll("is already in the enrollment request")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any(c.startswith("--import") for c in self.calls()))

    def test_an_unexpected_answer_stops_the_script(self):
        result = self.run_enroll("something else")
        self.assertEqual(result.returncode, 1)
        self.assertIn("unexpected answer", result.stderr)

    @unittest.skipIf(os.geteuid() == 0, "the non-root path needs an unprivileged user")
    def test_filing_the_request_needs_root(self):
        result = self.run_enroll("is not enrolled")
        self.assertEqual(result.returncode, 1)
        self.assertIn("needs root", result.stderr)
        self.assertFalse(any(c.startswith("--import") for c in self.calls()))

    def test_the_import_is_interactive_and_names_only_the_certificate(self):
        # Run as root through the EUID check rewritten away: mokutil --import must get the
        # certificate and nothing that would supply the password (--hash-file, --root-pw).
        self.script.write_text(
            self.script.read_text().replace("[[ $EUID -eq 0 ]]", "true")
        )
        result = self.run_enroll("is not enrolled")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"--import {self.cert}", self.calls())
        self.assertIn("Enroll MOK", result.stdout)


if __name__ == "__main__":
    unittest.main()
