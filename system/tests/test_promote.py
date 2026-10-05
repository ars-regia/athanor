"""Unit tests of system/promote.sh against an offline registry
(python3 -B -m unittest discover -s system/tests -v)."""

import datetime
import importlib.util
import json
import pathlib
import re
import subprocess
import unittest

from test_kernel_artifacts import Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
PROMOTE = ROOT / "system" / "promote.sh"
KEY = ROOT / "system/keys/athanor-image-1.pub"
OTHER_KEY = (
    ROOT / "forge/specs/athanor-update/athanor-update-1.0.0/tests/vectors/made/a.pub"
)
REG = "registry.example/owner"
NAMES = ["athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy"]
SIGNATURE = {
    "layers": [{"mediaType": "application/vnd.dev.cosign.simplesigning.v1+json"}]
}
BUNDLE = {
    "manifests": [{"artifactType": "application/vnd.dev.sigstore.bundle.v0.3+json"}]
}
REVISION = "c" * 40
WORKFLOWS = "https://github.com/hr-mes/athanor/.github/workflows"
# The certificate identity of iso-acceptance.yml on the trusted branch, and three it must not take.
TRUSTED = f"{WORKFLOWS}/iso-acceptance.yml@refs/heads/iso-v0"
OTHER_BRANCH = f"{WORKFLOWS}/iso-acceptance.yml@refs/heads/feature"
OTHER_WORKFLOW = f"{WORKFLOWS}/kernel-build.yml@refs/heads/iso-v0"
FORK = "https://github.com/someone/athanor/.github/workflows/iso-acceptance.yml@refs/heads/iso-v0"
BUILDER = f"{WORKFLOWS}/call-system-image.yml@refs/heads/iso-v0"


def built(identity=BUILDER, sha=REVISION, repository="hr-mes/athanor"):
    """The keyless signature call-system-image.yml leaves on each image digest it pushed."""
    return {"identity": identity, "sha": sha, "repository": repository}


def digest(n):
    return "sha256:" + f"{n:x}" * 64


def evidence_checks():
    spec = importlib.util.spec_from_file_location(
        "evidence", ROOT / "forge/test/iso/evidence.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.CHECKS


def build_run(**override):
    """The Orchestrator run that built the images, as GET /repos/{owner}/{repo}/actions/runs/{id} has it."""
    return {
        "path": ".github/workflows/athanor-forge-orchestrator.yml",
        "head_branch": "iso-v0",
        "head_sha": REVISION,
        "event": "push",
        "status": "completed",
        "conclusion": "success",
        "repository": {"full_name": "hr-mes/athanor"},
        "head_repository": {"full_name": "hr-mes/athanor"},
        **override,
    }


def predicate(run="412", result="pass", hours_ago=30, **override):
    """The acceptance evidence as forge/test/iso/evidence.py writes it for the published run."""
    finished = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(
        hours=hours_ago
    )
    doc = {
        "schema": 1,
        "kind": "athanor-acceptance",
        "run_id": run,
        "revision": REVISION,
        "result": result,
        "images": {name: digest(i + 1) for i, name in enumerate(NAMES)},
        "tested_image": "athanor-system",
        "iso": "sha256:" + "9" * 64,
        "tests": {name: True for name in evidence_checks()},
        "finished_at": finished.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "acceptance": {
            "repository": "hr-mes/athanor",
            "run_id": "900",
            "run_attempt": "1",
            "ref": "refs/heads/iso-v0",
            "event": "schedule",
            "sha": "e" * 40,
            "workflow_ref": "hr-mes/athanor/.github/workflows/athanor-forge-orchestrator.yml@refs/heads/iso-v0",
        },
    }
    doc.update(override)
    return doc


class Published(Tool):
    """The fake registry of one run with its acceptance attestations, for promote.sh and promote-auto.sh."""

    def published(
        self,
        run="412",
        run_created="2026-09-15T10:00:00Z",
        stable_created="2026-09-10T10:00:00Z",
        signature=SIGNATURE,
        signer=KEY,
        attestations=None,
        revision=REVISION,
        fx=None,
        base=1,
        build=None,
        signed_by=None,
    ):
        """attestations: [(identity, predicate)] on every image of the run (default: one trusted pass).
        base: the run's digests are digest(base), digest(base + 1), digest(base + 2).
        build: the run's record in the GitHub API (default: build_run()).
        signed_by: the keyless build signature on every digest (default: built(); False for none)."""
        if attestations is None:
            images = {name: digest(base + i) for i, name in enumerate(NAMES)}
            attestations = [(TRUSTED, predicate(run=run, images=images))]
        fx = fx or {
            "tags": {},
            "configs": {},
            "raw": {},
            "sigstore_keys": {},
            "attestations": {},
            "build_runs": {},
            "signatures": {},
        }
        fx["build_runs"][run] = build_run() if build is None else build
        for i, name in enumerate(NAMES):
            new, old = digest(base + i), digest(0xA + i)
            fx["tags"][f"{REG}/{name}:{run}"] = new
            labels = {"org.opencontainers.image.created": run_created}
            if revision:
                labels["org.opencontainers.image.revision"] = revision
            fx["configs"][f"{REG}/{name}@{new}"] = labels
            fx["raw"][f"{REG}/{name}:sha256-{new[7:]}.sig"] = signature
            fx["sigstore_keys"][f"{REG}/{name}@{new}"] = str(signer)
            if signed_by is not False:
                fx["signatures"][f"{REG}/{name}@{new}"] = signed_by or built()
            fx["attestations"][f"{REG}/{name}@{new}"] = [
                {"identity": who, "predicate": p} for who, p in attestations
            ]
            if stable_created:
                fx["tags"][f"{REG}/{name}:stable"] = old
                fx["configs"][f"{REG}/{name}@{old}"] = {
                    "org.opencontainers.image.created": stable_created
                }
        self.registry(fx)
        return fx

    def calls(self):
        log = self.dir / "calls.log"
        return (
            [json.loads(line) for line in log.read_text().splitlines()]
            if log.exists()
            else []
        )

    def promote(self, run="412", dwell=None, **env):
        env = {**self.env, "REGISTRY": REG, **env}
        if dwell is not None:
            env["PROMOTE_DWELL_HOURS"] = str(dwell)
        r = subprocess.run(["bash", str(PROMOTE), run], capture_output=True, text=True, env=env)
        return r, [
            (c[-2].removeprefix("docker://"), c[-1].removeprefix("docker://"))
            for c in self.calls()
            if c[:2] == ["skopeo", "copy"] and "--policy" not in c
        ]


class Promote(Published):
    def test_stable_moves_forward_and_the_previous_stable_keeps_a_tag(self):
        self.published()
        r, copies = self.promote()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(copies), 9)
        for name in NAMES:
            new = digest(NAMES.index(name) + 1)
            self.assertIn(
                (f"{REG}/{name}:stable", f"{REG}/{name}:stable-previous"), copies
            )
            self.assertIn((f"{REG}/{name}@{new}", f"{REG}/{name}:stable"), copies)
            self.assertLess(
                copies.index((f"{REG}/{name}:stable", f"{REG}/{name}:stable-previous")),
                copies.index((f"{REG}/{name}@{new}", f"{REG}/{name}:stable")),
            )
        self.assertTrue(
            any(dest.rsplit(":", 1)[1].startswith("stable-2") for _, dest in copies)
        )

    def test_the_attestation_is_checked_against_the_trusted_workflow_and_repository(
        self,
    ):
        self.published()
        self.promote()
        verify = next(
            c for c in self.calls() if c[:2] == ["cosign", "verify-attestation"]
        )
        regex = verify[verify.index("--certificate-identity-regexp") + 1]
        self.assertTrue(re.search(regex, TRUSTED))
        for identity in (OTHER_BRANCH, OTHER_WORKFLOW, FORK, TRUSTED + "-evil"):
            self.assertIsNone(re.search(regex, identity), identity)
        self.assertEqual(
            verify[verify.index("--certificate-github-workflow-repository") + 1],
            "hr-mes/athanor",
        )
        self.assertEqual(
            verify[verify.index("--certificate-oidc-issuer") + 1],
            "https://token.actions.githubusercontent.com",
        )

    def test_the_first_promotion_has_no_previous_stable(self):
        self.published(stable_created=None)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(copies), 6)
        self.assertFalse(any(dest.endswith(":stable-previous") for _, dest in copies))

    def test_an_older_or_equal_build_is_refused_and_nothing_moves(self):
        for run_created in ("2026-09-01T10:00:00Z", "2026-09-10T10:00:00Z"):
            self.published(run_created=run_created)
            (self.dir / "calls.log").unlink(missing_ok=True)
            r, copies = self.promote()
            self.assertEqual(r.returncode, 3)
            self.assertIn("not newer than the current stable", r.stderr)
            self.assertEqual(copies, [])

    def test_a_run_signed_only_with_a_cosign_3_bundle_is_refused(self):
        self.published(signature=BUNDLE)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("no signature a machine can verify", r.stderr)
        self.assertEqual(copies, [])

    def test_a_signature_made_with_another_key_is_not_promoted(self):
        self.published(signer=OTHER_KEY)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not verify with the keys under system/keys", r.stderr)
        self.assertEqual(copies, [])

    def ineligible(self, message, dwell=None, **kwargs):
        self.published(**kwargs)
        r, copies = self.promote(dwell=dwell)
        self.assertEqual(r.returncode, 4, r.stderr)
        self.assertIn(message, r.stderr)
        self.assertEqual(copies, [])

    def test_a_run_without_evidence_is_not_promoted(self):
        self.ineligible(
            "has no acceptance evidence signed by iso-acceptance.yml", attestations=[]
        )

    def test_evidence_signed_outside_the_trusted_workflow_and_branch_is_refused(self):
        # A branch, another workflow of this repository, or a fork: the predicate is perfect,
        # the signer is not.
        for identity in (OTHER_BRANCH, OTHER_WORKFLOW, FORK):
            with self.subTest(identity=identity):
                self.ineligible(
                    "has no acceptance evidence signed by iso-acceptance.yml",
                    attestations=[(identity, predicate())],
                )

    def test_a_run_tag_that_does_not_exist_is_not_eligible(self):
        fx = self.published()
        fx["build_runs"]["413"] = build_run()
        self.registry(fx)
        r, copies = self.promote(run="413")
        self.assertEqual(r.returncode, 4, r.stderr)
        self.assertIn("has no image tagged 413", r.stderr)
        self.assertEqual(copies, [])

    def test_images_not_built_by_the_orchestrator_on_a_trusted_branch_are_refused(self):
        # Passing evidence, signed by the trusted identity, for images whose build run is not a
        # successful Orchestrator run of a trusted branch of this repository: acceptance tests
        # whatever ISO it is given, so its pass says nothing about where the images came from.
        for field, value in (
            ("head_branch", "feature"),
            ("path", ".github/workflows/call-system-image.yml"),
            ("event", "pull_request"),
            ("conclusion", "failure"),
            ("status", "in_progress"),
            ("repository", {"full_name": "someone/athanor"}),
            ("head_repository", {"full_name": "someone/athanor"}),
            ("head_sha", "not-a-commit"),
        ):
            with self.subTest(field=field):
                self.ineligible(
                    "is not a successful Orchestrator run of a trusted branch",
                    build=build_run(**{field: value}),
                )

    def test_images_from_another_commit_than_the_build_run_are_refused(self):
        self.ineligible(
            "not from run 412's commit " + "d" * 40,
            build=build_run(head_sha="d" * 40),
            signed_by=built(sha="d" * 40),
        )

    def test_a_run_unknown_to_github_is_refused(self):
        fx = self.published()
        del fx["build_runs"]["412"]
        self.registry(fx)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 4, r.stderr)
        self.assertIn("is not a workflow run of hr-mes/athanor", r.stderr)
        self.assertEqual(copies, [])

    def test_an_image_without_its_commit_cannot_be_bound_to_evidence(self):
        self.ineligible("has no org.opencontainers.image.revision label", revision=None)

    def test_forged_or_mismatched_evidence_is_refused(self):
        # Each signed by the trusted identity, each wrong in one field: the verifier checks every
        # field against the promotion target, so none of them counts.
        images = predicate()["images"]
        tests = predicate()["tests"]
        context = predicate()["acceptance"]
        workflows = "hr-mes/athanor/.github/workflows"
        cases = {
            "a failed acceptance": predicate(result="fail"),
            "another run": predicate(run_id="411"),
            "another commit": predicate(revision="d" * 40),
            "another digest": predicate(
                images={**images, "athanor-system-nvidia": digest(9)}
            ),
            "a tag for a digest": predicate(
                images={**images, "athanor-system-nvidia-legacy": "latest"}
            ),
            "an image missing": predicate(
                images={
                    k: v
                    for k, v in images.items()
                    if k != "athanor-system-nvidia-legacy"
                }
            ),
            "an extra image": predicate(images={**images, "athanor-other": digest(7)}),
            "a check missing": predicate(
                tests={k: v for k, v in tests.items() if k != "session"}
            ),
            "a check false": predicate(tests={**tests, "greeter": False}),
            "a truthy check": predicate(tests={**tests, "greeter": "yes"}),
            "an extra check": predicate(tests={**tests, "something-else": True}),
            "no checks": predicate(tests={}),
            "another tested image": predicate(tested_image="athanor-system-nvidia"),
            "another kind": predicate(kind="something-else"),
            "another schema": predicate(schema=2),
            "another repository": predicate(
                acceptance={**context, "repository": "someone/athanor"}
            ),
            "another branch": predicate(
                acceptance={
                    **context,
                    "ref": "refs/heads/feature",
                    "workflow_ref": f"{workflows}/iso-acceptance.yml@refs/heads/feature",
                }
            ),
            "a pull request": predicate(
                acceptance={**context, "event": "pull_request"}
            ),
            "a pull_request_target": predicate(
                acceptance={**context, "event": "pull_request_target"}
            ),
            "another workflow": predicate(
                acceptance={
                    **context,
                    "workflow_ref": f"{workflows}/lint.yml@refs/heads/iso-v0",
                }
            ),
            "a workflow on another ref": predicate(
                acceptance={
                    **context,
                    "workflow_ref": f"{workflows}/iso-acceptance.yml@refs/heads/feature",
                }
            ),
            "a short commit": predicate(acceptance={**context, "sha": "e" * 12}),
            "no acceptance context": predicate(acceptance=None),
            "a malformed time": predicate(finished_at="yesterday"),
            "no ISO digest": predicate(iso=None),
            "an ISO tag": predicate(iso="latest"),
        }
        for name, doc in cases.items():
            with self.subTest(name):
                (self.dir / "calls.log").unlink(missing_ok=True)
                self.ineligible(
                    "has no passing acceptance evidence for run 412",
                    attestations=[(TRUSTED, doc)],
                )

    def test_each_image_checks_its_own_digest(self):
        # The evidence is attested on every image, and each checks the digest it holds.
        fx = self.published()
        bad = predicate(
            images={**predicate()["images"], "athanor-system-nvidia": digest(9)}
        )
        fx["attestations"][f"{REG}/athanor-system-nvidia@{digest(2)}"] = [
            {"identity": TRUSTED, "predicate": bad}
        ]
        self.registry(fx)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 4, r.stderr)
        self.assertIn(
            f"athanor-system-nvidia@{digest(2)} has no passing acceptance evidence",
            r.stderr,
        )
        self.assertEqual(copies, [])

    def test_a_valid_pass_counts_beside_a_failed_attempt(self):
        self.published(
            attestations=[
                (TRUSTED, predicate(result="fail", hours_ago=40)),
                (TRUSTED, predicate(hours_ago=30)),
            ]
        )
        r, copies = self.promote()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(copies), 9)

    def test_evidence_younger_than_the_dwell_time_waits(self):
        self.published(attestations=[(TRUSTED, predicate(hours_ago=2))])
        r, copies = self.promote(dwell=24)
        self.assertEqual(r.returncode, 5, r.stderr)
        self.assertIn("less than 24 h ago", r.stderr)
        self.assertEqual(copies, [])

    def test_an_image_the_trusted_build_did_not_sign_is_refused(self):
        # The :RUN tag moved to another image (or the build never signed): passing evidence on
        # that digest does not make it the run's image. Signed by a branch build, by another
        # workflow, in another repository or at another commit: the same.
        for signed_by in (
            False,
            built(identity=f"{WORKFLOWS}/call-system-image.yml@refs/heads/feature"),
            built(identity=f"{WORKFLOWS}/iso-acceptance.yml@refs/heads/iso-v0"),
            built(repository="someone/athanor"),
            built(sha="d" * 40),
        ):
            with self.subTest(signed_by=signed_by):
                self.ineligible("was not signed by call-system-image.yml", signed_by=signed_by)

    def test_evidence_older_than_the_dwell_time_is_promoted(self):
        self.published(attestations=[(TRUSTED, predicate(hours_ago=25))])
        r, copies = self.promote(dwell=24)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(copies), 9)

    def test_the_verified_predicate_is_written_for_the_record(self):
        self.published()
        out = self.dir / "verified.json"
        r, _ = self.promote(PROMOTE_EVIDENCE_OUT=str(out))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads(out.read_text())["run_id"], "412")

    def test_the_required_checks_are_the_ones_evidence_py_writes(self):
        line = next(
            line
            for line in PROMOTE.read_text().splitlines()
            if line.startswith("CHECKS=")
        )
        self.assertEqual(
            json.loads(line.split("=", 1)[1].strip("'")), list(evidence_checks())
        )

    def test_a_run_id_is_a_number(self):
        self.published()
        r, _ = self.promote(run="latest")
        self.assertEqual(r.returncode, 2)


if __name__ == "__main__":
    unittest.main()
