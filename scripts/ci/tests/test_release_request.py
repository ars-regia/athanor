"""scripts/ci/release_request.py and the schema subset of scripts/ci/schema.py
(docs/architecture/doc_pipeline.md, PL55)."""

import hashlib
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

CI = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(CI))
import schema  # noqa: E402

SCRIPT = CI / "release_request.py"
SHA = "c" * 40
BUILDER = "f" * 64
GITHUB = {
    "GITHUB_RUN_ID": "412",
    "GITHUB_RUN_ATTEMPT": "2",
    "GITHUB_REPOSITORY": "ars-regia/athanor",
    "GITHUB_WORKFLOW_REF": "ars-regia/athanor/.github/workflows/athanor-forge-orchestrator.yml@refs/heads/iso-v0",
    "GITHUB_REF": "refs/heads/iso-v0",
    "GITHUB_SHA": SHA,
    "BUILDER_HASH": BUILDER,
}


class ReleaseRequest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        (self.dir / "image-digests.txt").write_text(
            "ghcr.io/owner/athanor-system 412 sha256:" + "1" * 64 + "\n"
        )
        (self.dir / "kernel").mkdir()
        (self.dir / "kernel/kernel-digests.txt").write_text("k\n")
        (self.dir / "kernel/kernel-unsigned.txt").write_text("")

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, **over):
        return subprocess.run(
            [
                sys.executable,
                "-B",
                str(SCRIPT),
                "--image-digests",
                str(self.dir / "image-digests.txt"),
                "--kernel-digests",
                str(self.dir / "kernel"),
                "--out",
                str(self.dir / "out"),
            ],
            capture_output=True,
            text=True,
            env={**os.environ, **GITHUB, **over},
        )

    def test_the_request_names_the_run_and_hashes_the_three_files(self):
        r = self.write()
        self.assertEqual(r.returncode, 0, r.stderr)
        request = json.loads((self.dir / "out/release-request.json").read_text())
        self.assertEqual(request["build_run_id"], 412)
        self.assertEqual(request["build_run_attempt"], 2)
        self.assertEqual(
            request["workflow_path"], ".github/workflows/athanor-forge-orchestrator.yml"
        )
        self.assertEqual(request["commit"], SHA)
        self.assertEqual(
            request["sha256"]["kernel-unsigned.txt"], hashlib.sha256(b"").hexdigest()
        )
        schema.validate(request, schema.load("release-request"))

    def test_a_malformed_value_writes_nothing(self):
        for over in (
            {"GITHUB_SHA": "iso-v0"},
            {"BUILDER_HASH": "x"},
            {"GITHUB_RUN_ID": "12a"},
        ):
            with self.subTest(**over):
                r = self.write(**over)
                self.assertEqual(r.returncode, 1)
                self.assertFalse((self.dir / "out/release-request.json").exists())

    def test_a_missing_kernel_file_writes_nothing(self):
        (self.dir / "kernel/kernel-unsigned.txt").unlink()
        self.assertEqual(self.write().returncode, 1)
        self.assertFalse((self.dir / "out/release-request.json").exists())


class Schema(unittest.TestCase):
    S = {
        "type": "object",
        "required": ["a"],
        "additionalProperties": False,
        "properties": {
            "a": {"type": "integer", "minimum": 1},
            "b": {"type": ["string", "null"], "pattern": "^x+$"},
            "c": {"type": "array", "items": {"enum": ["p", "q"]}},
        },
    }

    def test_a_valid_document_passes(self):
        schema.validate({"a": 1, "b": None, "c": ["p"]}, self.S)

    def test_each_violation_is_named(self):
        for doc, word in (
            ({}, r"\$\.a: missing"),
            ({"a": 0}, "minimum"),
            ({"a": True}, "integer"),
            ({"a": 1, "z": 1}, r"\$\.z: not allowed"),
            ({"a": 1, "b": "y"}, "pattern"),
            ({"a": 1, "c": ["r"]}, "enum"),
        ):
            with self.subTest(doc=doc):
                with self.assertRaisesRegex(ValueError, word):
                    schema.validate(doc, self.S)

    def test_a_keyword_outside_the_subset_fails_instead_of_passing(self):
        with self.assertRaisesRegex(ValueError, "maxLength"):
            schema.validate("x", {"type": "string", "maxLength": 3})


if __name__ == "__main__":
    unittest.main()
