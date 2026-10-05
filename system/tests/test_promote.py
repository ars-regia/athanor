"""Unit tests of system/promote.sh against an offline registry
(python3 -B -m unittest discover -s system/tests -v)."""

import datetime
import json
import pathlib
import subprocess
import unittest

from test_kernel_artifacts import Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
PROMOTE = ROOT / "system" / "promote.sh"
KEY = ROOT / "system/keys/athanor-image-1.pub"
OTHER_KEY = ROOT / "forge/specs/athanor-update/athanor-update-1.0.0/tests/vectors/made/a.pub"
REG = "registry.example/owner"
NAMES = ["athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy"]
SIGNATURE = {"layers": [{"mediaType": "application/vnd.dev.cosign.simplesigning.v1+json"}]}
BUNDLE = {"manifests": [{"artifactType": "application/vnd.dev.sigstore.bundle.v0.3+json"}]}


def digest(n):
    return "sha256:" + f"{n:x}" * 64


class Published(Tool):
    """The fake registry and evidence of one run, for the tests of promote.sh and promote-auto.sh."""

    def published(self, run_created="2026-09-15T10:00:00Z", stable_created="2026-09-10T10:00:00Z", signature=SIGNATURE, signer=KEY):
        fx = {"tags": {}, "configs": {}, "raw": {}, "sigstore_keys": {}}
        for i, name in enumerate(NAMES):
            new, old = digest(i + 1), digest(i + 4)
            fx["tags"][f"{REG}/{name}:412"] = new
            fx["configs"][f"{REG}/{name}@{new}"] = {"org.opencontainers.image.created": run_created}
            fx["raw"][f"{REG}/{name}:sha256-{new[7:]}.sig"] = signature
            fx["sigstore_keys"][f"{REG}/{name}@{new}"] = str(signer)
            if stable_created:
                fx["tags"][f"{REG}/{name}:stable"] = old
                fx["configs"][f"{REG}/{name}@{old}"] = {"org.opencontainers.image.created": stable_created}
        self.registry(fx)

    def evidence(self, run="412", result="pass", hours_ago=30, **override):
        """acceptance-<run>.json as iso-acceptance.yml writes it, naming the published digests."""
        finished = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(hours=hours_ago)
        doc = {"schema": 1, "run_id": run, "acceptance_run_id": "900", "result": result,
               "images": {name: digest(i + 1) for i, name in enumerate(NAMES)},
               "tests": {"installed": True}, "finished_at": finished.strftime("%Y-%m-%dT%H:%M:%SZ")}
        doc.update(override)
        path = self.dir / f"acceptance-{run}.json"
        path.write_text(json.dumps(doc))
        return path

    def promote(self, run="412", evidence=None, dwell=None):
        evidence = self.evidence() if evidence is None else evidence
        env = {**self.env, "REGISTRY": REG}
        if dwell is not None:
            env["PROMOTE_DWELL_HOURS"] = str(dwell)
        r = subprocess.run(["bash", str(PROMOTE), run, str(evidence)], capture_output=True, text=True, env=env)
        log = self.dir / "calls.log"
        calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
        return r, [(c[-2].removeprefix("docker://"), c[-1].removeprefix("docker://")) for c in calls if c[:2] == ["skopeo", "copy"]]


class Promote(Published):
    def test_stable_moves_forward_and_the_previous_stable_keeps_a_tag(self):
        self.published()
        r, copies = self.promote()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(copies), 9)
        for name in NAMES:
            self.assertIn((f"{REG}/{name}:stable", f"{REG}/{name}:stable-previous"), copies)
            new = digest(NAMES.index(name) + 1)
            self.assertIn((f"{REG}/{name}@{new}", f"{REG}/{name}:stable"), copies)
            self.assertLess(copies.index((f"{REG}/{name}:stable", f"{REG}/{name}:stable-previous")), copies.index((f"{REG}/{name}@{new}", f"{REG}/{name}:stable")))
        self.assertTrue(any(dest.rsplit(":", 1)[1].startswith("stable-2") for _, dest in copies))

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
            self.assertEqual(r.returncode, 1)
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

    def refused(self, message, **kwargs):
        self.published()
        r, copies = self.promote(**kwargs)
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn(message, r.stderr)
        self.assertEqual(copies, [])

    def test_a_run_without_evidence_is_not_promoted(self):
        self.refused("is not acceptance evidence", evidence=self.dir / "missing.json")

    def test_evidence_of_another_run_is_refused(self):
        self.refused("is not acceptance evidence", evidence=self.evidence(run_id="411"))

    def test_a_failed_acceptance_is_not_promoted(self):
        self.refused("did not pass acceptance (result: fail)", evidence=self.evidence(result="fail"))

    def test_a_digest_the_evidence_does_not_name_is_not_promoted(self):
        # The run tag now points at a digest other than the one the acceptance installed.
        images = {name: digest(i + 1) for i, name in enumerate(NAMES)}
        images["athanor-system-nvidia"] = digest(9)
        self.refused("is not the digest the acceptance evidence names", evidence=self.evidence(images=images))

    def test_evidence_younger_than_the_dwell_time_is_refused(self):
        self.refused("less than 24 h ago", evidence=self.evidence(hours_ago=2), dwell=24)

    def test_evidence_older_than_the_dwell_time_is_promoted(self):
        self.published()
        r, copies = self.promote(evidence=self.evidence(hours_ago=25), dwell=24)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(copies), 9)

    def test_a_run_id_is_a_number(self):
        self.published()
        r, _ = self.promote(run="latest")
        self.assertEqual(r.returncode, 2)


if __name__ == "__main__":
    unittest.main()
