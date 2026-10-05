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

    def test_a_capability_bound_passes_and_the_empty_bound_drops_everything(self):
        for value in ("", "CAP_NET_ADMIN", "~CAP_SYS_ADMIN"):
            with self.subTest(value=value):
                self.assertEqual(
                    problems({PATH: UNIT + f"CapabilityBoundingSet={value}\n"}), []
                )

    def test_a_bound_reset_to_the_full_set_fails(self):
        text = UNIT + "CapabilityBoundingSet=\nCapabilityBoundingSet=~\n"
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

    def test_a_drop_in_on_an_upstream_unit_counts_only_when_it_replaces_exec_start(
        self,
    ):
        dropin = "pkg/SOURCES/usr/lib/systemd/system/greetd.service.d/10.conf"
        self.assertEqual(problems({dropin: "[Service]\nEnvironment=A=1\n"}), [])
        replaced = "[Service]\nExecStart=\nExecStart=/usr/bin/other\n"
        self.assertEqual(len(problems({dropin: replaced})), 1)

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
