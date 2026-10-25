"""Tests of rig.sh's atspi and orca cases that need no container
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import os
import re
import subprocess
import unittest
from pathlib import Path

SHELL = Path(__file__).resolve().parent.parent
RIG = SHELL / "rig.sh"


def rig(*args, compositor=None):
    env = {k: v for k, v in os.environ.items() if k != "RIG_COMPOSITOR"}
    if compositor:
        env["RIG_COMPOSITOR"] = compositor
    return subprocess.run(["bash", str(RIG), *args], capture_output=True, text=True, env=env, check=False)


def case(name, after):
    """The text of rig.sh's `after` case up to its next sibling, from the `name)` label."""
    text = RIG.read_text(encoding="utf-8")
    start = text.index(f"\n{after})\n")
    match = re.compile(rf"\n    {name}\)\n(.*?)\n        ;;\n", re.DOTALL).search(text, start)
    return match.group(1)


class GreeterOnlyUnderCosmicComp(unittest.TestCase):
    def test_the_greeter_is_refused_under_the_own_compositor(self):
        for command in ("atspi", "orca"):
            for language in ([], ["it"]):
                result = rig(command, "greeter", *language, compositor="athanor")
                self.assertEqual(result.returncode, 2, (command, result.stderr))
                self.assertIn("SN12", result.stderr)
                self.assertIn(command, result.stderr)


class AtspiReferences(unittest.TestCase):
    def test_bar_launcher_and_greeter_write_and_compare_their_transcripts(self):
        for surface in ("bar", "launcher", "greeter"):
            body = case(surface, "atspi")
            name = f"atspi-{surface}${{3:+-$3}}.json"
            self.assertIn(f'ATSPI_TRANSCRIPT="/out/{name}"', body, surface)
            self.assertIn(f"golden/atspi/{name}", body, surface)
            self.assertIn("atspi_compare.py", body, surface)


if __name__ == "__main__":
    unittest.main()
