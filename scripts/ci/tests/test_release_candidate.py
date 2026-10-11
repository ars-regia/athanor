"""scripts/ci/release_candidate.py against an offline GitHub and registry: the gate of PB13
(docs/architecture/doc_pipeline.md, PL55 and PL58). Every refusal below is a release that must
not be signed. The fakes are system/tests/fake_registry.py, through the Tool harness."""

import hashlib
import json
import pathlib
import subprocess
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "system/tests"))
from test_kernel_artifacts import Tool  # noqa: E402

SCRIPT = ROOT / "scripts/ci/release_candidate.py"
R = "ars-regia/athanor"
REG = "ghcr.io/ars-regia"
ORCH = ".github/workflows/athanor-forge-orchestrator.yml"
SHA, TIP, OLD = "c" * 40, "d" * 40, "b" * 40
IMAGE_JOB = "system-image / 💿 System Image & Disk Build"
NAMES = ["athanor-system", "athanor-system-nvidia"]
SUPERSEDED = (
    f"/repos/{R}/actions/workflows/athanor-forge-orchestrator.yml/runs"
    "?branch=iso-v0&status=success&created=>=2026-10-10T00:00:00Z&per_page=100"
)


def digest(n):
    return "sha256:" + f"{n:x}" * 64


DIGESTS = "".join(f"{REG}/{name} 412 {digest(i + 1)}\n" for i, name in enumerate(NAMES))
KERNEL, UNSIGNED = f"{REG}/azoth 412 {digest(7)}\n", ""


def h(text):
    return hashlib.sha256(text.encode()).hexdigest()


def request(**over):
    return {
        "schema_version": 1,
        "build_run_id": 412,
        "build_run_attempt": 1,
        "workflow_path": ORCH,
        "ref": "refs/heads/iso-v0",
        "commit": SHA,
        "builder_content_hash": "f" * 64,
        "sha256": {
            "image-digests.txt": h(DIGESTS),
            "kernel-digests.txt": h(KERNEL),
            "kernel-unsigned.txt": h(UNSIGNED),
        },
        **over,
    }


def run_object(**over):
    return {
        "id": 412,
        "run_attempt": 1,
        "head_sha": SHA,
        "path": ORCH,
        "head_branch": "iso-v0",
        "event": "push",
        "status": "in_progress",
        "conclusion": None,
        "created_at": "2026-10-11T04:00:00Z",
        "head_repository": {"full_name": R},
        **over,
    }


def jobs(*extra, image="success"):
    base = [
        {"name": "lint", "conclusion": "success"},
        {"name": "nvidia-kmod-sign", "conclusion": "skipped"},
        {"name": IMAGE_JOB, "conclusion": image},
        {"name": "request-release", "conclusion": None},
    ]
    return [{"jobs": base + list(extra)}]


class Candidate(Tool):
    def github(
        self,
        req=None,
        run=None,
        pages=None,
        files=None,
        latest=True,
        labels=None,
        compare=None,
        extra=None,
    ):
        """A build run 412 of iso-v0 at SHA, and a :latest built on 2026-10-10 from OLD."""
        files = {
            "image-digests.txt": DIGESTS,
            "kernel-digests.txt": KERNEL,
            "kernel-unsigned.txt": UNSIGNED,
            **(files or {}),
        }
        api = {
            f"/repos/{R}/actions/runs/412": run or run_object(),
            f"/repos/{R}/actions/runs/412/attempts/1/jobs?per_page=100": pages
            or jobs(),
            f"/repos/{R}/branches/iso-v0": {"commit": {"sha": TIP}},
            f"/repos/{R}/compare/{SHA}...{TIP}": {"status": "ahead"},
            f"/repos/{R}/compare/{OLD}...{TIP}": {"status": "ahead"},
            f"/repos/{R}/compare/{OLD}...{SHA}": {"status": "ahead"},
            SUPERSEDED: [
                {
                    "workflow_runs": [
                        {"id": 400, "created_at": "2026-10-10T12:00:00Z"},
                        {"id": 412, "created_at": "2026-10-11T04:00:00Z"},
                        {"id": 413, "created_at": "2026-10-11T05:00:00Z"},
                    ]
                }
            ],
            **{
                f"/repos/{R}/compare/{k}": {"status": v}
                for k, v in (compare or {}).items()
            },
        }
        tags, configs = {}, {}
        for i, name in enumerate(NAMES):
            configs[f"{REG}/{name}@{digest(i + 1)}"] = {
                "org.opencontainers.image.created": "2026-10-11T04:30:00Z",
                "org.opencontainers.image.revision": SHA,
            }
            if latest:
                tags[f"{REG}/{name}:latest"] = digest(i + 8)
                configs[f"{REG}/{name}@{digest(i + 8)}"] = {
                    "org.opencontainers.image.created": "2026-10-10T00:00:00Z",
                    "org.opencontainers.image.revision": OLD,
                    **(labels or {}),
                }
        self.registry(
            {
                "api": api,
                "tags": tags,
                "configs": configs,
                **(extra or {}),
                "run_artifacts": {
                    "412": {
                        "release-request": {
                            "release-request.json": json.dumps(req or request())
                        },
                        "image-digests": {
                            "image-digests.txt": files["image-digests.txt"]
                        },
                        "kernel-digests": {
                            "kernel-digests.txt": files["kernel-digests.txt"],
                            "kernel-unsigned.txt": files["kernel-unsigned.txt"],
                        },
                    }
                },
            }
        )

    def fixture(self):
        return json.loads((self.dir / "registry.json").read_text())

    def check(self, run_id="412", ref="refs/heads/iso-v0"):
        return subprocess.run(
            [
                sys.executable,
                "-B",
                str(SCRIPT),
                "check",
                "--out",
                str(self.dir / "out"),
            ],
            capture_output=True,
            text=True,
            cwd=ROOT,
            env={**self.env, "BUILD_RUN_ID": run_id, "GITHUB_REF": ref},
        )

    def order(self):
        return subprocess.run(
            [
                sys.executable,
                "-B",
                str(SCRIPT),
                "order",
                "--dir",
                str(self.dir / "out"),
            ],
            capture_output=True,
            text=True,
            cwd=ROOT,
            env={**self.env, "GITHUB_REF": "refs/heads/iso-v0"},
        )

    def candidate(self):
        return json.loads((self.dir / "out/release-candidate.json").read_text())

    def refused(self, word, **kw):
        r = self.check(**kw)
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn(word, r.stderr)
        self.assertFalse((self.dir / "out/release-candidate.json").exists())

    # The request check (PL55)

    def test_a_successful_build_of_iso_v0_is_a_candidate(self):
        self.github()
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        out = self.candidate()
        self.assertEqual(out["request"]["build_run_id"], 412)
        self.assertEqual([o["verdict"] for o in out["order"]], ["newer", "newer"])
        self.assertEqual(out["superseded"], [400])
        for name, text in (
            ("image-digests.txt", DIGESTS),
            ("kernel-digests.txt", KERNEL),
        ):
            self.assertEqual((self.dir / "out" / name).read_text(), text)

    def test_a_completed_build_is_a_candidate_too(self):
        self.github(run=run_object(status="completed", conclusion="success"))
        self.assertEqual(self.check().returncode, 0)

    def test_a_run_id_that_is_not_a_number_refuses(self):
        self.github()
        self.refused("build_run_id", run_id="412; echo")

    def test_a_release_run_on_another_ref_refuses(self):
        self.github()
        self.refused("releases only from refs/heads/iso-v0, not refs/heads/main", ref="refs/heads/main")

    def test_a_request_from_another_workflow_refuses(self):
        self.github(req=request(workflow_path=".github/workflows/other.yml"))
        self.refused("request names workflow .github/workflows/other.yml")

    def test_a_run_of_another_workflow_refuses(self):
        self.github(run=run_object(path=".github/workflows/other.yml"))
        self.refused("path .github/workflows/other.yml")

    def test_a_request_of_another_attempt_refuses(self):
        self.github(req=request(build_run_attempt=2))
        self.refused("request names attempt 2, the run is at 1")

    def test_a_request_of_another_run_refuses(self):
        self.github(req=request(build_run_id=411))
        self.refused("request names run id 411")

    def test_a_request_of_another_commit_refuses(self):
        self.github(req=request(commit="e" * 40))
        self.refused("request names commit " + "e" * 40)

    def test_a_request_of_another_ref_refuses(self):
        self.github(req=request(ref="refs/heads/main"))
        self.refused("request names ref refs/heads/main")

    def test_a_build_of_another_branch_refuses(self):
        self.github(run=run_object(head_branch="main"))
        self.refused("branch main")

    def test_a_tag_named_like_the_branch_refuses(self):
        self.github(compare={f"{SHA}...{TIP}": "diverged"})
        self.refused("not on iso-v0")

    def test_a_fork_refuses(self):
        self.github(run=run_object(head_repository={"full_name": "someone/athanor"}))
        self.refused("repository someone/athanor")

    def test_a_pull_request_event_refuses(self):
        self.github(run=run_object(event="pull_request"))
        self.refused("event pull_request")

    def test_a_queued_build_refuses(self):
        self.github(run=run_object(status="queued"))
        self.refused("status queued")

    def test_a_failed_job_refuses(self):
        self.github(
            pages=jobs({"name": "verify-system-images", "conclusion": "failure"})
        )
        self.refused("job verify-system-images failure")

    def test_a_failed_job_on_a_later_page_refuses(self):
        self.github(
            pages=jobs()
            + [{"jobs": [{"name": "dag-compile (x)", "conclusion": "cancelled"}]}]
        )
        self.refused("job dag-compile (x) cancelled")

    def test_a_skipped_image_job_refuses(self):
        self.github(pages=jobs(image="skipped"))
        self.refused("did not succeed once")

    def test_a_changed_file_refuses(self):
        for name in ("image-digests.txt", "kernel-digests.txt", "kernel-unsigned.txt"):
            with self.subTest(name=name):
                self.github(files={name: "tampered\n"})
                self.refused(f"{name} differs from the hash of the request")

    def test_a_malformed_request_refuses(self):
        self.github(req={**request(), "extra": 1})
        self.refused("$.extra: not allowed")

    def test_a_missing_artifact_refuses(self):
        self.github()
        fx = self.fixture()
        del fx["run_artifacts"]["412"]["kernel-digests"]
        self.registry(fx)
        self.refused("-n kernel-digests")

    # The order check (PL58)

    def test_no_latest_passes(self):
        self.github(latest=False)
        self.assertEqual(self.check().returncode, 0)
        out = self.candidate()
        self.assertEqual([o["verdict"] for o in out["order"]], ["absent", "absent"])
        self.assertEqual(out["superseded"], [])

    def test_a_registry_error_is_not_a_missing_latest(self):
        self.github(latest=False, extra={"errors": [f"{REG}/athanor-system:latest"]})
        self.refused("athanor-system:latest cannot be read")

    def test_latest_at_the_candidate_is_a_no_op(self):
        self.github()
        fx = self.fixture()
        fx["tags"][f"{REG}/athanor-system:latest"] = digest(1)
        self.registry(fx)
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.candidate()["order"][0]["verdict"], "equal")

    def test_an_older_candidate_refuses(self):
        self.github(labels={"org.opencontainers.image.created": "2026-10-12T00:00:00Z"})
        self.refused("is not later than :latest")

    def test_a_candidate_of_the_same_second_refuses(self):
        self.github(labels={"org.opencontainers.image.created": "2026-10-11T04:30:00Z"})
        self.refused("is not later than :latest")

    def test_a_latest_without_a_build_time_refuses(self):
        self.github(labels={"org.opencontainers.image.created": ""})
        self.refused("no org.opencontainers.image.created label")

    def test_a_latest_commit_on_iso_v0_but_not_an_ancestor_refuses(self):
        self.github(compare={f"{OLD}...{SHA}": "diverged"})
        self.refused(f":latest commit {OLD} is not an ancestor of {SHA}")

    def test_a_latest_commit_absent_from_the_repository_refuses(self):
        self.github()
        fx = self.fixture()
        del fx["api"][f"/repos/{R}/compare/{OLD}...{TIP}"]
        self.registry(fx)
        self.refused(f"compare/{OLD}...{TIP} --jq .status: gh: Not Found")

    def test_a_latest_revision_that_is_not_a_commit_refuses(self):
        self.github(labels={"org.opencontainers.image.revision": "iso-v0"})
        self.refused(":latest names revision 'iso-v0', not a commit")

    def test_a_latest_built_from_main_is_ordered_by_time_alone(self):
        self.github(
            compare={f"{OLD}...{TIP}": "diverged", f"{OLD}...{SHA}": "diverged"}
        )
        self.assertEqual(self.check().returncode, 0)

    def test_an_unlabelled_latest_is_ordered_by_time_alone(self):
        self.github(labels={"org.opencontainers.image.revision": ""})
        self.assertEqual(self.check().returncode, 0)

    def test_order_again_refuses_once_a_newer_release_moved_latest(self):
        self.github()
        self.assertEqual(self.check().returncode, 0)
        fx = self.fixture()
        fx["tags"][f"{REG}/athanor-system:latest"] = digest(14)
        fx["configs"][f"{REG}/athanor-system@{digest(14)}"] = {
            "org.opencontainers.image.created": "2026-10-12T00:00:00Z",
            "org.opencontainers.image.revision": SHA,
        }
        fx["api"][f"/repos/{R}/compare/{SHA}...{SHA}"] = {"status": "identical"}
        self.registry(fx)
        r = self.order()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("is not later than :latest", r.stderr)

    def test_order_again_passes_when_latest_is_already_the_candidate(self):
        self.github()
        self.assertEqual(self.check().returncode, 0)
        fx = self.fixture()
        for i, name in enumerate(NAMES):
            fx["tags"][f"{REG}/{name}:latest"] = digest(i + 1)
        self.registry(fx)
        r = self.order()
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_order_refuses_on_another_ref(self):
        self.github()
        self.assertEqual(self.check().returncode, 0)
        r = subprocess.run(
            [
                sys.executable,
                "-B",
                str(SCRIPT),
                "order",
                "--dir",
                str(self.dir / "out"),
            ],
            capture_output=True,
            text=True,
            cwd=ROOT,
            env={**self.env, "GITHUB_REF": "refs/heads/main"},
        )
        self.assertEqual(r.returncode, 1)


if __name__ == "__main__":
    unittest.main()
