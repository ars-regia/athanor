import fnmatch
import json
import posixpath
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
    inputs += rust_sources()
    return inputs + [path for path in DATA.iterdir() if path.is_file()]


def rust_sources():
    return sorted(path for top in ("forge/specs", "system", "forge/tools") for path in (ROOT / top).rglob("*.rs"))


def containerfiles():
    return outside_the_rig(path for top in ("forge", "system") for path in (ROOT / top).rglob("Containerfile*"))


# The one stage allowed to copy forge/ whole: the builder of forge/build/Containerfile compiles
# the workspace from it, and its runtime stage takes only `COPY --from=builder /out/bin/`.
EXEMPT_STAGES = {("forge/build/Containerfile", "builder")}
# The rig's allow-list and every directory that holds it: a source equal to one of these, or a
# glob that matches one, carries the rig's files into the stage.
RIG_PREFIXES = ("forge", "forge/test", "forge/test/shell", f"forge/test/shell/{RIG_LIST.name}")


def instructions(containerfile):
    """The logical instructions of `containerfile` as (first line number, stage, text): comment
    and blank lines dropped, continuation lines joined, heredoc bodies skipped. The stage is the
    name after AS of the FROM that opens it, or its index when it has none."""
    lines = containerfile.read_text(encoding="utf-8").splitlines()
    stage, stages, index = None, 0, 0
    while index < len(lines):
        number, text = index + 1, lines[index]
        index += 1
        if not text.strip() or text.lstrip().startswith("#"):
            continue
        while text.rstrip().endswith("\\") and index < len(lines):
            text = text.rstrip()[:-1] + " " + lines[index]
            index += 1
        # A heredoc body (RUN <<EOF ... EOF) is data, never an instruction: a FROM or COPY
        # inside one is skipped with it.
        for marker in re.findall(r"<<-?\s*[\"']?(\w+)[\"']?", text):
            while index < len(lines) and lines[index].strip() != marker:
                index += 1
            index += 1
        words = text.split()
        if words[0].upper() == "FROM":
            named = [word for position, word in enumerate(words) if position > 0 and words[position - 1].upper() == "AS"]
            stage = named[0] if named else str(stages)
            stages += 1
        yield number, stage, text.strip()


def copy_sources(text):
    """The sources of a COPY or ADD instruction, in its shell or its JSON form; none for
    COPY --from, whose sources come from another stage or image."""
    words = text.split()
    if words[0].upper() not in ("COPY", "ADD"):
        return []
    rest = text.strip()[len(words[0]) :].strip()
    while rest.startswith("--"):
        flag, _, rest = rest.partition(" ")
        if flag.startswith("--from"):
            return []
        rest = rest.strip()
    arguments = json.loads(rest) if rest.startswith("[") else rest.split()
    return arguments[:-1]


def carries_the_rig(source):
    """Whether a COPY source names the context's root, forge/ whole, anything under
    forge/test/, or a glob that can match one of them."""
    path = posixpath.normpath(source.lstrip("/")) if source.strip("/") else "."
    if path == "." or path.startswith(".."):
        return True
    if any(character in path for character in "*?["):
        # fnmatch lets * cross a slash, unlike the builder's match: it flags more, never less.
        return any(fnmatch.fnmatchcase(prefix, path) for prefix in RIG_PREFIXES)
    return path in ("forge", "forge/test") or path.startswith("forge/test/")


def broad_copies(containerfile):
    """The COPY and ADD instructions of every stage of `containerfile` that would carry
    forge/test/shell into it, as (stage, "path:line: text")."""
    return [
        (stage, f"{containerfile}:{number}: {text}")
        for number, stage, text in instructions(containerfile)
        if any(carries_the_rig(source) for source in copy_sources(text))
    ]


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
        # A Rust comment names the rig's scripts for the reader and never reaches a binary.
        and not (path.suffix == ".rs" and line.lstrip().startswith("//"))
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
        found, exempted = [], set()
        for path in files:
            for stage, line in broad_copies(path):
                key = (path.relative_to(ROOT).as_posix(), stage)
                if key in EXEMPT_STAGES:
                    exempted.add(key)
                else:
                    found.append(line)
        self.assertEqual(found, [])
        # An exemption that no longer matches anything is removed, not kept as a blind spot.
        self.assertEqual(exempted, EXEMPT_STAGES)

    def test_no_rust_source_includes_the_rig(self):
        sources = rust_sources()
        self.assertTrue(sources)
        self.assertEqual(rig_includes(sources), [])

    def lines_found(self, text):
        with tempfile.TemporaryDirectory() as scratch:
            containerfile = Path(scratch) / "Containerfile"
            containerfile.write_text(text, encoding="utf-8")
            return [line.split(": ", 1)[0].rsplit(":", 1)[1] for _, line in broad_copies(containerfile)]

    def test_a_broad_copy_in_any_stage_is_found(self):
        # system/Containerfile's shape: the content lives in a named stage that later stages
        # reach through a build argument, never in the final one.
        found = self.lines_found(
            "FROM base AS system\nCOPY system/scripts/ /scripts/\nCOPY forge/ /src/forge/\n"
            "FROM ${SYSTEM_IMAGE} AS gpu-none\nFROM gpu-${GPU} AS final\n"
            "COPY --from=system /out/bin/ /usr/bin/\nCOPY --chmod=0644 ./ /src/\n"
        )
        self.assertEqual(found, ["3", "7"])

    def test_copy_forms_are_parsed(self):
        found = self.lines_found(
            "FROM base\n"
            "COPY system/keys/ \\\n    forge/test/shell/ /src/\n"
            'COPY ["forge/test/shell/rig-privileged-clients", "/etc/"]\n'
            "COPY forge/* /src/\n"
            "COPY system/* /src/\n"
            "ADD forge/./test /src/\n"
            "COPY forge/config/packages.json /usr/share/\n"
        )
        self.assertEqual(found, ["2", "4", "5", "7"])

    def test_a_heredoc_body_is_not_read_as_instructions(self):
        found = self.lines_found(
            "FROM base AS one\nRUN <<EOF\nFROM other AS two\nCOPY . /src/\nEOF\nCOPY system/ /system/\n"
        )
        self.assertEqual(found, [])

    def test_a_rust_string_naming_the_rig_is_found(self):
        with tempfile.TemporaryDirectory() as scratch:
            source = Path(scratch) / "lib.rs"
            source.write_text(
                "//! Run forge/test/shell/rig.sh to see it.\n"
                'const RIG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../forge/test/shell/x"));\n',
                encoding="utf-8",
            )
            self.assertEqual(len(rig_references([source])), 1)

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
