"""Tests of forge/scripts/check_image_session.py
(python3 -B -m unittest discover -s forge/scripts/tests)."""

import importlib.util
import os
import pathlib
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "forge/scripts"))
spec = importlib.util.spec_from_file_location(
    "check_image_session", ROOT / "forge/scripts/check_image_session.py"
)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

SOURCES = ROOT / "forge/specs"
LAUNCHER = SOURCES / "athanor-system-config/SOURCES/usr/bin/athanor-desktop"
USER_UNITS = SOURCES / "athanor-system-services/SOURCES/usr/lib/systemd/user"


class ImageSession(unittest.TestCase):
    """A fake image root built from the repository's own sources, so the test also fails when a
    source drops the lock or the agent."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        for b in mod.BINARIES:
            self.write(b, "")
        self.write(mod.LAUNCHER, LAUNCHER.read_text())
        self.write("usr/lib/pam.d/cosmic-greeter", "auth substack system-auth\n")
        self.write(mod.AGENT_UNIT, (USER_UNITS / "cosmic-osd.service").read_text())
        self.write(
            mod.SESSION_TARGET, (USER_UNITS / "athanor-session.target").read_text()
        )
        self.write(mod.HELPER, "")
        os.chmod(self.root / mod.HELPER, 0o755)
        self.enable(mod.HELPER_SOCKET, "etc/systemd/system/sockets.target.wants")

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def enable(self, unit, wants):
        d = self.root / wants
        d.mkdir(parents=True, exist_ok=True)
        (d / unit).symlink_to("/usr/lib/systemd/system/" + unit)

    def edit(self, rel, old, new):
        path = self.root / rel
        text = path.read_text()
        self.assertIn(old, text)
        path.write_text(text.replace(old, new))

    def test_the_repository_sources_pass(self):
        self.assertEqual(mod.problems(self.root), [])

    def test_a_missing_locker_fails(self):
        (self.root / "usr/bin/cosmic-greeter").unlink()
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_a_launcher_without_the_locker_fails(self):
        self.edit(mod.LAUNCHER, 'session_component "$$" cosmic-greeter &\n', "")
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_a_commented_launcher_line_fails(self):
        self.edit(
            mod.LAUNCHER,
            'session_component "$$" cosmic-idle &',
            '# session_component "$$" cosmic-idle &',
        )
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_a_missing_locker_pam_service_fails(self):
        (self.root / "usr/lib/pam.d/cosmic-greeter").unlink()
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_an_agent_without_no_new_privileges_fails(self):
        self.edit(mod.AGENT_UNIT, "NoNewPrivileges=true", "NoNewPrivileges=false")
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_a_drop_in_that_lifts_no_new_privileges_fails(self):
        self.write(mod.AGENT_DROPINS[1] + "/50-x.conf", "[Service]\nNoNewPrivileges=no\n")
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_a_later_drop_in_that_restores_no_new_privileges_passes(self):
        self.write(mod.AGENT_DROPINS[0] + "/10-x.conf", "[Service]\nNoNewPrivileges=no\n")
        self.write(mod.AGENT_DROPINS[1] + "/20-y.conf", "[Service]\nNoNewPrivileges=true\n")
        self.assertEqual(mod.problems(self.root), [])

    def test_a_masked_agent_fails(self):
        (self.root / mod.AGENT_OVERRIDE).parent.mkdir(parents=True)
        (self.root / mod.AGENT_OVERRIDE).symlink_to("/dev/null")
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_a_session_that_does_not_want_the_agent_fails(self):
        self.edit(mod.SESSION_TARGET, " cosmic-osd.service", "")
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_a_setuid_helper_fails(self):
        os.chmod(self.root / mod.HELPER, 0o4755)
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_a_disabled_helper_socket_fails(self):
        (
            self.root / "etc/systemd/system/sockets.target.wants" / mod.HELPER_SOCKET
        ).unlink()
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_a_masked_helper_socket_fails(self):
        self.write("etc/systemd/system/" + mod.HELPER_SOCKET, "")
        self.assertEqual(len(mod.problems(self.root)), 1)

    def test_the_enabled_greeter_daemon_fails(self):
        self.enable(
            mod.GREETER_DAEMON, "usr/lib/systemd/system/multi-user.target.wants"
        )
        self.assertEqual(len(mod.problems(self.root)), 1)


if __name__ == "__main__":
    unittest.main()
