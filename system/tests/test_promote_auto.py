"""Unit tests of system/promote-auto.sh, the choice promote-stable.yml makes, against an offline
registry and run list (python3 -B -m unittest discover -s system/tests -v)."""

import json
import pathlib
import subprocess

from test_promote import (
    FORK,
    NAMES,
    OTHER_BRANCH,
    REG,
    TRUSTED,
    Published,
    digest,
    predicate,
)

ROOT = pathlib.Path(__file__).resolve().parents[2]
AUTO = ROOT / "system" / "promote-auto.sh"


class PromoteAuto(Published):
    def setUp(self):
        super().setUp()
        self.record = self.dir / "record.json"

    def runs(self, *runs, **published):
        """Runs listed by gh newest last, each with its own digests; published: run -> kwargs."""
        fx = None if runs else self.published()
        for i, run in enumerate(runs):
            fx = self.published(
                run=run, base=1 + 3 * i, fx=fx, **published.get(run, {})
            )
        fx["runs"] = [{"databaseId": int(run)} for run in runs]
        self.registry(fx)

    def auto(self, **env):
        r = subprocess.run(
            ["bash", str(AUTO), str(self.record)],
            capture_output=True,
            text=True,
            env={**self.env, "REGISTRY": REG, **env},
        )
        stable = [
            c[-2].removeprefix("docker://")
            for c in self.calls()
            if c[:2] == ["skopeo", "copy"] and c[-1].endswith(":stable")
        ]
        return r, stable

    def test_the_schedule_promotes_an_accepted_run_past_the_dwell_time(self):
        self.runs("412")
        r, stable = self.auto()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            stable, [f"{REG}/{name}@{digest(i + 1)}" for i, name in enumerate(NAMES)]
        )
        record = json.loads(self.record.read_text())
        self.assertEqual(
            (record["run_id"], record["mode"], record["dwell_hours"], record["reason"]),
            ("412", "scheduled", 24, None),
        )
        self.assertEqual(record["evidence"]["revision"], "c" * 40)

    def test_the_schedule_lists_only_orchestrator_runs_of_the_trusted_branch(self):
        self.runs("412")
        self.auto()
        listing = next(c for c in self.calls() if c[:3] == ["gh", "run", "list"])
        self.assertEqual(
            listing[listing.index("--workflow") + 1], "athanor-forge-orchestrator.yml"
        )
        self.assertEqual(listing[listing.index("--branch") + 1], "iso-v0")

    def test_the_schedule_waits_for_the_dwell_time(self):
        self.runs(
            "412", **{"412": {"attestations": [(TRUSTED, predicate(hours_ago=3))]}}
        )
        r, stable = self.auto()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("nothing to promote yet: run 412", r.stdout)
        self.assertEqual(stable, [])
        self.assertFalse(self.record.exists())

    def test_a_run_waiting_for_the_dwell_time_keeps_a_broken_older_one_quiet(self):
        # 413 waits out the dwell time; 412 has no evidence. Waiting is the reason: success.
        self.runs(
            "412",
            "413",
            **{
                "412": {"attestations": []},
                "413": {
                    "attestations": [
                        (
                            TRUSTED,
                            predicate(
                                run="413",
                                hours_ago=3,
                                images={n: digest(4 + i) for i, n in enumerate(NAMES)},
                            ),
                        )
                    ],
                    "run_created": "2026-09-16T10:00:00Z",
                },
            },
        )
        r, stable = self.auto()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("nothing to promote yet: run 413", r.stdout)
        self.assertEqual(stable, [])

    def test_a_failed_run_listing_fails(self):
        self.runs("412")
        fx = json.loads((self.dir / "registry.json").read_text())
        fx["runs_error"] = "HTTP 401: Bad credentials (https://api.github.com/graphql)"
        self.registry(fx)
        r, stable = self.auto()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("cannot list the Orchestrator runs", r.stderr)
        self.assertEqual(stable, [])

    def test_no_run_listed_is_nothing_to_promote(self):
        self.runs()
        r, stable = self.auto()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("no Orchestrator run on iso-v0", r.stdout)

    def test_the_schedule_falls_back_to_an_older_accepted_run(self):
        # 413 is newer but failed; 412 passed and is past the dwell time.
        fail = predicate(
            run="413",
            result="fail",
            images={n: digest(4 + i) for i, n in enumerate(NAMES)},
        )
        self.runs(
            "412",
            "413",
            **{
                "413": {
                    "attestations": [(TRUSTED, fail)],
                    "run_created": "2026-09-16T10:00:00Z",
                }
            },
        )
        r, stable = self.auto()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("run 413 is not eligible", r.stdout)
        self.assertEqual(json.loads(self.record.read_text())["run_id"], "412")
        self.assertEqual(
            stable, [f"{REG}/{name}@{digest(i + 1)}" for i, name in enumerate(NAMES)]
        )

    def test_forged_evidence_never_reaches_stable(self):
        # Evidence signed from a branch or a fork, however good it looks, makes no run eligible;
        # every run ineligible and none waiting is a broken chain, which fails loudly.
        for identity in (OTHER_BRANCH, FORK):
            with self.subTest(identity=identity):
                (self.dir / "calls.log").unlink(missing_ok=True)
                self.runs("412", **{"412": {"attestations": [(identity, predicate())]}})
                r, stable = self.auto()
                self.assertEqual(r.returncode, 1, r.stdout)
                self.assertIn("none of the last 1 Orchestrator runs is eligible", r.stderr)
                self.assertEqual(stable, [])
                self.assertFalse(self.record.exists())

    def test_a_run_without_the_build_signature_fails_the_schedule(self):
        self.runs("412", **{"412": {"signed_by": False}})
        r, stable = self.auto()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("was not signed by call-system-image.yml", r.stderr)
        self.assertEqual(stable, [])

    def test_the_schedule_is_quiet_when_stable_already_holds_the_run(self):
        self.runs("412", **{"412": {"run_created": "2026-09-01T10:00:00Z"}})
        r, stable = self.auto()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("already holds", r.stdout)
        self.assertEqual(stable, [])

    def test_a_registry_failure_is_not_nothing_to_promote(self):
        self.runs("412")
        fx = json.loads((self.dir / "registry.json").read_text())
        fx["errors"] = [f"{REG}/athanor-system:412"]
        self.registry(fx)
        r, stable = self.auto()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertEqual(stable, [])

    def test_a_security_fix_skips_the_dwell_time_and_records_why(self):
        self.runs(
            "412", **{"412": {"attestations": [(TRUSTED, predicate(hours_ago=1))]}}
        )
        r, stable = self.auto(SECURITY_REASON="CVE-2026-0001,GHSA-2c8m-xq3v-p9wf")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(stable), 3)
        record = json.loads(self.record.read_text())
        self.assertEqual(
            (record["mode"], record["dwell_hours"], record["reason"]),
            ("security", 0, "CVE-2026-0001,GHSA-2c8m-xq3v-p9wf"),
        )
        self.assertEqual(record["evidence"]["run_id"], "412")

    def test_a_security_reason_must_name_advisories(self):
        self.runs("412")
        for reason in (
            "CVE-2026-0001 in openssl",
            "urgent",
            "CVE-2026-0001;rm",
            "CVE-26-1",
            "GHSA-aaaa-bbbb-cccc",
            "CVE-2026-0001,",
        ):
            with self.subTest(reason=reason):
                r, stable = self.auto(SECURITY_REASON=reason)
                self.assertEqual(r.returncode, 2, r.stdout)
                self.assertIn("SECURITY_REASON must be advisory ids", r.stderr)
                self.assertEqual(stable, [])

    def test_a_manual_run_needs_passing_evidence(self):
        self.runs(
            "412", **{"412": {"attestations": [(TRUSTED, predicate(result="fail"))]}}
        )
        r, stable = self.auto(RUN_ID="412")
        self.assertEqual(r.returncode, 4)
        self.assertIn("has no passing acceptance evidence for run 412", r.stderr)
        self.assertEqual(stable, [])
        self.assertFalse(self.record.exists())


if __name__ == "__main__":
    import unittest

    unittest.main()
