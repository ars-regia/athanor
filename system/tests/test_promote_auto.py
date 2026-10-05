"""Unit tests of system/promote-auto.sh, the choice promote-stable.yml makes, against an offline
registry (python3 -B -m unittest discover -s system/tests -v)."""

import json
import pathlib
import subprocess

from test_promote import NAMES, REG, Published, digest

ROOT = pathlib.Path(__file__).resolve().parents[2]
AUTO = ROOT / "system" / "promote-auto.sh"


class PromoteAuto(Published):
    def setUp(self):
        super().setUp()
        self.evidence_dir = self.dir / "evidence"
        self.evidence_dir.mkdir()
        self.record = self.dir / "record.json"

    def accepted(self, run="412", **kwargs):
        """One downloaded acceptance artifact, in its own directory as gh run download leaves it."""
        path = self.evidence(run=run, **kwargs)
        target = (
            self.evidence_dir
            / f"acceptance-{run}-{len(list(self.evidence_dir.iterdir()))}"
        )
        target.mkdir()
        path.rename(target / path.name)

    def auto(self, **env):
        variables = dict(self.env)
        variables.update({"REGISTRY": REG}, **env)
        r = subprocess.run(
            ["bash", str(AUTO), str(self.evidence_dir), str(self.record)],
            capture_output=True,
            text=True,
            env=variables,
        )
        log = self.dir / "calls.log"
        calls = (
            [json.loads(line) for line in log.read_text().splitlines()]
            if log.exists()
            else []
        )
        stable = [
            c[-2].removeprefix("docker://")
            for c in calls
            if c[:2] == ["skopeo", "copy"] and c[-1].endswith(":stable")
        ]
        return r, stable

    def test_the_schedule_promotes_an_accepted_run_past_the_dwell_time(self):
        self.published()
        self.accepted(hours_ago=30)
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

    def test_the_schedule_waits_for_the_dwell_time(self):
        self.published()
        self.accepted(hours_ago=3)
        r, stable = self.auto()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("nothing to promote", r.stdout)
        self.assertEqual(stable, [])
        self.assertFalse(self.record.exists())

    def test_the_schedule_ignores_a_failed_run(self):
        self.published()
        self.accepted(result="fail", hours_ago=30)
        r, stable = self.auto()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(stable, [])

    def test_the_schedule_is_quiet_when_stable_already_holds_the_run(self):
        self.published(run_created="2026-09-01T10:00:00Z")
        self.accepted(hours_ago=30)
        r, stable = self.auto()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("already holds", r.stdout)
        self.assertEqual(stable, [])

    def test_a_security_fix_skips_the_dwell_time_and_records_why(self):
        self.published()
        self.accepted(hours_ago=1)
        r, stable = self.auto(SECURITY_REASON="CVE-2026-0001 in openssl")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(stable), 3)
        record = json.loads(self.record.read_text())
        self.assertEqual(
            (record["mode"], record["dwell_hours"], record["reason"]),
            ("security", 0, "CVE-2026-0001 in openssl"),
        )
        self.assertEqual(record["evidence"]["run_id"], "412")

    def test_a_manual_run_needs_passing_evidence(self):
        self.published()
        self.accepted(result="fail")
        r, stable = self.auto(RUN_ID="412")
        self.assertEqual(r.returncode, 1)
        self.assertIn("no passing acceptance evidence for run 412", r.stderr)
        self.assertEqual(stable, [])


if __name__ == "__main__":
    import unittest

    unittest.main()
