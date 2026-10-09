"""system/trusted-run.sh and system/accept-target.sh against an offline GitHub and registry
(python3 -B -m unittest discover -s system/tests -v)."""

import pathlib
import subprocess
import unittest

from test_kernel_artifacts import Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
TRUSTED = ROOT / "system" / "trusted-run.sh"
TARGET = ROOT / "system" / "accept-target.sh"
REG = "registry.example/owner"
NAMES = ["athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy"]
ORCH = ".github/workflows/athanor-forge-orchestrator.yml"


def digest(n):
    return "sha256:" + f"{n:x}" * 64


def run_object(**over):
    run = {
        "id": 412,
        "path": ORCH,
        "head_branch": "iso-v0",
        "event": "push",
        "status": "completed",
        "conclusion": "success",
        "head_repository": {"full_name": "ars-regia/athanor"},
    }
    return {**run, **over}


class AcceptTarget(Tool):
    def github(self, run=None, iso_version="412"):
        digests = "".join(
            f"{REG}/{name} 412 {digest(i + 1)}\n" for i, name in enumerate(NAMES)
        )
        iso = digest(9)
        self.registry(
            {
                "api": {
                    "/repos/ars-regia/athanor/actions/runs/412": run or run_object()
                },
                "run_artifacts": {
                    "412": {"image-digests": {"image-digests.txt": digests}}
                },
                "tags": {f"{REG}/athanor-iso:412": iso},
                "configs": {
                    f"{REG}/athanor-iso@{iso}": {
                        "org.opencontainers.image.version": iso_version
                    }
                },
            }
        )

    def trusted(self, *flags):
        return subprocess.run(
            ["bash", str(TRUSTED), *flags, "412", ORCH],
            capture_output=True,
            text=True,
            env=self.env,
        )

    def target(self, run_id="412"):
        out = self.dir / "target"
        r = subprocess.run(
            ["bash", str(TARGET), run_id, str(out)],
            capture_output=True,
            text=True,
            env={**self.env, "REGISTRY": REG},
        )
        return r, out

    def test_a_successful_orchestrator_run_on_the_release_branch_is_trusted(self):
        self.github()
        self.assertEqual(self.trusted().returncode, 0)

    def test_a_pull_request_run_is_not_trusted(self):
        for event in ("pull_request", "pull_request_target"):
            with self.subTest(event=event):
                self.github(run_object(event=event))
                r = self.trusted()
                self.assertEqual(r.returncode, 10)
                self.assertIn("event", r.stderr)

    def test_another_workflow_is_not_trusted(self):
        self.github(run_object(path=".github/workflows/pr.yml"))
        self.assertIn("path", self.trusted().stderr)

    def test_a_fork_run_is_not_trusted(self):
        self.github(run_object(head_repository={"full_name": "someone/athanor"}))
        self.assertIn("repository", self.trusted().stderr)

    def test_another_branch_or_a_failed_run_is_not_trusted(self):
        for over, word in (
            ({"head_branch": "main"}, "branch"),
            ({"conclusion": "failure"}, "conclusion"),
        ):
            with self.subTest(word=word):
                self.github(run_object(**over))
                r = self.trusted()
                self.assertEqual(r.returncode, 10)
                self.assertIn(word, r.stderr)

    def test_a_failed_acceptance_run_is_trusted_as_completed(self):
        self.github(run_object(conclusion="failure"))
        self.assertEqual(self.trusted("--completed").returncode, 0)

    def test_a_cancelled_or_running_run_is_never_trusted(self):
        for over in (
            {"conclusion": "cancelled"},
            {"status": "in_progress", "conclusion": None},
        ):
            with self.subTest(**{k: str(v) for k, v in over.items()}):
                self.github(run_object(**over))
                self.assertEqual(self.trusted("--completed").returncode, 10)

    def test_a_failure_of_gh_is_not_a_verdict(self):
        self.registry({"api": {}})
        r = self.trusted()
        self.assertNotIn(r.returncode, (0, 10))

    def test_an_untrusted_build_run_resolves_no_target(self):
        self.github(run_object(event="pull_request"))
        r, out = self.target()
        self.assertEqual(r.returncode, 10)
        self.assertFalse((out / "image-digests.txt").exists())

    def test_the_target_names_the_run_digests_and_the_iso_by_digest(self):
        self.github()
        r, out = self.target()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((out / "default-digest").read_text().strip(), digest(1))
        self.assertEqual(
            (out / "iso-ref").read_text().strip(), f"{REG}/athanor-iso@{digest(9)}"
        )

    def test_an_iso_labelled_with_another_run_is_refused(self):
        self.github(iso_version="411")
        r, out = self.target()
        self.assertEqual(r.returncode, 1)
        self.assertIn("version", r.stderr)
        self.assertFalse((out / "iso-ref").exists())

    def test_a_run_id_that_is_not_a_number_is_refused(self):
        self.github()
        self.assertEqual(self.target("412; true")[0].returncode, 2)


if __name__ == "__main__":
    unittest.main()
