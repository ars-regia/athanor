"""Unit tests of system/tier-digests.sh against the offline registry of fake_registry.py
(python3 -B -m unittest discover -s system/tests -v)."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "tier-digests.sh"
DIGESTS = {f"tier{n}": "sha256:" + "0123"[n] * 64 for n in range(4)}


class TierDigests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        (bin_dir / "skopeo").symlink_to(HERE / "fake_registry.py")
        self.fixture = self.dir / "registry.json"
        self.env = {
            k: v
            for k, v in os.environ.items()
            if k not in ("REGISTRY_HOST", "GITHUB_REPOSITORY_OWNER")
        }
        self.env.update(
            PATH=f"{bin_dir}:{os.environ['PATH']}",
            FAKE_REGISTRY=str(self.fixture),
            FAKE_LOG=str(self.dir / "calls.log"),
            TIER_DIGESTS_DIR=str(self.dir / "out"),
            RETRY_ATTEMPTS="1",
        )
        self.out = self.dir / "out" / "tier-digests.json"

    def tearDown(self):
        self.tmp.cleanup()

    def publish(self, registry, **digests):
        tags = {
            f"{registry}/athanor-forge-{tier}-repo:latest": d
            for tier, d in digests.items()
        }
        self.fixture.write_text(json.dumps({"tags": tags}))

    def resolve(self, *args):
        return subprocess.run(
            ["bash", str(SCRIPT), *args], capture_output=True, text=True, env=self.env
        )

    def test_resolve_records_the_registry_and_the_digest_each_tier_names(self):
        self.publish("ghcr.io/ars-regia", **DIGESTS)
        r = self.resolve("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            json.loads(self.out.read_text()),
            {"registry": "ghcr.io/ars-regia", **DIGESTS},
        )

    def test_the_registry_follows_the_variables_in_lower_case(self):
        self.publish("registry.example/hr-mes", **DIGESTS)
        self.env.update(
            REGISTRY_HOST="registry.example", GITHUB_REPOSITORY_OWNER="HR-MES"
        )
        r = self.resolve("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            json.loads(self.out.read_text())["registry"], "registry.example/hr-mes"
        )

    def test_a_tier_that_cannot_be_read_writes_no_file(self):
        self.publish(
            "ghcr.io/ars-regia", **{k: v for k, v in DIGESTS.items() if k != "tier2"}
        )
        r = self.resolve("resolve")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("athanor-forge-tier2-repo:latest", r.stderr)
        self.assertFalse(self.out.exists())

    def test_anything_but_resolve_is_a_usage_error(self):
        for args in ((), ("get",), ("resolve", "extra")):
            with self.subTest(args=args):
                r = self.resolve(*args)
                self.assertEqual(r.returncode, 2)
                self.assertIn("resolve", r.stderr)


if __name__ == "__main__":
    unittest.main()
