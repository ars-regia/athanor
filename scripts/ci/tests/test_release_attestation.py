"""scripts/ci/release_attestation.py: the unsigned release statements of PB4b
(docs/architecture/doc_pipeline.md, ADR-0127) that sign-images attests with cosign."""

import json
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import release_attestation as ra  # noqa: E402

REG = "ghcr.io/owner"
SHA = "c" * 40
BUILD_TIME = "2026-10-21T04:00:00Z"


def digest(n):
    return "sha256:" + f"{n:x}" * 64


def record(name, n, cls="feature", advisories=(), reason="", stamp=None):
    return {
        "repository": f"{REG}/{name}",
        "baseline_digest": digest(n + 8),
        "candidate_digest": digest(n),
        "class": cls,
        "advisories": list(advisories),
        "class_reason": reason,
        "updateinfo_timestamp": stamp,
    }


def candidate(*records):
    return {
        "schema_version": 2,
        "build_time": BUILD_TIME,
        "request": {"build_run_id": 412, "commit": SHA},
        "order": [],
        "classes": list(records),
        "superseded": [],
    }


class Statement(unittest.TestCase):
    def test_statement_shape(self):
        rec = record(
            "athanor-system", 1, "security", ["CVE-2026-0001"], stamp=1760000000
        )
        s = ra.statement(rec, BUILD_TIME, SHA, "412")
        self.assertEqual(s["_type"], "https://in-toto.io/Statement/v1")
        self.assertEqual(s["predicateType"], ra.PREDICATE_TYPE)
        self.assertEqual(
            ra.PREDICATE_TYPE,
            "https://github.com/ars-regia/athanor/blob/iso-v0/docs/architecture/doc_pipeline.md#pb4b",
        )
        self.assertEqual(
            s["subject"],
            [{"name": f"{REG}/athanor-system", "digest": {"sha256": "1" * 64}}],
        )
        self.assertEqual(
            set(s["predicate"]),
            {
                "class",
                "advisories",
                "class_reason",
                "build_time",
                "revision",
                "build_run_id",
                "updateinfo_timestamp",
            },
        )
        self.assertEqual(s["predicate"]["class"], "security")
        self.assertEqual(s["predicate"]["advisories"], ["CVE-2026-0001"])
        self.assertEqual(s["predicate"]["build_time"], BUILD_TIME)
        self.assertEqual(s["predicate"]["revision"], SHA)
        self.assertEqual(s["predicate"]["build_run_id"], "412")
        self.assertEqual(s["predicate"]["updateinfo_timestamp"], 1760000000)

    def test_feature_record_keeps_empty_advisories(self):
        s = ra.statement(
            record("athanor-system", 1, reason="no-baseline"), BUILD_TIME, SHA, "412"
        )
        self.assertEqual(s["predicate"]["class"], "feature")
        self.assertEqual(s["predicate"]["advisories"], [])
        self.assertEqual(s["predicate"]["class_reason"], "no-baseline")
        self.assertIsNone(s["predicate"]["updateinfo_timestamp"])

    def test_write_statements_one_file_per_image(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            path = tmp / "release-candidate.json"
            path.write_text(
                json.dumps(
                    candidate(
                        record("athanor-system", 1), record("athanor-system-nvidia", 2)
                    )
                )
            )
            written = ra.write_statements(path, tmp / "attestations")
            self.assertEqual(
                [p.relative_to(tmp / "attestations").as_posix() for p in written],
                [
                    f"athanor-system/{'1' * 64}.json",
                    f"athanor-system-nvidia/{'2' * 64}.json",
                ],
            )
            nvidia = json.loads(written[1].read_text())
            self.assertEqual(
                nvidia["subject"][0]["name"], f"{REG}/athanor-system-nvidia"
            )
            self.assertEqual(nvidia["predicate"]["build_run_id"], "412")

    def test_write_statements_refuses_a_candidate_outside_the_schema(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            path = tmp / "release-candidate.json"
            bad = candidate(record("athanor-system", 1))
            bad["classes"][0]["class"] = "urgent"
            path.write_text(json.dumps(bad))
            with self.assertRaises(ValueError):
                ra.write_statements(path, tmp / "attestations")
            self.assertFalse((tmp / "attestations").exists())

    def test_advisories_are_unique_and_sorted(self):
        """The schema refuses a repeated advisory; a statement whose advisories are not sorted
        is not one candidate wrote, so the sign script refuses it."""
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            path = tmp / "release-candidate.json"
            twice = ("CVE-2026-0001", "CVE-2026-0001")
            path.write_text(
                json.dumps(candidate(record("athanor-system", 1, "security", twice)))
            )
            with self.assertRaisesRegex(ValueError, "repeats an item"):
                ra.write_statements(path, tmp / "attestations")
            for advisories in (["CVE-2026-0002", "CVE-2026-0001"], ["A", "A"]):
                with self.subTest(advisories=advisories):
                    s = ra.statement(
                        record("athanor-system", 1, "security", advisories),
                        "2026-10-21T04:00:00Z",
                        "c" * 40,
                        "412",
                    )
                    path = tmp / "s.json"
                    path.write_text(json.dumps(s))
                    with self.assertRaises(ra.Refused):
                        ra.load_statement(path, f"{REG}/athanor-system", digest(1))


if __name__ == "__main__":
    unittest.main()
