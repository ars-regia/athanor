"""The desktop session keeps cosmic-comp's key bindings and the greeter does not (#144)
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

A command on cosmic-comp's command line makes it a kiosk, which loads no key bindings.
athanor-session passes --keep-shortcuts, an option of Athanor's cosmic-comp patch; the
greeter must stay a kiosk, or a binding could spawn a program as the greeter's user.
"""

import shlex
import unittest
from pathlib import Path

SPECS = Path(__file__).resolve().parents[2]
BIN = SPECS / "athanor-system-config/SOURCES/usr/bin"
COMP = SPECS / "cosmic-comp"


def compositor_args(script):
    """The arguments of the cosmic-comp the script execs, up to its client command."""
    lines = [
        line
        for line in (BIN / script).read_text().splitlines()
        if line.startswith("exec ")
    ]
    words = shlex.split(lines[-1])
    return words[words.index("cosmic-comp") + 1 :]


class CompositorShortcuts(unittest.TestCase):
    def test_the_session_keeps_its_key_bindings(self):
        args = compositor_args("athanor-session")
        self.assertEqual(args, ["--keep-shortcuts", "/usr/bin/athanor-desktop"])

    def test_the_greeter_stays_a_kiosk_without_key_bindings(self):
        self.assertNotIn("--keep-shortcuts", compositor_args("athanor-greeter-session"))

    def test_the_cosmic_comp_build_applies_the_patch_that_adds_the_option(self):
        spec = (COMP / "cosmic-comp.spec").read_text()
        patches = [
            line.split()[1] for line in spec.splitlines() if line.startswith("Patch")
        ]
        adding = [
            name
            for name in patches
            if '+            Some("--keep-shortcuts") => {'
            in (COMP / "SOURCES" / name).read_text()
        ]
        self.assertEqual(len(adding), 1, patches)


if __name__ == "__main__":
    unittest.main()
