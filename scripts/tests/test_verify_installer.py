"""Unit tests of scripts/verify.py installer: every kickstart that installs carries the
machine check and the MOK enrolment request of system/athanor-install.ks word for word,
and the image ships the certificate they enrol
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import shutil
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

FILES = (
    *verify.INSTALLER_KICKSTARTS,
    "system/Containerfile",
    verify.SECUREBOOT_PIN,
    "forge/specs/azoth/keys/secureboot/athanor-secureboot.der",
)


class Installer(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        for relative in FILES:
            (self.root / relative).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(ROOT / relative, self.root / relative)

    def tearDown(self):
        self.tmp.cleanup()

    def edit(self, relative, old, new, count=1):
        path = self.root / relative
        text = path.read_text()
        self.assertEqual(text.count(old), count, f"{old!r} in {relative}")
        path.write_text(text.replace(old, new))

    def test_the_shipped_files_pass(self):
        self.assertEqual(verify.installer_problems(), [])
        self.assertEqual(verify.installer_problems(self.root), [])

    def test_a_copy_that_drifted_fails(self):
        self.edit(
            "forge/test/iso/collaudo.ks", "x86-64-v3 (supported", "x86-64-v2 (supported"
        )
        self.assertEqual(
            verify.installer_problems(self.root),
            [
                "forge/test/iso/collaudo.ks: lacks the machine check of system/athanor-install.ks, "
                "or carries a copy that drifted",
            ],
        )

    def test_a_kickstart_without_the_mok_request_fails(self):
        self.edit(
            "system/disk_config/iso.toml",
            "--hash-file /dev/stdin",
            "--hash-file /dev/null",
        )
        self.assertEqual(
            verify.installer_problems(self.root),
            [
                "system/disk_config/iso.toml: lacks the MOK enrolment request of system/athanor-install.ks, "
                "or carries a copy that drifted",
            ],
        )

    def test_a_toml_escape_counts_as_drift(self):
        # A backslash in the TOML basic string is an escape: the ISO would run other text.
        self.edit("system/disk_config/iso.toml", "grep '^[$]6[$]'", "grep '^\\$6\\$'")
        self.edit("system/athanor-install.ks", "grep '^[$]6[$]'", "grep '^\\$6\\$'")
        self.edit("forge/test/iso/collaudo.ks", "grep '^[$]6[$]'", "grep '^\\$6\\$'")
        problems = verify.installer_problems(self.root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn(
            "system/disk_config/iso.toml: cannot read its kickstart", problems[0]
        )

    def test_a_check_that_no_longer_stops_the_install_fails(self):
        for relative in verify.INSTALLER_KICKSTARTS:
            self.edit(relative, "%pre --erroronfail\n", "%pre\n")
        self.assertEqual(
            verify.installer_problems(self.root),
            [
                "system/athanor-install.ks: the machine check must open with '%pre --erroronfail', "
                "so that it stops the install",
            ],
        )

    def test_a_block_that_lost_its_test_fails_even_when_copied_everywhere(self):
        for relative in verify.INSTALLER_KICKSTARTS:
            self.edit(relative, "if [ ! -d /sys/firmware/efi ]; then", "if false; then")
        self.assertEqual(
            verify.installer_problems(self.root),
            [
                "system/athanor-install.ks: the machine check lacks '[ ! -d /sys/firmware/efi ]'",
            ],
        )

    def test_a_password_on_the_command_line_fails(self):
        for relative in verify.INSTALLER_KICKSTARTS:
            self.edit(
                relative,
                "| mokutil --generate-hash |",
                '| mokutil --generate-hash="$password" |',
            )
        self.assertIn(
            "system/athanor-install.ks: the MOK password is passed on a command line; mokutil reads it on stdin",
            verify.installer_problems(self.root),
        )

    def test_an_image_without_the_certificate_fails(self):
        self.edit(
            "system/Containerfile",
            f" {verify.SECUREBOOT_CERT}\n",
            " /usr/share/athanor/other.der\n",
        )
        self.assertEqual(
            verify.installer_problems(self.root),
            [
                f"system/Containerfile: does not COPY the certificate the installer enrols to {verify.SECUREBOOT_CERT}",
            ],
        )


    def test_a_certificate_that_differs_from_the_pin_fails(self):
        (self.root / "forge/specs/azoth/keys/secureboot/athanor-secureboot.der").write_bytes(b"another certificate")
        problems = verify.installer_problems(self.root)
        self.assertEqual(len(problems), 1, problems)
        self.assertTrue(problems[0].startswith("forge/specs/azoth/keys/secureboot/athanor-secureboot.der: SHA-256 "), problems)
        self.assertIn("a key rotation updates the pin in the same pull request", problems[0])

    def test_a_pin_on_another_file_fails(self):
        self.edit(verify.SECUREBOOT_PIN, "  forge/", "  system/")
        problems = verify.installer_problems(self.root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("but system/Containerfile ships forge/specs/azoth/keys/secureboot/athanor-secureboot.der", problems[0])


if __name__ == "__main__":
    unittest.main()
