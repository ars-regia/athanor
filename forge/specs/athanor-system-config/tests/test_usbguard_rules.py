"""The shipped USBGuard baseline against the device classes it admits and blocks (doc_disks.md DK21)
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

evaluate() models the rules of the shipped policy: the file RuleFile names (the one line
tmpfiles.d/10-athanor-greetd.conf seeds into /etc/usbguard/rules.conf), then the *.conf files of
RuleFolder in name order (the baseline), assuming usbguard-daemon reads them in that order. The first
rule whose with-interface condition matches decides; a device no rule matches meets
ImplicitPolicyTarget. Only the forms these files use are understood (all-of, one-of, equals over
with-interface); any other form fails the test, so it cannot pass unread. Rules a person adds at run
time, and every attribute other than the interfaces, are not modelled.
"""

import fnmatch
import re
import unittest
from pathlib import Path

SOURCES = Path(__file__).resolve().parents[1] / "SOURCES/etc/usbguard"
TMPFILES = Path(__file__).resolve().parents[1] / "SOURCES/usr/lib/tmpfiles.d/10-athanor-greetd.conf"
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
        found = [r for r in rules() if r[1] != "equals"]
        targets = [r[0] for r in found]
        self.assertLess(max(i for i, t in enumerate(targets) if t == "reject"), targets.index("allow"))

    def test_inserted_devices_are_blocked_and_present_ones_follow_the_rules(self):
        conf = (SOURCES / "usbguard-daemon.conf").read_text()
        self.assertIn("\nInsertedDevicePolicy=block\n", conf)
        self.assertIn("\nPresentDevicePolicy=apply-policy\n", conf)


if __name__ == "__main__":
    unittest.main()
