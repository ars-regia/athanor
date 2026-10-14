"""Tests of forge/scripts/check_image_session.py
(python3 -B -m unittest discover -s forge/scripts/tests).

Paths and names are written out here rather than read from the module, so a wrong constant in
the checker fails a test instead of moving the fake image with it."""

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
LAUNCHER_SOURCE = SOURCES / "athanor-system-config/SOURCES/usr/bin/athanor-desktop"
USER_UNIT_SOURCES = SOURCES / "athanor-system-services/SOURCES/usr/lib/systemd/user"
FAILLOCK_DROPIN_SOURCE = (
    SOURCES / "polkit/SOURCES/polkit-agent-helper-service-faillock.conf"
)

LAUNCHER = "usr/bin/athanor-desktop"
AGENT_UNIT = "usr/lib/systemd/user/cosmic-osd.service"
AGENT_LIB_DROPINS = "usr/lib/systemd/user/cosmic-osd.service.d"
AGENT_ETC_DROPINS = "etc/systemd/user/cosmic-osd.service.d"
SESSION_TARGET = "usr/lib/systemd/user/athanor-session.target"
HELPER = "usr/lib/polkit-1/polkit-agent-helper-1"
HELPER_SOCKET_UNIT = "usr/lib/systemd/system/polkit-agent-helper.socket"
SOCKETS_WANTS = "etc/systemd/system/sockets.target.wants"
HELPER_UNIT = "usr/lib/systemd/system/polkit-agent-helper@.service"
HELPER_LIB_DROPINS = "usr/lib/systemd/system/polkit-agent-helper@.service.d"
HELPER_ETC_DROPINS = "etc/systemd/system/polkit-agent-helper@.service.d"
# The sandbox line of polkit 127's own helper unit that matters here.
HELPER_UNIT_TEXT = "[Service]\nExecStart=/usr/lib/polkit-1/polkit-agent-helper-1\nProtectSystem=strict\n"


class ImageSession(unittest.TestCase):
    """A fake image root built from the repository's own sources, so the test also fails when a
    source drops the lock, the agent or the helper's faillock drop-in."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        for b in (
            "usr/bin/cosmic-greeter",
            "usr/bin/cosmic-idle",
            "usr/bin/cosmic-osd",
        ):
            self.write(b, "")
        self.write(LAUNCHER, LAUNCHER_SOURCE.read_text())
        self.write("usr/lib/pam.d/cosmic-greeter", "auth substack system-auth\n")
        self.write(AGENT_UNIT, (USER_UNIT_SOURCES / "cosmic-osd.service").read_text())
        self.write(
            SESSION_TARGET, (USER_UNIT_SOURCES / "athanor-session.target").read_text()
        )
        self.write(HELPER, "")
        os.chmod(self.root / HELPER, 0o755)
        self.write(HELPER_SOCKET_UNIT, "[Socket]\n")
        self.enable("polkit-agent-helper.socket", SOCKETS_WANTS)
        self.write(HELPER_UNIT, HELPER_UNIT_TEXT)
        self.write(
            f"{HELPER_LIB_DROPINS}/50-athanor-faillock.conf",
            FAILLOCK_DROPIN_SOURCE.read_text(),
        )

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

    def assertProblem(self, words):
        found = mod.problems(self.root)
        self.assertEqual(len(found), 1, found)
        self.assertIn(words, found[0])

    def test_the_repository_sources_pass(self):
        self.assertEqual(mod.problems(self.root), [])

    def test_a_missing_locker_fails(self):
        (self.root / "usr/bin/cosmic-greeter").unlink()
        self.assertProblem("/usr/bin/cosmic-greeter is missing")

    def test_a_missing_launcher_fails(self):
        (self.root / LAUNCHER).unlink()
        self.assertProblem("/usr/bin/athanor-desktop is missing")

    def test_a_launcher_without_the_locker_fails(self):
        self.edit(LAUNCHER, 'session_component "$$" cosmic-greeter &\n', "")
        self.assertProblem("cosmic-greeter &")

    def test_a_commented_launcher_line_fails(self):
        self.edit(
            LAUNCHER,
            'session_component "$$" cosmic-idle &',
            '# session_component "$$" cosmic-idle &',
        )
        self.assertProblem("cosmic-idle &")

    def test_a_missing_locker_pam_service_fails(self):
        (self.root / "usr/lib/pam.d/cosmic-greeter").unlink()
        self.assertProblem("PAM service cosmic-greeter is missing")

    def test_a_missing_agent_unit_fails(self):
        (self.root / AGENT_UNIT).unlink()
        self.assertProblem("/usr/lib/systemd/user/cosmic-osd.service is missing")

    def test_an_agent_without_no_new_privileges_fails(self):
        self.edit(AGENT_UNIT, "NoNewPrivileges=true", "NoNewPrivileges=false")
        self.assertProblem("NoNewPrivileges=true")

    def test_a_drop_in_that_lifts_no_new_privileges_fails(self):
        self.write(f"{AGENT_ETC_DROPINS}/50-x.conf", "[Service]\nNoNewPrivileges=no\n")
        self.assertProblem("NoNewPrivileges=true")

    def test_a_later_drop_in_that_restores_no_new_privileges_passes(self):
        self.write(f"{AGENT_LIB_DROPINS}/10-x.conf", "[Service]\nNoNewPrivileges=no\n")
        self.write(
            f"{AGENT_ETC_DROPINS}/20-y.conf", "[Service]\nNoNewPrivileges=true\n"
        )
        self.assertEqual(mod.problems(self.root), [])

    def test_an_etc_drop_in_replaces_the_usr_lib_one_of_the_same_name(self):
        self.write(f"{AGENT_LIB_DROPINS}/50-x.conf", "[Service]\nNoNewPrivileges=no\n")
        self.write(f"{AGENT_ETC_DROPINS}/50-x.conf", "[Service]\n")
        self.assertEqual(mod.problems(self.root), [])

    def test_an_etc_drop_in_wins_over_the_usr_lib_one_of_the_same_name(self):
        self.write(
            f"{AGENT_LIB_DROPINS}/50-x.conf", "[Service]\nNoNewPrivileges=true\n"
        )
        self.write(f"{AGENT_ETC_DROPINS}/50-x.conf", "[Service]\nNoNewPrivileges=no\n")
        self.assertProblem("NoNewPrivileges=true")

    def test_a_drop_in_without_the_conf_suffix_is_ignored(self):
        self.write(
            f"{AGENT_ETC_DROPINS}/50-x.conf.off", "[Service]\nNoNewPrivileges=no\n"
        )
        self.write(
            f"{AGENT_ETC_DROPINS}/60-y", "[Service]\nExecStart=\nExecStart=/bin/true\n"
        )
        self.assertEqual(mod.problems(self.root), [])

    def test_a_drop_in_that_overrides_the_agent_exec_start_fails(self):
        self.write(
            f"{AGENT_ETC_DROPINS}/50-x.conf",
            "[Service]\nExecStart=\nExecStart=/usr/bin/true\n",
        )
        self.assertProblem("etc/systemd/user/cosmic-osd.service.d/50-x.conf overrides")

    def test_a_masked_agent_fails(self):
        mask = self.root / "etc/systemd/user/cosmic-osd.service"
        mask.parent.mkdir(parents=True)
        mask.symlink_to("/dev/null")
        self.assertProblem("masks or replaces")

    def test_a_session_that_does_not_want_the_agent_fails(self):
        self.edit(SESSION_TARGET, " cosmic-osd.service", "")
        self.assertProblem("does not want cosmic-osd.service")

    def test_a_missing_helper_fails(self):
        (self.root / HELPER).unlink()
        self.assertProblem("/usr/lib/polkit-1/polkit-agent-helper-1 is missing")

    def test_a_setuid_helper_fails(self):
        os.chmod(self.root / HELPER, 0o4755)
        self.assertProblem("is setuid")

    def test_a_disabled_helper_socket_fails(self):
        (self.root / SOCKETS_WANTS / "polkit-agent-helper.socket").unlink()
        self.assertProblem("polkit-agent-helper.socket is not enabled")

    def test_a_masked_helper_socket_fails(self):
        self.write("etc/systemd/system/polkit-agent-helper.socket", "")
        self.assertProblem("polkit-agent-helper.socket is not enabled")

    def test_an_enabled_helper_socket_without_its_unit_file_fails(self):
        (self.root / HELPER_SOCKET_UNIT).unlink()
        self.assertProblem(
            "polkit-agent-helper.socket is enabled but its unit file is missing"
        )

    def test_a_missing_helper_unit_fails(self):
        (self.root / HELPER_UNIT).unlink()
        self.assertProblem(
            "/usr/lib/systemd/system/polkit-agent-helper@.service is missing"
        )

    def test_a_strict_helper_without_the_faillock_drop_in_fails(self):
        (self.root / HELPER_LIB_DROPINS / "50-athanor-faillock.conf").unlink()
        self.assertProblem("no ReadWritePaths= covering /run/faillock")

    def test_a_later_drop_in_that_resets_read_write_paths_fails(self):
        self.write(f"{HELPER_ETC_DROPINS}/90-x.conf", "[Service]\nReadWritePaths=\n")
        self.assertProblem("no ReadWritePaths= covering /run/faillock")

    def test_an_etc_drop_in_of_the_same_name_replaces_the_faillock_one(self):
        self.write(f"{HELPER_ETC_DROPINS}/50-athanor-faillock.conf", "[Service]\n")
        self.assertProblem("no ReadWritePaths= covering /run/faillock")

    def test_a_parent_directory_or_an_optional_entry_covers_the_tally(self):
        for entry in ("/run", "-/run/faillock", "/run/faillock/"):
            with self.subTest(entry=entry):
                self.write(
                    f"{HELPER_LIB_DROPINS}/50-athanor-faillock.conf",
                    f"[Service]\nReadWritePaths={entry}\n",
                )
                self.assertEqual(mod.problems(self.root), [])

    def test_a_sibling_path_does_not_cover_the_tally(self):
        self.write(
            f"{HELPER_LIB_DROPINS}/50-athanor-faillock.conf",
            "[Service]\nReadWritePaths=/run/fail\n",
        )
        self.assertProblem("no ReadWritePaths= covering /run/faillock")

    def test_a_helper_without_protect_system_strict_needs_no_drop_in(self):
        (self.root / HELPER_LIB_DROPINS / "50-athanor-faillock.conf").unlink()
        self.edit(HELPER_UNIT, "ProtectSystem=strict", "ProtectSystem=full")
        self.assertEqual(mod.problems(self.root), [])

    def test_the_enabled_greeter_daemon_fails(self):
        self.enable(
            "cosmic-greeter-daemon.service",
            "usr/lib/systemd/system/multi-user.target.wants",
        )
        self.assertProblem("cosmic-greeter-daemon.service is enabled")


if __name__ == "__main__":
    unittest.main()
