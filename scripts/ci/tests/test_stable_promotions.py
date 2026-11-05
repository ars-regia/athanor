"""scripts/ci/stable_promotions.py against an offline registry, evidence registry and GitHub API."""

import hashlib
import io
import json
import os
import pathlib
import subprocess
import sys
import tarfile
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "stable_promotions.py"
HOST = "registry.example"
JOB = "Point stable at the run"
OWNER = "acme"
REG, EVID, REPO = f"{HOST}/{OWNER}", f"quay.io/{OWNER}/athanor-evidence", f"{OWNER}/athanor"
D1, D2 = "sha256:" + "1" * 64, "sha256:" + "2" * 64
IMAGE = f"{REG}/athanor-system"
DEPLOYMENTS = f"repos/{REPO}/deployments?environment=release"


def layer(files):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w") as tar:
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return buf.getvalue()


def bundle(promotion):
    """The files of a signed evidence bundle: promotion.json, the sums and the signature."""
    files = {"promotion.json": json.dumps(promotion).encode()}
    sums = "".join(
        f"{hashlib.sha256(d).hexdigest()}  {n}\n" for n, d in sorted(files.items())
    )
    return layer(
        {**files, "bundle.sha256": sums.encode(), "bundle.sha256.sigstore.json": b"{}"}
    )


def plan(run, digest, promotion_run):
    return {
        "run_id": run,
        "trigger": f"https://github.com/{REPO}/actions/runs/{promotion_run}",
        "images": [{"name": "athanor-system", "digest": digest}],
    }


class StablePromotions(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        for tool in ("skopeo", "gh", "cosign"):
            (bin_dir / tool).symlink_to(HERE / "fake_stable_tools.py")
        self.environ = {
            **os.environ,
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "FAKE_STABLE": str(self.dir / "fx.json"),
            "REGISTRY": REG,
            "EVIDENCE_REGISTRY": "quay.io/acme",
            "GITHUB_REPOSITORY": REPO,
        }
        self.fx = {
            "tags": {
                f"{IMAGE}:stable": D1,
                f"{IMAGE}:stable-previous": D2,
                f"{IMAGE}:stable-20261024": D1,
            },
            "list": {
                IMAGE: [
                    "stable",
                    "stable-previous",
                    "stable-20261017",
                    "stable-20261024",
                    "7",
                ],
                EVID: [],
            },
            "bundles": {},
            "api": {DEPLOYMENTS: []},
        }

    def tearDown(self):
        self.tmp.cleanup()

    def promotion(self, run, promotion_run, digest, approved=True, trigger_run=None):
        """A bundle in the evidence registry; approved: the promote job succeeded in `release`."""
        path = self.dir / f"{run}-{promotion_run}.tar"
        path.write_bytes(bundle(plan(run, digest, trigger_run or promotion_run)))
        tag = f"{run}-{promotion_run}"
        self.fx["list"][EVID].append(tag)
        self.fx["bundles"][f"{EVID}:{tag}"] = str(path)
        job = 1000 + promotion_run
        self.fx["api"][f"repos/{REPO}/actions/runs/{promotion_run}/jobs"] = {
            "jobs": [{"id": job, "name": JOB, "conclusion": "success"}]
        }
        if approved:
            self.fx["api"][DEPLOYMENTS].append({"id": job})
            self.fx["api"][f"repos/{REPO}/deployments/{job}/statuses"] = [
                {"state": "queued", "log_url": "x"},
                {
                    "state": "success",
                    "log_url": f"https://github.com/{REPO}/actions/runs/{promotion_run}/job/{job}",
                },
            ]

    def check(self):
        (self.dir / "fx.json").write_text(json.dumps(self.fx))
        return subprocess.run(
            [sys.executable, "-B", str(SCRIPT), "--image", "athanor-system"],
            capture_output=True,
            text=True,
            env=self.environ,
        )

    def test_digests_with_an_approved_record_pass(self):
        self.promotion(10, 20, D1)
        self.promotion(5, 6, D2)
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(
            f"athanor-system:stable-20261024: {D1} has a promotion record", r.stdout
        )

    def test_a_stable_with_no_record_fails_and_names_the_digest(self):
        self.promotion(5, 6, D2)
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn(f"athanor-system:stable: {D1} has no promotion record", r.stderr)
        self.assertNotIn(f"stable-previous: {D2}", r.stderr)

    def test_a_plan_whose_promote_job_never_ran_in_release_is_no_record(self):
        self.promotion(10, 20, D1, approved=False)
        self.promotion(5, 6, D2)
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn(f"athanor-system:stable: {D1} has no promotion record", r.stderr)
        self.assertIn("did not succeed in the environment release", r.stderr)

    def test_a_bundle_that_names_another_run_is_no_record(self):
        self.promotion(10, 20, D1, trigger_run=99)
        self.promotion(5, 6, D2)
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("not promotion run 20", r.stderr)

    def test_a_bundle_whose_signature_does_not_verify_is_no_record(self):
        self.promotion(10, 20, D1)
        self.fx["cosign_ok"] = False
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not verify", r.stderr)

    def test_an_unreadable_registry_fails(self):
        self.promotion(10, 20, D1)
        self.fx["errors"] = [f"{IMAGE}:stable-previous"]
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("unreadable, so not checked", r.stderr)

    def test_an_unreadable_bundle_fails(self):
        self.promotion(10, 20, D1)
        pathlib.Path(self.fx["bundles"][f"{EVID}:10-20"]).write_bytes(b"not a tar")
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("unreadable, so not checked", r.stderr)

    def test_an_unreadable_github_api_fails(self):
        self.promotion(10, 20, D1)
        self.fx["errors"] = ["deployments"]
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("unreadable, so not checked", r.stderr)

    def test_a_tag_that_does_not_exist_is_skipped(self):
        del self.fx["tags"][f"{IMAGE}:stable-previous"]
        self.promotion(10, 20, D1)
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("stable-previous: no such tag", r.stdout)

    def test_the_newest_dated_tag_is_the_one_checked(self):
        self.fx["tags"][f"{IMAGE}:stable-20261024"] = "sha256:" + "3" * 64
        self.promotion(10, 20, D1)
        self.promotion(5, 6, D2)
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("stable-20261024: sha256:" + "3" * 64, r.stderr)

    def test_no_registry_owner_is_assumed(self):
        del self.environ["REGISTRY"]
        r = self.check()
        self.assertEqual(r.returncode, 2)


if __name__ == "__main__":
    unittest.main()
