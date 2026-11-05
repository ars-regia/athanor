"""system/check_evidence_copies.py against an offline evidence registry, GitHub API and Releases."""

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
SCRIPT = HERE.parent / "check_evidence_copies.py"
OWNER = "acme"
REPO = f"{OWNER}/athanor"
EVID = f"quay.io/{OWNER}/athanor-evidence"
JOB = "Point stable at the run"
DEPLOYMENTS = f"repos/{REPO}/deployments?environment=release"
RELEASES = f"repos/{REPO}/releases"


def bundle(promotion):
    """A signed evidence bundle as a tar: promotion.json, the sums and the signature."""
    files = {"promotion.json": json.dumps(promotion).encode()}
    sums = "".join(
        f"{hashlib.sha256(d).hexdigest()}  {n}\n" for n, d in sorted(files.items())
    )
    files |= {"bundle.sha256": sums.encode(), "bundle.sha256.sigstore.json": b"{}"}
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w") as tar:
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return buf.getvalue()


def plan(build, promotion):
    return {
        "run_id": build,
        "trigger": f"https://github.com/{REPO}/actions/runs/{promotion}",
    }


class EvidenceCopies(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        for tool in ("skopeo", "gh", "cosign"):
            (bin_dir / tool).symlink_to(HERE / "fake_evidence_tools.py")
        self.environ = {
            **os.environ,
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "FAKE_EVIDENCE": str(self.dir / "fx.json"),
            "EVIDENCE_REGISTRY": f"quay.io/{OWNER}",
            "GITHUB_REPOSITORY": REPO,
            "GITHUB_SERVER_URL": "https://github.com",
        }
        self.fx = {
            "list": {EVID: []},
            "bundles": {},
            "assets": {},
            "api": {DEPLOYMENTS: [], RELEASES: []},
        }

    def tearDown(self):
        self.tmp.cleanup()

    def deploy(self, promotion, job, name=JOB, conclusion="success", state="success"):
        """A deployment in `release` of job `job` of run `promotion`."""
        self.fx["api"][DEPLOYMENTS].append({"id": job})
        self.fx["api"][f"repos/{REPO}/deployments/{job}/statuses"] = [
            {"state": "queued", "log_url": "x"},
            {
                "state": state,
                "log_url": f"https://github.com/{REPO}/actions/runs/{promotion}/job/{job}",
            },
        ]
        self.fx["api"][f"repos/{REPO}/actions/jobs/{job}"] = {
            "name": name,
            "conclusion": conclusion,
        }

    def copy(self, build, promotion, content=None):
        """The bundle of `content` (default: the plan of build, promotion) at <build>-<promotion>."""
        path = self.dir / f"copy-{build}-{promotion}.tar"
        path.write_bytes(bundle(content or plan(build, promotion)))
        self.fx["list"][EVID].append(f"{build}-{promotion}")
        self.fx["bundles"][f"{EVID}:{build}-{promotion}"] = str(path)

    def release(self, tag, build, promotion, content=None):
        asset = f"evidence-{build}-{promotion}.tar"
        path = self.dir / asset
        path.write_bytes(bundle(content or plan(build, promotion)))
        self.fx["assets"][asset] = str(path)
        self.fx["api"][RELEASES].append(
            {"tag_name": tag, "assets": [{"name": asset}, {"name": "notes.md"}]}
        )

    def promoted(self, build, promotion, release=None):
        self.deploy(promotion, 1000 + promotion)
        self.copy(build, promotion)
        if release:
            self.release(release, build, promotion)

    def check(self):
        (self.dir / "fx.json").write_text(json.dumps(self.fx))
        return subprocess.run(
            [sys.executable, "-B", str(SCRIPT)],
            capture_output=True,
            text=True,
            check=False,
            env=self.environ,
        )

    def assertFails(self, r, text):
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn(text, r.stderr)

    def test_every_intact_copy_passes(self):
        self.promoted(5, 6)
        self.promoted(10, 20, release="v0.9.0")
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(
            f"promotion run 6: {EVID}:5-6 verifies against its own signed plan",
            r.stdout,
        )
        self.assertIn(
            f"promotion run 20: {EVID}:10-20 verifies against the Release v0.9.0",
            r.stdout,
        )

    def test_a_deleted_copy_fails_and_names_the_promotion(self):
        self.promoted(5, 6)
        self.promoted(10, 20, release="v0.9.0")
        self.fx["list"][EVID].remove("5-6")
        self.assertFails(self.check(), "promotion run 6: no copy")

    def test_a_listed_copy_that_cannot_be_pulled_fails(self):
        self.promoted(10, 20)
        del self.fx["bundles"][f"{EVID}:10-20"]
        self.assertFails(self.check(), "unreadable, so not checked")

    def test_a_copy_whose_signature_fails_does_not_verify(self):
        self.promoted(10, 20)
        self.fx["cosign_ok"] = False
        self.assertFails(self.check(), f"{EVID}:10-20: does not verify")

    def test_a_copy_that_is_not_the_plan_on_the_release_fails(self):
        self.promoted(10, 20)
        self.release("v0.9.0", 10, 20, content={**plan(10, 20), "day": "20261024"})
        self.assertFails(self.check(), "does not verify against the Release v0.9.0")

    def test_a_genuine_bundle_of_another_promotion_under_this_tag_fails(self):
        self.deploy(20, 1020)
        self.copy(10, 20, content=plan(10, 19))
        self.assertFails(
            self.check(),
            "not build run 10 planned by https://github.com/acme/athanor/actions/runs/20",
        )

    def test_only_a_successful_promote_job_in_release_is_a_promotion(self):
        self.deploy(30, 1030, name="Point the installer ISO's stable at the run")
        self.deploy(31, 1031, conclusion="failure")
        self.deploy(32, 1032, state="failure")
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("no promotion in the environment release", r.stdout)

    def test_an_unreadable_api_or_registry_fails(self):
        self.promoted(10, 20, release="v0.9.0")
        for text in ("deployments", "list-tags", "releases", "release download"):
            with self.subTest(text=text):
                self.fx["errors"] = [text]
                self.assertFails(self.check(), "unreadable, so not checked")

    def test_the_evidence_registry_is_required(self):
        del self.environ["EVIDENCE_REGISTRY"]
        self.assertEqual(self.check().returncode, 2)


if __name__ == "__main__":
    unittest.main()
