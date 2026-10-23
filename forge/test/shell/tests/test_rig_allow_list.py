import unittest
from pathlib import Path

SHELL = Path(__file__).resolve().parent.parent
ROOT = SHELL.parent.parent.parent
DATA = ROOT / "forge/specs/athanor-compositor/athanor-compositor-1.0.0/data"
RIG_LIST = SHELL / "rig-privileged-clients"


def grants(path):
    """The grant lines of an allow-list, as lists of fields; `#` starts a comment line."""
    lines = (line.strip() for line in path.read_text(encoding="utf-8").splitlines())
    return [line.split() for line in lines if line and not line.startswith("#")]


def grantable():
    """The interfaces the allow-list may name: the privileged and layer-gated rows of data/globals."""
    return {
        interface
        for interface, kind in grants(DATA / "globals")
        if kind in ("privileged", "layer-gated")
    }


class TestRigAllowList(unittest.TestCase):
    """The rig's own grants (grim, wtype, the toolkit's test input method) never ship."""

    def test_rig_lines_never_ship(self):
        shipped = grants(DATA / "privileged-clients")
        shipped_executables = {fields[0] for fields in shipped}
        for fields in grants(RIG_LIST):
            self.assertNotIn(fields, shipped)
            self.assertNotIn(fields[0], shipped_executables)

    def test_the_rig_list_is_under_no_packaged_path(self):
        relative = RIG_LIST.relative_to(ROOT).as_posix()
        self.assertTrue(relative.startswith("forge/test/shell/"), relative)
        for packaged in ("forge/specs/", "system/"):
            self.assertFalse(relative.startswith(packaged), relative)

    def test_rig_lines_parse(self):
        lines = grants(RIG_LIST)
        self.assertTrue(lines)
        allowed = grantable()
        for executable, unit, *interfaces in lines:
            self.assertTrue(executable.startswith("/usr/bin/"), executable)
            self.assertNotIn("..", Path(executable).parts)
            self.assertTrue(unit.endswith(".service") and "/" not in unit, unit)
            self.assertTrue(interfaces, executable)
            for interface in interfaces:
                self.assertIn(interface, allowed)


if __name__ == "__main__":
    unittest.main()
