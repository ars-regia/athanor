"""The D-Bus filter of athanor-greeter-client
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

The greeter is the process that reads the password: every name and method its filtered
buses reach is listed here, and a rule added to the launcher without this list changing
fails the test.
"""

import re
import unittest
from pathlib import Path

SCRIPT = (
    Path(__file__).resolve().parents[1] / "SOURCES/usr/libexec/athanor-greeter-client"
)

SYSTEM_BUS = [
    "--call=org.freedesktop.login1=org.freedesktop.login1.Manager.Suspend@/org/freedesktop/login1",
    "--call=org.freedesktop.login1=org.freedesktop.login1.Manager.Reboot@/org/freedesktop/login1",
    "--call=org.freedesktop.login1=org.freedesktop.login1.Manager.PowerOff@/org/freedesktop/login1",
    # The seal's trust state (doc_update_trust.md, UT7): the read method alone, not Apply
    # or GoBack.
    "--call=os.athanor.Update1=os.athanor.Update1.State@/os/athanor/Update1",
]
SESSION_BUS = ["--own=os.athanor.Greeter", "--talk=org.a11y.Bus"]


class GreeterClientFilter(unittest.TestCase):
    def test_the_filter_grants_exactly_the_listed_rules(self):
        rules = re.findall(
            r"--(?:call|talk|see|own|broadcast)=[^\s\\]+", SCRIPT.read_text()
        )
        self.assertEqual(rules, SYSTEM_BUS + SESSION_BUS)

    def test_both_buses_are_filtered(self):
        self.assertEqual(SCRIPT.read_text().count("--filter"), 2)

    def test_the_trust_state_is_not_bound_into_the_sandbox(self):
        # The state is read over State(), whose reply the bus attributes to root; a bound
        # file would show root as the overflow uid, which UT7 forbids trusting.
        self.assertNotIn("/run/athanor-update", SCRIPT.read_text())


if __name__ == "__main__":
    unittest.main()
