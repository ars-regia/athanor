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
import signal
import subprocess
import tempfile
import time
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
        self.pending = self.work / "state/usbguard-internal.pending"

    def device(self, name, vid, pid, interfaces, serial=None, configured=True, vanished=False):
        d = self.sysfs / name
        d.mkdir()
        if not vanished:
            (d / "idVendor").write_text(vid + "\n")
        (d / "idProduct").write_text(pid + "\n")
        (d / "bConfigurationValue").write_text(("1" if configured else "") + "\n")
        if serial is not None:
            (d / "serial").write_text(serial + "\n")
        if not configured:
            return
        for i, (cls, sub, proto) in enumerate(interfaces):
            f = d / f"{name}:1.{i}"
            f.mkdir()
            (f / "bInterfaceClass").write_text(cls + "\n")
            (f / "bInterfaceSubClass").write_text(sub + "\n")
            (f / "bInterfaceProtocol").write_text(proto + "\n")

    def run_script(self, sync=None, sleep=None, check=True, wait_tenths=50):
        """Runs the script; `sync` and `sleep` are bodies of stand-ins for sync(1) and sleep(1),
        put first on PATH. A device is recorded on the second pass that reads it the same, so the
        default bound leaves room for a slow machine; the cases that reach it set a short one."""
        env = self.environment(sync, sleep, wait_tenths)
        return subprocess.run([LIBEXEC], env=env, capture_output=True, text=True, check=check, timeout=30)

    def environment(self, sync, sleep, wait_tenths):
        env = dict(
            os.environ,
            ATHANOR_USB_SYSFS=str(self.sysfs),
            ATHANOR_USBGUARD_RULES=str(self.rules),
            ATHANOR_USBGUARD_MARKER=str(self.marker),
            ATHANOR_USBGUARD_PENDING=str(self.pending),
            ATHANOR_USBGUARD_WAIT_TENTHS=str(wait_tenths),
            LC_ALL="C",  # the messages bash gives in English
        )
        bin_dir = self.work / "bin"
        for name, body in (("sync", sync), ("sleep", sleep)):
            if body is not None:
                bin_dir.mkdir(exist_ok=True)
                (bin_dir / name).write_text("#!/bin/bash\n" + body)
                (bin_dir / name).chmod(0o755)
        if bin_dir.exists():
            env["PATH"] = f"{bin_dir}:{env['PATH']}"
        return env

    def on_wait(self, *steps):
        """A sleep(1) stand-in that runs the n-th step at the n-th wait, and nothing after."""
        count = self.work / "waits"
        cases = "".join(f"  {n}) {step} ;;\n" for n, step in enumerate(steps, 1))
        return f"""echo >> {count}
case $(wc -l < {count}) in
{cases}esac
"""

    def assert_not_done(self, done, *unsettled):
        self.assertEqual(done.returncode, 1, done.stderr)
        self.assertFalse(self.marker.exists())
        for name in unsettled:
            self.assertIn(f"<3>{name} did not settle within 0.3 s: ", done.stderr)
        self.assertIn("Not recording the snapshot as done: ", done.stderr)

    def configure(self, name, interfaces):
        """Finishes the enumeration of a device made with configured=False."""
        d = self.sysfs / name
        for i, (cls, sub, proto) in enumerate(interfaces):
            f = d / f"{name}:1.{i}"
            f.mkdir()
            (f / "bInterfaceClass").write_text(cls + "\n")
            (f / "bInterfaceSubClass").write_text(sub + "\n")
            (f / "bInterfaceProtocol").write_text(proto + "\n")
        (d / "bConfigurationValue").write_text("1\n")

    def first_run_leaves_1_1_pending(self):
        """A first run in which 1-1 never configures and 1-2 is admitted."""
        self.device("1-1", "0c45", "6a10", [], configured=False)
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        done = self.run_script(check=False, wait_tenths=3)
        self.assert_not_done(done, "1-1")
        self.assertEqual(self.pending.read_text(), 'runs 1\n1-1 0c45:6a10 ""\n')
        return done

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

    @unittest.skipIf(os.geteuid() == 0, "root reads a file whatever its mode")
    def test_a_rules_file_that_cannot_be_read_is_kept_and_leaves_no_marker(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        earlier = '# header\nallow id 058f:9540 serial "" via-port "1-2" with-interface equals { 0b:00:00 }\n'
        self.rules.write_text(earlier)
        self.rules.chmod(0)
        done = self.run_script(check=False)
        self.rules.chmod(0o600)
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("Permission denied", done.stderr)
        self.assertEqual(self.rules.read_text(), earlier)
        self.assertFalse(self.marker.exists())
        self.assertTrue(self.pending.exists())  # the next boot reads the same devices again

    def test_the_marker_is_written_after_the_rules_are_on_disk(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        log = self.work / "sync.log"
        self.run_script(
            sync=f'echo "$([[ -e {self.pending} ]] && echo P)$([[ -e {self.rules} ]] && echo R)'
            f'$([[ -e {self.marker} ]] && echo M)- $2" >> {log}\n'
        )
        # What exists at each sync: the pending file first, then the rules, then the marker,
        # and the pending file goes last.
        calls = [line.split() for line in log.read_text().splitlines()]
        self.assertTrue(calls[0][1].startswith(f"{self.pending}."), calls)
        self.assertEqual(calls[0][0], "-", calls)
        rules_at = next(i for i, c in enumerate(calls) if c[1].startswith(f"{self.rules}."))
        marker_at = next(i for i, c in enumerate(calls) if c[1].startswith(f"{self.marker}."))
        self.assertEqual(calls[rules_at][0], "P-", calls)
        self.assertEqual(calls[rules_at + 1], ["PR-", str(self.work)], calls)
        self.assertEqual(calls[marker_at][0], "PR-", calls)
        self.assertEqual(calls[-1], ["RM-", str(self.pending.parent)], calls)
        self.assertLess(rules_at, marker_at)

    def test_a_device_that_leaves_the_bus_is_not_waited_for(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")], vanished=True)
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        # The descriptor of 1-1 cannot be read, then the kernel removes the device from sysfs.
        self.run_script(sleep=self.on_wait(f"rm -r {self.sysfs / '1-1'}"))
        self.assertEqual(len(self.written()), 1)
        self.assertIn("058f:9540", self.written()[0])
        self.assertTrue(self.marker.exists())

    def test_a_device_that_leaves_the_bus_after_its_ids_were_read_is_not_pending(self):
        self.device("1-1", "0c45", "6a10", [], configured=False)
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        self.run_script(sleep=self.on_wait(f"rm -r {self.sysfs / '1-1'}"))
        self.assertEqual(len(self.written()), 1)
        self.assertIn("058f:9540", self.written()[0])
        self.assertTrue(self.marker.exists())
        self.assertFalse(self.pending.exists())

    def test_a_descriptor_unreadable_then_readable_is_recorded(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")], vanished=True)
        self.run_script(sleep=self.on_wait(f"echo 0c45 > {self.sysfs / '1-1/idVendor'}"))
        self.assertEqual(len(self.written()), 1)
        self.assertIn("0c45:6a10", self.written()[0])
        self.assertTrue(self.marker.exists())

    def test_a_device_reconfigured_between_passes_is_recorded_once_it_reads_the_same_twice(self):
        # usb_modeswitch, or a udev rule writing bConfigurationValue: configuration 1 offers mass
        # storage; configuration 2, the modem, gains its interfaces one at a time. A pass that
        # falls inside the change sees half of them, and the next pass reads differently.
        self.device("1-1", "12d1", "1506", [("08", "06", "50")])
        d = self.sysfs / "1-1"
        iface = (
            "mkdir {d}/1-1:2.{n} && echo {c} > {d}/1-1:2.{n}/bInterfaceClass && "
            "echo {s} > {d}/1-1:2.{n}/bInterfaceSubClass && echo {p} > {d}/1-1:2.{n}/bInterfaceProtocol"
        )
        self.run_script(
            sleep=self.on_wait(
                f"rm -r {d}/1-1:1.0 && echo 2 > {d}/bConfigurationValue && "
                + iface.format(d=d, n=0, c="02", s="0e", p="00"),
                iface.format(d=d, n=1, c="0a", s="00", p="01"),
            )
        )
        self.assertEqual(
            self.written(),
            ['allow id 12d1:1506 serial "" via-port "1-1" with-interface equals { 02:0e:00 0a:00:01 }'],
        )
        self.assertEqual(len((self.work / "waits").read_text().splitlines()), 3)
        self.assertTrue(self.marker.exists())

    def test_a_device_that_never_reads_the_same_twice_is_left_pending_with_its_serial(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")], "CAM1")
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        proto = self.sysfs / "1-1/1-1:1.0/bInterfaceProtocol"
        done = self.run_script(sleep=f"echo 0$((1 - $(< {proto}))) > {proto}\n", check=False, wait_tenths=3)
        self.assert_not_done(done, "1-1")
        self.assertIn("1-1 did not settle within 0.3 s: no two consecutive passes read it the same", done.stderr)
        self.assertEqual(self.pending.read_text(), "runs 1\n1-1 0c45:6a10 CAM1\n")
        # The device that settled is admitted for this boot; the snapshot is taken again.
        self.assertEqual(len(self.written()), 1)
        self.assertIn("058f:9540", self.written()[0])

    def test_the_serial_of_a_device_not_configured_at_the_bound_is_recorded(self):
        self.device("1-1", "0c45", "6a10", [], "CAM1", configured=False)
        done = self.run_script(check=False, wait_tenths=3)
        self.assert_not_done(done, "1-1")
        self.assertEqual(self.pending.read_text(), "runs 1\n1-1 0c45:6a10 CAM1\n")

    def test_a_device_with_no_configuration_at_the_bound_is_left_pending(self):
        done = self.first_run_leaves_1_1_pending()
        self.assertIn("1-1 did not settle within 0.3 s: not configured", done.stderr)
        self.assertIn("1 device(s) pending after run 1 of 3", done.stderr)
        self.assertEqual(len(self.written()), 1)
        self.assertIn("058f:9540", self.written()[0])
        self.assertEqual(self.pending.stat().st_mode & 0o777, 0o600)

    def test_the_next_boot_admits_the_pending_device_and_never_a_new_one(self):
        self.first_run_leaves_1_1_pending()
        first = self.written()
        self.configure("1-1", [("0e", "01", "00")])
        # Connected since the first run: a keyboard at another port, and a webcam.
        self.device("1-4", "046d", "c52b", [("03", "01", "01")], "KBD")
        self.device("1-5", "0c45", "6a11", [("0e", "01", "00")])
        done = self.run_script()
        self.assertEqual(
            self.written(),
            first + ['allow id 0c45:6a10 serial "" via-port "1-1" with-interface equals { 0e:01:00 }'],
        )
        self.assertTrue(self.marker.exists())
        self.assertFalse(self.pending.exists())
        self.assertNotIn("1-4", done.stderr)
        self.assertNotIn("1-5", done.stderr)

    def test_a_pending_device_still_unsettled_is_read_again_and_nothing_else(self):
        self.first_run_leaves_1_1_pending()
        self.device("1-4", "046d", "c52b", [("03", "01", "01")])
        done = self.run_script(check=False, wait_tenths=3)
        self.assert_not_done(done, "1-1")
        self.assertEqual(self.pending.read_text(), 'runs 2\n1-1 0c45:6a10 ""\n')
        self.assertEqual(len(self.written()), 1)
        self.assertNotIn("1-4", done.stderr)

    def test_after_three_runs_the_devices_left_are_named_and_the_snapshot_ends(self):
        self.first_run_leaves_1_1_pending()
        self.assertEqual(self.run_script(check=False, wait_tenths=3).returncode, 1)
        done = self.run_script(check=False, wait_tenths=3)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue(self.marker.exists())
        self.assertFalse(self.pending.exists())
        self.assertIn(
            '<3>Leaving 1-1 (0c45:6a10 "") blocked after 3 runs: allow it from Settings, USB devices, with "Allow always".',
            done.stderr,
        )
        self.assertEqual(len(self.written()), 1)

    def test_runs_killed_by_the_timeout_count(self):
        # The third run counted itself and was killed: the fourth boot reads nothing.
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        self.pending.parent.mkdir()
        self.pending.write_text("runs 3\n1-1 0c45:6a10\n")
        done = self.run_script()
        self.assertTrue(self.marker.exists())
        self.assertFalse(self.rules.exists())
        self.assertIn("<3>Leaving 1-1 (0c45:6a10) blocked after 3 runs", done.stderr)

    def test_a_pending_port_that_holds_another_device_is_not_admitted(self):
        self.device("1-1", "046d", "c52b", [("03", "01", "01")], "KBD")
        self.device("1-2", "0c45", "6a10", [("0e", "01", "00")], "CAM2")
        self.pending.parent.mkdir()
        self.pending.write_text("runs 1\n1-1 0c45:6a10\n1-2 0c45:6a10 CAM1\n")
        done = self.run_script(check=False)
        self.assertEqual(done.returncode, 1, done.stderr)
        self.assertIn("<4>1-1 now holds a device other than the 0c45:6a10 of the first run", done.stderr)
        self.assertIn("<4>1-2 now holds a device other than the 0c45:6a10 CAM1 of the first run", done.stderr)
        self.assertEqual(self.written(), [])
        self.assertFalse(self.marker.exists())
        self.assertEqual(self.pending.read_text(), "runs 2\n1-1 0c45:6a10\n1-2 0c45:6a10 CAM1\n")

    def test_a_pending_device_with_its_recorded_serial_is_admitted(self):
        self.device("1-2", "0c45", "6a10", [("0e", "01", "00")], "CAM 2")
        self.pending.parent.mkdir()
        self.pending.write_text("runs 2\n1-2 0c45:6a10 CAM 2\n")
        self.run_script()
        self.assertEqual(
            self.written(),
            ['allow id 0c45:6a10 serial "CAM 2" via-port "1-2" with-interface equals { 0e:01:00 }'],
        )
        self.assertTrue(self.marker.exists())
        self.assertFalse(self.pending.exists())

    def test_a_pending_device_that_enumerates_late_is_waited_for_and_admitted(self):
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        later = self.work / "later"
        later.mkdir()
        (self.sysfs / "1-2").rename(later / "1-2")
        self.pending.parent.mkdir()
        self.pending.write_text("runs 1\n1-2 058f:9540\n")
        self.run_script(sleep=self.on_wait("", f"mv {later / '1-2'} {self.sysfs}"))
        self.assertEqual(
            self.written(),
            ['allow id 058f:9540 serial "" via-port "1-2" with-interface equals { 0b:00:00 }'],
        )
        self.assertTrue(self.marker.exists())
        self.assertFalse(self.pending.exists())

    def test_a_pending_device_never_connected_is_waited_for_until_the_bound(self):
        self.pending.parent.mkdir()
        self.pending.write_text("runs 1\n1-2 058f:9540\n")
        start = time.monotonic()
        done = self.run_script(check=False, wait_tenths=3)
        self.assertGreaterEqual(time.monotonic() - start, 0.3)
        self.assert_not_done(done, "1-2")
        self.assertIn("<3>1-2 did not settle within 0.3 s: not connected", done.stderr)
        self.assertEqual(self.pending.read_text(), "runs 2\n1-2 058f:9540\n")

    def test_a_pending_device_read_with_no_serial_admits_only_a_device_with_none(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")], "CAM9")
        self.device("1-2", "0c45", "6a10", [("0e", "01", "00")])
        self.pending.parent.mkdir()
        self.pending.write_text('runs 1\n1-1 0c45:6a10 ""\n1-2 0c45:6a10 ""\n')
        done = self.run_script(check=False)
        self.assertIn('<4>1-1 now holds a device other than the 0c45:6a10 "" of the first run', done.stderr)
        self.assertEqual(
            self.written(),
            ['allow id 0c45:6a10 serial "" via-port "1-2" with-interface equals { 0e:01:00 }'],
        )
        self.assertEqual(self.pending.read_text(), 'runs 2\n1-1 0c45:6a10 ""\n')

    def test_a_pending_device_whose_serial_was_never_read_matches_by_port_and_ids(self):
        # The residual limit: with no description from the first run, any serial is accepted.
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")], "CAM9")
        self.pending.parent.mkdir()
        self.pending.write_text("runs 1\n1-1 0c45:6a10\n")
        self.run_script()
        self.assertEqual(
            self.written(),
            ['allow id 0c45:6a10 serial "CAM9" via-port "1-1" with-interface equals { 0e:01:00 }'],
        )

    def test_a_pending_file_that_cannot_be_trusted_fails_without_a_new_snapshot(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        for body in (
            "",
            "runs 0\n",
            "1-1 0c45:6a10\n",
            "runs 1\n1-1 0c45:6a10\nallow id 0781:5583\n",
            "runs 1\n1-1 0c45:6a10",  # no final newline: a line cut short
            "runs 1\n1-1 0c45:6a1\n",
            None,  # a directory: cannot be read
        ):
            with self.subTest(body=body):
                self.pending.parent.mkdir(exist_ok=True)
                if self.pending.is_dir():
                    self.pending.rmdir()
                for leftover in (self.pending, self.marker, self.rules):
                    if leftover.is_file():
                        leftover.unlink()
                if body is None:
                    self.pending.mkdir()
                else:
                    self.pending.write_text(body)
                done = self.run_script(check=False)
                self.assertEqual(done.returncode, 1)
                self.assertIn(f"<3>{self.pending} is not valid; no device is recorded.", done.stderr)
                self.assertFalse(self.marker.exists())
                self.assertFalse(self.rules.exists())

    def test_a_kill_while_a_read_blocks_leaves_no_marker_and_no_rules(self):
        # A read of bConfigurationValue waits for the device lock while a driver probe holds it;
        # a FIFO with no writer blocks the same way. The unit's start timeout then stops the run.
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        (self.sysfs / "1-1/bConfigurationValue").unlink()
        os.mkfifo(self.sysfs / "1-1/bConfigurationValue")
        self.kill_when_blocked(self.start(), lambda: True)
        self.assertFalse(self.marker.exists())
        self.assertEqual(list(self.work.glob("50-athanor-internal.conf*")), [])
        # Its ids and its lack of a serial were read before the blocking read, so the next boot
        # reads that port only, for a device with no serial.
        self.assertEqual(self.pending.read_text(), 'runs 1\n1-1 0c45:6a10 ""\n')

    def start(self, sleep=None):
        """Starts the script in its own process group, with a bound of 0.3 s: a run that did not
        block would end long before kill_when_blocked looks."""
        return subprocess.Popen(
            [LIBEXEC],
            env=self.environment(None, sleep, 3),
            start_new_session=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def kill_when_blocked(self, run, ready):
        """Waits for `ready`, then checks the run is still blocked a second later and stops it as
        the unit's start timeout would."""
        try:
            for _ in range(100):
                if ready():
                    break
                self.assertIsNone(run.poll(), "the run ended before it blocked")
                time.sleep(0.1)
            else:
                self.fail("the run never reached the blocking read")
            with self.assertRaises(subprocess.TimeoutExpired):
                run.wait(1)
        finally:
            os.killpg(run.pid, signal.SIGTERM)
        self.assertEqual(run.wait(30), -signal.SIGTERM)

    def test_a_first_run_killed_after_seeing_two_devices_is_continued_at_their_ports_only(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")], "CAM1")
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        config = self.sysfs / "1-2/bConfigurationValue"
        # After the first pass 1-1 settles, and the read of 1-2's configuration blocks.
        self.kill_when_blocked(
            self.start(sleep=self.on_wait(f"rm {config} && mkfifo {config}")),
            lambda: (self.work / "waits").exists(),
        )
        self.assertFalse(self.marker.exists())
        self.assertFalse(self.rules.exists())
        # 1-2 was read once, with no serial, before its configuration blocked.
        self.assertEqual(self.pending.read_text(), 'runs 1\n1-1 0c45:6a10 CAM1\n1-2 058f:9540 ""\n')
        config.unlink()
        config.write_text("1\n")
        self.device("1-4", "046d", "c52b", [("03", "01", "01")], "KBD")  # connected since
        done = self.run_script()
        self.assertEqual(
            sorted(self.written()),
            [
                'allow id 058f:9540 serial "" via-port "1-2" with-interface equals { 0b:00:00 }',
                'allow id 0c45:6a10 serial "CAM1" via-port "1-1" with-interface equals { 0e:01:00 }',
            ],
        )
        self.assertTrue(self.marker.exists())
        self.assertFalse(self.pending.exists())
        self.assertNotIn("1-4", done.stderr)

    def test_a_first_run_killed_before_it_read_any_ids_admits_nothing(self):
        # A FIFO as idVendor blocks the first read of all, which the kernel serves without a lock.
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        vendor = self.sysfs / "1-1/idVendor"
        vendor.unlink()
        os.mkfifo(vendor)
        self.kill_when_blocked(self.start(), lambda: self.pending.exists())
        self.assertEqual(self.pending.read_text(), "runs 1\n")
        vendor.unlink()
        vendor.write_text("0c45\n")
        done = self.run_script()
        self.assertIn(
            "<3>The first run of the snapshot stopped before it read any device: no internal device is admitted.",
            done.stderr,
        )
        self.assertTrue(self.marker.exists())
        self.assertFalse(self.pending.exists())
        self.assertFalse(self.rules.exists())

    def test_the_bound_is_measured_on_the_time_since_boot(self):
        # The stand-in for sleep(1) returns at once and moves the time since boot on by 0.1 s:
        # the bound of 0.3 s is three waits, whatever the wall clock does meanwhile.
        self.device("1-1", "0c45", "6a10", [], configured=False)
        uptime = self.work / "uptime"
        uptime.write_text("12.00 40.00\n")
        waits = self.work / "waits"
        sleep = f'''echo >> {waits}
read -r up _ < {uptime}
c=$((10#${{up/./}} + 10))
printf '%d.%02d 0.00\\n' $((c / 100)) $((c % 100)) > {uptime}
'''
        env = dict(self.environment(None, sleep, 3), ATHANOR_UPTIME=str(uptime))
        done = subprocess.run([LIBEXEC], env=env, capture_output=True, text=True, timeout=30)
        self.assert_not_done(done, "1-1")
        self.assertEqual(len(waits.read_text().splitlines()), 3)
        self.assertEqual(uptime.read_text(), "12.30 0.00\n")

    def test_the_wait_lets_a_device_finish_enumerating(self):
        self.device("1-1", "0c45", "6a10", [], configured=False)
        d = self.sysfs / "1-1"

        waits = self.work / "waits"
        # The script sleeps only while a device is pending: its first wait finishes the
        # enumeration, the interface with its attributes, as sysfs shows it, and the pass after
        # the second wait reads the device as the one before it did.
        finish = f"""echo >> {waits}
if [[ ! -e {d}/1-1:1.0 ]]; then
  mkdir {d}/pending
  for a in Class:0e SubClass:01 Protocol:00; do echo "${{a#*:}}" > "{d}/pending/bInterface${{a%:*}}"; done
  mv {d}/pending {d}/1-1:1.0
  echo 1 > {d}/bConfigurationValue
fi
"""
        self.run_script(sleep=finish)
        self.assertEqual(len(waits.read_text().splitlines()), 2)
        self.assertEqual(len(self.written()), 1)
        self.assertIn("0c45:6a10", self.written()[0])

    def test_an_empty_configuration_value_is_not_configured(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        # The kernel shows no configuration as an empty attribute, with no newline.
        (self.sysfs / "1-1/bConfigurationValue").write_text("")
        done = self.run_script(check=False, wait_tenths=3)
        self.assert_not_done(done, "1-1")
        self.assertIn("1-1 did not settle within 0.3 s: not configured", done.stderr)
        self.assertEqual(self.written(), [])

    def test_the_rules_file_is_the_renamed_temporary_file(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        log = self.work / "inodes.log"
        self.run_script(sync=f'if [[ $2 == {self.rules}.* ]]; then stat -c %i "$2" >> {log}; fi\n')
        first = log.read_text().split()[0]
        self.assertEqual(str(self.rules.stat().st_ino), first)
        self.assertEqual(list(self.work.glob("50-athanor-internal.conf.*")), [])

    def test_a_serial_that_cannot_be_read_leaves_no_marker(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        (self.sysfs / "1-1/serial").mkdir()  # reading a directory fails, as a vanished file does
        self.device("1-2", "058f", "9540", [("0b", "00", "00")])
        done = self.run_script(check=False, wait_tenths=3)
        self.assert_not_done(done, "1-1")
        self.assertIn("Is a directory", done.stderr)
        self.assertEqual(len(self.written()), 1)
        self.assertIn("058f:9540", self.written()[0])

    def test_a_missing_serial_is_an_empty_serial(self):
        self.device("1-1", "0c45", "6a10", [("0e", "01", "00")])
        self.run_script()
        self.assertIn('serial ""', self.written()[0])

    def test_unit_orders_before_usbguard_and_runs_once(self):
        unit = UNIT.read_text()
        self.assertIn("Before=usbguard.service greetd.service", unit)
        self.assertIn("ConditionPathExists=!/var/lib/athanor/usbguard-internal.done", unit)

    def test_unit_start_is_bounded_beyond_the_scripts_own_bound(self):
        # The script's bound (10 s) fits within the start timeout, and greetd and usbguard.service
        # are ordered after the unit without depending on it. No udevadm settle: under
        # NoNewPrivileges= SELinux denies it udev's socket and its failure skipped the script.
        unit = UNIT.read_text()
        self.assertNotRegex(unit, r"(?m)^Exec\w*=.*udevadm")
        self.assertIn("\nTimeoutStartSec=30s\nTimeoutStopSec=5s\n", unit)
        self.assertIn("\nwait_tenths=${ATHANOR_USBGUARD_WAIT_TENTHS:-100}\n", LIBEXEC.read_text())
        self.assertNotIn("Requires=", unit)
        self.assertNotIn("RequiredBy=", unit)


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
