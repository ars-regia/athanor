"""system/fetch-evidence.sh: the evidence of one run, from trusted runs only."""

import json
import pathlib
import subprocess
import unittest

from test_kernel_artifacts import Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "fetch-evidence.sh"
ORCH = ".github/workflows/athanor-forge-orchestrator.yml"
ACCEPT = ".github/workflows/accept.yml"
API = "/repos/ars-regia/athanor/actions"
SHA = "c" * 40
BRANCH_SHA = "d" * 40
# trusted-run.sh checks that the commit a run built is on the release branch.
ON_BRANCH = {
    "/repos/ars-regia/athanor/branches/iso-v0": {"commit": {"sha": BRANCH_SHA}},
    f"/repos/ars-regia/athanor/compare/{SHA}...{BRANCH_SHA}": {"status": "ahead"},
}


def run_object(run_id, workflow, **over):
    return {
        "id": run_id,
        "head_sha": SHA,
        "path": workflow,
        "head_branch": "iso-v0",
        "event": "workflow_dispatch",
        "status": "completed",
        "conclusion": "success",
        "head_repository": {"full_name": "ars-regia/athanor"},
        **over,
    }


def artifact(name, run_id, created, expired=False):
    return {
        "name": name,
        "expired": expired,
        "created_at": created,
        "workflow_run": {"id": run_id},
    }


class FetchEvidence(Tool):
    def github(
        self,
        accept_runs=((500, "2026-10-09T10:00:00Z", {}),),
        signature=True,
        build=None,
    ):
        api = {
            f"{API}/runs/412": build or run_object(412, ORCH, event="push"),
            **ON_BRANCH,
        }
        files = {
            "412": {
                "image-digests": {
                    "image-digests.txt": "r/athanor-system 412 sha256:"
                    + "a" * 64
                    + "\n"
                },
                "package-sets": {"athanor-system.txt": "bash-5.2-1.fc43.x86_64 bb\n"},
            }
        }
        if signature:
            files["412"]["evidence-412-signature"] = {
                "signature.athanor-system.json": json.dumps({"gate": "signature"})
            }
        api[f"{API}/artifacts?name=evidence-412-signature&per_page=100"] = {
            "artifacts": [
                artifact("evidence-412-signature", 412, "2026-10-09T09:00:00Z")
            ]
            if signature
            else []
        }
        listed = []
        for run_id, created, over in accept_runs:
            api[f"{API}/runs/{run_id}"] = run_object(run_id, ACCEPT, **over)
            listed.append(artifact("evidence-412-iso-acceptance", run_id, created))
            files[str(run_id)] = {
                "evidence-412-iso-acceptance": {
                    "iso-acceptance.athanor-system.json": json.dumps({"from": run_id})
                }
            }
        api[f"{API}/artifacts?name=evidence-412-iso-acceptance&per_page=100"] = {
            "artifacts": listed
        }
        self.registry({"api": api, "run_artifacts": files})

    def fetch(self):
        return subprocess.run(
            ["bash", str(SCRIPT), "412", str(self.artifacts)],
            capture_output=True,
            text=True,
            env=self.env,
        )

    def acceptance(self):
        path = self.artifacts / "evidence/iso-acceptance.athanor-system.json"
        return json.loads(path.read_text())["from"] if path.exists() else None

    def test_the_run_inputs_and_the_evidence_land_in_one_directory(self):
        self.github()
        r = self.fetch()
        self.assertEqual(r.returncode, 0, r.stderr)
        for rel in (
            "image-digests.txt",
            "packages/athanor-system.txt",
            "evidence/signature.athanor-system.json",
            "evidence/iso-acceptance.athanor-system.json",
        ):
            self.assertTrue((self.artifacts / rel).is_file(), rel)

    def test_the_newest_trusted_acceptance_decides(self):
        self.github(
            accept_runs=(
                (500, "2026-10-09T10:00:00Z", {}),
                (501, "2026-10-09T12:00:00Z", {}),
            )
        )
        self.assertEqual(self.fetch().returncode, 0)
        self.assertEqual(self.acceptance(), 501)

    def test_evidence_from_an_untrusted_run_is_never_picked(self):
        for over in (
            {"event": "pull_request"},
            {"head_branch": "feature"},
            {"path": ".github/workflows/pr.yml"},
            {"head_repository": {"full_name": "someone/athanor"}},
        ):
            with self.subTest(**{k: str(v) for k, v in over.items()}):
                self.github(accept_runs=((500, "2026-10-09T10:00:00Z", over),))
                r = self.fetch()
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertIsNone(self.acceptance())
                self.assertIn("not from a trusted release run", r.stdout)

    def test_a_newer_untrusted_artifact_does_not_hide_a_trusted_one(self):
        self.github(
            accept_runs=(
                (500, "2026-10-09T10:00:00Z", {}),
                (501, "2026-10-09T12:00:00Z", {"event": "pull_request"}),
            )
        )
        r = self.fetch()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.acceptance(), 500)
        self.assertIn("run 501", r.stdout)

    def test_a_failed_acceptance_is_fetched(self):
        self.github(
            accept_runs=((500, "2026-10-09T10:00:00Z", {"conclusion": "failure"}),)
        )
        self.assertEqual(self.fetch().returncode, 0)
        self.assertEqual(self.acceptance(), 500)

    def test_a_cancelled_or_running_acceptance_is_not_picked(self):
        for over in (
            {"conclusion": "cancelled"},
            {"status": "in_progress", "conclusion": None},
        ):
            with self.subTest(**{k: str(v) for k, v in over.items()}):
                self.github(accept_runs=((500, "2026-10-09T10:00:00Z", over),))
                self.assertEqual(self.fetch().returncode, 0)
                self.assertIsNone(self.acceptance())

    def test_an_untrusted_build_run_fetches_nothing(self):
        self.github(build=run_object(412, ORCH, event="pull_request"))
        r = self.fetch()
        self.assertEqual(r.returncode, 10)
        self.assertFalse((self.artifacts / "image-digests.txt").exists())

    def test_a_missing_gate_is_left_to_the_promotion(self):
        self.github(signature=False)
        r = self.fetch()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse(
            (self.artifacts / "evidence/signature.athanor-system.json").exists()
        )
        self.assertIn("no trusted evidence-412-signature", r.stderr)


if __name__ == "__main__":
    unittest.main()
