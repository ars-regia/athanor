"""athanor-secureboot-enroll against a fake mokutil (doc_kernel_profile.md section 4, #252)
(python3 -B -m unittest discover -s system/tests -p test_secureboot_enroll.py -v).

The fake prints the exact answers of mokutil 0.7.2 and records its arguments, so the tests see
which state each answer is read as and that nothing but the request path calls --import. The
request path runs as root inside a user namespace, where unshare allows one.
"""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "secureboot/athanor-secureboot-enroll"

FAKE_MOKUTIL = """#!/bin/sh
echo "$*" >> "$FAKE_LOG"
case $1 in
--sb-state)
    if [ -n "$FAKE_SB_STDERR" ]; then printf '%b\\n' "$FAKE_SB" >&2; else printf '%b\\n' "$FAKE_SB"; fi
    exit "${FAKE_SB_RC:-0}" ;;
--test-key)
    if [ -e "$FAKE_LOG.request" ]; then echo "$2 is already in the enrollment request"; exit 1; fi
    [ -z "$FAKE_KEY_STDERR" ] || echo "$FAKE_KEY_STDERR" >&2
    printf '%s\\n' "$2$FAKE_KEY"; exit "${FAKE_KEY_RC:-0}" ;;
--import) [ -n "$FAKE_IMPORT_FILES_NOTHING" ] || touch "$FAKE_LOG.request"; exit 0 ;;
*) exit 0 ;;
esac
"""


def can_be_root():
    unshare = shutil.which("unshare")
    return (
        unshare is not None
        and subprocess.run(
            [unshare, "--map-root-user", "true"], capture_output=True
        ).returncode
        == 0
    )


class SecureBootEnroll(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = Path(tmp.name)
        (self.dir / "bin").mkdir()
        fake = self.dir / "bin/mokutil"
        fake.write_text(FAKE_MOKUTIL)
        fake.chmod(0o755)
        (self.dir / "efi").mkdir()
        self.cert = self.dir / "athanor-secureboot.der"
        self.cert.write_bytes(b"not parsed by the fake")
        self.log = self.dir / "mokutil.log"
        self.log.touch()

    def run_script(
        self,
        *args,
        sb="SecureBoot enabled",
        key=" is not enrolled",
        sb_rc=0,
        key_rc=0,
        root=False,
        **extra,
    ):
        env = {
            "PATH": f"{self.dir / 'bin'}:/usr/bin:/bin",
            "ATHANOR_SECUREBOOT_CERT": str(self.cert),
            "ATHANOR_SECUREBOOT_EFI": str(self.dir / "efi"),
            "FAKE_LOG": str(self.log),
            "FAKE_SB": sb,
            "FAKE_SB_RC": str(sb_rc),
            "FAKE_KEY": key,
            "FAKE_KEY_RC": str(key_rc),
            **extra,
        }
        prefix = ["unshare", "--map-root-user"] if root else []
        return subprocess.run(
            [*prefix, SCRIPT, *args],
            env=env,
            capture_output=True,
            text=True,
            timeout=30,
        )

    def calls(self):
        return [line.split()[0] for line in self.log.read_text().splitlines()]

    def test_enabled_and_enrolled(self):
        r = self.run_script("--status", key=" is already enrolled", key_rc=1)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            r.stdout,
            "Secure Boot: enabled\nAthanor certificate: enrolled as a Machine Owner Key\n",
        )

    def test_validation_disabled_in_shim_is_not_enforced(self):
        r = self.run_script(
            "--status",
            sb=r"SecureBoot enabled\nSecureBoot validation is disabled in shim",
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(
            "Secure Boot: not enforced: on in the firmware, but shim's validation is disabled",
            r.stdout,
        )

    def test_disabled_reads_the_same_under_either_exit_status(self):
        for rc in (0, 1):
            with self.subTest(exit_status=rc):
                r = self.run_script("--status", sb="SecureBoot disabled", sb_rc=rc)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertIn("Secure Boot: disabled\n", r.stdout)

    def test_setup_mode_is_disabled(self):
        r = self.run_script(
            "--status", sb=r"SecureBoot disabled\nPlatform is in Setup Mode"
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(
            "Secure Boot: disabled: the firmware is in setup mode\n", r.stdout
        )

    def test_firmware_without_secure_boot_stops_before_the_key(self):
        """mokutil prints this answer on its error output."""
        r = self.run_script(
            sb="This system doesn't support Secure Boot",
            sb_rc=255,
            FAKE_SB_STDERR="1",
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("Secure Boot: not reported", r.stdout)
        self.assertEqual(self.calls(), ["--sb-state"])

    def test_an_unexpected_state_stops_with_mokutil_answer_and_status(self):
        for sb in (
            "SecureBoot enabled\nStrange data size 2",
            "Cannot determine secure boot state.",
            "SecureBoot enabledx",
        ):
            with self.subTest(sb=sb):
                r = self.run_script("--status", sb=sb, sb_rc=255)
                self.assertEqual(r.returncode, 1)
                self.assertIn(
                    "mokutil --sb-state gave an unexpected answer (exit 255)", r.stderr
                )
                self.assertEqual(r.stdout, "")

    def test_requested_and_db(self):
        cases = {
            " is already in the enrollment request": "enrolment requested, to be confirmed in MokManager",
            " is already in db": "trusted by the firmware's own database",
        }
        for key, line in cases.items():
            with self.subTest(key=key):
                r = self.run_script(key=key)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertIn(line, r.stdout)
                self.assertNotIn("--import", self.calls())

    def test_an_unexpected_key_answer_stops(self):
        for key in (" is blocked in dbx", " is not enrolled\nextra"):
            with self.subTest(key=key):
                r = self.run_script("--status", key=key, key_rc=1)
                self.assertEqual(r.returncode, 1)
                self.assertIn(
                    "mokutil --test-key gave an unexpected answer (exit 1)", r.stderr
                )

    def test_a_key_diagnostic_does_not_change_the_answer(self):
        """mokutil 0.7.2 run without root prints this before its answer."""
        diagnostic = "Failed to access kernel trusted keyring: Required key not available"
        r = self.run_script("--status", FAKE_KEY_STDERR=diagnostic)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("Athanor certificate: not enrolled\n", r.stdout)
        self.assertIn(diagnostic, r.stderr, "the diagnostic reaches the terminal")

    def test_status_never_files_a_request(self):
        r = self.run_script("--status")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("Athanor certificate: not enrolled\n", r.stdout)
        self.assertEqual(self.calls(), ["--sb-state", "--test-key"])

    def test_the_step_that_enforces_follows_the_state(self):
        cases = {
            "SecureBoot enabled": None,
            "SecureBoot disabled": "Turn Secure Boot on in the firmware settings.",
            r"SecureBoot disabled\nPlatform is in Setup Mode": "restore its factory Secure Boot keys",
            r"SecureBoot enabled\nSecureBoot validation is disabled in shim": "sudo mokutil --enable-validation",
        }
        for sb, step in cases.items():
            with self.subTest(sb=sb):
                r = self.run_script("--status", sb=sb, key=" is already enrolled")
                self.assertEqual(r.returncode, 0, r.stderr)
                lines = r.stdout.splitlines()
                self.assertEqual(len(lines), 2 if step is None else 3, r.stdout)
                if step is not None:
                    self.assertIn(step, lines[2])
        r = self.run_script("--status", sb="SecureBoot disabled")
        self.assertNotIn("Turn Secure Boot on", r.stdout, "before the enrolment")

    @unittest.skipUnless(can_be_root(), "unshare --map-root-user is not allowed here")
    def test_the_request_is_filed_and_checked(self):
        for sb, step in (
            ("SecureBoot enabled", None),
            ("SecureBoot disabled", "  4. Turn Secure Boot on in the firmware settings."),
        ):
            with self.subTest(sb=sb):
                self.log.write_text("")
                Path(f"{self.log}.request").unlink(missing_ok=True)
                r = self.run_script(sb=sb, root=True)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertEqual(
                    self.calls(),
                    ["--sb-state", "--test-key", "--timeout", "--import", "--test-key"],
                )
                self.assertIn("--timeout -1", self.log.read_text())
                self.assertIn("The enrolment request is filed", r.stdout)
                last = r.stdout.splitlines()[-1]
                if step is None:
                    self.assertNotIn("4.", r.stdout)
                else:
                    self.assertEqual(last, step)

    @unittest.skipUnless(can_be_root(), "unshare --map-root-user is not allowed here")
    def test_an_import_that_files_nothing_stops(self):
        r = self.run_script(root=True, FAKE_IMPORT_FILES_NOTHING="1")
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not see the request", r.stderr)
        self.assertNotIn("The enrolment request is filed", r.stdout)

    @unittest.skipIf(
        os.geteuid() == 0, "the request path runs mokutil --import as root"
    )
    def test_the_request_needs_root(self):
        r = self.run_script()
        self.assertEqual(r.returncode, 1)
        self.assertIn("needs root", r.stderr)
        self.assertNotIn("--import", self.calls())

    def test_no_uefi_and_no_certificate(self):
        (self.dir / "efi").rmdir()
        r = self.run_script("--status")
        self.assertEqual(r.returncode, 1)
        self.assertIn("did not boot through UEFI", r.stderr)
        (self.dir / "efi").mkdir()
        self.cert.unlink()
        r = self.run_script("--status")
        self.assertEqual(r.returncode, 1)
        self.assertIn("missing from the image", r.stderr)
        self.assertEqual(self.calls(), [])

    def test_unknown_arguments(self):
        for args in (["--import"], ["--status", "--status"]):
            with self.subTest(args=args):
                self.assertEqual(self.run_script(*args).returncode, 2)


if __name__ == "__main__":
    unittest.main()
