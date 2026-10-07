"""Unit tests of the service hardening check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

UNIT = "[Unit]\nDescription=x\n\n[Service]\nExecStart=/usr/bin/x\n"
PATH = "pkg/SOURCES/usr/lib/systemd/system/x.service"


def problems(files, exempt=None):
    with mock.patch.dict(verify.SERVICE_EXEMPT, exempt or {}, clear=True):
        return verify.service_problems(files)


class ServicesTest(unittest.TestCase):
    def test_a_unit_without_either_setting_fails(self):
        self.assertEqual(len(problems({PATH: UNIT})), 1)

    def test_no_new_privileges_in_any_true_spelling_passes(self):
        for value in ("yes", "true", "on", "1", "Yes"):
            with self.subTest(value=value):
                self.assertEqual(
                    problems({PATH: UNIT + f"NoNewPrivileges={value}\n"}), []
                )

    def test_no_new_privileges_false_fails(self):
        self.assertEqual(len(problems({PATH: UNIT + "NoNewPrivileges=no\n"})), 1)

    def test_a_setting_outside_the_service_section_does_not_count(self):
        text = "[Unit]\nNoNewPrivileges=yes\n[Service]\nExecStart=/usr/bin/x\n"
        self.assertEqual(len(problems({PATH: text})), 1)

    def test_a_commented_setting_does_not_count(self):
        self.assertEqual(len(problems({PATH: UNIT + "#NoNewPrivileges=yes\n"})), 1)

    def test_an_allow_list_bound_passes_and_the_empty_bound_drops_everything(self):
        for lines in (
            [""],
            ["CAP_NET_ADMIN"],
            ["CAP_NET_ADMIN CAP_CHOWN", "~CAP_CHOWN"],
            ["", "~CAP_SYS_ADMIN"],
        ):
            with self.subTest(lines=lines):
                text = UNIT + "".join(f"CapabilityBoundingSet={v}\n" for v in lines)
                self.assertEqual(problems({PATH: text}), [])

    def test_a_deny_list_bound_fails_even_when_it_removes_cap_sys_admin(self):
        for lines in (
            ["~CAP_SYS_ADMIN"],
            ["~CAP_SYS_MODULE CAP_SYS_BOOT CAP_SYS_RAWIO CAP_NET_ADMIN"],
            ["~CAP_SYS_ADMIN", "CAP_SYS_ADMIN"],
        ):
            with self.subTest(lines=lines):
                text = UNIT + "".join(f"CapabilityBoundingSet={v}\n" for v in lines)
                self.assertEqual(len(problems({PATH: text})), 1)
        deny_with_nnp = (
            UNIT + "CapabilityBoundingSet=~CAP_SYS_ADMIN\nNoNewPrivileges=yes\n"
        )
        self.assertEqual(problems({PATH: deny_with_nnp}), [])

    def test_an_allow_list_that_keeps_a_root_giving_capability_fails_without_nnp(self):
        for cap in (
            "CAP_SYS_ADMIN",
            "CAP_SYS_MODULE",
            "CAP_DAC_OVERRIDE",
            "CAP_SYS_PTRACE",
        ):
            with self.subTest(cap=cap):
                text = UNIT + f"CapabilityBoundingSet=CAP_CHOWN {cap}\n"
                self.assertEqual(len(problems({PATH: text})), 1)
                self.assertEqual(problems({PATH: text + "NoNewPrivileges=yes\n"}), [])
        removed = UNIT + "CapabilityBoundingSet=CAP_SYS_ADMIN CAP_CHOWN\n"
        removed += "CapabilityBoundingSet=~CAP_SYS_ADMIN\n"
        self.assertEqual(problems({PATH: removed}), [])

    def test_a_continued_line_is_joined_and_never_read_as_a_directive(self):
        text = UNIT + "Environment=A=1 \\\nNoNewPrivileges=yes\n"
        self.assertEqual(len(problems({PATH: text})), 1)
        split = UNIT + "CapabilityBoundingSet=CAP_CHOWN \\\n  CAP_SYS_ADMIN\n"
        self.assertEqual(len(problems({PATH: split})), 1)

    def test_a_bound_reset_to_the_full_set_fails(self):
        for lines in (["", "~"], ["~", "CAP_NET_ADMIN"]):
            with self.subTest(lines=lines):
                text = UNIT + "".join(f"CapabilityBoundingSet={v}\n" for v in lines)
                self.assertEqual(len(problems({PATH: text})), 1)

    def test_a_drop_in_completes_its_unit_and_the_last_assignment_wins(self):
        dropin = "pkg/SOURCES/usr/lib/systemd/system/x.service.d/10-a.conf"
        self.assertEqual(
            problems({PATH: UNIT, dropin: "[Service]\nNoNewPrivileges=yes\n"}), []
        )
        later = "pkg/SOURCES/usr/lib/systemd/system/x.service.d/20-b.conf"
        self.assertEqual(
            len(
                problems(
                    {
                        PATH: UNIT + "NoNewPrivileges=yes\n",
                        dropin: "[Service]\nNoNewPrivileges=yes\n",
                        later: "[Service]\nNoNewPrivileges=no\n",
                    }
                )
            ),
            1,
        )

    def test_a_drop_in_on_an_upstream_unit_counts_only_when_it_adds_a_command(self):
        dropin = "pkg/SOURCES/usr/lib/systemd/system/greetd.service.d/10.conf"
        self.assertEqual(problems({dropin: "[Service]\nEnvironment=A=1\n"}), [])
        for command in (
            "ExecStart=\nExecStart=/usr/bin/other\n",
            "ExecStartPre=/usr/bin/check\n",
            "ExecStopPost=/usr/bin/clean\n",
        ):
            with self.subTest(command=command):
                found = problems({dropin: "[Service]\n" + command})
                self.assertEqual(len(found), 1)
                self.assertIn("greetd.service.d", found[0])
                hardened = "[Service]\n" + command + "NoNewPrivileges=yes\n"
                self.assertEqual(problems({dropin: hardened}), [])

    def test_the_top_level_service_d_applies_to_every_unit(self):
        top = "pkg/SOURCES/usr/lib/systemd/system/service.d/10-nnp.conf"
        self.assertEqual(
            problems({PATH: UNIT, top: "[Service]\nNoNewPrivileges=yes\n"}), []
        )
        found = problems({top: "[Service]\nExecStartPre=/usr/bin/x\n"})
        self.assertEqual(len(found), 1)
        self.assertIn("service.d", found[0])

    def test_a_dash_prefix_drop_in_applies_to_the_units_it_prefixes(self):
        unit = "pkg/SOURCES/usr/lib/systemd/system/foo-bar.service"
        prefix = "pkg/SOURCES/usr/lib/systemd/system/foo-.service.d/10.conf"
        nnp = "[Service]\nNoNewPrivileges=yes\n"
        self.assertEqual(problems({unit: UNIT, prefix: nnp}), [])
        other = "pkg/SOURCES/usr/lib/systemd/system/foobar.service"
        self.assertEqual(len(problems({other: UNIT, prefix: nnp})), 1)

    def test_a_template_drop_in_applies_to_its_instances(self):
        instance = "pkg/SOURCES/usr/lib/systemd/system/foo@main.service"
        template = "pkg/SOURCES/usr/lib/systemd/system/foo@.service.d/10.conf"
        nnp = "[Service]\nNoNewPrivileges=yes\n"
        self.assertEqual(problems({instance: UNIT, template: nnp}), [])
        found = problems({template: "[Service]\nExecStartPre=/usr/bin/x\n"})
        self.assertEqual(len(found), 1)

    def test_a_more_specific_drop_in_replaces_a_same_named_one(self):
        top = "pkg/SOURCES/usr/lib/systemd/system/service.d/10-nnp.conf"
        own = "pkg/SOURCES/usr/lib/systemd/system/x.service.d/10-nnp.conf"
        found = problems(
            {
                PATH: UNIT,
                top: "[Service]\nNoNewPrivileges=yes\n",
                own: "[Service]\nNoNewPrivileges=no\n",
            }
        )
        self.assertEqual(len(found), 1)

    def test_masks_and_dbus_activation_files_are_not_services(self):
        dbus = "[D-BUS Service]\nName=org.example.X\nExec=/usr/bin/x\n"
        self.assertEqual(
            problems({PATH: "", "pkg/dbus-1/services/org.example.X.service": dbus}), []
        )

    def test_a_unit_written_by_a_spec_heredoc_is_checked_under_its_name(self):
        spec_text = (
            "%install\n"
            "cat > %{buildroot}%{_unitdir}/%{name}.service <<'EOF'\n" + UNIT + "EOF\n"
        )
        found = problems({"forge/specs/demo/demo.spec": spec_text})
        self.assertEqual(len(found), 1)
        self.assertIn("forge/specs/demo/demo.spec:demo.service", found[0])
        hardened = spec_text.replace("EOF\n", "NoNewPrivileges=yes\nEOF\n", 1)
        self.assertEqual(problems({"forge/specs/demo/demo.spec": hardened}), [])

    def test_an_exempt_unit_passes_and_a_stale_exemption_fails(self):
        self.assertEqual(problems({PATH: UNIT}, {PATH: "reason"}), [])
        stale = problems({PATH: UNIT + "NoNewPrivileges=yes\n"}, {PATH: "reason"})
        self.assertEqual(len(stale), 1)
        self.assertIn("remove the entry", stale[0])


if __name__ == "__main__":
    unittest.main()
