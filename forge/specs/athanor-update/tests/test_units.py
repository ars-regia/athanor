"""The units of athanor-update carry the hardening spike U1 measured, and nothing it found
to break bootc (python3 -B -m unittest discover -s forge/specs/athanor-update/tests -v)."""

import pathlib
import re
import shutil
import subprocess
import unittest
import xml.dom.minidom

SOURCES = pathlib.Path(__file__).resolve().parents[1] / "SOURCES"
UNITS = SOURCES / "usr/lib/systemd/system"
MEASURED = [
    "NoNewPrivileges=yes", "ProtectHome=yes", "PrivateTmp=yes", "ProtectKernelTunables=yes",
    "ProtectKernelModules=yes", "ProtectControlGroups=yes", "ProtectProc=invisible", "LockPersonality=yes",
    "MemoryDenyWriteExecute=yes", "UMask=0022", "SystemCallFilter=@system-service @mount",
    "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK", "RestrictNamespaces=~user",
    "CapabilityBoundingSet=~CAP_SYS_MODULE CAP_SYS_BOOT CAP_SYS_RAWIO CAP_NET_ADMIN",
]
# unit -> (reaches the network, systemd-analyze exposure threshold in tenths)
SERVICES = {
    "athanor-update-check.service": (True, 60),
    "athanor-update-migrate.service": (True, 60),
    "athanor-update.service": (False, 55),
    "athanor-update-state.service": (False, 55),
}


def directives(unit):
    return [line.strip() for line in (UNITS / unit).read_text().splitlines() if "=" in line and not line.lstrip().startswith("#")]


class Units(unittest.TestCase):
    def test_every_service_carries_the_measured_list_verbatim(self):
        for unit in SERVICES:
            for directive in MEASURED:
                self.assertIn(directive, directives(unit), f"{unit}: {directive}")

    def test_no_service_carries_what_breaks_bootc(self):
        for unit in SERVICES:
            text = "\n".join(directives(unit))
            self.assertNotIn("RestrictSUIDSGID", text, unit)
            self.assertNotIn("ProtectSystem", text, unit)
            for bounding in re.findall(r"^CapabilityBoundingSet=(.*)$", text, re.M):
                self.assertTrue(bounding.startswith("~"), f"{unit}: capabilities are subtracted, never enumerated")

    def test_only_the_units_that_need_the_registry_reach_the_network(self):
        for unit, (network, _) in SERVICES.items():
            lines = directives(unit)
            self.assertEqual("PrivateNetwork=yes" not in lines, network, unit)
            self.assertFalse(any(line.startswith("IPAddressDeny") for line in lines) and network, unit)

    def test_each_unit_runs_the_subcommand_it_is_named_for(self):
        expected = {"athanor-update-check.service": "check", "athanor-update-migrate.service": "migrate",
                    "athanor-update.service": "serve", "athanor-update-state.service": "check --offline"}
        for unit, arguments in expected.items():
            self.assertIn(f"ExecStart=/usr/bin/athanor-update {arguments}", directives(unit))

    def test_the_timer_is_fifteen_minutes_then_six_hours_with_a_random_delay(self):
        lines = directives("athanor-update-check.timer")
        for directive in ("OnBootSec=15min", "OnUnitActiveSec=6h"):
            self.assertIn(directive, lines)
        self.assertTrue(any(line.startswith("RandomizedDelaySec=") for line in lines))

    @unittest.skipUnless(shutil.which("systemd-analyze"), "systemd-analyze is not installed")
    def test_exposure_stays_below_the_stated_threshold(self):
        for unit, (_, threshold) in SERVICES.items():
            r = subprocess.run(["systemd-analyze", "security", "--offline=true", f"--threshold={threshold}", "--no-pager", str(UNITS / unit)],
                               capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, f"{unit}: {r.stdout[-200:]}{r.stderr[-200:]}")


class Presets(unittest.TestCase):
    def test_our_units_are_enabled_and_the_stock_timer_is_not(self):
        preset = (SOURCES / "usr/lib/systemd/system-preset/80-athanor-update.preset").read_text()
        for line in ("enable athanor-update-check.timer", "enable athanor-update-state.service",
                     "enable athanor-update-migrate.service", "disable bootc-fetch-apply-updates.timer"):
            self.assertIn(line, preset.splitlines())
        self.assertIn("enable athanor-update-notify.service", (SOURCES / "usr/lib/systemd/user-preset/80-athanor-update.preset").read_text())

    def test_tmpfiles_declares_both_directories_and_not_the_disk_key_directory(self):
        lines = [l for l in (SOURCES / "usr/lib/tmpfiles.d/athanor-update.conf").read_text().splitlines() if l and not l.startswith("#")]
        self.assertIn("d /run/athanor-update 0755 root root -", lines)
        self.assertIn("d /var/lib/athanor-update 0755 root root -", lines)
        self.assertFalse(any(re.search(r"\s/run/athanor(\s|/\s|$)", line) for line in lines))


class Bus(unittest.TestCase):
    def test_the_bus_policy_allows_two_members_and_introspection_only(self):
        doc = xml.dom.minidom.parse(str(SOURCES / "usr/share/dbus-1/system.d/os.athanor.Update1.conf"))
        allows = [dict(node.attributes.items()) for node in doc.getElementsByTagName("allow")]
        sends = [a for a in allows if "send_destination" in a]
        self.assertEqual(sorted((a["send_interface"], a["send_member"]) for a in sends),
                         [("org.freedesktop.DBus.Introspectable", "Introspect"), ("os.athanor.Update1", "Apply"), ("os.athanor.Update1", "GoBack")])
        self.assertTrue(all(a["send_destination"] == "os.athanor.Update1" for a in sends))
        self.assertEqual([a for a in allows if "own" in a], [{"own": "os.athanor.Update1"}])

    def test_the_activation_file_starts_the_systemd_unit_as_root(self):
        lines = (SOURCES / "usr/share/dbus-1/system-services/os.athanor.Update1.service").read_text().splitlines()
        self.assertEqual(lines, ["[D-BUS Service]", "Name=os.athanor.Update1", "Exec=/bin/false", "User=root", "SystemdService=athanor-update.service"])

    def test_the_polkit_defaults_are_logind_s_for_apply_and_auth_admin_for_going_back(self):
        doc = xml.dom.minidom.parse(str(SOURCES / "usr/share/polkit-1/actions/os.athanor.update.policy"))
        found = {}
        for action in doc.getElementsByTagName("action"):
            defaults = action.getElementsByTagName("defaults")[0]
            found[action.getAttribute("id")] = tuple(defaults.getElementsByTagName(tag)[0].firstChild.data for tag in ("allow_any", "allow_inactive", "allow_active"))
        self.assertEqual(found, {"os.athanor.update.apply": ("auth_admin_keep", "auth_admin_keep", "yes"),
                                 "os.athanor.update.rollback": ("auth_admin", "auth_admin", "auth_admin")})


if __name__ == "__main__":
    unittest.main()
