import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SHELL = Path(__file__).resolve().parent.parent


def digest(directory):
    return subprocess.run(
        ["bash", str(SHELL / "build-inputs.sh"), str(directory)],
        check=True, capture_output=True, text=True,
    ).stdout.strip()


class BuildInputs(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        shutil.copy(SHELL / "Containerfile", self.dir)

    def tearDown(self):
        self.tmp.cleanup()

    def test_stable_and_a_sha256(self):
        self.assertRegex(digest(self.dir), r"[0-9a-f]{64}")
        self.assertEqual(digest(self.dir), digest(self.dir))

    def test_changes_with_the_containerfile(self):
        before = digest(self.dir)
        with open(self.dir / "Containerfile", "a", encoding="utf-8") as f:
            f.write("RUN true\n")
        self.assertNotEqual(before, digest(self.dir))

    def test_changes_with_the_rig_base(self):
        before = digest(self.dir)
        (self.dir / "rig-image.digest").write_text("sha256:" + "0" * 64 + "\n")
        self.assertNotEqual(before, digest(self.dir))

    def test_a_missing_containerfile_fails(self):
        (self.dir / "Containerfile").unlink()
        with self.assertRaises(subprocess.CalledProcessError):
            digest(self.dir)

    @staticmethod
    def pinned_stages(text):
        """Stages whose FROM is pinned by a digest that build-inputs.sh hashes: a literal
        sha256, or an ARG whose default is a literal sha256 in the same Containerfile. An ARG
        without a default (or one rig.sh could override) is not a pin."""
        defaults = dict(re.findall(r"(?m)^ARG\s+(\w+)=(sha256:[0-9a-f]{64})\s*$", text))
        stages = []
        for ref, name in re.findall(r"(?im)^FROM\s+(\S+)\s+AS\s+(\S+)$", text):
            digest_part = ref.partition("@")[2]
            var = re.fullmatch(r"\$\{(\w+)\}", digest_part)
            if re.fullmatch(r"sha256:[0-9a-f]{64}", digest_part) or (var and var.group(1) in defaults):
                stages.append(name)
        return stages

    def test_the_containerfile_takes_nothing_from_the_context(self):
        # build-inputs.sh hashes only the Containerfile and rig-image.digest: a file copied or
        # mounted into the build would change the stage without changing the hash. A mount from a
        # stage the Containerfile pins by digest (the tier 2 repository) is in the hash already.
        text = (SHELL / "Containerfile").read_text(encoding="utf-8")
        pinned = self.pinned_stages(text)
        found = [
            line
            for line in re.findall(r"(?im)^\s*(?:COPY|ADD)\b.*$|--mount\b.*$", text)
            if not any(re.match(rf"--mount=type=bind,from={re.escape(s)},", line) for s in pinned)
        ]
        self.assertEqual(found, [], "add these inputs to build-inputs.sh, then update this test")

    def test_the_tier2_repository_is_pinned_by_a_default_in_the_containerfile(self):
        text = (SHELL / "Containerfile").read_text(encoding="utf-8")
        self.assertIn("tier2-repo", self.pinned_stages(text))

    def test_an_arg_without_a_default_is_not_a_pin(self):
        text = "ARG D\nFROM r/x@${D} AS tier2-repo\n"
        self.assertEqual(self.pinned_stages(text), [])
        self.assertEqual(self.pinned_stages("ARG D=sha256:" + "a" * 64 + "\nFROM r/x@${D} AS t\n"), ["t"])
        self.assertEqual(self.pinned_stages("FROM r/x@sha256:" + "b" * 64 + " AS t\n"), ["t"])

    def test_rig_sh_never_overrides_the_tier2_digest(self):
        # a --build-arg would change the stage without changing the hash of its inputs
        args = re.findall(r"--build-arg(?:\s+|=)\S+", (SHELL / "rig.sh").read_text(encoding="utf-8"))
        self.assertTrue(args)
        self.assertFalse([a for a in args if "TIER2_DIGEST" in a], args)


class SpecNevr(unittest.TestCase):
    def nevr(self, text):
        with tempfile.TemporaryDirectory() as d:
            spec = Path(d) / "x.spec"
            spec.write_text(text, encoding="utf-8")
            return subprocess.run(
                ["awk", "-f", str(SHELL / "spec_nevr.awk"), str(spec)],
                check=True, capture_output=True, text=True,
            ).stdout.strip()

    def test_the_real_spec(self):
        spec = (SHELL.parent.parent / "specs" / "cosmic-comp" / "cosmic-comp.spec").read_text(encoding="utf-8")
        macros = dict(re.findall(r"(?m)^%global\s+(\w+)\s+(.+?)\s*$", spec))
        version = re.search(r"(?m)^Version:\s*(\S+)", spec).group(1)
        release = re.search(r"(?m)^Release:\s*(\S+)", spec).group(1)
        release = re.sub(r"%\{(\w+)\}", lambda m: macros[m.group(1)], release)
        self.assertEqual(self.nevr(spec), f"{version}-{release}")

    def test_nested_macros_expand(self):
        self.assertEqual(self.nevr("%global a 2\n%global b %{a}.fc43\nVersion: 1\nRelease: %{b}.x\n"), "1-2.fc43.x")

    def test_an_undefined_macro_fails_closed(self):
        self.assertEqual(self.nevr("Version: 1\nRelease: %{nope}.x\n"), "")

    def test_a_self_reference_fails_closed(self):
        self.assertEqual(self.nevr("%global a %{a}\nVersion: 1\nRelease: %{a}\n"), "")

    def test_a_global_takes_the_text_after_its_name(self):
        self.assertEqual(self.nevr("%global a g\nVersion: 1\nRelease: %{a}\n"), "1-g")

    def test_version_expands_too(self):
        self.assertEqual(self.nevr("%global v 3.4\nVersion: %{v}\nRelease: 1\n"), "3.4-1")
        self.assertEqual(self.nevr("Version: %{v}\nRelease: 1\n"), "")

    def test_a_shell_macro_fails_closed(self):
        self.assertEqual(self.nevr("Version: 1\nRelease: %(echo x)\n"), "")

    def test_define_is_not_handled_and_fails_closed(self):
        self.assertEqual(self.nevr("%define a 2\nVersion: 1\nRelease: %{a}\n"), "")

    def test_a_missing_release_prints_nothing(self):
        self.assertEqual(self.nevr("Version: 1\n"), "")

    def test_only_the_first_version_and_release_count(self):
        self.assertEqual(self.nevr("Version: 1\nRelease: 2\n%package sub\nVersion: 9\nRelease: 8\n"), "1-2")

    def test_a_conditional_fails_closed(self):
        self.assertEqual(self.nevr("Version: 1\nRelease: %{?dist}\n"), "")

    def test_an_if_block_fails_closed(self):
        self.assertEqual(self.nevr("%if 0\nVersion: 5\n%endif\nVersion: 1\nRelease: 2\n"), "")

    def test_trailing_blanks_of_a_global_are_dropped(self):
        self.assertEqual(self.nevr("%global a 2 \t\nVersion: 1\nRelease: %{a}.x\n"), "1-2.x")


if __name__ == "__main__":
    unittest.main()
