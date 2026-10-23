import tempfile
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


def packaged_inputs():
    """What builds a package or an image: every spec and Containerfile under forge/ and system/,
    the rig's own under forge/test/ aside, and the compositor's shipped data/."""
    inputs = [
        path
        for top in ("forge", "system")
        for pattern in ("*.spec", "Containerfile*")
        for path in (ROOT / top).rglob(pattern)
        if not path.relative_to(ROOT).as_posix().startswith("forge/test/")
    ]
    return inputs + [path for path in DATA.iterdir() if path.is_file()]


def rig_references(paths):
    """The lines of `paths` that name the rig's allow-list or the rig's directory."""
    needles = (RIG_LIST.name, "forge/test/shell")
    return [
        f"{path}:{number}: {line.strip()}"
        for path in paths
        for number, line in enumerate(path.read_text(encoding="utf-8", errors="replace").splitlines(), 1)
        if any(needle in line for needle in needles)
    ]


class TestRigAllowList(unittest.TestCase):
    """The rig's own grants (grim, wtype, the toolkit's test input method) never ship."""

    def test_rig_lines_never_ship(self):
        shipped = grants(DATA / "privileged-clients")
        shipped_executables = {fields[0] for fields in shipped}
        for fields in grants(RIG_LIST):
            self.assertNotIn(fields, shipped)
            self.assertNotIn(fields[0], shipped_executables)

    def test_no_packaged_input_names_the_rig(self):
        inputs = packaged_inputs()
        self.assertTrue(any(path.name.endswith(".spec") for path in inputs))
        self.assertTrue(any(path.name.startswith("Containerfile") for path in inputs))
        self.assertEqual(rig_references(inputs), [])

    def test_a_packaged_input_naming_the_rig_list_is_found(self):
        spec = next(path for path in packaged_inputs() if path.name.endswith(".spec"))
        with tempfile.TemporaryDirectory() as scratch:
            copy = Path(scratch) / spec.name
            copy.write_text(
                spec.read_text(encoding="utf-8") + f"install -m 0644 {RIG_LIST.name} %{{buildroot}}\n",
                encoding="utf-8",
            )
            self.assertEqual(len(rig_references([copy])), 1)

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
