"""athanor-firewall-zone-migrate against copies of the zones it meets (ADR-0116)
(python3 -B -m unittest discover -s forge/specs/athanor-base-config/tests -v).

The installer's zones are the files Anaconda's firewall-offline-cmd wrote: Fedora's zone with ssh
on installs older than the image's own zone, the image's zone plus ssh on newer ones.
"""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SOURCES = Path(__file__).resolve().parents[1] / "SOURCES"
SCRIPT = SOURCES / "usr/libexec/athanor-firewall-zone-migrate"
UNIT = SOURCES / "usr/lib/systemd/system/athanor-firewall-zone-migrate.service"
PRESET = SOURCES / "usr/lib/systemd/system-preset/80-athanor-base.preset"
IMAGE_ZONE = SOURCES / "etc/firewalld/zones/public.xml"

HEAD = """<?xml version="1.0" encoding="utf-8"?>
<zone>
  <short>Public</short>
  <description>For use in public areas. You do not trust the other computers on networks to not harm your computer. Only selected incoming connections are accepted.</description>
"""
OLD_INSTALL = (
    HEAD
    + """  <service name="ssh"/>
  <service name="mdns"/>
  <service name="dhcpv6-client"/>
  <forward/>
</zone>
"""
)
NEW_INSTALL = (
    HEAD
    + """  <service name="dhcpv6-client"/>
  <service name="ssh"/>
  <forward/>
</zone>
"""
)


class FirewallZoneMigrate(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = Path(tmp.name)
        self.zones = self.dir / "zones"
        self.zones.mkdir()
        self.zone = self.zones / "public.xml"
        self.old = self.zones / "public.xml.old"
        self.marker = self.dir / "state/firewall-public-zone.done"
        self.marker.parent.mkdir()
        self.log = self.dir / "anaconda.log"

    def install(self, zone, old=True):
        """The installer writes the zone, then its log a little later."""
        self.zone.write_text(zone)
        if old:
            self.old.write_text(zone)
        self.log.write_text("")
        installed = self.zone.stat().st_mtime + 2
        os.utime(self.log, (installed, installed))

    def run_script(self):
        env = {
            "PATH": "/usr/bin:/bin",
            "ATHANOR_FIREWALL_ZONES": str(self.zones),
            "ATHANOR_FIREWALL_IMAGE_ZONE": str(IMAGE_ZONE),
            "ATHANOR_FIREWALL_MARKER": str(self.marker),
            "ATHANOR_INSTALL_LOG": str(self.log),
        }
        return subprocess.run(
            [SCRIPT], env=env, capture_output=True, text=True, timeout=30
        )

    def assert_left_alone(self, zone, reason):
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("left alone", r.stdout)
        self.assertIn(reason, r.stdout)
        self.assertEqual(self.zone.read_text(), zone)
        self.assertTrue(self.marker.exists())

    def test_the_installer_zones_are_replaced_with_the_image_zone(self):
        for name, zone in (("old install", OLD_INSTALL), ("new install", NEW_INSTALL)):
            with self.subTest(name):
                self.marker.unlink(missing_ok=True)
                self.install(zone)
                r = self.run_script()
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertIn("replaced with the image's zone", r.stdout)
                self.assertEqual(self.zone.read_bytes(), IMAGE_ZONE.read_bytes())
                self.assertEqual(self.zone.stat().st_mode & 0o777, 0o644)
                self.assertFalse(self.old.exists())
                self.assertEqual(
                    sorted(p.name for p in self.zones.iterdir()), ["public.xml"]
                )
                self.assertTrue(self.marker.exists())

    def test_the_image_zone_opens_neither_ssh_nor_mdns(self):
        text = IMAGE_ZONE.read_text()
        self.assertNotIn('"ssh"', text)
        self.assertNotIn('"mdns"', text)

    def test_a_zone_edited_by_hand_is_left_alone(self):
        edits = {
            "another service": '  <service name="cockpit"/>\n',
            "a port": '  <port protocol="tcp" port="8080"/>\n',
            "a rule": '  <rule family="ipv4"><source address="10.0.0.0/8"/><accept/></rule>\n',
            "an interface": '  <interface name="eth0"/>\n',
            "a source": '  <source address="192.168.1.0/24"/>\n',
            "masquerade": "  <masquerade/>\n",
        }
        for name, line in edits.items():
            with self.subTest(name):
                self.marker.unlink(missing_ok=True)
                zone = OLD_INSTALL.replace("  <forward/>\n", line + "  <forward/>\n")
                self.install(zone)
                self.assert_left_alone(zone, "holds more than the installer wrote")
                self.assertTrue(self.old.exists())

    def test_an_attribute_is_an_edit(self):
        for zone in (
            OLD_INSTALL.replace("<zone>", '<zone target="ACCEPT">'),
            OLD_INSTALL.replace('name="ssh"/>', 'name="ssh" helper="x"/>'),
        ):
            with self.subTest(zone=zone):
                self.marker.unlink(missing_ok=True)
                self.install(zone)
                self.assert_left_alone(zone, "holds more than the installer wrote")

    def test_without_the_old_file_the_zone_is_not_the_installer(self):
        self.install(OLD_INSTALL, old=False)
        self.assert_left_alone(OLD_INSTALL, "there is no public.xml.old")

    def test_a_zone_changed_after_the_installation_is_left_alone(self):
        """firewall-cmd --permanent leaves a public.xml.old too: a person who opened ssh on an
        install whose zone was the image's gets a zone that looks like the installer's."""
        self.install(NEW_INSTALL)
        later = self.log.stat().st_mtime + 60
        os.utime(self.zone, (later, later))
        self.assert_left_alone(NEW_INSTALL, "it changed after the installation")

    def test_without_the_installer_log_the_origin_is_unknown(self):
        self.install(OLD_INSTALL)
        self.log.unlink()
        self.assert_left_alone(OLD_INSTALL, "nothing shows the installer wrote it")

    def test_a_zone_that_is_not_xml_is_left_alone(self):
        self.install("<zone>")
        self.assert_left_alone("<zone>", "not well-formed XML")

    def test_no_local_zone(self):
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("the image's zone applies", r.stdout)
        self.assertFalse(self.zone.exists())
        self.assertTrue(self.marker.exists())

    def test_it_runs_once(self):
        self.marker.touch()
        self.install(OLD_INSTALL)
        r = self.run_script()
        self.assertEqual((r.returncode, r.stdout), (0, ""))
        self.assertEqual(self.zone.read_text(), OLD_INSTALL)

    def test_a_missing_image_zone_fails_and_changes_nothing(self):
        self.install(OLD_INSTALL)
        env = {
            "ATHANOR_FIREWALL_ZONES": str(self.zones),
            "ATHANOR_FIREWALL_MARKER": str(self.marker),
            "ATHANOR_INSTALL_LOG": str(self.log),
        }
        env["ATHANOR_FIREWALL_IMAGE_ZONE"] = str(self.dir / "absent.xml")
        r = subprocess.run(
            [SCRIPT], env=env, capture_output=True, text=True, timeout=30
        )
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.zone.read_text(), OLD_INSTALL)
        self.assertTrue(self.old.exists())
        self.assertEqual(
            sorted(p.name for p in self.zones.iterdir()),
            ["public.xml", "public.xml.old"],
        )
        self.assertFalse(self.marker.exists())

    def test_the_unit_runs_once_before_firewalld_and_is_enabled(self):
        unit = UNIT.read_text()
        self.assertIn("Before=firewalld.service\n", unit)
        self.assertIn(
            f"ConditionPathExists=!/var/lib/athanor/{self.marker.name}\n", unit
        )
        self.assertIn("ReadWritePaths=/etc/firewalld/zones\n", unit)
        self.assertIn(
            "enable athanor-firewall-zone-migrate.service\n", PRESET.read_text()
        )
        self.assertTrue(os.access(SCRIPT, os.X_OK))


if __name__ == "__main__":
    unittest.main()
