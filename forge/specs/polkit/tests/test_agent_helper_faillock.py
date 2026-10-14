"""The polkit agent helper may write the pam_faillock tally directory (issue #144)
(python3 -B -m unittest discover -s forge/specs/polkit/tests -v).

The upstream unit runs the helper under ProtectSystem=strict. system-auth carries
pam_faillock, which opens the user's tally in /run/faillock read-write, so without this
drop-in every prompt fails once a tally exists.
"""

import configparser
import re
import unittest
from pathlib import Path

SPEC_DIR = Path(__file__).resolve().parents[1]
DROP_IN = SPEC_DIR / "SOURCES/polkit-agent-helper-service-faillock.conf"
INSTALLED = "%{_unitdir}/polkit-agent-helper@.service.d/50-athanor-faillock.conf"


class AgentHelperFaillock(unittest.TestCase):
    def test_drop_in_opens_only_the_tally_directory(self):
        unit = configparser.ConfigParser(strict=True)
        unit.optionxform = str
        unit.read_string(DROP_IN.read_text())
        self.assertEqual(unit.sections(), ["Service"])
        self.assertEqual(dict(unit["Service"]), {"ReadWritePaths": "/run/faillock"})

    def test_spec_installs_and_owns_the_drop_in(self):
        spec = (SPEC_DIR / "polkit.spec").read_text()
        source = re.search(
            r"^Source(\d+):\s*" + re.escape(DROP_IN.name) + r"$", spec, re.M
        )
        self.assertIsNotNone(source, "the drop-in is not a Source of polkit.spec")
        install = (
            f"install -Dpm 0644 %{{SOURCE{source.group(1)}}} %{{buildroot}}{INSTALLED}"
        )
        self.assertIn(install, spec)
        files = spec.split("%files", 1)[1].split("%changelog", 1)[0]
        self.assertIn(INSTALLED, files.splitlines())


if __name__ == "__main__":
    unittest.main()
