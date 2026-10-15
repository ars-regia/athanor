"""fetch_sources.sh reads the spec through rpmspec: a spec that does not parse fails the
script (issue #497) instead of reading as a spec with no remote source. rpmspec and curl
are stubs on PATH."""

import hashlib
import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "forge/scripts/fetch_sources.sh"
PAYLOAD = b"payload\n"


class FetchSources(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmp = pathlib.Path(tmp.name)
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        self.spec_dir = self.tmp / "spec"
        (self.spec_dir / "SOURCES").mkdir(parents=True)
        (self.spec_dir / "pkg.spec").write_text("Name: pkg\n")
        self.out = self.tmp / "out"

    def stub(self, name, body):
        path = self.bin / name
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)

    def run_script(self):
        env = dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}")
        return subprocess.run(
            ["bash", str(SCRIPT), str(self.spec_dir), str(self.out)],
            env=env, capture_output=True, text=True, check=False,
        )

    def manifest(self, digest):
        (self.spec_dir / "SOURCES/sources.sha256").write_text(f"{digest}  a.tar.gz\n")

    def remote_spec(self):
        self.stub("rpmspec", "echo 'Source0: https://example.invalid/a.tar.gz'\n")
        # curl -o <file> <url>: write the payload to the -o target.
        self.stub("curl", f'while [ "$1" != -o ]; do shift; done; printf "payload\\n" > "$2"\n')

    def test_rpmspec_failure_fails_the_script(self):
        self.stub("rpmspec", "echo 'error: bad spec' >&2\nexit 1\n")
        r = self.run_script()
        self.assertEqual(r.returncode, 1)
        self.assertIn("rpmspec cannot parse pkg.spec", r.stderr)
        self.assertNotIn("no remote source", r.stdout)

    def test_no_remote_source_exits_zero(self):
        self.stub("rpmspec", "echo 'Source0: local.tar.gz'\n")
        r = self.run_script()
        self.assertEqual(r.returncode, 0)
        self.assertIn("no remote source in pkg.spec", r.stdout)

    def test_remote_source_is_downloaded_and_verified(self):
        self.remote_spec()
        self.manifest(hashlib.sha256(PAYLOAD).hexdigest())
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("a.tar.gz verified", r.stdout)

    def test_checksum_mismatch_fails(self):
        self.remote_spec()
        self.manifest("0" * 64)
        r = self.run_script()
        self.assertEqual(r.returncode, 1)
        self.assertIn("checksum mismatch for a.tar.gz", r.stderr)

    def test_cached_file_is_verified_too(self):
        self.remote_spec()
        self.manifest(hashlib.sha256(PAYLOAD).hexdigest())
        self.out.mkdir()
        (self.out / "a.tar.gz").write_bytes(b"tampered\n")
        r = self.run_script()
        self.assertEqual(r.returncode, 1)
        self.assertIn("checksum mismatch for a.tar.gz", r.stderr)

    def test_missing_manifest_fails(self):
        self.remote_spec()
        r = self.run_script()
        self.assertEqual(r.returncode, 1)
        self.assertIn("the manifest is missing", r.stderr)

    def test_source_outside_the_manifest_fails(self):
        self.remote_spec()
        (self.spec_dir / "SOURCES/sources.sha256").write_text(f"{'0' * 64}  other.tar.gz\n")
        r = self.run_script()
        self.assertEqual(r.returncode, 1)
        self.assertIn("a.tar.gz is not in the manifest", r.stderr)

    def test_a_name_that_leaves_the_source_dir_fails(self):
        for fragment in ("../evil", ".hidden", "dir/evil"):
            with self.subTest(fragment=fragment):
                self.stub("rpmspec", f"echo 'Source0: https://example.invalid/a.tar.gz#/{fragment}'\n")
                self.manifest("0" * 64)
                r = self.run_script()
                self.assertEqual(r.returncode, 1)
                self.assertIn("invalid file name", r.stderr)

    def test_pin_writes_the_manifest(self):
        self.remote_spec()
        env = dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}")
        r = subprocess.run(
            ["bash", str(SCRIPT), "--pin", str(self.spec_dir), str(self.out)],
            env=env, capture_output=True, text=True, check=False,
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.spec_dir / "SOURCES/sources.sha256").read_text(),
                         f"{hashlib.sha256(PAYLOAD).hexdigest()}  a.tar.gz\n")


if __name__ == "__main__":
    unittest.main()
