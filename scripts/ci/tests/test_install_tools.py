"""scripts/ci/install-tools.sh: every tool it installs is checked against a pinned digest."""

import os
import re
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "install-tools.sh"
REQUIREMENTS = SCRIPT.parent / "requirements.txt"
BUILD_REQUIREMENTS = SCRIPT.parent / "requirements-build.txt"

# Stands in for python3: `python3 -m venv DIR` makes a DIR/bin/pip that appends its arguments,
# one call per line, to DIR/pip-args.
FAKE_PYTHON = """#!/usr/bin/env bash
[[ $1 == -m && $2 == venv ]] || exit 1
mkdir -p "$3/bin"
printf '#!/usr/bin/env bash\\necho "$*" >> "%s/pip-args"\\n' "$3" > "$3/bin/pip"
chmod +x "$3/bin/pip"
"""

# Writes a payload that cannot match any pinned digest to the -o target.
FAKE_CURL = """#!/usr/bin/env bash
while [[ $# -gt 0 ]]; do
    if [[ $1 == -o ]]; then printf 'not the archive\\n' > "$2"; shift; fi
    shift
done
"""


class ChecksumTest(unittest.TestCase):
    def test_a_mismatching_download_stops_the_install(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = pathlib.Path(tmp) / "fake"
            fake.mkdir()
            (fake / "curl").write_text(FAKE_CURL)
            (fake / "curl").chmod(0o755)
            out = pathlib.Path(tmp) / "tools"
            env = {**os.environ, "LC_ALL": "C", "PATH": f"{fake}:{os.environ['PATH']}"}
            run = subprocess.run(
                ["bash", str(SCRIPT), str(out)],
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(run.returncode, 0)
            self.assertIn("FAILED", run.stdout + run.stderr)
            self.assertFalse((out / "bin" / "just").exists())

    def test_python_packages_are_installed_by_hash_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = pathlib.Path(tmp) / "fake"
            fake.mkdir()
            for name, text in (("curl", FAKE_CURL), ("python3", FAKE_PYTHON)):
                (fake / name).write_text(text)
                (fake / name).chmod(0o755)
            out = pathlib.Path(tmp) / "tools"
            env = {**os.environ, "LC_ALL": "C", "PATH": f"{fake}:{os.environ['PATH']}"}
            subprocess.run(["bash", str(SCRIPT), str(out)], env=env, capture_output=True)
            calls = [line.split() for line in (out / "pip-args").read_text().splitlines()]
        # The build backend first, from wheels; then the tools, built against it.
        self.assertEqual(len(calls), 2)
        for args, requirements in zip(calls, (BUILD_REQUIREMENTS, REQUIREMENTS)):
            self.assertIn("--require-hashes", args)
            self.assertEqual(args[args.index("-r") + 1], str(requirements))
        self.assertIn("--only-binary=:all:", calls[0])
        self.assertIn("--no-build-isolation", calls[1])

    def test_every_requirement_is_pinned_with_a_hash(self):
        entries = [
            entry
            for path in (REQUIREMENTS, BUILD_REQUIREMENTS)
            for entry in re.split(r"\n(?=\S)", path.read_text().strip())
        ]
        names = set()
        for entry in entries:
            if entry.startswith("#"):
                continue
            with self.subTest(entry=entry.split()[0]):
                self.assertRegex(entry, r"^[A-Za-z0-9._-]+==[^\s\\]+ \\")
                self.assertRegex(entry, r"--hash=sha256:[0-9a-f]{64}")
                names.add(entry.split("==")[0].lower())
        self.assertLessEqual({"pykickstart", "pyyaml", "setuptools"}, names)

    def test_the_directory_is_required(self):
        run = subprocess.run(["bash", str(SCRIPT)], capture_output=True, text=True)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("usage", run.stderr)


if __name__ == "__main__":
    unittest.main()
