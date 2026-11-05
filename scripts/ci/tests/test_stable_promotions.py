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


def plan(run, digest, promotion_run, day, previous):
    return {
        "run_id": run,
        "trigger": f"https://github.com/{REPO}/actions/runs/{promotion_run}",
        "day": day,
        "images": [{"name": "athanor-system", "digest": digest, "previous_stable": previous}],
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
                f"{IMAGE}:stable-20261017": D2,
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

    def promotion(self, run, promotion_run, digest, approved=True, trigger_run=None, day="20261024", previous=None):
        """A bundle in the evidence registry; approved: the promote job succeeded in `release`."""
        path = self.dir / f"{run}-{promotion_run}.tar"
        path.write_bytes(bundle(plan(run, digest, trigger_run or promotion_run, day, previous)))
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

    def chain(self):
        """The matching chain: D2 promoted on the 17th, then D1 on the 24th over it."""
        self.promotion(5, 6, D2, day="20261017")
        self.promotion(10, 20, D1, previous=D2)

    def tag(self, name, digest, listed=True):
        self.fx["tags"][f"{IMAGE}:{name}"] = digest
        if listed:
            self.fx["list"][IMAGE].append(name)

    def drop(self, name):
        """Delete a tag from the registry, the listing included."""
        self.fx["tags"].pop(f"{IMAGE}:{name}", None)
        if name in self.fx["list"][IMAGE]:
            self.fx["list"][IMAGE].remove(name)

    def fails(self, *texts):
        r = self.check()
        self.assertEqual(r.returncode, 1, r.stdout)
        for text in texts:
            self.assertIn(text, r.stderr)
        return r

    def test_the_matching_chain_passes(self):
        self.chain()
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("newest run 20", r.stdout)

    def test_a_stable_with_no_record_fails_and_names_the_digest(self):
        self.promotion(5, 6, D2, day="20261017")
        self.fails(f"athanor-system:stable: {D1}, but the newest promotion (run 6) set {D2}")

    def test_an_image_with_no_record_and_a_stable_fails_and_names_the_digest(self):
        self.fails(f"athanor-system:stable: {D1} has no promotion record")

    def test_an_image_never_promoted_is_skipped(self):
        for name in ("stable", "stable-previous", "stable-20261024", "stable-20261017"):
            del self.fx["tags"][f"{IMAGE}:{name}"]
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("never promoted", r.stdout)

    def test_a_forged_stable_previous_fails_when_the_newest_record_has_none(self):
        self.promotion(5, 6, D2, day="20261024")
        self.tag("stable", D2, listed=False)
        self.tag("stable-previous", D1, listed=False)
        self.fails(f"athanor-system:stable-previous: {D1}, but the newest promotion (run 6) recorded no previous stable")

    def test_no_stable_previous_is_right_when_the_newest_record_has_none(self):
        self.promotion(5, 6, D2, day="20261024")
        self.tag("stable", D2, listed=False)
        self.tag("stable-20261024", D2, listed=False)
        self.drop("stable-20261017")
        del self.fx["tags"][f"{IMAGE}:stable-previous"]
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_an_image_with_no_record_fails_on_a_lone_stable_previous(self):
        for name in ("stable", "stable-20261024", "stable-20261017"):
            del self.fx["tags"][f"{IMAGE}:{name}"]
        self.fails(f"athanor-system:stable-previous: {D2} has no promotion record")

    def test_an_image_with_no_record_fails_on_a_lone_dated_tag(self):
        for name in ("stable", "stable-previous", "stable-20261017"):
            del self.fx["tags"][f"{IMAGE}:{name}"]
        self.fails(f"athanor-system:stable-20261024: {D1} has no promotion record")

    def test_a_deleted_newest_dated_tag_fails(self):
        self.chain()
        self.drop("stable-20261024")
        self.fails("athanor-system:stable-20261024: missing")

    def test_deleted_dated_tags_fail(self):
        self.chain()
        for day in ("20261017", "20261024"):
            self.drop(f"stable-{day}")
        self.fails("athanor-system:stable-20261024: missing")

    def test_an_older_dated_tag_moved_fails(self):
        self.chain()
        self.tag("stable-20261017", D1, listed=False)
        self.fails(f"athanor-system:stable-20261017: {D1}, but the last promotion of that day (run 6) set {D2}")

    def test_a_dated_tag_before_the_first_record_fails(self):
        self.chain()
        self.tag("stable-20261001", D2)
        self.fails(f"athanor-system:stable-20261001: {D2}, but no promotion record has that day")

    def test_a_previous_stable_no_earlier_record_set_fails(self):
        d3 = "sha256:" + "3" * 64
        self.promotion(5, 6, D2, day="20261017")
        self.promotion(10, 20, D1, previous=d3)
        self.tag("stable-previous", d3, listed=False)
        self.fails(f"recorded previous stable {d3}, which no earlier promotion record set")

    def test_a_first_record_with_a_previous_stable_fails(self):
        self.promotion(10, 20, D1, previous=D2, day="20261024")
        self.fails(f"recorded previous stable {D2}, which no earlier promotion record set")

    def test_a_leftover_stable_previous_names_the_deleted_stable_case(self):
        self.promotion(5, 6, D2, day="20261024")
        self.tag("stable", D2, listed=False)
        self.fails(":stable was deleted before the promotion, or :stable-previous was moved")

    def test_a_rollback_to_an_older_recorded_digest_fails(self):
        self.chain()
        self.tag("stable", D2, listed=False)
        self.fails(f"athanor-system:stable: {D2}, but the newest promotion (run 20) set {D1}")

    def test_a_stable_previous_out_of_step_fails_with_what_to_do(self):
        self.chain()
        self.tag("stable-previous", D1, listed=False)
        self.fails(f"athanor-system:stable-previous: {D1}, but the newest promotion (run 20) recorded {D2}", "re-run its failed job")

    def test_a_promote_that_stopped_after_the_day_tag_fails_and_says_so(self):
        self.chain()
        self.tag("stable-20261030", D2)
        self.promotion(30, 40, D2, previous=D1, day="20261030", approved=False)
        self.fails(f"athanor-system:stable-20261030: {D2}, but no promotion record has that day", "re-run its failed job")

    def test_a_missing_stable_with_a_record_fails(self):
        self.chain()
        del self.fx["tags"][f"{IMAGE}:stable"]
        self.fails(f"athanor-system:stable: missing, but the newest promotion (run 20) set {D1}")

    def test_a_missing_stable_previous_with_a_record_fails(self):
        self.chain()
        del self.fx["tags"][f"{IMAGE}:stable-previous"]
        self.fails("athanor-system:stable-previous: missing")

    def test_a_future_dated_tag_fails(self):
        self.chain()
        self.tag("stable-99991231", D1)
        self.fails("athanor-system:stable-99991231: not a date, or a date in the future")

    def test_an_invalid_date_cannot_hide_the_real_newest_tag(self):
        self.chain()
        self.tag("stable-99999999", D1)
        self.tag("stable-20261024", D2, listed=False)
        self.fails("athanor-system:stable-99999999: not a date", f"athanor-system:stable-20261024: {D2}, but the last promotion of that day (run 20) set {D1}")

    def test_the_newest_dated_tag_must_be_the_last_record_of_its_day(self):
        self.chain()
        self.tag("stable-20261024", D2, listed=False)
        self.fails(f"athanor-system:stable-20261024: {D2}, but the last promotion of that day (run 20) set {D1}")

    def test_a_plan_whose_promote_job_never_ran_in_release_is_no_record(self):
        self.promotion(5, 6, D2, day="20261017")
        self.promotion(10, 20, D1, approved=False, previous=D2)
        self.fails("did not succeed in the environment release", "athanor-system:stable: " + D1)

    def test_a_bundle_that_names_another_run_is_no_record(self):
        self.promotion(5, 6, D2, day="20261017")
        self.promotion(10, 20, D1, trigger_run=99, previous=D2)
        self.fails("not promotion run 20")

    def test_a_bundle_whose_signature_does_not_verify_is_no_record(self):
        self.chain()
        self.fx["cosign_ok"] = False
        self.fails("does not verify")

    def test_an_unreadable_registry_fails(self):
        self.chain()
        self.fx["errors"] = [f"{IMAGE}:stable-previous"]
        self.fails("unreadable, so not checked")

    def test_an_unreadable_bundle_fails(self):
        self.chain()
        pathlib.Path(self.fx["bundles"][f"{EVID}:10-20"]).write_bytes(b"not a tar")
        self.fails("unreadable, so not checked")

    def test_an_unreadable_github_api_fails(self):
        self.chain()
        self.fx["errors"] = ["deployments"]
        self.fails("unreadable, so not checked")

    def test_no_registry_owner_is_assumed(self):
        del self.environ["REGISTRY"]
        r = self.check()
        self.assertEqual(r.returncode, 2)


if __name__ == "__main__":
    unittest.main()
