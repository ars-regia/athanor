"""system/fetch-evidence.sh: the evidence of one run, from trusted runs only."""

import json
import pathlib
import shutil
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
GATE_WORKFLOW = {"iso-acceptance": ACCEPT, "signature": ORCH}


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


class FetchEvidence(Tool):
    def github(
        self,
        acceptance=((500, "2026-10-09T10:00:00Z", {}),),
        signature=((600, "2026-10-09T09:00:00Z", {}),),
        build=None,
    ):
        """Each gate lists (run id, created, overrides) per artifact. The overrides change the
        uploading run, except `expired` (the artifact's flag), `files` (its content), `api`
        (False: the run is unknown to the API) and `total_count` (of the listing). Subtests
        call it again, so it also starts a fresh artifacts directory."""
        shutil.rmtree(self.artifacts, ignore_errors=True)
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
        for gate, runs in (("iso-acceptance", acceptance), ("signature", signature)):
            name = f"evidence-412-{gate}"
            listing = {"artifacts": []}
            for run_id, created, over in runs:
                over = dict(over)
                expired = over.pop("expired", False)
                content = over.pop(
                    "files",
                    {f"{gate}.athanor-system.json": json.dumps({"from": run_id})},
                )
                if "total_count" in over:
                    listing["total_count"] = over.pop("total_count")
                if over.pop("api", True):
                    api[f"{API}/runs/{run_id}"] = run_object(
                        run_id, GATE_WORKFLOW[gate], **over
                    )
                listing["artifacts"].append(
                    {
                        "name": name,
                        "expired": expired,
                        "created_at": created,
                        "workflow_run": {"id": run_id},
                    }
                )
                files.setdefault(str(run_id), {})[name] = content
            api[f"{API}/artifacts?name={name}&per_page=100"] = listing
        self.registry({"api": api, "run_artifacts": files})

    def fetch(self):
        return subprocess.run(
            ["bash", str(SCRIPT), "412", str(self.artifacts)],
            capture_output=True,
            text=True,
            env=self.env,
        )

    def picked(self, gate):
        path = self.artifacts / f"evidence/{gate}.athanor-system.json"
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
            acceptance=(
                (500, "2026-10-09T10:00:00Z", {}),
                (501, "2026-10-09T12:00:00Z", {}),
            )
        )
        self.assertEqual(self.fetch().returncode, 0)
        self.assertEqual(self.picked("iso-acceptance"), 501)

    def test_evidence_from_an_untrusted_run_is_never_picked(self):
        for gate in ("iso-acceptance", "signature"):
            for over in (
                {"event": "pull_request"},
                {"head_branch": "feature"},
                {"path": ".github/workflows/pr.yml"},
                {"head_repository": {"full_name": "someone/athanor"}},
            ):
                with self.subTest(gate=gate, **{k: str(v) for k, v in over.items()}):
                    self.github(
                        **{
                            gate.replace("iso-", ""): (
                                (500, "2026-10-09T10:00:00Z", over),
                            )
                        }
                    )
                    r = self.fetch()
                    self.assertEqual(r.returncode, 0, r.stderr)
                    self.assertIsNone(self.picked(gate))
                    self.assertIn("not from a trusted release run", r.stdout)

    def test_each_gate_trusts_only_its_own_workflows(self):
        self.github(
            acceptance=((500, "2026-10-09T10:00:00Z", {"path": ORCH}),),
            signature=((600, "2026-10-09T09:00:00Z", {"path": ACCEPT}),),
        )
        r = self.fetch()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIsNone(self.picked("iso-acceptance"))
        self.assertIsNone(self.picked("signature"))

    def test_a_newer_untrusted_artifact_does_not_hide_a_trusted_one(self):
        for gate in ("acceptance", "signature"):
            with self.subTest(gate=gate):
                self.github(
                    **{
                        gate: (
                            (500, "2026-10-09T10:00:00Z", {}),
                            (501, "2026-10-09T12:00:00Z", {"event": "pull_request"}),
                        )
                    }
                )
                r = self.fetch()
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertEqual(
                    self.picked("iso-" + gate if gate == "acceptance" else gate), 500
                )
                self.assertIn("run 501", r.stdout)

    def test_an_expired_artifact_is_not_picked(self):
        self.github(
            acceptance=(
                (500, "2026-10-09T10:00:00Z", {}),
                (501, "2026-10-09T12:00:00Z", {"expired": True}),
            )
        )
        self.assertEqual(self.fetch().returncode, 0)
        self.assertEqual(self.picked("iso-acceptance"), 500)

    def test_a_failed_acceptance_is_fetched_and_a_failed_signature_is_not(self):
        self.github(
            acceptance=((500, "2026-10-09T10:00:00Z", {"conclusion": "failure"}),),
            signature=((600, "2026-10-09T09:00:00Z", {"conclusion": "failure"}),),
        )
        self.assertEqual(self.fetch().returncode, 0)
        self.assertEqual(self.picked("iso-acceptance"), 500)
        self.assertIsNone(self.picked("signature"))

    def test_a_cancelled_or_running_acceptance_is_not_picked(self):
        for over in (
            {"conclusion": "cancelled"},
            {"status": "in_progress", "conclusion": None},
        ):
            with self.subTest(**{k: str(v) for k, v in over.items()}):
                self.github(acceptance=((500, "2026-10-09T10:00:00Z", over),))
                self.assertEqual(self.fetch().returncode, 0)
                self.assertIsNone(self.picked("iso-acceptance"))

    def test_an_untrusted_build_run_fetches_nothing(self):
        self.github(build=run_object(412, ORCH, event="pull_request"))
        r = self.fetch()
        self.assertEqual(r.returncode, 10)
        self.assertFalse((self.artifacts / "image-digests.txt").exists())

    def test_a_missing_gate_is_left_to_the_promotion(self):
        self.github(signature=())
        r = self.fetch()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse(
            (self.artifacts / "evidence/signature.athanor-system.json").exists()
        )
        self.assertIn("no trusted evidence-412-signature", r.stderr)

    def test_a_failure_to_check_a_candidate_ends_the_script(self):
        self.github(acceptance=((500, "2026-10-09T10:00:00Z", {"api": False}),))
        r = self.fetch()
        self.assertNotIn(r.returncode, (0, 10), r.stderr)
        self.assertIsNone(self.picked("iso-acceptance"))

    def test_a_listing_longer_than_one_page_is_refused(self):
        self.github(acceptance=((500, "2026-10-09T10:00:00Z", {"total_count": 101}),))
        r = self.fetch()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("101 artifacts", r.stderr)

    def test_an_artifact_carrying_another_gate_s_file_is_refused(self):
        self.github(
            acceptance=(
                (
                    500,
                    "2026-10-09T10:00:00Z",
                    {"files": {"signature.athanor-system.json": "{}"}},
                ),
            ),
            signature=(),
        )
        r = self.fetch()
        self.assertNotEqual(r.returncode, 0)
        self.assertFalse(
            (self.artifacts / "evidence/signature.athanor-system.json").exists()
        )
        self.assertIn("signature.athanor-system.json", r.stderr)

    def test_a_directory_with_content_is_refused(self):
        self.github()
        (self.artifacts / "evidence").mkdir(parents=True)
        (self.artifacts / "evidence/signature.athanor-system.json").write_text("{}")
        r = self.fetch()
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertIn("not empty", r.stderr)


if __name__ == "__main__":
    unittest.main()
