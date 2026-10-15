#!/usr/bin/env python3
"""report.sh: a directory that failed does not skip the next, and the status says so."""

import os
import pathlib
import subprocess
import tempfile

SCRIPT = pathlib.Path(__file__).resolve().parent / "report.sh"


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        (root / "a").mkdir()
        (root / "a" / "verdict.md").write_text("x")
        (root / "a" / "verdict.md").chmod(0)  # unreadable: cat fails
        (root / "b").mkdir()
        (root / "b" / "verdict.md").write_text("PASS-B\n")
        summary = root / "summary.md"
        env = {**os.environ, "GITHUB_STEP_SUMMARY": str(summary)}
        r = subprocess.run(["bash", str(SCRIPT), "a", "b", "missing"], cwd=root, env=env)
        assert r.returncode != 0, "a failed directory must fail the script"
        assert "PASS-B" in summary.read_text(), "the second directory was skipped"
        (root / "a" / "verdict.md").unlink()
        r = subprocess.run(["bash", str(SCRIPT), "a", "b", "missing"], cwd=root, env=env)
        assert r.returncode == 0, "empty and missing directories are skipped"
    print("all checks passed")


if __name__ == "__main__":
    main()
