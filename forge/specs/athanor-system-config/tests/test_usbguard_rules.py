"""The shipped USBGuard baseline against the device classes it admits and blocks (doc_disks.md DK21)
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

Also athanor-usbguard-internal, which admits the USB devices connected at the first boot, once
(ADR-0103 D12, DK21), run here against a fake sysfs tree.

evaluate() models the rules of the shipped policy: the file RuleFile names (the one line
tmpfiles.d/10-athanor-greetd.conf seeds into /etc/usbguard/rules.conf), then the *.conf files of
RuleFolder in name order (the baseline), assuming usbguard-daemon reads them in that order. The first
rule whose with-interface condition matches decides; a device no rule matches meets
ImplicitPolicyTarget. Only the forms these files use are understood (all-of, one-of, equals over
with-interface); any other form fails the test, so it cannot pass unread. Rules a person adds at run
time, and every attribute other than the interfaces, are not modelled.
"""

import fnmatch
import os
import re
import subprocess
import tempfile
import threading
import unittest
from pathlib import Path

SOURCES = Path(__file__).resolve().parents[1] / "SOURCES/etc/usbguard"
TMPFILES = Path(__file__).resolve().parents[1] / "SOURCES/usr/lib/tmpfiles.d/10-athanor-greetd.conf"
LIBEXEC = Path(__file__).resolve().parents[1] / "SOURCES/usr/libexec/athanor-usbguard-internal"
UNIT = Path(__file__).resolve().parents[1] / "SOURCES/usr/lib/systemd/system/athanor-usbguard-internal.service"
RULE = re.compile(r"^(allow|reject|block) with-interface (all-of|one-of|equals) \{ ([0-9a-f:* ]+) \}$")


def seeded_rules_conf():
    """The lines tmpfiles.d writes into /etc/usbguard/rules.conf (the `f` line's argument)."""
    for line in TMPFILES.read_text().splitlines():
        if line.startswith("f /etc/usbguard/rules.conf"):
            return [line.split(" - ", 1)[1]]
    raise AssertionError("tmpfiles.d does not seed rules.conf")


def rules():
    found = []
    lines = seeded_rules_conf() + (SOURCES / "rules.d/10-athanor-baseline.conf").read_text().splitlines()
    for line in lines:
        if not line.strip() or line.startswith("#"):
            continue
        m = RULE.match(line)
        if not m:
            raise AssertionError(f"form the test does not model: {line}")
        found.append((m[1], m[2], m[3].split()))
    return found


def implicit_target():
    for line in (SOURCES / "usbguard-daemon.conf").read_text().splitlines():
        if line.startswith("ImplicitPolicyTarget="):
            return line.partition("=")[2]
    raise AssertionError("ImplicitPolicyTarget is not set")


def evaluate(interfaces):
    """The target for a device that presents these interfaces ("class:subclass:protocol", lower case hex)."""
    for target, quantifier, patterns in rules():
        hits = [any(fnmatch.fnmatchcase(i, p) for i in interfaces) for p in patterns]
        if quantifier == "equals":
            matched = len(interfaces) == len(patterns) and all(hits) and all(
                any(fnmatch.fnmatchcase(i, p) for p in patterns) for i in interfaces
            )
        else:
            matched = all(hits) if quantifier == "all-of" else any(hits)
        if matched:
            return target
    return implicit_target()


KEYBOARD = "03:01:01"
MOUSE = "03:01:02"
HUB = "09:00:00"
STORAGE = "08:06:50"


class BaselineTest(unittest.TestCase):
    def test_keyboards_mice_and_hubs_are_admitted(self):
        for device in ([KEYBOARD], [MOUSE], [HUB], [KEYBOARD, MOUSE], ["03:00:00"]):
            self.assertEqual(evaluate(device), "allow", device)

    def test_mass_storage_is_blocked_and_never_allowed_by_class(self):
        self.assertEqual(evaluate([STORAGE]), "block")
        self.assertFalse([r for r in rules() if r[0] == "allow" and any(p.startswith("08:") for p in r[2])])

    def test_a_hid_device_that_also_presents_storage_networking_or_wireless_is_rejected(self):
        for other in (STORAGE, "02:02:00", "e0:01:01"):
            self.assertEqual(evaluate([KEYBOARD, other]), "reject", other)

    def test_classes_with_no_rule_stay_under_the_implicit_block(self):
        # Internal devices present at install (webcam, Bluetooth, readers) are meant to be admitted
        # (ADR-0103 D12) and are not yet, so no class beyond these is asserted here.
        self.assertEqual(implicit_target(), "block")
        for device in (["01:01:00"], ["02:06:00"], ["07:01:02"], ["ff:ff:ff"], []):
            self.assertEqual(evaluate(device), "block", device)

    def test_rejections_come_before_every_allow(self):
        # The seeded rules.conf line comes first but admits only a device that is a hub and nothing else.
        found = rules()
        first_reject = [r[0] for r in found].index("reject")
        self.assertEqual(found[:first_reject], [("allow", "equals", ["09:*:*"])])
        after = [r[0] for r in found[first_reject:]]
        self.assertLess(max(i for i, t in enumerate(after) if t == "reject"), after.index("allow"))

    def test_inserted_devices_are_blocked_and_present_ones_follow_the_rules(self):
        conf = (SOURCES / "usbguard-daemon.conf").read_text()
        self.assertIn("\nInsertedDevicePolicy=block\n", conf)
        self.assertIn("\nPresentDevicePolicy=apply-policy\n", conf)


class InternalSnapshot(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.work = Path(tmp.name)
        self.sysfs = self.work / "sys"
        self.sysfs.mkdir()
        self.rules = self.work / "50-athanor-internal.conf"
        self.marker = self.work / "state/usbguard-internal.done"

    def device(self, name, vid, pid, interfaces, serial=None, configured=True, vanished=False):
        d = self.sysfs / name
        d.mkdir()
        if not vanished:
            (d / "idVendor").write_text(vid + "\n")
        (d / "idProduct").write_text(pid + "\n")
        (d / "bConfigurationValue").write_text(("1" if configured else "") + "\n")
        if not configured:
            return
        if serial is not None:
            (d / "serial").write_text(serial + "\n")
        for i, (cls, sub, proto) in enumerate(interfaces):
            f = d / f"{name}:1.{i}"
            f.mkdir()
            (f / "bInterfaceClass").write_text(cls + "\n")
            (f / "bInterfaceSubClass").write_text(sub + "\n")
            (f / "bInterfaceProtocol").write_text(proto + "\n")

    def run_script(self, sync=None, check=True, wait_tenths=2):
        """Runs the script; `sync` is the body of a stand-in for sync(1), put first on PATH."""
        env = dict(
            os.environ,
            ATHANOR_USB_SYSFS=str(self.sysfs),
            ATHANOR_USBGUARD_RULES=str(self.rules),
            ATHANOR_USBGUARD_MARKER=str(self.marker),
            ATHANOR_USBGUARD_WAIT_TENTHS=str(wait_tenths),
        )
        if sync is not None:
            bin_dir = self.work / "bin"
            bin_dir.mkdir(exist_ok=True)
            (bin_dir / "sync").write_text("#!/bin/bash\n" + sync)
            (bin_dir / "sync").chmod(0o755)
            env["PATH"] = f"{bin_dir}:{env['PATH']}"
        return subprocess.run([LIBEXEC], env=env, capture_output=True, text=True, check=check, timeout=30)

    def written(self):
        return [l for l in self.rules.read_text().splitlines() if not l.startswith("#")]

    def test_admits_webcam_reader_bluetooth_and_hub(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00"), ("0e", "02", "00")], "CAM1")
        self.device("1-2", "058f", "9540", [("0b", "00", "00")], "RD1")
        self.device("1-3", "8087", "0029", [("e0", "01", "01"), ("e0", "01", "01")])
        self.device("usb1", "1d6b", "0002", [("09", "00", "00")])
        self.run_script()
        rules = self.written()
        self.assertEqual(len(rules), 4)
        self.assertIn(
            'allow id 0c45:6a10 serial "CAM1" via-port "1-1" with-interface equals { 0e:01:00 0e:02:00 }',
            rules,
        )
        self.assertTrue(any("id 058f:9540" in r and "{ 0b:00:00 }" in r for r in rules))
        self.assertTrue(any("id 8087:0029" in r and "e0:01:01" in r for r in rules))
        self.assertTrue(any("id 1d6b:0002" in r and "09:00:00" in r for r in rules))
        self.assertEqual(self.rules.stat().st_mode & 0o777, 0o600)

    def test_no_rule_for_mass_storage(self):
        self.device("1-1", "0781", "5583", [("08", "06", "50")], "STICK")
        self.device("1-2", "046d", "c52b", [("03", "01", "01"), ("08", "06", "50")], "MIX")
        self.device("1-3", "0c45", "6a10", [("0e", "01", "00")])
        self.run_script()
        rules = self.written()
        self.assertEqual(len(rules), 1)
        self.assertIn("0c45:6a10", rules[0])

    def test_skips_descriptor_that_would_inject_a_rule(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")], 'x" allow id 0781:5583 "')
        self.run_script()
        self.assertEqual(self.written(), [])

    def test_runs_once(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        self.run_script()
        self.assertTrue(self.marker.exists())
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        self.run_script()
        self.assertEqual(len(self.written()), 1)

    def test_a_failure_writing_the_rules_leaves_no_marker_and_no_rules_file(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        done = self.run_script(sync="exit 1\n", check=False)
        self.assertNotEqual(done.returncode, 0)
        self.assertFalse(self.marker.exists())
        self.assertEqual(list(self.work.glob("50-athanor-internal.conf*")), [])

    def test_the_marker_is_written_after_the_rules_are_on_disk(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        log = self.work / "sync.log"
        self.run_script(
            sync=f'echo "rules=$([[ -e {self.rules} ]] && echo yes || echo no) '
            f'marker=$([[ -e {self.marker} ]] && echo yes || echo no) $*" >> {log}\n'
        )
        calls = log.read_text().splitlines()
        self.assertEqual(len(calls), 3, calls)
        self.assertTrue(calls[0].startswith("rules=no marker=no "), calls[0])
        self.assertTrue(calls[1].startswith("rules=yes marker=no "), calls[1])
        self.assertTrue(calls[2].startswith("rules=yes marker=yes "), calls[2])

    def test_a_vanished_device_is_skipped_and_the_others_are_written(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")], vanished=True)
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        done = self.run_script()
        self.assertEqual(len(self.written()), 1)
        self.assertIn("058f:9540", self.written()[0])
        self.assertIn("Skipping 1-1", done.stderr)
        self.assertTrue(self.marker.exists())

    def test_a_device_with_no_configuration_is_skipped_after_the_wait(self):
        self.device("1-1", "0c45", "6a10", [], configured=False)
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        done = self.run_script()
        self.assertEqual(len(self.written()), 1)
        self.assertIn("Skipping 0c45:6a10 on 1-1", done.stderr)

    def test_the_wait_lets_a_device_finish_enumerating(self):
        self.device("1-1", "0c45", "6a10", [], configured=False)
        d = self.sysfs / "1-1"

        def finish():
            (d / "bConfigurationValue").write_text("1\n")
            f = d / "1-1:1.0"
            f.mkdir()
            for name, value in (("Class", "0e"), ("SubClass", "01"), ("Protocol", "00")):
                (f / f"bInterface{name}").write_text(value + "\n")

        timer = threading.Timer(0.4, finish)
        timer.start()
        self.addCleanup(timer.join)
        self.run_script(wait_tenths=50)
        self.assertEqual(len(self.written()), 1)
        self.assertIn("0c45:6a10", self.written()[0])

    def test_an_empty_configuration_value_is_not_configured(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        (self.sysfs / "1-1/bConfigurationValue").write_text("\n")
        done = self.run_script()
        self.assertEqual(self.written(), [])
        self.assertIn("not configured", done.stderr)

    def test_the_rules_file_is_the_renamed_temporary_file(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        log = self.work / "inodes.log"
        self.run_script(sync=f'if [[ -f "$2" ]]; then stat -c %i "$2" >> {log}; fi\n')
        first = log.read_text().split()[0]
        self.assertEqual(str(self.rules.stat().st_ino), first)
        self.assertEqual(list(self.work.glob("50-athanor-internal.conf.*")), [])

    def test_a_serial_that_cannot_be_read_skips_the_device(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        (self.sysfs / "1-1/serial").mkdir()  # reading a directory fails, as a vanished file does
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        done = self.run_script()
        self.assertEqual(len(self.written()), 1)
        self.assertIn("serial cannot be read", done.stderr)

    def test_a_missing_serial_is_an_empty_serial(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        self.run_script()
        self.assertIn('serial ""', self.written()[0])

    def test_unit_orders_before_usbguard_and_runs_once(self):
        unit = UNIT.read_text()
        self.assertIn("Before=usbguard.service greetd.service", unit)
        self.assertIn("ConditionPathExists=!/var/lib/athanor/usbguard-internal.done", unit)


class BaselineUnchangedTest(unittest.TestCase):
    def test_baseline_is_unchanged(self):
        rules = [l for l in (SOURCES / 'rules.d/10-athanor-baseline.conf').read_text().splitlines() if l and not l.startswith("#")]
        self.assertEqual(
            rules,
            [
                "reject with-interface all-of { 08:*:* 03:*:* }",
                "reject with-interface all-of { 02:*:* 03:*:* }",
                "reject with-interface all-of { e0:*:* 03:*:* }",
                "allow with-interface one-of { 09:00:* }",
                "allow with-interface one-of { 03:*:* }",
            ],
        )

    def test_daemon_blocks_by_default_and_reads_the_rule_folder(self):
        conf = (SOURCES / 'usbguard-daemon.conf').read_text().splitlines()
        self.assertIn("RuleFolder=/etc/usbguard/rules.d/", conf)
        self.assertIn("ImplicitPolicyTarget=block", conf)


if __name__ == "__main__":
    unittest.main()
