"""Issue #1176: no path reads a registry tag as a cache hit, or as the image to take RPMs from,
without verify_image.sh checking the signature of the digest the tag names. The behaviour is
tested next to each script; this test fails when a new path skips the gate
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import pathlib
import re
import unittest

REPO = pathlib.Path(__file__).resolve().parents[3]
# Where the pipeline's scripts and workflows live; tests and documents are not read.
ROOTS = (
    "forge/scripts",
    "forge/Justfile",
    "system",
    "scripts",
    "Justfile",
    ".github/workflows",
)
SUFFIXES = {".sh", ".py", ".yml", ".yaml", ""}
# registry_probe.sh answers only whether a tag exists. verify_image.sh is the gate built on it;
# check_idempotency.sh asks it directly for the builder alone, an image no run signs yet, and
# check_idempotency_test.sh tests it against the live registry, and the CI path filter of
# scripts/ci/changes.py names it without running it. Comments do not count.
PROBE_USERS = {
    "forge/scripts/verify_image.sh",
    "forge/scripts/check_idempotency.sh",
    "forge/scripts/check_idempotency_test.sh",
    "scripts/ci/changes.py",
}
PROBE_USE = re.compile(r"^(?!\s*#).*registry_probe\.sh", re.M)
# A file that names a package image's hash tag reads it, and so must pass it through the gate:
# only tag_signed_image.sh writes the tag, after signing the digest it points the tag at.
HASH_TAG = re.compile(r":hash-|HASH_TAG_PREFIX=\"hash-\"")
HASH_TAG_WRITERS = {"forge/scripts/tag_signed_image.sh"}


def pipeline_files():
    for root in ROOTS:
        path = REPO / root
        candidates = (
            [path]
            if path.is_file()
            else sorted(p for p in path.rglob("*") if p.is_file())
        )
        for file in candidates:
            rel = file.relative_to(REPO).as_posix()
            if "/tests/" in f"/{rel}" or file.suffix not in SUFFIXES:
                continue
            yield rel, file.read_text(errors="replace")


class CacheHitGateTest(unittest.TestCase):
    def test_the_scan_sees_the_hit_paths(self):
        names = {rel for rel, _ in pipeline_files()}
        for rel in (
            "forge/scripts/dag_orchestrator.py",
            "forge/scripts/check_idempotency.sh",
            "forge/scripts/resolve_node_image.sh",
            "forge/scripts/fetch_repo_rpms.sh",
        ):
            self.assertIn(rel, names)

    def test_only_the_gate_asks_the_registry_whether_a_tag_exists(self):
        users = {
            rel
            for rel, text in pipeline_files()
            if PROBE_USE.search(text) and rel != "forge/scripts/registry_probe.sh"
        }
        self.assertEqual(PROBE_USERS, users)

    def test_the_builder_is_the_only_unverified_hit(self):
        text = (REPO / "forge/scripts/check_idempotency.sh").read_text()
        branch = re.search(
            r'"builder" \]\]; then\n  PROBE_CMD=\((.*?)\)\n.*?\nelse\n  PROBE_CMD=\((.*?)\)\n',
            text,
            re.S,
        )
        self.assertIsNotNone(
            branch, "check_idempotency.sh no longer chooses its probe by package"
        )
        self.assertIn("registry_probe.sh", branch.group(1))
        self.assertIn("verify_image.sh", branch.group(2))
        self.assertEqual(1, len(PROBE_USE.findall(text)))

    def test_every_reader_of_a_hash_tag_verifies_it(self):
        for rel, text in pipeline_files():
            if rel in HASH_TAG_WRITERS or not HASH_TAG.search(text):
                continue
            with self.subTest(file=rel):
                self.assertIn("verify_image.sh", text)

    def test_the_brain_takes_only_a_signed_image_as_a_hit(self):
        text = (REPO / "forge/scripts/dag_orchestrator.py").read_text()
        body = re.search(r"\ndef image_exists\(ref\):\n(.*?)\n\n\n", text, re.S)
        self.assertIsNotNone(body)
        self.assertIn('"verify_image.sh"), ref, "call-dag-compile.yml"', body.group(1))
        self.assertIn('return verdict[0] == "signed"', body.group(1))


if __name__ == "__main__":
    unittest.main()
