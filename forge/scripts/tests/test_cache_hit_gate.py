"""Issue #1176: no path reads a registry tag as a cache hit, or as the image to take RPMs from,
without verify_image.sh checking the signature of the digest the tag names. The behaviour is
tested next to each script; this test fails when a new path skips the gate
(python3 -B -m unittest discover -s forge/scripts/tests -v).

The scan is a heuristic, not a proof: it reads shell with regular expressions and Python with
the tokenizer and the AST, and a determined author can still build a call it does not see (a
command assembled in a variable, `bash -c`, a string run through eval). It stops the slips the
tests below name: a mention in a comment, a string or a docstring, a dead assignment of the
path, and a hash tag spelled so that no pattern finds it. The behaviour tests are the guard."""

import ast
import io
import pathlib
import re
import tokenize
import unittest

REPO = pathlib.Path(__file__).resolve().parents[3]
# Where the pipeline's code lives: scripts, workflows, actions, recipes, container builds, Nix
# and the Rust and JavaScript tools. Unit tests and documents are not read.
ROOTS = ("forge", "system", "scripts", ".github", "Justfile", "flake.nix")
SUFFIXES = {".sh", ".py", ".yml", ".yaml", ".nix", ".rs", ".toml", ".mjs", ".js", ""}
GATE = {"forge/scripts/verify_image.sh", "forge/scripts/registry_probe.sh"}
# registry_probe.sh answers only whether a tag exists. verify_image.sh is the gate built on it;
# check_idempotency.sh asks it directly for the builder alone, an image no run signs yet, and
# check_idempotency_test.sh tests it against the live registry, and the CI path filter of
# scripts/ci/changes.py names it without running it. Comments do not count.
PROBE_USERS = {
    "forge/scripts/verify_image.sh",
    "forge/scripts/check_idempotency.sh",
    "forge/scripts/check_idempotency_test.sh",
    "scripts/ci/changes.py",
}
PROBE_USE = re.compile(r"registry_probe\.sh")
# A file that names a package image's hash tag reads it, and so must pass it through the gate:
# only tag_signed_image.sh writes the tag, after signing the digest it points the tag at. The
# token "hash-" counts anywhere in code, however the tag is put together (":hash-<h>",
# "hash-" + h, P=hash-, hash-${h}); "--hash-only" and git's "hash-object" are other words.
HASH_TAG = re.compile(r"(?<![\w-])hash-(?!object\b)")
HASH_TAG_WRITERS = {"forge/scripts/tag_signed_image.sh"}
# A call of the gate: verify_image.sh is the command run, at the start of a command (after an
# optional bash or exec), or after && || ; | ! if then do else elif while until $( ` or (, or
# the first word of an array that is run later (PROBE_CMD=(bash ... verify_image.sh)). The path
# may hold a quoted word or a $(...) ("$(dirname "${BASH_SOURCE[0]}")/verify_image.sh"). A
# mention in a string, an assignment of the path and an argument of another command are not.
_WORD = r"""(?:\$\((?:[^()]|\([^()]*\))*\)|[^\s;|&()<>=])*"""
GATE_CALL = re.compile(
    r"(?:^|&&|\|\||[;|!`(]|\$\(|\b(?:if|then|do|else|elif|while|until)\b)\s*"
    r"(?:(?:(?:bash|sh|exec)\s+)*|\w+=\(\s*bash\s+)"
    + _WORD
    + r"""verify_image\.sh["']?(?=[\s;|&)"']|$)""",
    re.M,
)
SHELL_SUFFIXES = {".sh", ".yml", ".yaml", ".nix", ".toml", ""}
SUBPROCESS_CALLS = {"run", "call", "check_call", "check_output", "Popen"}
COMMENT = re.compile(r"^\s*//")


def strip_shell_comment(line):
    """LINE up to a comment: a # outside quotes that starts a word. ${#x}, $# and a # inside a
    string are not comments."""
    quote = None
    i = 0
    while i < len(line):
        c = line[i]
        if quote == "'":
            if c == "'":
                quote = None
        elif quote == '"':
            if c == "\\":
                i += 1
            elif c == '"':
                quote = None
        elif c == "\\":
            i += 1
        elif c in "'\"":
            quote = c
        elif c == "#" and (i == 0 or line[i - 1] in " \t;|&("):
            return line[:i]
        i += 1
    return line


def strip_python_comments(text):
    """TEXT without its # comments, found by the tokenizer, so that a # in a string stays."""
    lines = text.splitlines()
    for tok in tokenize.generate_tokens(io.StringIO(text).readline):
        if tok.type == tokenize.COMMENT:
            row, col = tok.start
            lines[row - 1] = lines[row - 1][:col]
    return "\n".join(lines)


def code(text, suffix=".sh"):
    """TEXT without its comments, which never call anything. A line continued with a backslash
    is joined to the next, so that a command split over lines is one command."""
    if suffix == ".py":
        return strip_python_comments(text)
    strip = strip_shell_comment if suffix in SHELL_SUFFIXES else lambda ln: "" if COMMENT.match(ln) else ln
    lines = [strip(ln) for ln in text.splitlines()]
    return re.sub(r"\\\n", " ", "\n".join(lines))


def _python_gate_call(text):
    """Whether a subprocess call has the gate as argv[0], or as argv[1] after bash or sh: a
    string constant ending in verify_image.sh, possibly inside os.path.join."""
    for node in ast.walk(ast.parse(text)):
        if not isinstance(node, ast.Call):
            continue
        func = node.func
        name = func.attr if isinstance(func, ast.Attribute) else getattr(func, "id", "")
        argv = node.args[0] if node.args else next((k.value for k in node.keywords if k.arg == "args"), None)
        if name not in SUBPROCESS_CALLS or not isinstance(argv, (ast.List, ast.Tuple)) or not argv.elts:
            continue
        first = argv.elts[0]
        slot = first
        if isinstance(first, ast.Constant) and first.value in ("bash", "sh") and len(argv.elts) > 1:
            slot = argv.elts[1]
        if any(
            isinstance(n, ast.Constant) and isinstance(n.value, str) and n.value.endswith("verify_image.sh")
            for n in ast.walk(slot)
        ):
            return True
    return False


def gate_called(rel, text):
    """Whether the code TEXT of the file REL runs verify_image.sh."""
    if rel.endswith(".py"):
        return _python_gate_call(text)
    return bool(GATE_CALL.search(text))


def ungated_reader(rel, text):
    """Whether the file REL names a hash tag without running the gate."""
    return bool(HASH_TAG.search(text)) and not gate_called(rel, text)


def pipeline_files():
    for root in ROOTS:
        path = REPO / root
        candidates = (
            [path]
            if path.is_file()
            else sorted(p for p in path.rglob("*") if p.is_file())
        )
        for file in candidates:
            rel = file.relative_to(REPO).as_posix()
            if file.suffix not in SUFFIXES and file.name not in ("Justfile", "Containerfile"):
                continue
            if "/tests/" not in f"/{rel}" and rel not in GATE:
                yield rel, code(file.read_text(errors="replace"), file.suffix)


class CacheHitGateTest(unittest.TestCase):
    def test_the_scan_sees_the_hit_paths(self):
        names = {rel for rel, _ in pipeline_files()}
        for rel in (
            "forge/scripts/dag_orchestrator.py",
            "forge/scripts/check_idempotency.sh",
            "forge/scripts/resolve_node_image.sh",
            "forge/scripts/fetch_repo_rpms.sh",
        ):
            self.assertIn(rel, names)

    def test_only_the_gate_asks_the_registry_whether_a_tag_exists(self):
        users = {
            rel
            for rel, text in pipeline_files()
            if PROBE_USE.search(text)
        }
        self.assertEqual(PROBE_USERS - GATE, users)

    def test_the_builder_is_the_only_unverified_hit(self):
        text = code((REPO / "forge/scripts/check_idempotency.sh").read_text(), ".sh")
        branch = re.search(
            r'"builder" \]\]; then\n  PROBE_CMD=\((.*?)\)\n.*?\nelse\n  PROBE_CMD=\((.*?)\)\n',
            text,
            re.S,
        )
        self.assertIsNotNone(
            branch, "check_idempotency.sh no longer chooses its probe by package"
        )
        self.assertIn("registry_probe.sh", branch.group(1))
        self.assertIn("verify_image.sh", branch.group(2))
        self.assertEqual(1, len(PROBE_USE.findall(text)))

    def test_every_reader_of_a_hash_tag_verifies_it(self):
        for rel, text in pipeline_files():
            if rel in HASH_TAG_WRITERS:
                continue
            with self.subTest(file=rel):
                self.assertFalse(ungated_reader(rel, text))

    def test_a_comment_or_a_mention_is_not_a_call_of_the_gate(self):
        text = code('# then verify_image.sh checks it\nref=$r:hash-$h  # verify_image.sh\n', ".sh")
        self.assertRegex(text, HASH_TAG)
        self.assertNotRegex(text, GATE_CALL)
        for spelling in ('"hash-" + h', "'hash-'", "hash-${h}", "hash-$h", ":hash-<h>", "P=hash-"):
            self.assertRegex(spelling, HASH_TAG)
        for other in ("--hash-only content-hash", "g hash-object -w"):
            self.assertNotRegex(other, HASH_TAG)

    def test_the_four_evasions_are_caught(self):
        reader = 'ref="$r:hash-$h"\n'
        evasions = {
            "trailing comment": (".sh", reader + 'true # "$here/verify_image.sh"\n'),
            "dead assignment": (".sh", 'gate="$here/verify_image.sh"\n' + reader),
            "echo string": (".sh", 'echo "run $here/verify_image.sh first"\n' + reader),
            "docstring": (".py", '"""Calls verify_image.sh."""\nref = f"{r}:hash-{h}"\n'),
            "python assignment": (".py", 'gate = "verify_image.sh"\nref = f"{r}:hash-{h}"\n'),
            "python echo argv": (".py", 'subprocess.run(["echo", "verify_image.sh"])\nref = f"{r}:hash-{h}"\n'),
            "hash- after =": (".sh", 'P=hash-\nskopeo inspect "docker://$img:$P$h"\n'),
        }
        for name, (suffix, text) in evasions.items():
            with self.subTest(evasion=name):
                self.assertTrue(ungated_reader(f"x{suffix}", code(text, suffix)))

    def test_a_real_call_of_the_gate_is_seen(self):
        reader = 'ref="$r:hash-$h"\n'
        calls = (
            'v=$(bash "$here/verify_image.sh" "$ref" w)\n',
            'v=$(bash "$(dirname "${BASH_SOURCE[0]}")/verify_image.sh" "$ref" w)\n',
            'if ! bash "$here/verify_image.sh" "$ref" w; then exit 1; fi\n',
            'exec "$here/verify_image.sh" "$ref" w\n',
            'true && bash "$here/verify_image.sh" "$ref" w\n',
            'PROBE_CMD=(bash "$S/verify_image.sh" "$ref" w)\n',
            'bash \\\n  "$here/verify_image.sh" "$ref" w\n',
        )
        for call in calls:
            with self.subTest(call=call):
                self.assertFalse(ungated_reader("x.sh", code(reader + call, ".sh")))
        for text in (
            'subprocess.run(["bash", os.path.join(D, "verify_image.sh"), ref, "w"], text=True)',
            'r = subprocess.run(["bash", "verify_image.sh", ref])',
        ):
            with self.subTest(python=text):
                self.assertFalse(ungated_reader("x.py", code('ref = f"{r}:hash-{h}"\n' + text, ".py")))

    def test_a_hash_sign_is_a_comment_only_where_the_shell_says_so(self):
        self.assertEqual('n=${#x} ', strip_shell_comment("n=${#x} # c"))
        self.assertEqual('echo "a # b" $# ', strip_shell_comment('echo "a # b" $# # c'))
        self.assertEqual("echo 'it'\\''s # x'", strip_shell_comment("echo 'it'\\''s # x'"))
        self.assertEqual("x = '#'", strip_python_comments("x = '#'  # c").rstrip())

    def test_the_brain_takes_only_a_signed_image_as_a_hit(self):
        text = code((REPO / "forge/scripts/dag_orchestrator.py").read_text(), ".py")
        body = re.search(r"\ndef image_exists\(ref\):\n(.*?)\n\n\n", text, re.S)
        self.assertIsNotNone(body)
        self.assertIn('"verify_image.sh"), ref, "call-dag-compile.yml"', body.group(1))
        self.assertIn('return verdict[0] == "signed"', body.group(1))


if __name__ == "__main__":
    unittest.main()
