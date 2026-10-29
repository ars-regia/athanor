"""Unit tests of the session-units check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

TARGET = (
    "[Unit]\nBindsTo=graphical-session.target\nBefore=xdg-desktop-autostart.target\n"
    "Requires=athanor-desktop.service\nWants=x-wanted.service\n"
)
SURFACE = (
    "[Unit]\nAfter=graphical-session.target athanor-desktop.service\n"
    "Requisite=athanor-desktop.service\nPartOf=graphical-session.target\n"
    "StartLimitIntervalSec=10min\nStartLimitBurst=10\n"
    "[Service]\nType=notify\nRestart=on-failure\nRestartSec=100ms\n"
    "[Install]\nWantedBy=athanor-session.target\n"
)
BAR = SURFACE.replace(
    "After=graphical-session.target athanor-desktop.service\n",
    "Wants=athanor-shelld.service\n"
    "After=graphical-session.target athanor-desktop.service athanor-shelld.service\n",
)
GATE = "[Unit]\nPartOf=graphical-session.target\n[Service]\nType=oneshot\nRemainAfterExit=yes\n"
HELPER = "[Unit]\nPartOf=graphical-session.target\n[Service]\nType=dbus\n"
TABLE = {
    "athanor-bar.service": ("surface", True, "preset"),
    "athanor-desktop.service": ("step", False, "target"),
    "x-helper.service": ("helper", False, "activated"),
}


def problems(units, enabled=("athanor-bar.service",), table=None):
    shipped = {
        "athanor-session.target": TARGET,
        "athanor-bar.service": BAR,
        "athanor-desktop.service": GATE,
        "x-helper.service": HELPER,
    }
    shipped.update(units)
    with (
        mock.patch.dict(verify.SESSION_UNITS, table or TABLE, clear=True),
        mock.patch.dict(verify.NOT_SESSION_UNITS, {}, clear=True),
    ):
        return [key for key, _ in verify.session_unit_problems(shipped, set(enabled))]


class SessionUnitsTest(unittest.TestCase):
    def test_units_that_follow_the_rules_pass(self):
        self.assertEqual(problems({}), [])

    def test_a_unit_without_the_binding_fails(self):
        bar = BAR.replace("PartOf=graphical-session.target\n", "")
        self.assertEqual(
            problems({"athanor-bar.service": bar}), [("athanor-bar.service", "binding")]
        )

    def test_an_eager_unit_under_another_target_fails(self):
        bar = BAR.replace(
            "WantedBy=athanor-session.target", "WantedBy=graphical-session.target"
        )
        self.assertIn(
            ("athanor-bar.service", "install"), problems({"athanor-bar.service": bar})
        )

    def test_an_eager_unit_no_preset_enables_fails(self):
        self.assertEqual(problems({}, enabled=()), [("athanor-bar.service", "install")])

    def test_an_activated_unit_with_an_install_section_fails(self):
        helper = HELPER + "[Install]\nWantedBy=athanor-session.target\n"
        self.assertEqual(
            problems({"x-helper.service": helper}), [("x-helper.service", "install")]
        )

    def test_a_unit_the_target_does_not_pull_in_fails(self):
        target = TARGET.replace("Requires=athanor-desktop.service\n", "")
        self.assertEqual(
            problems({"athanor-session.target": target}),
            [("athanor-desktop.service", "install")],
        )

    def test_a_graphical_unit_needs_both_edges_to_the_gate(self):
        bar = BAR.replace("Requisite=athanor-desktop.service\n", "")
        self.assertEqual(
            problems({"athanor-bar.service": bar}), [("athanor-bar.service", "edge")]
        )

    def test_a_headless_unit_has_no_edge_to_the_gate(self):
        helper = HELPER.replace("[Service]", "After=athanor-desktop.service\n[Service]")
        self.assertEqual(
            problems({"x-helper.service": helper}), [("x-helper.service", "edge")]
        )

    def test_a_surface_with_a_slow_or_stepped_restart_fails(self):
        for change in ("RestartSec=1s\n", "RestartSec=100ms\nRestartSteps=5\n"):
            with self.subTest(change=change):
                bar = BAR.replace("RestartSec=100ms\n", change)
                self.assertEqual(
                    problems({"athanor-bar.service": bar}),
                    [("athanor-bar.service", "class")],
                )

    def test_a_surface_without_its_start_limit_fails(self):
        for line in ("StartLimitBurst=10\n", "StartLimitIntervalSec=10min\n"):
            with self.subTest(line=line):
                bar = BAR.replace(line, "")
                self.assertEqual(problems({"athanor-bar.service": bar}), [("athanor-bar.service", "class")])

    def test_a_restart_setting_outside_the_service_section_does_not_count(self):
        bar = BAR.replace("Restart=on-failure\n", "").replace("[Unit]\n", "[Unit]\nRestart=on-failure\n")
        self.assertEqual(problems({"athanor-bar.service": bar}), [("athanor-bar.service", "class")])

    def test_an_activated_unit_a_preset_enables_fails(self):
        self.assertEqual(
            problems({}, enabled=("athanor-bar.service", "x-helper.service")),
            [("x-helper.service", "install")],
        )

    def test_a_preset_enabling_an_exempt_activated_unit_still_fails(self):
        helper = HELPER + "[Install]\nWantedBy=athanor-session.target\n"
        exempt = {("x-helper.service", verify.ACTIVATED_INSTALL): "why"}
        for enabled, expected in (((), []), (("x-helper.service",), 1)):
            with self.subTest(enabled=enabled):
                with (
                    mock.patch.dict(verify.SESSION_UNITS, TABLE, clear=True),
                    mock.patch.dict(verify.NOT_SESSION_UNITS, {}, clear=True),
                    mock.patch.dict(verify.SESSION_EXEMPT, exempt, clear=True),
                ):
                    shipped = {
                        "athanor-session.target": TARGET,
                        "athanor-bar.service": BAR,
                        "athanor-desktop.service": GATE,
                        "x-helper.service": helper,
                    }
                    found = verify.session_exempt_problems(
                        verify.session_unit_problems(
                            shipped, {"athanor-bar.service", *enabled}
                        )
                    )
                if expected == []:
                    self.assertEqual(found, [])
                else:
                    self.assertEqual(len(found), expected)
                    self.assertIn(verify.ACTIVATED_PRESET, found[0])

    def test_time_spans_compare_in_seconds(self):
        bar = BAR.replace("StartLimitIntervalSec=10min", "StartLimitIntervalSec=600")
        self.assertEqual(problems({"athanor-bar.service": bar}), [])

    def test_a_helper_that_restarts_fails(self):
        helper = HELPER + "Restart=on-failure\n"
        self.assertEqual(
            problems({"x-helper.service": helper}), [("x-helper.service", "class")]
        )

    def test_a_step_that_is_not_a_oneshot_fails(self):
        gate = GATE.replace("Type=oneshot", "Type=simple")
        self.assertEqual(
            problems({"athanor-desktop.service": gate}),
            [("athanor-desktop.service", "class")],
        )

    def test_the_bar_wants_and_follows_shelld(self):
        self.assertEqual(
            problems({"athanor-bar.service": SURFACE}),
            [("athanor-bar.service", "order")],
        )

    def test_the_target_binds_to_the_graphical_session_before_autostart(self):
        for line in (
            "BindsTo=graphical-session.target\n",
            "Before=xdg-desktop-autostart.target\n",
        ):
            with self.subTest(line=line):
                target = TARGET.replace(line, "")
                self.assertEqual(
                    problems({"athanor-session.target": target}),
                    [("athanor-session.target", "order")],
                )

    def test_an_unclassified_or_unshipped_unit_fails(self):
        self.assertEqual(
            problems({"x-new.service": HELPER}), [("x-new.service", "class")]
        )
        table = dict(TABLE, **{"x-gone.service": ("surface", True, "preset")})
        self.assertEqual(problems({}, table=table), [("x-gone.service", "class")])


class ExemptTest(unittest.TestCase):
    FOUND = [(("x.service", "class"), "RestartSec=30, too slow")]

    def exempt_problems(self, exempt):
        with mock.patch.dict(verify.SESSION_EXEMPT, exempt, clear=True):
            return verify.session_exempt_problems(self.FOUND)

    def test_an_exempt_finding_passes(self):
        self.assertEqual(self.exempt_problems({("x.service", "RestartSec=30, too slow"): "why"}), [])

    def test_an_exemption_covers_its_own_finding_only(self):
        found = self.exempt_problems({("x.service", "RestartSteps=5, too many"): "why"})
        self.assertEqual(len(found), 2)
        self.assertIn("RestartSec=30", found[0])
        self.assertIn("remove the entry", found[1])

    def test_the_tree_s_exemptions_all_match(self):
        units, _ = verify.shipped_user_units(verify.ROOT)
        enabled = {unit for verb, unit, _ in verify.preset_lines(verify.ROOT) if verb == "enable"}
        self.assertEqual(
            verify.session_exempt_problems(verify.session_unit_problems(units, enabled)), []
        )


class TimeSpanTest(unittest.TestCase):
    def test_spans(self):
        for value, seconds in (
            ("100ms", 0.1),
            ("2s", 2),
            ("600", 600),
            ("10min", 600),
            ("1min 30s", 90),
        ):
            with self.subTest(value=value):
                self.assertEqual(verify.time_span(value), seconds)

    def test_an_unknown_unit_is_not_a_span(self):
        self.assertIsNone(verify.time_span("5parsecs"))


class SpecFilesTest(unittest.TestCase):
    def files(self, *lines):
        return verify.spec_user_units("%files\n" + "\n".join(lines) + "\n%changelog\n")

    def test_the_forms_of_a_listed_unit(self):
        for line in (
            "/usr/lib/systemd/user/a.service",
            "%attr(0644,root,root) /usr/lib/systemd/user/a.service",
            "%config(noreplace) %attr(0644,root,root) /usr/lib/systemd/user/a.service",
            "%config /usr/lib/systemd/user/a.service",
            "%verify(not md5 size mtime) /usr/lib/systemd/user/a.service",
            "%{_userunitdir}/a.service",
            "  /usr/lib/systemd/user/a.service  ",
        ):
            with self.subTest(line=line):
                self.assertEqual(self.files(line), (["a.service"], []))

    def test_a_line_it_cannot_classify_is_reported(self):
        for line in (
            "/usr/lib/systemd/user/*.service",
            "%{_userunitdir}/*",
            "/usr/lib/systemd/user/a.service.bak",
            "%{?_userunitdir}/a.service",
            "/usr/lib/systemd/user/sub/a.service",
        ):
            with self.subTest(line=line):
                names, found = self.files(line)
                self.assertEqual((names, found), ([], [line]))

    def test_any_other_section_ends_the_files_list(self):
        for section in ("%verifyscript", "%post", "%sepolicy", "%generate_buildrequires"):
            with self.subTest(section=section):
                text = f"%files\n/usr/lib/systemd/user/a.service\n{section}\n/usr/lib/systemd/user/c.service\n"
                self.assertEqual(verify.spec_user_units(text), (["a.service"], []))

    def test_an_unbraced_macro_line_does_not_end_the_files_list(self):
        for first in ("%_bindir/x", "%docdir /x", "%ifos linux"):
            with self.subTest(first=first):
                text = f"%files\n{first}\n/usr/lib/systemd/user/a.service\n"
                self.assertEqual(verify.spec_user_units(text), (["a.service"], []))

    def test_conditionals_stay_in_the_files_list(self):
        text = "%files\n%if 1\n/usr/lib/systemd/user/a.service\n%endif\n"
        self.assertEqual(verify.spec_user_units(text), (["a.service"], []))

    def test_another_directory_macro_is_reported(self):
        self.assertEqual(self.files("%{systemd_user_dir}/a.service"), ([], ["%{systemd_user_dir}/a.service"]))

    def test_what_is_not_a_user_unit_is_left_alone(self):
        text = (
            "%install\ninstall -D a /usr/lib/systemd/user/a.service\n"
            "%files\n/usr/lib/systemd/user-preset/80-a.preset\n"
            "/usr/lib/systemd/system/b.service\n"
            "%dir /usr/lib/systemd/user/x.socket.d\n"
            "/usr/lib/systemd/user/x.socket.d/50-a.conf\n"
            "# /usr/lib/systemd/user/*.service\n"
            "%changelog\n- /usr/lib/systemd/user/*.service\n"
        )
        self.assertEqual(verify.spec_user_units(text), ([], []))

    def test_an_unclassified_line_fails_the_check(self):
        with mock.patch.object(verify, "read", return_value="%files\n/usr/lib/systemd/user/*.service\n"):
            _, found = verify.shipped_user_units(verify.ROOT)
        self.assertTrue(found and "cannot tell which user unit" in found[0])


class ShippedTest(unittest.TestCase):
    def test_every_unit_the_tree_ships_is_found_once(self):
        units, found = verify.shipped_user_units(verify.ROOT)
        self.assertEqual(found, [])
        self.assertIn("athanor-desktop.service", units)
        self.assertIn("athanor-session.target", units)


if __name__ == "__main__":
    unittest.main()
