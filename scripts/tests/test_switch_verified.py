"""Unit tests of scripts/switch-verified.sh (python3 -B -m unittest discover -s scripts/tests -v).

Only the refusals that come before anything touches the machine are tested here: the switch
itself needs root, bootc and a registry (docs/architecture/doc_update_delivery.md, UD51).
"""

import pathlib
import subprocess
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "switch-verified.sh"
REPOSITORY = "ghcr.io/owner/athanor-system"


def switch(image):
    return subprocess.run(
        ["bash", str(SCRIPT), image], capture_output=True, text=True, check=False
    )


class OnlyAChannel(unittest.TestCase):
    def test_a_run_tag_or_a_digest_is_refused_with_the_channel_to_use(self):
        for suffix in (":37691917204", "@sha256:" + "0" * 64):
            result = switch(REPOSITORY + suffix)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn(f"{suffix} is not a channel", result.stderr)
            self.assertIn(f"switch to {REPOSITORY}:latest", result.stderr)

    def test_a_channel_tag_passes_the_check(self):
        for tag in ("latest", "stable"):
            self.assertNotIn(
                "is not a channel", switch(f"{REPOSITORY}:{tag}").stderr, tag
            )


if __name__ == "__main__":
    unittest.main()
