"""The recovery console: what greetd's failure starts, what it says, and that the graphical
kiosk is no longer in the package (python3 -B -m unittest discover -s forge/specs/athanor-recovery/tests -v)."""

import pathlib
import re
import shutil
import subprocess
import unittest

PACKAGE = pathlib.Path(__file__).resolve().parents[1]
SOURCES = PACKAGE / "SOURCES"
UNITS = SOURCES / "usr/lib/systemd/system"
SPEC = (PACKAGE / "athanor-recovery.spec").read_text()


def directives(path):
    return [line.strip() for line in path.read_text().splitlines() if "=" in line and not line.lstrip().startswith("#")]


class Trigger(unittest.TestCase):
    def test_greetd_failing_three_times_in_a_minute_starts_the_recovery_target(self):
        lines = directives(UNITS / "greetd.service.d/recovery-fallback.conf")
        for line in ("OnFailure=athanor-recovery.target", "StartLimitBurst=3", "StartLimitIntervalSec=60s"):
            self.assertIn(line, lines)

    def test_the_target_wants_a_login_on_tty1_and_the_notice(self):
        wants = " ".join(line.split("=", 1)[1] for line in directives(UNITS / "athanor-recovery.target") if line.startswith("Wants="))
        self.assertIn("getty@tty1.service", wants.split())
        self.assertIn("athanor-recovery-notice.service", wants.split())

    def test_the_target_does_not_require_what_could_stop_the_login(self):
        # A failed notice must not keep the person from a login: Wants=, never Requires=.
        self.assertFalse(any(line.startswith("Requires=") for line in directives(UNITS / "athanor-recovery.target")))


class Notice(unittest.TestCase):
    NOTICE = UNITS / "athanor-recovery-notice.service"

    def test_the_message_is_written_before_the_login_prints_its_banner(self):
        self.assertIn("Before=getty@tty1.service", directives(self.NOTICE))

    def test_it_writes_the_message_where_the_banner_fragment_links_to(self):
        lines = directives(self.NOTICE)
        self.assertIn("ExecStart=/usr/bin/install -m 0644 /usr/share/athanor-recovery/recovery.issue /run/athanor-recovery/recovery.issue", lines)
        self.assertIn("RuntimeDirectory=athanor-recovery", lines)

    def test_it_can_write_only_its_own_run_directory_and_has_no_capability(self):
        lines = directives(self.NOTICE)
        for line in ("ProtectSystem=strict", "CapabilityBoundingSet=", "NoNewPrivileges=yes", "PrivateNetwork=yes", "RestrictAddressFamilies=AF_UNIX"):
            self.assertIn(line, lines)
        self.assertFalse(any(line.startswith("ReadWritePaths=") for line in lines))

    def test_the_banner_fragment_is_a_link_to_that_file_and_agetty_reads_only_etc(self):
        # agetty reads /etc/issue.d, and /run/issue.d only when /etc/issue is missing (util-linux 2.39,
        # measured): the fragment is a link in /etc, named *.issue, that points into /run.
        lines = [l for l in (SOURCES / "usr/lib/tmpfiles.d/athanor-recovery.conf").read_text().splitlines() if l and not l.startswith("#")]
        self.assertEqual(lines, ["d /etc/issue.d 0755 root root -",
                                 "L /etc/issue.d/50-athanor-recovery.issue - - - - /run/athanor-recovery/recovery.issue"])

    @unittest.skipUnless(shutil.which("systemd-analyze"), "systemd-analyze is not installed")
    def test_exposure_stays_below_the_stated_threshold(self):
        r = subprocess.run(["systemd-analyze", "security", "--offline=true", "--threshold=30", "--no-pager", str(self.NOTICE)],
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, f"{r.stdout[-200:]}{r.stderr[-200:]}")


class Message(unittest.TestCase):
    TEXT = (SOURCES / "usr/share/athanor-recovery/recovery.issue").read_text()

    def test_it_names_the_command_that_goes_back_and_the_one_that_shows_why(self):
        self.assertIn("sudo athanor-update go-back", self.TEXT)
        self.assertIn("journalctl -b -u greetd", self.TEXT)

    def test_it_is_in_both_shipped_languages(self):
        self.assertIn("The desktop did not start.", self.TEXT)
        self.assertIn("Il desktop non si è avviato.", self.TEXT)

    def test_it_has_no_agetty_escape(self):
        # agetty expands backslash sequences (\n, \l, ...) in an issue file; the message has none.
        self.assertNotIn("\\", self.TEXT)


class Package(unittest.TestCase):
    def test_it_is_files_only(self):
        self.assertIn("BuildArch:      noarch", SPEC)
        for build_dependency in ("cargo", "gtk4", "cosmic-comp", "rpm-ostree"):
            self.assertNotIn(build_dependency, SPEC.split("%changelog", 1)[0], build_dependency)

    def test_it_installs_and_lists_every_file_it_carries(self):
        body = SPEC.split("%changelog", 1)[0]
        installed = set(re.findall(r"%\{buildroot\}(\S+)", body))
        listed = {line.split()[-1] for line in body.split("%files", 1)[1].splitlines() if line.startswith(("/", "%dir"))}
        self.assertTrue(installed <= listed, installed - listed)
        for path in installed:
            self.assertTrue((SOURCES / path.lstrip("/")).is_file(), path)

    def test_the_kiosk_unit_is_not_shipped(self):
        self.assertNotIn("athanor-recovery.service", SPEC.split("%changelog", 1)[0])
        self.assertFalse((UNITS / "athanor-recovery.service").exists())


if __name__ == "__main__":
    unittest.main()
