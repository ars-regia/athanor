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
# the workspace from it, and its runtime stage takes only `COPY --from=builder /out/bin/`. The
# exemption covers that stage's broad copies only, never what the scan cannot classify.
EXEMPT_STAGES = {("forge/build/Containerfile", "builder")}
# The rig's allow-list and every directory that holds it: a source equal to one of these, or a
# glob that matches one, carries the rig's files into the stage.
RIG_PREFIXES = ("forge", "forge/test", "forge/test/shell", f"forge/test/shell/{RIG_LIST.name}")
# A heredoc opener (<<WORD, <<-WORD, <<'WORD', <<"WORD") that is neither a here-string (<<<)
# nor a shift: arithmetic $(( ... )) is cut out of the text before this is searched.
HEREDOC = re.compile(r"(?<!<)<<(?!<)(-?)\s*(?:'([^']+)'|\"([^\"]+)\"|([A-Za-z0-9_.-]+))")


def instructions(containerfile):
    """The logical instructions of `containerfile`, as (first line number, stage, text, problem).
    Comment and blank lines are dropped, inside a continuation too, as the builder drops them;
    continuations are joined; the bodies of the heredocs of RUN, COPY and ADD are skipped. The
    stage is the name after AS of the FROM that opens it, or its index when it has none.
    `problem` names what the scan cannot follow, such as a heredoc with no terminator."""
    lines = containerfile.read_text(encoding="utf-8").splitlines()
    stage, stages, index = None, 0, 0
    while index < len(lines):
        number, text = index + 1, lines[index]
        index += 1
        if not text.strip() or text.lstrip().startswith("#"):
            continue
        while text.rstrip().endswith("\\") and index < len(lines):
            following = lines[index]
            index += 1
            if not following.strip() or following.lstrip().startswith("#"):
                continue
            text = text.rstrip()[:-1] + " " + following
        words = text.split()
        problem = None
        if words[0].upper() in ("RUN", "COPY", "ADD"):
            arithmetic = re.sub(r"\$\(\(.*?\)\)", " ", text)
            for dash, single, double, bare in HEREDOC.findall(arithmetic):
                marker = single or double or bare
                while index < len(lines):
                    body = lines[index]
                    index += 1
                    if (body.lstrip("\t") if dash else body) == marker:
                        break
                else:
                    problem = f"heredoc {marker} has no terminator"
        if words[0].upper() == "FROM":
            named = [word for position, word in enumerate(words) if position > 0 and words[position - 1].upper() == "AS"]
            stage = named[0] if named else str(stages)
            stages += 1
        yield number, stage, text.strip(), problem


def unclassified(text):
    """Why the scan cannot vouch for an instruction, or None: what it does not read fails the
    test rather than pass it."""
    words = text.split()
    keyword = words[0].upper()
    if keyword == "ONBUILD":
        return "ONBUILD runs in an image built later, out of this scan's sight"
    if keyword == "RUN":
        for mount in re.findall(r"--mount=(\S+)", text):
            options = dict(option.partition("=")[::2] for option in mount.split(","))
            if options.get("type", "bind") == "bind" and "from" not in options:
                return "RUN --mount of type bind without from= mounts the build context"
    return None


def copy_sources(text):
    """The sources of a COPY or ADD instruction, in its shell or its JSON form, and whether
    they come from another stage or image (--from). Raises ValueError on a form it cannot read."""
    words = text.split()
    rest = text.strip()[len(words[0]) :].strip()
    other_stage = False
    while rest.startswith("--"):
        flag, _, rest = rest.partition(" ")
        other_stage = other_stage or flag.startswith("--from")
        rest = rest.strip()
    arguments = json.loads(rest) if rest.startswith("[") else rest.split()
    if len(arguments) < 2 or not all(isinstance(argument, str) for argument in arguments):
        raise ValueError("no source and destination")
    return arguments[:-1], other_stage


def carries_the_rig(source):
    """Whether a COPY source names the context's root, forge/ whole, anything under
    forge/test/, or a glob that can match one of them."""
    path = posixpath.normpath(source.lstrip("/")) if source.strip("/") else "."
    if path == "." or path.startswith(".."):
        return True
    glob = re.search(r"[*?\[]", path)
    if glob:
        literal = path[: glob.start()]
        # fnmatch lets * cross a slash, unlike the builder's match: it flags more, never less.
        return (
            literal.startswith("forge/test/")
            or "forge/test".startswith(literal)
            or any(fnmatch.fnmatchcase(prefix, path) for prefix in RIG_PREFIXES)
        )
    return path in ("forge", "forge/test") or path.startswith("forge/test/")


def containerfile_findings(containerfile):
    """(stage, kind, line number, finding) for each instruction of `containerfile` that would
    carry forge/test/shell into a stage (kind "copy") or that the scan cannot classify (kind
    "unclassified")."""
    found = []
    for number, stage, text, problem in instructions(containerfile):
        where = f"{containerfile}:{number}"
        problem = problem or unclassified(text)
        keyword = text.split()[0].upper()
        if problem is None and keyword in ("COPY", "ADD"):
            try:
                sources, other_stage = copy_sources(text)
            except ValueError as error:
                problem = f"unreadable {keyword}: {error}"
            else:
                if any("$" in source for source in sources):
                    problem = f"a {keyword} source names a variable"
                elif keyword == "ADD" and any("://" in source or source.startswith("git@") for source in sources):
                    problem = "ADD from a URL or a git reference"
                elif not other_stage and any(carries_the_rig(source) for source in sources):
                    found.append((stage, "copy", number, f"{where}: {text}"))
        if problem:
            found.append((stage, "unclassified", number, f"{where}: {problem}: {text}"))
    return found


# Non-literal include!, include_str!, include_bytes! or #[path] arguments read by hand and found
# not to reach forge/test/, as "relative/path.rs: <the macro's text>". Empty today.
ALLOWED_INCLUDES = set()
INCLUDE = re.compile(r'(include(?:_str|_bytes)?!\s*\(\s*|#\[path\s*=\s*)("([^"\\]*)")?')


def rig_includes(paths):
    """The include!, include_str!, include_bytes! and #[path] of `paths` whose file lies under
    forge/test/, and those whose argument is not a plain string literal, unless allowed."""
    found = []
    for path in paths:
        text = path.read_text(encoding="utf-8")
        for match in INCLUDE.finditer(text):
            line_start = text.rfind("\n", 0, match.start()) + 1
            if text[line_start : match.start()].lstrip().startswith("//"):
                continue
            if match.group(2) is None:
                key = f"{path.relative_to(ROOT).as_posix() if path.is_relative_to(ROOT) else path}: {match.group(0).strip()}"
                if key not in ALLOWED_INCLUDES:
                    found.append(f"{path}: a non-literal argument the scan cannot resolve: {match.group(0).strip()}")
                continue
            target = (path.parent / match.group(3)).resolve()
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
            for stage, kind, _, finding in containerfile_findings(path):
                key = (path.relative_to(ROOT).as_posix(), stage)
                if kind == "copy" and key in EXEMPT_STAGES:
                    exempted.add(key)
                else:
                    found.append(finding)
        self.assertEqual(found, [])
        # An exemption that no longer matches anything is removed, not kept as a blind spot.
        self.assertEqual(exempted, EXEMPT_STAGES)

    def test_no_rust_source_includes_the_rig(self):
        sources = rust_sources()
        self.assertTrue(sources)
        self.assertEqual(rig_includes(sources), [])

    def findings(self, text):
        """(line number, kind) of each finding in a Containerfile of `text`."""
        with tempfile.TemporaryDirectory() as scratch:
            containerfile = Path(scratch) / "Containerfile"
            containerfile.write_text(text, encoding="utf-8")
            return [(number, kind) for _, kind, number, _ in containerfile_findings(containerfile)]

    def lines_found(self, text):
        found = self.findings(text)
        self.assertEqual({kind for _, kind in found} - {"copy"}, set(), found)
        return [str(number) for number, _ in found]

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

    def test_a_glob_is_flagged_by_its_literal_part(self):
        found = self.lines_found(
            "FROM base\nCOPY forge/test/sh*/ /src/\nCOPY forge/te?t /src/\nCOPY f[o]rge /src/\n"
            "COPY system/sc*/ /src/\nCOPY forge/config/*.json /src/\n"
        )
        self.assertEqual(found, ["2", "3", "4"])

    def test_a_heredoc_is_read_only_where_it_is_one(self):
        # A quoted marker is kept whole; a here-string and a shift open no heredoc; the body of
        # the COPY heredoc hides a COPY of the root.
        found = self.lines_found(
            "FROM base\n"
            "RUN cat <<<'COPY . /x' && echo $((1 << 3))\n"
            "COPY <<'END-X' /etc/note\nCOPY . /src/\nEND-X\n"
            "COPY forge/test/shell/ /src/\n"
        )
        self.assertEqual(found, ["6"])
        self.assertEqual(self.findings("FROM base\nRUN <<EOF\necho no end\n"), [(2, "unclassified")])

    def test_what_the_scan_cannot_read_fails(self):
        found = self.findings(
            "FROM base\n"
            "COPY ${SOURCE} /src/\n"
            "ADD https://example.org/a.tar /src/\n"
            "ADD git@example.org:a/b.git /src/\n"
            "RUN --mount=type=bind,source=.,target=/src make\n"
            "RUN --mount=source=forge,target=/src make\n"
            "ONBUILD COPY . /src/\n"
            "RUN --mount=type=bind,from=build,source=/,target=/b --mount=type=cache,dst=/c true\n"
            "COPY --from=build /out/bin/ /usr/bin/\n"
        )
        self.assertEqual(found, [(number, "unclassified") for number in range(2, 8)])

    def test_a_comment_inside_a_continuation_is_dropped(self):
        found = self.lines_found("FROM base\nCOPY system/keys/ \\\n# a note\n\n    forge/ /src/\nFROM next\n")
        self.assertEqual(found, ["2"])

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
            rig = RIG_LIST.parent
            source.write_text(
                f'const RIG: &str = include_str!("{RIG_LIST}");\n'
                f'include!("{rig}/x.rs");\n'
                f'#[path = "{rig}/y.rs"]\nmod y;\n'
                'include!(concat!(env!("OUT_DIR"), "/z.rs"));\n'
                '// include!(concat!("a", "b")) in a comment is prose.\n'
                'const FINE: &[u8] = include_bytes!("data/fine.bin");\n',
                encoding="utf-8",
            )
            found = rig_includes([source])
            self.assertEqual(len(found), 4, found)
            self.assertIn("non-literal", found[3])

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
