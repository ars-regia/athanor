"""scripts/ci/kernel_unchanged.py: a pull request skips the kernel build only when its kernel
inputs, and what boot and kmod read, equal its base's; any failure means build required."""

import contextlib
import importlib.util
import io
import json
import pathlib
import re
import subprocess
import sys
import tempfile
import unittest

import yaml

ROOT = pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "scripts/ci"))
SCRIPT = ROOT / "scripts/ci/kernel_unchanged.py"
spec = importlib.util.spec_from_file_location("kernel_unchanged", SCRIPT)
unchanged = importlib.util.module_from_spec(spec)
spec.loader.exec_module(unchanged)

# A stand-in for build-inputs.py, which reads only its own directory.
STUB = """import json, pathlib
k = pathlib.Path(__file__).resolve().parent
print(json.dumps({"kernel_local": (k / "kernel-local").read_text()}))
"""
DIRECTORIES = {
    "forge/specs/azoth/boot",
    "forge/specs/azoth/microvm",
    "forge/specs/azoth/keys",
    "forge/specs/azoth/nvidia",
    ".github/actions/kvm",
}


def file_of(path):
    """A file at PATH, or in it when the CHECKED path is a directory."""
    return f"{path}/file" if path in DIRECTORIES else path


TREE = {
    "forge/specs/azoth/build-inputs.py": STUB,
    "forge/specs/azoth/kernel-local": "CONFIG_A=y\n",
    "forge/specs/azoth/KERNEL.md": "notes\n",
    "docs/note": "base\n",
    **{file_of(path): f"{path} 1\n" for path in unchanged.CHECKED},
}


def git(cwd, *args):
    return subprocess.run(
        ["git", "-C", str(cwd), *args], check=True, capture_output=True, text=True
    ).stdout.strip()


def commit(repo, message, files):
    for path, text in files.items():
        target = repo / path
        if text is None:
            target.unlink()
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "--allow-empty", "-m", message)
    return git(repo, "rev-parse", "HEAD")


class Case(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmp = pathlib.Path(tmp.name)

    def history(self, head, base=None):
        """origin: base, then head (the pull request's merge commit); checkout is a depth-1
        clone of head, as actions/checkout makes it, which must fetch the base itself."""
        root = pathlib.Path(tempfile.mkdtemp(dir=self.tmp))
        origin = root / "origin"
        origin.mkdir()
        git(origin, "init", "-q", "-b", "iso-v0")
        git(origin, "config", "user.email", "test@example.invalid")
        git(origin, "config", "user.name", "test")
        git(origin, "config", "uploadpack.allowAnySHA1InWant", "true")
        sha = commit(origin, "base", {**TREE, **(base or {})})
        commit(origin, "head", head)
        checkout = root / "checkout"
        git(
            root,
            "clone",
            "-q",
            "--depth=1",
            "--branch",
            "iso-v0",
            origin.as_uri(),
            str(checkout),
        )
        return sha, checkout


class DecisionTest(Case):
    def test_a_documentation_change_skips_the_build(self):
        base, checkout = self.history(
            {"docs/note": "head\n", "forge/specs/azoth/KERNEL.md": "more\n"}
        )
        decision = unchanged.check(checkout, base)
        self.assertTrue(decision["unchanged"], decision["reason"])
        self.assertEqual(decision["base"], base)
        self.assertEqual(set(decision["paths"]), set(unchanged.CHECKED))

    def test_different_kernel_inputs_build(self):
        # The negative control: the change the skip must never hide.
        base, checkout = self.history(
            {"forge/specs/azoth/kernel-local": "CONFIG_A=n\n"}
        )
        decision = unchanged.check(checkout, base)
        self.assertFalse(decision["unchanged"])
        self.assertEqual(
            decision["reason"], f"build-inputs.py differs from the base's {base}"
        )

    def test_each_commit_runs_its_own_build_inputs(self):
        # A new input at the head only: the base's script does not print it.
        script = STUB.replace("read_text()}", 'read_text(), "new": 1}')
        base, checkout = self.history({"forge/specs/azoth/build-inputs.py": script})
        self.assertIn(
            "build-inputs.py differs", unchanged.check(checkout, base)["reason"]
        )

    def test_what_boot_and_kmod_read_builds_when_it_changes(self):
        for path in unchanged.CHECKED:
            with self.subTest(path=path):
                base, checkout = self.history({file_of(path): "changed\n"})
                decision = unchanged.check(checkout, base)
                self.assertFalse(decision["unchanged"])
                self.assertEqual(
                    decision["reason"], f"{path} differs from the base's {base}"
                )

    def test_a_base_git_cannot_fetch_builds(self):
        _, checkout = self.history({"docs/note": "head\n"})
        decision = unchanged.check(checkout, "0" * 40)
        self.assertFalse(decision["unchanged"])
        self.assertTrue(
            decision["reason"].startswith("error: CalledProcessError"),
            decision["reason"],
        )

    def test_an_empty_base_builds(self):
        _, checkout = self.history({"docs/note": "head\n"})
        self.assertTrue(unchanged.check(checkout, "")["reason"].startswith("error: "))

    def test_a_build_inputs_that_fails_at_the_base_builds(self):
        base, checkout = self.history(
            {"forge/specs/azoth/build-inputs.py": STUB},
            base={"forge/specs/azoth/build-inputs.py": "raise SystemExit(1)\n"},
        )
        decision = unchanged.check(checkout, base)
        self.assertFalse(decision["unchanged"])
        self.assertTrue(
            decision["reason"].startswith("error: CalledProcessError"),
            decision["reason"],
        )

    def test_a_checked_path_missing_at_the_head_builds(self):
        base, checkout = self.history({"forge/specs/azoth/cmdline": None})
        decision = unchanged.check(checkout, base)
        self.assertFalse(decision["unchanged"])
        self.assertTrue(decision["reason"].startswith("error: "), decision["reason"])


class MainTest(Case):
    def main(self, checkout, base):
        out = self.tmp / "out"
        with contextlib.redirect_stdout(io.StringIO()) as printed:
            status = unchanged.main(
                ["--base", base, "--out", str(out), "--repo", str(checkout)]
            )
        return (
            status,
            printed.getvalue(),
            json.loads((out / "decision.json").read_text()),
            (out / "summary.md").read_text(),
        )

    def test_a_skip_says_the_push_build_of_the_base_publishes_the_kernel(self):
        base, checkout = self.history({"docs/note": "head\n"})
        status, printed, decision, summary = self.main(checkout, base)
        self.assertEqual(status, 0)
        self.assertTrue(decision["unchanged"])
        self.assertIn("::notice title=Kernel inputs::", printed)
        self.assertIn("identical to the base's", summary)
        self.assertIn(
            "The push build of the base builds and publishes this kernel", summary
        )

    def test_an_error_is_a_warning_and_exits_zero(self):
        _, checkout = self.history({"docs/note": "head\n"})
        status, printed, decision, summary = self.main(checkout, "0" * 40)
        self.assertEqual(status, 0)
        self.assertFalse(decision["unchanged"])
        self.assertIn("::warning title=Kernel inputs::build required: error: ", printed)
        self.assertIn("Build required: error: ", summary)


class CheckedTest(unittest.TestCase):
    def test_every_checked_path_exists(self):
        for path in unchanged.CHECKED:
            with self.subTest(path=path):
                self.assertTrue((ROOT / path).exists())

    def test_every_package_lock_the_build_checks_is_checked(self):
        # build.sh runs `lock.sh check`, which verifies the lock of every */*.packages.
        dirs = {p.parent for p in (ROOT / "forge/specs/azoth").glob("*/*.packages")}
        self.assertTrue(dirs)
        for d in dirs:
            path = d.relative_to(ROOT).as_posix()
            with self.subTest(path=path):
                self.assertIn(path, unchanged.CHECKED)

    def test_every_path_boot_and_kmod_name_is_checked(self):
        # The build-context directory forge/specs/azoth alone is what build-inputs.py covers.
        workflows = ROOT / ".github/workflows"
        boot = yaml.safe_load((workflows / "call-kernel.yml").read_text())["jobs"][
            "boot"
        ]
        text = json.dumps(boot) + (workflows / "nvidia-build.yml").read_text()
        found = re.findall(r"(?:forge/specs/azoth|system)/[\w./-]+", text)
        named = {path.rstrip(".") for path in found}
        self.assertTrue(named)
        for path in named:
            with self.subTest(path=path):
                self.assertTrue(
                    any(
                        path == c or path.startswith(c + "/") for c in unchanged.CHECKED
                    )
                )


if __name__ == "__main__":
    unittest.main()
