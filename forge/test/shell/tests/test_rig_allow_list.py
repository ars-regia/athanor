import re
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


def outside_the_rig(paths):
    return [path for path in paths if not path.relative_to(ROOT).as_posix().startswith("forge/test/")]


def packaged_inputs():
    """What builds a package or an image: every spec and Containerfile under forge/ and system/,
    the rig's own under forge/test/ aside, the scripts under system/, and the compositor's
    shipped data/."""
    inputs = outside_the_rig(
        path
        for top in ("forge", "system")
        for pattern in ("*.spec", "Containerfile*")
        for path in (ROOT / top).rglob(pattern)
    )
    inputs += sorted((ROOT / "system").rglob("*.sh"))
    return inputs + [path for path in DATA.iterdir() if path.is_file()]


def containerfiles():
    return outside_the_rig(path for top in ("forge", "system") for path in (ROOT / top).rglob("Containerfile*"))


def broad_copies(containerfile):
    """The COPY and ADD lines of the final stage of `containerfile` that take the build context's
    root, forge/ whole, or anything under forge/test/: each would carry forge/test/shell into
    the image. An earlier stage reaches the image only through the final stage's COPY --from."""
    # ponytail: a COPY --from of an earlier stage's whole tree is not followed; read the stage
    # if one ever takes more than built files.
    lines = containerfile.read_text(encoding="utf-8").splitlines()
    final = max((number for number, line in enumerate(lines) if re.match(r"\s*FROM\b", line, re.I)), default=0)
    found = []
    for number, line in enumerate(lines[final:], final + 1):
        words = line.split()
        if not words or words[0].upper() not in ("COPY", "ADD"):
            continue
        flags = [word for word in words[1:] if word.startswith("--")]
        if any(flag.startswith("--from") for flag in flags):
            continue
        sources = [word for word in words[1:] if not word.startswith("--")][:-1]
        for source in sources:
            path = source.removeprefix("./").rstrip("/")
            if path in ("", ".", "forge") or path == "forge/test" or path.startswith("forge/test/"):
                found.append(f"{containerfile}:{number}: {line.strip()}")
                break
    return found


def rig_includes(paths):
    """The include_str! and include_bytes! of `paths` whose file lies under forge/test/."""
    found = []
    for path in paths:
        text = path.read_text(encoding="utf-8")
        for match in re.finditer(r'include_(?:str|bytes)!\(\s*"([^"]+)"', text):
            target = (path.parent / match.group(1)).resolve()
            if target.is_relative_to(ROOT / "forge/test"):
                found.append(f"{path}: {match.group(0)}")
    return found


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

    def test_no_containerfile_copies_the_rig(self):
        files = containerfiles()
        self.assertIn(ROOT / "system/Containerfile", files)
        self.assertEqual([line for path in files for line in broad_copies(path)], [])

    def test_no_rust_source_includes_the_rig(self):
        sources = [path for top in ("forge/specs", "system") for path in (ROOT / top).rglob("*.rs")]
        self.assertTrue(sources)
        self.assertEqual(rig_includes(sources), [])

    def test_a_broad_copy_in_the_final_stage_is_found(self):
        with tempfile.TemporaryDirectory() as scratch:
            containerfile = Path(scratch) / "Containerfile"
            containerfile.write_text(
                "FROM builder AS build\nCOPY forge/ forge/\n"
                "FROM runtime\nCOPY --from=build /out/bin/ /usr/bin/\nCOPY --chmod=0644 ./ /src/\n",
                encoding="utf-8",
            )
            self.assertEqual(len(broad_copies(containerfile)), 1)
            self.assertIn(":5: ", broad_copies(containerfile)[0])

    def test_an_include_of_the_rig_is_found(self):
        with tempfile.TemporaryDirectory() as scratch:
            source = Path(scratch) / "lib.rs"
            source.write_text(f'const RIG: &str = include_str!("{RIG_LIST}");\n', encoding="utf-8")
            self.assertEqual(len(rig_includes([source])), 1)

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
