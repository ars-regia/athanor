"""Tests of forge/scripts/check_image_session.py
(python3 -B -m unittest discover -s forge/scripts/tests).

Paths and names are written out here rather than read from the module, so a wrong constant in
the checker fails a test instead of moving the fake image with it."""

import importlib.util
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "forge/scripts"))
spec = importlib.util.spec_from_file_location(
    "check_image_session", ROOT / "forge/scripts/check_image_session.py"
)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

SOURCES = ROOT / "forge/specs"
SUPERVISOR_SOURCE = (
    SOURCES / "athanor-system-config/SOURCES/usr/libexec/athanor/session-components"
)
COMPOSITOR_SESSION_SOURCE = (
    SOURCES
    / "athanor-compositor/athanor-compositor-1.0.0/data/athanor-compositor-session"
)
LAUNCHER_SOURCE = SOURCES / "athanor-system-config/SOURCES/usr/bin/athanor-desktop"
USER_UNIT_SOURCES = SOURCES / "athanor-system-services/SOURCES/usr/lib/systemd/user"
FAILLOCK_DROPIN_SOURCE = (
    SOURCES / "polkit/SOURCES/polkit-agent-helper-service-faillock.conf"
)
PORTAL_SOURCES = (
    SOURCES / "athanor-xdg-desktop-portal-athanor/xdg-desktop-portal-athanor-1.0.0"
)

LAUNCHER = "usr/bin/athanor-desktop"
SUPERVISOR = "usr/libexec/athanor/session-components"
COMPOSITOR_SESSION = "usr/bin/athanor-compositor-session"
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
HELPER_RUN_DROPINS = "run/systemd/system/polkit-agent-helper@.service.d"
READ_ONLY_TALLY = "leaves /run/faillock read-only"
SETTINGS_DAEMON_UNIT = "usr/lib/systemd/user/cosmic-settings-daemon.service"
# The user unit directories, the lowest priority first (systemd.unit(5)).
USER_DIRS = (
    "usr/lib/systemd/user",
    "usr/local/lib/systemd/user",
    "usr/share/systemd/user",
    "usr/local/share/systemd/user",
    "run/systemd/user",
    "etc/systemd/user",
    "etc/xdg/systemd/user",
)
SETTINGS_DAEMON_ETC_DROPINS = "etc/systemd/user/cosmic-settings-daemon.service.d"
READ_ONLY_RUNTIME = "leaves /run/user/UID read-only"
PORTALS_CONF = "usr/share/xdg-desktop-portal/athanor-portals.conf"
PORTALS = "usr/share/xdg-desktop-portal/portals"
DBUS_SERVICES = "usr/share/dbus-1/services"
FILE_CHOOSER_LINE = "org.freedesktop.impl.portal.FileChooser=gtk\n"
# The portal files of xdg-desktop-portal-gtk 1.15.3 and gnome-keyring 48, as Fedora 43 ships them.
GTK_PORTAL_TEXT = (
    "[portal]\nDBusName=org.freedesktop.impl.portal.desktop.gtk\n"
    "Interfaces=org.freedesktop.impl.portal.FileChooser;org.freedesktop.impl.portal.AppChooser;"
    "org.freedesktop.impl.portal.Print;org.freedesktop.impl.portal.Notification;"
    "org.freedesktop.impl.portal.Inhibit;org.freedesktop.impl.portal.Access;"
    "org.freedesktop.impl.portal.Account;org.freedesktop.impl.portal.Email;"
    "org.freedesktop.impl.portal.DynamicLauncher;org.freedesktop.impl.portal.Settings;\n"
    "UseIn=gnome\n"
)
KEYRING_PORTAL_TEXT = (
    "[portal]\nDBusName=org.freedesktop.secrets\n"
    "Interfaces=org.freedesktop.impl.portal.Secret\nUseIn=gnome\n"
)


def activation(name, exec_path=None, extra=""):
    """A D-Bus activation file for `name`, running a binary the fixture installs."""
    return f"[D-BUS Service]\nName={name}\nExec={exec_path or '/usr/libexec/' + name}\n{extra}"


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
        self.write(SUPERVISOR, SUPERVISOR_SOURCE.read_text())
        self.write(COMPOSITOR_SESSION, COMPOSITOR_SESSION_SOURCE.read_text())
        self.write("usr/lib/pam.d/cosmic-greeter", "auth substack system-auth\n")
        self.write(AGENT_UNIT, (USER_UNIT_SOURCES / "cosmic-osd.service").read_text())
        self.write(
            SESSION_TARGET, (USER_UNIT_SOURCES / "athanor-session.target").read_text()
        )
        self.write(
            SETTINGS_DAEMON_UNIT,
            (USER_UNIT_SOURCES / "cosmic-settings-daemon.service").read_text(),
        )
        self.write(PORTALS_CONF, (PORTAL_SOURCES / "athanor-portals.conf").read_text())
        self.write(
            f"{PORTALS}/athanor.portal", (PORTAL_SOURCES / "athanor.portal").read_text()
        )
        self.write(f"{PORTALS}/gtk.portal", GTK_PORTAL_TEXT)
        self.write(f"{PORTALS}/gnome-keyring.portal", KEYRING_PORTAL_TEXT)
        for name in (
            "org.freedesktop.impl.portal.desktop.athanor",
            "org.freedesktop.impl.portal.desktop.gtk",
            "org.freedesktop.secrets",
        ):
            self.write(f"{DBUS_SERVICES}/{name}.service", activation(name))
            self.write(f"usr/libexec/{name}", "")
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

    def test_each_missing_binary_fails(self):
        for binary in (
            "usr/bin/cosmic-greeter",
            "usr/bin/cosmic-idle",
            "usr/bin/cosmic-osd",
        ):
            with self.subTest(binary=binary):
                (self.root / binary).unlink()
                self.assertProblem(f"/{binary} is missing")
                self.write(binary, "")

    def test_an_assignment_with_spaces_around_the_equals_sign_counts(self):
        self.write(
            f"{AGENT_ETC_DROPINS}/50-x.conf", "[Service]\n  NoNewPrivileges = no  \n"
        )
        self.assertProblem("NoNewPrivileges=true")

    def test_a_setting_outside_the_service_section_is_ignored(self):
        self.write(f"{AGENT_ETC_DROPINS}/50-x.conf", "[Unit]\nNoNewPrivileges=no\n")
        self.assertEqual(mod.problems(self.root), [])

    def test_a_session_target_that_wants_the_agent_outside_unit_fails(self):
        self.write(SESSION_TARGET, "[Install]\nWants=cosmic-osd.service\n")
        self.assertProblem("does not want cosmic-osd.service")

    def test_a_missing_launcher_fails(self):
        (self.root / LAUNCHER).unlink()
        self.assertProblem("/usr/bin/athanor-desktop is missing")

    def test_a_launcher_that_does_not_start_the_supervisor_fails(self):
        self.edit(LAUNCHER, 'session_components "$$" &\n', "")
        self.assertProblem('session_components "$$" &')

    def test_a_launcher_that_does_not_source_the_supervisor_fails(self):
        self.edit(LAUNCHER, ". /usr/libexec/athanor/session-components\n", "")
        self.assertProblem(". /usr/libexec/athanor/session-components")

    def test_a_missing_supervisor_fails(self):
        (self.root / SUPERVISOR).unlink()
        self.assertProblem("/usr/libexec/athanor/session-components is missing")

    def test_a_supervisor_without_the_locker_scope_fails(self):
        self.edit(
            SUPERVISOR, "--unit=cosmic-greeter-locker.scope", "--unit=other.scope"
        )
        self.assertProblem("cosmic-greeter-locker.scope")

    def test_a_commented_launcher_line_fails(self):
        self.edit(
            LAUNCHER,
            'session_components "$$" &',
            '# session_components "$$" &',
        )
        self.assertProblem('session_components "$$" &')

    def test_a_commented_supervisor_line_fails(self):
        self.edit(
            SUPERVISOR,
            "systemd-run --user --scope --collect --property=TimeoutStopSec=5s --unit=cosmic-idle.scope cosmic-idle &",
            "# systemd-run --user --scope --collect --property=TimeoutStopSec=5s --unit=cosmic-idle.scope cosmic-idle &",
        )
        self.assertProblem("cosmic-idle.scope")

    def test_a_compositor_session_that_does_not_start_the_supervisor_fails(self):
        self.edit(COMPOSITOR_SESSION, 'session_components "$$" target < /dev/null &\n', "")
        self.assertProblem('session_components "$$" target < /dev/null &')

    def test_an_image_without_the_compositor_passes(self):
        (self.root / COMPOSITOR_SESSION).unlink()
        self.assertEqual(mod.problems(self.root), [])

    def test_an_image_with_the_compositor_but_no_session_script_fails(self):
        self.write("usr/bin/athanor-compositor", "")
        (self.root / COMPOSITOR_SESSION).unlink()
        self.assertProblem("/usr/bin/athanor-compositor-session is missing")

    def test_a_commented_restart_wrapper_fails(self):
        self.edit(
            SUPERVISOR,
            'session_component "$anchor" cosmic-greeter \\',
            '# session_component "$anchor" cosmic-greeter \\',
        )
        self.assertProblem('session_component "$anchor" cosmic-greeter')

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
        self.assertProblem(READ_ONLY_TALLY)

    def test_a_later_drop_in_that_resets_read_write_paths_fails(self):
        self.write(f"{HELPER_ETC_DROPINS}/90-x.conf", "[Service]\nReadWritePaths=\n")
        self.assertProblem(READ_ONLY_TALLY)

    def test_an_etc_drop_in_of_the_same_name_replaces_the_faillock_one(self):
        self.write(f"{HELPER_ETC_DROPINS}/50-athanor-faillock.conf", "[Service]\n")
        self.assertProblem(READ_ONLY_TALLY)

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
        self.assertProblem(READ_ONLY_TALLY)

    def test_a_helper_without_protect_system_strict_needs_no_drop_in(self):
        (self.root / HELPER_LIB_DROPINS / "50-athanor-faillock.conf").unlink()
        self.edit(HELPER_UNIT, "ProtectSystem=strict", "ProtectSystem=full")
        self.assertEqual(mod.problems(self.root), [])

    def faillock_dropin(self, text):
        self.write(f"{HELPER_LIB_DROPINS}/50-athanor-faillock.conf", text)

    def test_the_faillock_drop_in_outside_the_service_section_fails(self):
        self.faillock_dropin("[Unit]\nReadWritePaths=/run/faillock\n")
        self.assertProblem(READ_ONLY_TALLY)

    def test_a_later_protect_system_decides(self):
        (self.root / HELPER_LIB_DROPINS / "50-athanor-faillock.conf").unlink()
        self.write(f"{HELPER_ETC_DROPINS}/90-x.conf", "[Service]\nProtectSystem=full\n")
        self.assertEqual(mod.problems(self.root), [])
        self.edit(HELPER_UNIT, "ProtectSystem=strict", "ProtectSystem=full")
        self.write(
            f"{HELPER_ETC_DROPINS}/90-x.conf", "[Service]\nProtectSystem=strict\n"
        )
        self.assertProblem(READ_ONLY_TALLY)

    def test_read_write_paths_add_up_across_drop_ins(self):
        self.write(
            f"{HELPER_ETC_DROPINS}/90-x.conf", "[Service]\nReadWritePaths=/var/tmp\n"
        )
        self.assertEqual(mod.problems(self.root), [])

    def test_a_read_only_or_inaccessible_tally_fails(self):
        for key in ("ReadOnlyPaths", "InaccessiblePaths"):
            for entry in ("/run/faillock", "/run/faillock/", "-/run//faillock"):
                with self.subTest(key=key, entry=entry):
                    self.write(
                        f"{HELPER_ETC_DROPINS}/90-x.conf", f"[Service]\n{key}={entry}\n"
                    )
                    self.assertProblem(READ_ONLY_TALLY)

    def test_a_deeper_read_write_path_wins_over_a_read_only_parent(self):
        self.write(f"{HELPER_ETC_DROPINS}/90-x.conf", "[Service]\nReadOnlyPaths=/run\n")
        self.assertEqual(mod.problems(self.root), [])

    def test_a_deeper_read_only_path_wins_over_a_writable_parent(self):
        self.faillock_dropin(
            "[Service]\nReadWritePaths=/run\nReadOnlyPaths=/run/faillock\n"
        )
        self.assertProblem(READ_ONLY_TALLY)

    def test_slashes_are_normalised(self):
        for entry in ("//run//faillock", "/run/faillock//", "+/run/faillock"):
            with self.subTest(entry=entry):
                self.faillock_dropin(f"[Service]\nReadWritePaths={entry}\n")
                self.assertEqual(mod.problems(self.root), [])

    def test_the_runtime_directory_specifier_expands_to_run(self):
        self.faillock_dropin("[Service]\nReadWritePaths=%t/faillock\n")
        self.assertEqual(mod.problems(self.root), [])

    def test_another_specifier_fails_as_unsupported(self):
        self.faillock_dropin("[Service]\nReadWritePaths=%S/faillock\n")
        self.assertProblem("specifier other than %t")

    def test_a_masked_helper_unit_fails(self):
        for d in ("etc/systemd/system", "run/systemd/system"):
            with self.subTest(dir=d):
                mask = self.root / d / "polkit-agent-helper@.service"
                mask.parent.mkdir(parents=True, exist_ok=True)
                mask.symlink_to("/dev/null")
                self.assertProblem("polkit-agent-helper@.service is masked")
                mask.unlink()

    def test_an_etc_helper_unit_replaces_the_usr_lib_one(self):
        self.write(
            "etc/systemd/system/polkit-agent-helper@.service",
            HELPER_UNIT_TEXT.replace("strict", "full"),
        )
        (self.root / HELPER_LIB_DROPINS / "50-athanor-faillock.conf").unlink()
        self.assertEqual(mod.problems(self.root), [])

    def test_an_etc_helper_unit_replaces_the_run_one(self):
        (self.root / HELPER_LIB_DROPINS / "50-athanor-faillock.conf").unlink()
        relaxed = HELPER_UNIT_TEXT.replace("strict", "full")
        self.write("run/systemd/system/polkit-agent-helper@.service", HELPER_UNIT_TEXT)
        self.write("etc/systemd/system/polkit-agent-helper@.service", relaxed)
        self.assertEqual(mod.problems(self.root), [])
        self.write("run/systemd/system/polkit-agent-helper@.service", relaxed)
        self.write("etc/systemd/system/polkit-agent-helper@.service", HELPER_UNIT_TEXT)
        self.assertProblem(READ_ONLY_TALLY)

    def test_a_run_drop_in_counts(self):
        self.write(
            f"{HELPER_RUN_DROPINS}/90-x.conf",
            "[Service]\nReadOnlyPaths=/run/faillock\n",
        )
        self.assertProblem(READ_ONLY_TALLY)

    def test_an_etc_drop_in_replaces_the_run_one_of_the_same_name(self):
        self.write(
            f"{HELPER_RUN_DROPINS}/90-x.conf",
            "[Service]\nReadOnlyPaths=/run/faillock\n",
        )
        self.write(f"{HELPER_ETC_DROPINS}/90-x.conf", "[Service]\n")
        self.assertEqual(mod.problems(self.root), [])
        self.write(f"{HELPER_RUN_DROPINS}/90-x.conf", "[Service]\n")
        self.write(
            f"{HELPER_ETC_DROPINS}/90-x.conf",
            "[Service]\nReadOnlyPaths=/run/faillock\n",
        )
        self.assertProblem(READ_ONLY_TALLY)

    def test_an_inaccessible_parent_fails_whatever_its_depth(self):
        for entry in ("/run", "/"):
            with self.subTest(entry=entry):
                self.faillock_dropin(
                    f"[Service]\nReadWritePaths=/run/faillock\nInaccessiblePaths={entry}\n"
                )
                self.assertProblem(READ_ONLY_TALLY)

    def test_a_relative_path_is_dropped(self):
        self.faillock_dropin("[Service]\nReadWritePaths=run/faillock\n")
        self.assertProblem(READ_ONLY_TALLY)

    def test_a_sibling_with_a_common_prefix_does_not_cover(self):
        self.faillock_dropin("[Service]\nReadWritePaths=/run/fail\n")
        self.assertProblem(READ_ONLY_TALLY)

    def test_an_indented_drop_in_counts(self):
        self.faillock_dropin("  [Service]\n  ReadWritePaths=/run/faillock\n")
        self.assertEqual(mod.problems(self.root), [])

    def test_the_root_covers_at_depth_zero(self):
        self.faillock_dropin("[Service]\nReadWritePaths=/\n")
        self.assertEqual(mod.problems(self.root), [])
        self.faillock_dropin("[Service]\nReadWritePaths=/\nReadOnlyPaths=/run\n")
        self.assertProblem(READ_ONLY_TALLY)
        self.faillock_dropin("[Service]\nReadWritePaths=/run\nReadOnlyPaths=/\n")
        self.assertEqual(mod.problems(self.root), [])

    def test_the_deepest_of_several_entries_counts(self):
        self.faillock_dropin(
            "[Service]\nReadWritePaths=/run /run/faillock\nReadOnlyPaths=/run\n"
        )
        self.assertEqual(mod.problems(self.root), [])
        self.faillock_dropin(
            "[Service]\nReadWritePaths=/run/faillock\nReadOnlyPaths=/run /run/faillock\n"
        )
        self.assertProblem(READ_ONLY_TALLY)

    def test_a_settings_daemon_without_its_runtime_directory_fails(self):
        self.edit(SETTINGS_DAEMON_UNIT, "ReadWritePaths=%t\n", "")
        self.assertProblem(READ_ONLY_RUNTIME)

    def test_a_settings_daemon_drop_in_that_resets_read_write_paths_fails(self):
        self.write(
            f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf", "[Service]\nReadWritePaths=\n"
        )
        self.assertProblem(READ_ONLY_RUNTIME)

    def test_a_settings_daemon_runtime_directory_made_read_only_fails(self):
        self.write(
            f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf", "[Service]\nReadOnlyPaths=%t\n"
        )
        self.assertProblem(READ_ONLY_RUNTIME)

    def test_t_expands_to_the_user_runtime_directory_below_run(self):
        self.write(
            f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf",
            "[Service]\nReadOnlyPaths=/run\n",
        )
        self.assertEqual(mod.problems(self.root), [])

    def test_a_settings_daemon_without_protect_system_strict_passes(self):
        self.edit(SETTINGS_DAEMON_UNIT, "ReadWritePaths=%t\n", "")
        self.edit(SETTINGS_DAEMON_UNIT, "ProtectSystem=strict", "ProtectSystem=full")
        self.edit(SETTINGS_DAEMON_UNIT, "ProtectHome=read-only", "ProtectHome=no")
        self.assertEqual(mod.problems(self.root), [])

    def test_a_missing_or_masked_settings_daemon_fails(self):
        self.write("etc/systemd/user/cosmic-settings-daemon.service", "")
        self.assertProblem("cosmic-settings-daemon.service is masked")
        (self.root / "etc/systemd/user/cosmic-settings-daemon.service").unlink()
        (self.root / SETTINGS_DAEMON_UNIT).unlink()
        self.assertProblem(f"/{SETTINGS_DAEMON_UNIT} is missing")

    def test_a_missing_portals_conf_fails(self):
        (self.root / PORTALS_CONF).unlink()
        self.assertProblem(f"/{PORTALS_CONF} is missing")

    def test_a_file_chooser_routed_to_none_or_not_routed_fails(self):
        for line in ("org.freedesktop.impl.portal.FileChooser=none\n", ""):
            with self.subTest(line=line):
                self.write(
                    PORTALS_CONF,
                    (PORTAL_SOURCES / "athanor-portals.conf").read_text(),
                )
                self.edit(PORTALS_CONF, FILE_CHOOSER_LINE, line)
                self.assertProblem("FileChooser no backend")

    def test_a_wildcard_resolves_to_the_first_portal_file_that_lists_the_interface(
        self,
    ):
        self.edit(
            PORTALS_CONF,
            FILE_CHOOSER_LINE,
            "org.freedesktop.impl.portal.FileChooser=*\n",
        )
        self.assertEqual(mod.problems(self.root), [])
        # athanor.portal sorts before gtk.portal: once it lists FileChooser, `*` is validated
        # against it, and its bus name has no activation file.
        self.write(
            f"{PORTALS}/athanor.portal",
            "[portal]\nDBusName=org.absent\n"
            "Interfaces=org.freedesktop.impl.portal.FileChooser;\n",
        )
        found = mod.problems(self.root)
        self.assertTrue(
            any(
                f.startswith(
                    "org.freedesktop.impl.portal.FileChooser goes to athanor, and nothing activates"
                )
                for f in found
            ),
            found,
        )

    def test_a_wildcard_prefers_a_portal_file_used_in_the_session_desktop(self):
        self.edit(
            PORTALS_CONF,
            FILE_CHOOSER_LINE,
            "org.freedesktop.impl.portal.FileChooser=*\n",
        )
        # zz.portal sorts after gtk.portal by name, but UseIn=COSMIC puts it first.
        self.write(
            f"{PORTALS}/zz.portal",
            "[portal]\nDBusName=org.absent\nUseIn=gnome;COSMIC\n"
            "Interfaces=org.freedesktop.impl.portal.FileChooser;\n",
        )
        self.assertProblem("FileChooser goes to zz, and nothing activates")
        # Athanor comes before COSMIC in XDG_CURRENT_DESKTOP, so a file used in it wins.
        self.write(
            f"{PORTALS}/zzz.portal",
            "[portal]\nDBusName=org.absent\nUseIn=Athanor\n"
            "Interfaces=org.freedesktop.impl.portal.FileChooser;\n",
        )
        self.assertProblem("FileChooser goes to zzz, and nothing activates")

    def test_a_wildcard_that_no_portal_file_lists_fails(self):
        self.edit(
            PORTALS_CONF,
            FILE_CHOOSER_LINE,
            "org.freedesktop.impl.portal.FileChooser=*\n",
        )
        for rel in ("gtk.portal", "athanor.portal", "gnome-keyring.portal"):
            (self.root / PORTALS / rel).unlink()
        found = mod.problems(self.root)
        self.assertIn(
            "org.freedesktop.impl.portal.FileChooser goes to *, and no portal file lists it",
            found,
        )

    def test_a_wildcard_is_resolved_for_every_interface(self):
        self.edit(
            PORTALS_CONF,
            "org.freedesktop.impl.portal.Screenshot=none\n",
            "org.freedesktop.impl.portal.Screenshot=*\n",
        )
        self.assertProblem("Screenshot goes to *, and no portal file lists it")

    def test_cosmic_portals_conf_alone_is_read(self):
        none = (
            (PORTAL_SOURCES / "athanor-portals.conf")
            .read_text()
            .replace(
                FILE_CHOOSER_LINE, "org.freedesktop.impl.portal.FileChooser=none\n"
            )
        )
        (self.root / PORTALS_CONF).unlink()
        self.write("usr/share/xdg-desktop-portal/cosmic-portals.conf", none)
        self.assertProblem("FileChooser no backend")

    def test_the_data_directories_of_portals_conf_are_ranked(self):
        shipped = (PORTAL_SOURCES / "athanor-portals.conf").read_text()
        none = shipped.replace(
            FILE_CHOOSER_LINE, "org.freedesktop.impl.portal.FileChooser=none\n"
        )
        local = "usr/local/share/xdg-desktop-portal/athanor-portals.conf"
        defaults = (
            "usr/share/athanor/cosmic-defaults/xdg-desktop-portal/athanor-portals.conf"
        )
        self.write(local, none)
        self.assertProblem("FileChooser no backend")
        self.write(defaults, shipped)
        self.assertEqual(mod.problems(self.root), [])
        self.write(defaults, none)
        self.write(local, shipped)
        self.assertProblem("FileChooser no backend")

    def test_an_etc_portals_conf_overrides_the_shipped_one(self):
        shipped = (PORTAL_SOURCES / "athanor-portals.conf").read_text()
        none = shipped.replace(
            FILE_CHOOSER_LINE, "org.freedesktop.impl.portal.FileChooser=none\n"
        )
        for rel in (
            "etc/xdg-desktop-portal/athanor-portals.conf",
            "etc/xdg/xdg-desktop-portal/athanor-portals.conf",
            "etc/xdg-desktop-portal/portals.conf",
        ):
            with self.subTest(rel=rel):
                self.write(rel, none)
                self.assertProblem("FileChooser no backend")
                (self.root / rel).unlink()

    def test_the_portals_conf_search_order_decides(self):
        shipped = (PORTAL_SOURCES / "athanor-portals.conf").read_text()
        none = shipped.replace(
            FILE_CHOOSER_LINE, "org.freedesktop.impl.portal.FileChooser=none\n"
        )
        # /etc/xdg beats /etc, and a desktop-specific name beats portals.conf in one directory.
        self.write("etc/xdg-desktop-portal/athanor-portals.conf", none)
        self.write("etc/xdg/xdg-desktop-portal/athanor-portals.conf", shipped)
        self.assertEqual(mod.problems(self.root), [])
        self.write("etc/xdg/xdg-desktop-portal/athanor-portals.conf", none)
        self.write("etc/xdg-desktop-portal/athanor-portals.conf", shipped)
        self.assertProblem("FileChooser no backend")
        (self.root / "etc/xdg/xdg-desktop-portal/athanor-portals.conf").unlink()
        self.write("etc/xdg-desktop-portal/portals.conf", none)
        self.assertEqual(mod.problems(self.root), [])

    def test_a_file_chooser_routed_to_a_missing_backend_fails(self):
        self.edit(
            PORTALS_CONF,
            FILE_CHOOSER_LINE,
            "org.freedesktop.impl.portal.FileChooser=athanor-files\n",
        )
        self.assertProblem(f"/{PORTALS}/athanor-files.portal is missing")

    def test_an_interface_routed_to_a_backend_that_does_not_list_it_fails(self):
        self.edit(
            PORTALS_CONF,
            "org.freedesktop.impl.portal.Screenshot=none\n",
            "org.freedesktop.impl.portal.Screenshot=gtk\n",
        )
        self.assertProblem("Screenshot goes to gtk, whose portal file does not list it")

    def test_a_backend_the_bus_cannot_activate_fails(self):
        (self.root / DBUS_SERVICES / "org.freedesktop.secrets.service").unlink()
        self.assertProblem("nothing activates its bus name 'org.freedesktop.secrets'")

    def activation_file(self, name="org.freedesktop.secrets"):
        return f"{DBUS_SERVICES}/{name}.service"

    def test_an_activation_file_must_name_the_bus_name_and_a_present_binary(self):
        name = "org.freedesktop.secrets"
        words = f"nothing activates its bus name '{name}'"
        cases = {
            "wrong Name": "[D-BUS Service]\nName=org.other\nExec=/usr/libexec/"
            + name
            + "\n",
            "no Name": "[D-BUS Service]\nExec=/usr/libexec/" + name + "\n",
            "bare": "[D-BUS Service]\n",
            "missing binary": activation(name, "/usr/libexec/absent"),
            "no Exec": f"[D-BUS Service]\nName={name}\n",
            "missing unit": activation(
                name, "/usr/libexec/absent", "SystemdService=x.service\n"
            ),
        }
        for label, text in cases.items():
            with self.subTest(label):
                self.write(self.activation_file(), text)
                found = mod.problems(self.root)
                self.assertEqual(len(found), 1, found)
                self.assertIn(words, found[0])

    def test_an_activation_file_with_a_systemd_unit_or_arguments_passes(self):
        name = "org.freedesktop.secrets"
        self.write("usr/lib/systemd/user/x.service", "[Service]\n")
        self.write(
            self.activation_file(),
            activation(name, "/usr/libexec/absent", "SystemdService=x.service\n"),
        )
        self.assertEqual(mod.problems(self.root), [])
        self.write(
            self.activation_file(), activation(name, f"/usr/libexec/{name} --replace")
        )
        self.assertEqual(mod.problems(self.root), [])

    def test_a_present_exec_does_not_replace_a_missing_systemd_unit(self):
        name = "org.freedesktop.secrets"
        self.write(
            self.activation_file(), activation(name, extra="SystemdService=x.service\n")
        )
        self.assertProblem(f"nothing activates its bus name '{name}'")

    def test_a_masked_activation_unit_does_not_count(self):
        name = "org.freedesktop.secrets"
        self.write("usr/lib/systemd/user/x.service", "[Service]\n")
        (self.root / "etc/systemd/user").mkdir(parents=True, exist_ok=True)
        (self.root / "etc/systemd/user/x.service").symlink_to("/dev/null")
        self.write(
            self.activation_file(),
            activation(name, extra="SystemdService=x.service\n"),
        )
        self.assertProblem(f"nothing activates its bus name '{name}'")

    def test_an_empty_or_missing_dbus_name_fails(self):
        for text in (
            "[portal]\nInterfaces=org.freedesktop.impl.portal.Secret\n",
            "[portal]\nDBusName=\nInterfaces=org.freedesktop.impl.portal.Secret\n",
        ):
            with self.subTest(text=text):
                self.write(f"{PORTALS}/gnome-keyring.portal", text)
                self.assertProblem("nothing activates its bus name ''")

    def test_a_portal_file_without_interfaces_fails(self):
        self.write(
            f"{PORTALS}/gnome-keyring.portal",
            "[portal]\nDBusName=org.freedesktop.secrets\n",
        )
        self.assertProblem(
            "Secret goes to gnome-keyring, whose portal file does not list it"
        )

    def test_an_exec_start_pre_drop_in_does_not_override_exec_start(self):
        self.write(
            f"{AGENT_ETC_DROPINS}/60-y.conf", "[Service]\nExecStartPre=/bin/true\n"
        )
        self.assertEqual(mod.problems(self.root), [])

    def test_a_user_unit_in_a_higher_directory_replaces_the_usr_lib_one(self):
        daemon = "cosmic-settings-daemon.service"
        relaxed = (USER_UNIT_SOURCES / daemon).read_text().replace("strict", "full")
        for d in USER_DIRS[1:]:
            with self.subTest(dir=d):
                self.write(f"{d}/{daemon}", relaxed)
                self.edit(SETTINGS_DAEMON_UNIT, "ReadWritePaths=%t\n", "")
                self.assertEqual(mod.problems(self.root), [])
                (self.root / d / daemon).unlink()
                self.write(
                    SETTINGS_DAEMON_UNIT, (USER_UNIT_SOURCES / daemon).read_text()
                )

    def test_the_user_unit_directories_are_ranked_as_systemd_ranks_them(self):
        daemon = "cosmic-settings-daemon.service"
        good = (USER_UNIT_SOURCES / daemon).read_text()
        bad = good.replace("ReadWritePaths=%t\n", "")
        # Each directory beats every one before it in USER_DIRS and loses to every one after it.
        for i, high in enumerate(USER_DIRS[1:], 1):
            for low in USER_DIRS[:i]:
                with self.subTest(high=high, low=low):
                    self.write(f"{low}/{daemon}", bad)
                    self.write(f"{high}/{daemon}", good)
                    self.assertEqual(mod.problems(self.root), [])
                    self.write(f"{low}/{daemon}", good)
                    self.write(f"{high}/{daemon}", bad)
                    self.assertProblem(READ_ONLY_RUNTIME)
                    (self.root / high / daemon).unlink()
                    (self.root / low / daemon).unlink()
                    self.write(SETTINGS_DAEMON_UNIT, good)

    def test_a_dev_null_symlink_masks_a_user_unit_in_any_higher_directory(self):
        daemon = "cosmic-settings-daemon.service"
        for d in USER_DIRS[1:]:
            with self.subTest(dir=d):
                (self.root / d).mkdir(parents=True, exist_ok=True)
                (self.root / d / daemon).symlink_to("/dev/null")
                self.assertProblem(f"{daemon} is masked in /{d}")
                (self.root / d / daemon).unlink()

    def test_a_higher_real_unit_beats_a_lower_mask(self):
        daemon = "cosmic-settings-daemon.service"
        (self.root / "run/systemd/user").mkdir(parents=True)
        (self.root / "run/systemd/user" / daemon).symlink_to("/dev/null")
        self.write(
            f"etc/systemd/user/{daemon}", (USER_UNIT_SOURCES / daemon).read_text()
        )
        self.assertEqual(mod.problems(self.root), [])

    def test_a_usr_lib_drop_in_counts(self):
        self.write(
            "usr/lib/systemd/user/cosmic-settings-daemon.service.d/90-x.conf",
            "[Service]\nReadWritePaths=\n",
        )
        self.assertProblem(READ_ONLY_RUNTIME)

    def test_drop_ins_of_every_user_directory_count(self):
        for d in USER_DIRS[1:]:
            with self.subTest(dir=d):
                self.write(
                    f"{d}/cosmic-settings-daemon.service.d/90-x.conf",
                    "[Service]\nReadOnlyPaths=%t\n",
                )
                self.assertProblem(READ_ONLY_RUNTIME)
                (self.root / d / "cosmic-settings-daemon.service.d/90-x.conf").unlink()

    def test_a_user_drop_in_beats_the_same_name_in_a_lower_directory(self):
        name = "cosmic-settings-daemon.service.d/90-x.conf"
        for i, high in enumerate(USER_DIRS[1:], 1):
            for low in USER_DIRS[:i]:
                with self.subTest(high=high, low=low):
                    self.write(f"{low}/{name}", "[Service]\nReadWritePaths=\n")
                    self.write(f"{high}/{name}", "[Service]\n")
                    self.assertEqual(mod.problems(self.root), [])
                    self.write(f"{low}/{name}", "[Service]\n")
                    self.write(f"{high}/{name}", "[Service]\nReadWritePaths=\n")
                    self.assertProblem(READ_ONLY_RUNTIME)
                    (self.root / low / name).unlink()
                    (self.root / high / name).unlink()

    def test_a_replaced_agent_unit_fails_in_every_user_directory(self):
        for d in USER_DIRS[1:]:
            with self.subTest(dir=d):
                self.write(f"{d}/cosmic-osd.service", "[Service]\n")
                self.assertProblem("masks or replaces")
                (self.root / d / "cosmic-osd.service").unlink()

    def test_a_masked_agent_in_a_run_or_local_directory_fails(self):
        for d in (
            "run/systemd/user",
            "usr/local/lib/systemd/user",
            "etc/xdg/systemd/user",
        ):
            with self.subTest(dir=d):
                (self.root / d).mkdir(parents=True, exist_ok=True)
                (self.root / d / "cosmic-osd.service").symlink_to("/dev/null")
                self.assertProblem("masks or replaces")
                (self.root / d / "cosmic-osd.service").unlink()

    def test_a_hiding_protect_home_makes_the_runtime_directory_unusable(self):
        for value in ("yes", "y", "true", "t", "tmpfs", "on", "1"):
            with self.subTest(value=value):
                self.write(
                    f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf",
                    f"[Service]\nProtectHome={value}\n",
                )
                self.assertProblem(READ_ONLY_RUNTIME)

    def test_a_read_only_protect_home_without_strict_needs_the_runtime_directory(self):
        self.edit(SETTINGS_DAEMON_UNIT, "ProtectSystem=strict", "ProtectSystem=full")
        self.write(
            f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf",
            "[Service]\nProtectHome=Read-Only\n",
        )
        self.assertEqual(mod.problems(self.root), [])
        self.edit(SETTINGS_DAEMON_UNIT, "ReadWritePaths=%t\n", "")
        self.assertProblem(READ_ONLY_RUNTIME)

    def test_a_read_only_runtime_directory_without_strict_fails(self):
        self.edit(SETTINGS_DAEMON_UNIT, "ProtectSystem=strict", "ProtectSystem=full")
        self.edit(SETTINGS_DAEMON_UNIT, "ProtectHome=read-only", "ProtectHome=no")
        self.write(
            f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf",
            "[Service]\nReadWritePaths=\nReadOnlyPaths=%t\n",
        )
        self.assertProblem(READ_ONLY_RUNTIME)

    def test_protect_home_is_case_insensitive(self):
        self.write(
            f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf", "[Service]\nProtectHome=YES\n"
        )
        self.assertProblem(READ_ONLY_RUNTIME)

    def test_the_last_protect_home_decides(self):
        self.write(
            f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf",
            "[Service]\nProtectHome=yes\nProtectHome=read-only\n",
        )
        self.assertEqual(mod.problems(self.root), [])
        self.write(
            f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf",
            "[Service]\nProtectHome=no\nProtectHome=yes\n",
        )
        self.assertProblem(READ_ONLY_RUNTIME)

    def test_protect_home_does_not_touch_the_tally_directory(self):
        self.faillock_dropin(
            "[Service]\nReadWritePaths=/run/faillock\nProtectHome=yes\n"
        )
        self.assertEqual(mod.problems(self.root), [])

    def test_a_temporary_file_system_over_the_runtime_directory_fails(self):
        for entry in ("/run/user", "/run", "/run/user/UID:ro", "/"):
            with self.subTest(entry=entry):
                self.write(
                    f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf",
                    f"[Service]\nTemporaryFileSystem={entry}\n",
                )
                self.assertProblem(READ_ONLY_RUNTIME)

    def test_a_temporary_file_system_elsewhere_or_reset_passes(self):
        self.write(
            f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf",
            "[Service]\nTemporaryFileSystem=/var:ro\nTemporaryFileSystem=\n"
            "TemporaryFileSystem=/run/user/UID2:ro\n",
        )
        self.assertEqual(mod.problems(self.root), [])

    def test_protect_home_hides_a_path_below_home_or_root(self):
        for path in ("/home/user", "/root/x"):
            with self.subTest(path=path):
                unit = (
                    "[Service]\nProtectHome=yes\nProtectSystem=strict\nReadWritePaths="
                    + path
                )
                self.write("usr/lib/systemd/user/probe.service", unit + "\n")
                found = mod.writable_problems(
                    self.root,
                    USER_DIRS,
                    "probe.service",
                    path,
                    "/run/user/UID",
                    "m",
                    "r",
                )
                self.assertEqual(len(found), 1, found)

    def test_hiding_does_not_depend_on_protect_system(self):
        for text in (
            "[Service]\nProtectSystem=full\nProtectHome=yes\n",
            "[Service]\nTemporaryFileSystem=/run/user\n",
        ):
            with self.subTest(text=text):
                self.write(f"{SETTINGS_DAEMON_ETC_DROPINS}/90-x.conf", text)
                self.edit(
                    SETTINGS_DAEMON_UNIT, "ProtectSystem=strict", "ProtectSystem=full"
                )
                self.assertProblem(READ_ONLY_RUNTIME)
                self.edit(SETTINGS_DAEMON_UNIT, "ProtectSystem=full\n", "")
                self.assertProblem(READ_ONLY_RUNTIME)
                self.write(
                    SETTINGS_DAEMON_UNIT,
                    (USER_UNIT_SOURCES / "cosmic-settings-daemon.service").read_text(),
                )

    def test_a_dev_null_unit_in_the_lowest_directory_is_missing_not_masked(self):
        # The lowest directory holds the packaged unit; a null link there is no override.
        daemon = self.root / SETTINGS_DAEMON_UNIT
        daemon.unlink()
        daemon.symlink_to("/dev/null")
        self.assertProblem(f"/{SETTINGS_DAEMON_UNIT} is missing")

    def test_the_system_unit_directories_are_ranked(self):
        relaxed = HELPER_UNIT_TEXT.replace("strict", "full")
        (self.root / HELPER_LIB_DROPINS / "50-athanor-faillock.conf").unlink()
        self.write(HELPER_UNIT, relaxed)
        self.write(
            "usr/local/lib/systemd/system/polkit-agent-helper@.service",
            HELPER_UNIT_TEXT,
        )
        self.assertProblem(READ_ONLY_TALLY)
        self.write("run/systemd/system/polkit-agent-helper@.service", relaxed)
        self.assertEqual(mod.problems(self.root), [])

    def test_the_enabled_greeter_daemon_fails(self):
        self.enable(
            "cosmic-greeter-daemon.service",
            "usr/lib/systemd/system/multi-user.target.wants",
        )
        self.assertProblem("cosmic-greeter-daemon.service is enabled")


class SchemaStubTest(unittest.TestCase):
    """schema_problems with stub glib-compile-schemas and gsettings first on PATH, so these run
    on any machine."""

    VALUES = {
        "accent-color": "'purple'",
        "icon-theme": "'Adwaita'",
        "accent-mode": "'fixed'",
        "display-mode": "'disabled'",
    }

    def stub(self, name, body):
        path = self.bin / name
        path.write_text(f"#!/bin/sh\n{body}\n")
        path.chmod(0o755)

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.bin = pathlib.Path(tmp.name)
        patcher = mock.patch.dict(
            os.environ, {"PATH": f"{self.bin}{os.pathsep}{os.environ['PATH']}"}
        )
        patcher.start()
        self.addCleanup(patcher.stop)
        self.stub("glib-compile-schemas", "exit 0")
        self.stub(
            "gsettings",
            'case "$3" in\n'
            + "\n".join(f'{k}) echo "{v}";;' for k, v in self.VALUES.items())
            + "\nesac",
        )

    def test_good_schemas_give_no_finding(self):
        self.assertEqual(mod.schema_problems("/schemas"), [])

    def test_a_failed_compile_is_a_finding(self):
        self.stub("glib-compile-schemas", "echo bad key >&2; exit 1")
        found = mod.schema_problems("/schemas")
        self.assertEqual(len(found), 1)
        self.assertIn(
            "glib-compile-schemas --strict --dry-run /schemas fails", found[0]
        )
        self.assertIn("bad key", found[0])

    def test_a_mismatched_readback_is_a_finding(self):
        self.stub("gsettings", "echo \"'blue'\"")
        found = mod.schema_problems("/schemas")
        self.assertEqual(len(found), 4)
        self.assertIn("accent-color gives \"'blue'\", not 'purple'", found[0])

    def test_a_failed_gsettings_is_a_finding(self):
        self.stub("gsettings", "echo no schema >&2; exit 1")
        found = mod.schema_problems("/schemas")
        self.assertEqual(len(found), 4)
        self.assertIn("gives 'no schema'", found[0])

    def test_a_missing_tool_is_a_finding(self):
        (self.bin / "gsettings").unlink()
        with mock.patch.dict(os.environ, {"PATH": str(self.bin)}):
            found = mod.schema_problems("/schemas")
        self.assertEqual(len(found), 1)
        self.assertIn("gsettings cannot run in the image", found[0])


class SchemaMainTest(unittest.TestCase):
    def test_schemas_are_checked_only_for_the_running_image(self):
        with (
            tempfile.TemporaryDirectory() as tmp,
            mock.patch.object(mod, "problems", return_value=[]),
            mock.patch.object(mod, "schema_problems", return_value=[]) as schemas,
        ):
            self.assertEqual(mod.main(["x", tmp]), 0)
            schemas.assert_not_called()
            self.assertEqual(mod.main(["x", "/"]), 0)
            schemas.assert_called_once_with()
            schemas.return_value = ["broken"]
            self.assertEqual(mod.main(["x"]), 1)


class SchemaTest(unittest.TestCase):
    """schema_problems with the real glib tools, on the schemas of the host plus the shipped
    ones; skipped where they are not installed."""

    SHIPPED = SOURCES / "athanor-system-config/SOURCES/usr/share/glib-2.0/schemas"

    def setUp(self):
        if not (
            shutil.which("glib-compile-schemas")
            and shutil.which("gsettings")
            and pathlib.Path("/usr/share/glib-2.0/schemas").is_dir()
        ):
            self.skipTest("glib tools or the base schemas are not installed")
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = pathlib.Path(tmp.name)
        base = pathlib.Path("/usr/share/glib-2.0/schemas")
        for name in (
            "org.gnome.desktop.enums.xml",
            "org.gnome.desktop.interface.gschema.xml",
            "org.gnome.desktop.wm.preferences.gschema.xml",
            "org.gnome.system.gvfs.enums.xml",
            "org.gnome.system.wsdd.gschema.xml",
        ):
            if not (base / name).is_file():
                self.skipTest(f"{name} is not installed")
            shutil.copy(base / name, self.dir)
        for f in self.SHIPPED.iterdir():
            shutil.copy(f, self.dir)
        self.compile()

    def compile(self):
        # What glib's transfiletriggerin does in the image; gsettings reads the compiled file.
        subprocess.run(["glib-compile-schemas", str(self.dir)], check=True)

    def test_the_shipped_schemas_pass(self):
        self.assertEqual(mod.schema_problems(str(self.dir)), [])

    def test_an_override_of_a_missing_key_fails_the_compile(self):
        with open(self.dir / "90_athanor-appearance.gschema.override", "a") as f:
            f.write("[org.gnome.desktop.interface]\no-such-key='x'\n")
        found = mod.schema_problems(str(self.dir))
        self.assertEqual(len(found), 1)
        self.assertIn("glib-compile-schemas --strict --dry-run", found[0])

    def test_a_changed_default_fails_the_readback(self):
        override = self.dir / "90_athanor-appearance.gschema.override"
        override.write_text(override.read_text().replace("'purple'", "'blue'"))
        self.compile()
        found = mod.schema_problems(str(self.dir))
        self.assertEqual(len(found), 1)
        self.assertIn("accent-color gives \"'blue'\", not 'purple'", found[0])


if __name__ == "__main__":
    unittest.main()
