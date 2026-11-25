"""scripts/ci/check-x86-64-v3.sh: passes on a loader that reports x86-64-v3 as supported, and fails
with the level and the CPU model otherwise, including when the loader prints no such line (#1508)."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "check-x86-64-v3.sh"
CPUINFO = "processor\t: 0\nmodel name\t: Fake CPU 9000\n"


def run(loader_output, loader_status=0, cpuinfo_text=CPUINFO):
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        loader = tmp / "ld.so"
        loader.write_text(
            f"#!/bin/sh\ncat <<'EOT'\n{loader_output}\nEOT\nexit {loader_status}\n"
        )
        loader.chmod(0o755)
        cpuinfo = tmp / "cpuinfo"
        if cpuinfo_text is not None:
            cpuinfo.write_text(cpuinfo_text)
        return subprocess.run(
            ["bash", str(SCRIPT)],
            env={
                **os.environ,
                "ATHANOR_LD_SO": str(loader),
                "ATHANOR_CPUINFO": str(cpuinfo),
            },
            capture_output=True,
            text=True,
            timeout=60,
        )


class CheckX8664V3(unittest.TestCase):
    def test_supported_passes(self):
        done = run(
            "  x86-64-v4\n  x86-64-v3 (supported, searched)\n  x86-64-v2 (supported, searched)"
        )
        self.assertEqual(done.returncode, 0, done.stderr)

    def test_listed_but_not_supported_fails_naming_level_and_cpu(self):
        done = run("  x86-64-v4\n  x86-64-v3\n  x86-64-v2 (supported, searched)")
        self.assertEqual(done.returncode, 1)
        self.assertIn("x86-64-v3", done.stderr)
        self.assertIn("Fake CPU 9000", done.stderr)

    def test_no_line_at_all_fails(self):
        done = run("Usage: ld.so [OPTION]...")
        self.assertEqual(done.returncode, 1)
        self.assertIn("no x86-64-v3 line", done.stderr)

    def test_missing_cpuinfo_still_names_the_level(self):
        done = run("  x86-64-v3\n", cpuinfo_text=None)
        self.assertEqual(done.returncode, 1)
        self.assertIn("below x86-64-v3", done.stderr)
        self.assertIn("unknown (", done.stderr)
        self.assertIn("unreadable)", done.stderr)

    def test_a_long_cpuinfo_names_the_first_model(self):
        text = "".join(f"processor\t: {n}\nmodel name\t: Fake CPU {n}\n\n" for n in range(256))
        done = run("  x86-64-v3\n", cpuinfo_text=text)
        self.assertEqual(done.returncode, 1, done.stderr)
        self.assertIn("below x86-64-v3", done.stderr)
        self.assertIn("Fake CPU 0.", done.stderr)

    def test_failing_loader_fails(self):
        self.assertEqual(run("", loader_status=3).returncode, 1)


if __name__ == "__main__":
    unittest.main()
