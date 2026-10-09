"""scripts/ci/build-builder.sh: only a pull_request run may build the builder with cached layers."""

import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "build-builder.sh"

# Stands in for podman: appends its arguments, one call per line, to $FAKE_LOG.
FAKE_PODMAN = """#!/usr/bin/env bash
echo "$*" >> "$FAKE_LOG"
"""


def podman_args(event):
    """The arguments the script gives podman when GITHUB_EVENT_NAME is EVENT (unset when None)."""
    with tempfile.TemporaryDirectory() as tmp:
        fake = pathlib.Path(tmp) / "podman"
        fake.write_text(FAKE_PODMAN)
        fake.chmod(0o755)
        log = pathlib.Path(tmp) / "log"
        env = {**os.environ, "PATH": f"{tmp}:{os.environ['PATH']}", "FAKE_LOG": str(log)}
        env.pop("GITHUB_EVENT_NAME", None)
        if event is not None:
            env["GITHUB_EVENT_NAME"] = event
        subprocess.run(["bash", str(SCRIPT)], env=env, check=True)
        return log.read_text().split()


class CacheTest(unittest.TestCase):
    def test_a_pull_request_keeps_the_layer_cache(self):
        args = podman_args("pull_request")
        self.assertNotIn("--no-cache", args)
        self.assertNotIn("--pull=always", args)

    def test_every_other_event_builds_fresh(self):
        for event in ("push", "schedule", "workflow_dispatch", "merge_group", None):
            with self.subTest(event=event):
                args = podman_args(event)
                self.assertIn("--no-cache", args)
                self.assertIn("--pull=always", args)

    def test_the_image_is_the_one_the_workflows_run(self):
        self.assertIn("localhost/azoth-builder", podman_args("push"))


if __name__ == "__main__":
    unittest.main()
