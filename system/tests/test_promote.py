"""Unit tests of scripts/ci/promotion-plan.sh and system/promote.sh against an offline registry
(python3 -B -m unittest discover -s system/tests -v)."""

import datetime
import hashlib
import json
import pathlib
import subprocess
import unittest

from test_kernel_artifacts import Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
PROMOTE = ROOT / "system" / "promote.sh"
PLAN = ROOT / "scripts" / "ci" / "promotion-plan.sh"
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
OLD = "2026-09-15T11:00:00Z"


def digest(n):
    return "sha256:" + f"{n:x}" * 64


class Promote(Tool):
    def published(
        self,
        names=NAMES,
        run_created="2026-09-15T10:00:00Z",
        stable_created="2026-09-10T10:00:00Z",
        signature=SIGNATURE,
        signer=KEY,
        moves=True,
        evidence=True,
        finished=OLD,
    ):
        fx = {"tags": {}, "configs": {}, "raw": {}, "sigstore_keys": {}, "moves": moves}
        lines = []
        for i, name in enumerate(NAMES):
            new, old = digest(i + 1), digest(i + 4)
            fx["tags"][f"{REG}/{name}:412"] = new
            fx["configs"][f"{REG}/{name}@{new}"] = {
                "org.opencontainers.image.created": run_created
            }
            fx["raw"][f"{REG}/{name}:sha256-{new[7:]}.sig"] = signature
            fx["sigstore_keys"][f"{REG}/{name}@{new}"] = str(signer)
            if stable_created:
                fx["tags"][f"{REG}/{name}:stable"] = old
                fx["configs"][f"{REG}/{name}@{old}"] = {
                    "org.opencontainers.image.created": stable_created
                }
            if name in names:
                lines.append(f"{REG}/{name} 412 {new}")
                (self.artifacts / "packages").mkdir(parents=True, exist_ok=True)
                (self.artifacts / "packages" / f"{name}.txt").write_text(
                    "bash-5.2-1.fc43.x86_64 bb\n"
                )
                if evidence:
                    self.evidence("signature", name, new, finished=finished)
                    if name == "athanor-system":
                        self.evidence("iso-acceptance", name, new, finished=finished)
        self.artifacts.mkdir(parents=True, exist_ok=True)
        (self.artifacts / "image-digests.txt").write_text("\n".join(lines) + "\n")
        self.registry(fx)

    def evidence(self, gate, name, digest_, run_id=412, verdict="pass", finished=OLD):
        (self.artifacts / "evidence").mkdir(parents=True, exist_ok=True)
        (self.artifacts / "evidence" / f"{gate}.{name}.json").write_text(
            json.dumps(
                {
                    "gate": gate,
                    "image": name,
                    "digest": digest_,
                    "run_id": run_id,
                    "verdict": verdict,
                    "workflow_run_url": "https://github.com/ars-regia/athanor/actions/runs/9",
                    "finished_at": finished,
                }
            )
        )

    def set_tag(self, ref, value, created=None):
        fx = json.loads((self.dir / "registry.json").read_text())
        fx["tags"][ref] = value
        if created:
            fx["configs"][f"{ref.rsplit(':', 1)[0]}@{value}"] = {
                "org.opencontainers.image.created": created
            }
        self.registry(fx)

    def promote(self, *flags, run="412", plan=True, between=None, apply_env=None):
        """The two jobs of promote-stable.yml: promotion-plan.sh, then BETWEEN (what may change
        while the approval waits), then promote.sh with APPLY_ENV added to its environment.
        Returns the last result and its tag copies."""
        env = {**self.env, "REGISTRY": REG, "PROMOTE_ARTIFACTS": str(self.artifacts)}
        if plan:
            p = subprocess.run(
                ["bash", str(PLAN), *flags, run],
                capture_output=True,
                text=True,
                env=env,
            )
            if p.returncode != 0:
                return p, []
        if between:
            between()
        (self.dir / "calls.log").unlink(missing_ok=True)
        r = subprocess.run(
            ["bash", str(PROMOTE), *flags, run],
            capture_output=True,
            text=True,
            env={**env, **(apply_env or {})},
        )
        log = self.dir / "calls.log"
        calls = (
            [json.loads(line) for line in log.read_text().splitlines()]
            if log.exists()
            else []
        )
        copies = [
            (c[-2].removeprefix("docker://"), c[-1].removeprefix("docker://"))
            for c in calls
            if c[:2] == ["skopeo", "copy"] and "--policy" not in c
        ]
        return r, copies

    def record(self):
        return json.loads((self.artifacts / "promotion.json").read_text())

    def test_the_default_image_promotes_alone_without_an_override(self):
        self.published()
        r, copies = self.promote()
        self.assertEqual(r.returncode, 0, r.stderr)
        base = f"{REG}/athanor-system"
        self.assertEqual(
            copies,
            [
                (f"{base}@{digest(4)}", f"{base}:stable-previous"),
                (f"{base}@{digest(1)}", f"{base}:stable-{self.record()['day']}"),
                (f"{base}@{digest(1)}", f"{base}:stable"),
            ],
        )
        skipped = {s["name"]: s["reason"] for s in self.record()["skipped"]}
        self.assertIn("hardware", skipped["athanor-system-nvidia"])
        self.assertIn("0.9", skipped["athanor-system-nvidia-legacy"])

    def test_the_first_promotion_has_no_previous_stable(self):
        self.published(stable_created=None)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(copies), 2)
        self.assertFalse(any(dest.endswith(":stable-previous") for _, dest in copies))
        self.assertIsNone(self.record()["images"][0]["previous_stable"])

    def test_the_override_promotes_nvidia_and_is_recorded(self):
        self.published()
        r, copies = self.promote("--hardware-override", "athanor-system-nvidia")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(copies), 6)
        self.assertEqual(self.record()["overrides"], ["athanor-system-nvidia"])

    def test_the_legacy_variant_cannot_be_overridden(self):
        self.published()
        r, copies = self.promote("--hardware-override", "athanor-system-nvidia-legacy")
        self.assertEqual(r.returncode, 2)
        self.assertEqual(copies, [])

    def test_a_variant_the_run_did_not_build_is_skipped(self):
        self.published(names=["athanor-system"])
        r, copies = self.promote("--hardware-override", "athanor-system-nvidia")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(copies), 3)
        reasons = {s["name"]: s["reason"] for s in self.record()["skipped"]}
        self.assertEqual(reasons["athanor-system-nvidia"], "not built by run 412")

    def test_missing_evidence_is_refused_and_nothing_moves(self):
        for gate in ("iso-acceptance", "signature"):
            with self.subTest(gate=gate):
                self.published()
                (self.artifacts / "evidence" / f"{gate}.athanor-system.json").unlink()
                r, copies = self.promote()
                self.assertEqual(r.returncode, 1)
                self.assertIn(f"no {gate} evidence", r.stderr)
                self.assertEqual(copies, [])

    def test_a_fail_verdict_another_digest_or_another_run_is_refused(self):
        for kwargs, word in (
            ({"verdict": "fail"}, "verdict is fail"),
            ({"digest_": digest(7)}, "digest"),
            ({"run_id": 411}, "run id"),
        ):
            with self.subTest(word=word):
                self.published()
                args = {"digest_": digest(1), **kwargs}
                self.evidence(
                    "iso-acceptance", "athanor-system", args.pop("digest_"), **args
                )
                r, copies = self.promote()
                self.assertEqual(r.returncode, 1)
                self.assertIn(word, r.stderr)
                self.assertEqual(copies, [])

    def test_evidence_that_fails_after_the_plan_is_refused_at_apply(self):
        self.published()
        r, copies = self.promote(
            between=lambda: self.evidence(
                "iso-acceptance", "athanor-system", digest(1), verdict="fail"
            )
        )
        self.assertEqual(r.returncode, 1)
        self.assertIn("verdict is fail", r.stderr)
        self.assertEqual(copies, [])

    def test_evidence_replaced_after_the_plan_is_refused_at_apply(self):
        self.published()
        r, copies = self.promote(
            between=lambda: self.evidence(
                "iso-acceptance",
                "athanor-system",
                digest(1),
                finished="2026-09-15T12:00:00Z",
            )
        )
        self.assertEqual(r.returncode, 1)
        self.assertIn("differs from the plan", r.stderr)
        self.assertEqual(copies, [])

    def test_a_stable_moved_after_the_plan_is_refused_at_apply(self):
        self.published()
        r, copies = self.promote(
            between=lambda: self.set_tag(
                f"{REG}/athanor-system:stable", digest(5), "2026-09-12T10:00:00Z"
            )
        )
        self.assertEqual(r.returncode, 1)
        self.assertIn("differs from the plan", r.stderr)
        self.assertEqual(copies, [])

    def test_a_stable_that_cannot_be_read_is_refused(self):
        # Only "manifest unknown" means there is no stable yet: a registry outage must not
        # skip the UT5 check and stable-previous.
        self.published()
        fx = json.loads((self.dir / "registry.json").read_text())
        fx["errors"] = [f"{REG}/athanor-system:stable"]
        self.registry(fx)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("i/o timeout", r.stderr)
        self.assertEqual(copies, [])

    def test_a_stable_moved_during_the_apply_is_not_overwritten(self):
        # nvidia's :stable moves from outside while athanor-system's copies run.
        self.published()
        fx = json.loads((self.dir / "registry.json").read_text())
        nvidia = f"{REG}/athanor-system-nvidia"
        fx["on_copy"] = {
            f"{REG}/athanor-system:stable": {f"{nvidia}:stable": digest(9)}
        }
        self.registry(fx)
        r, copies = self.promote("--hardware-override", "athanor-system-nvidia")
        self.assertEqual(r.returncode, 1)
        self.assertIn(f"{nvidia}:stable moved to {digest(9)}", r.stderr)
        self.assertEqual([dest for _, dest in copies if dest.startswith(nvidia)], [])

    def test_a_stable_that_cannot_be_read_again_before_the_copies_is_refused(self):
        # The re-read of the apply stops on a registry error as the record does: a swallowed
        # error would read as "no stable" and move an image whose stable was never seen.
        self.published()
        fx = json.loads((self.dir / "registry.json").read_text())
        nvidia = f"{REG}/athanor-system-nvidia"
        # No stable at plan time: a swallowed error would match the empty record and move it.
        del fx["tags"][f"{nvidia}:stable"]
        fx["errors_on_copy"] = {f"{REG}/athanor-system:stable": [f"{nvidia}:stable"]}
        self.registry(fx)
        r, copies = self.promote("--hardware-override", "athanor-system-nvidia")
        self.assertEqual(r.returncode, 1)
        self.assertIn("i/o timeout", r.stderr)
        self.assertEqual([dest for _, dest in copies if dest.startswith(nvidia)], [])

    def test_a_package_set_changed_after_the_plan_is_refused_at_apply(self):
        self.published()
        r, copies = self.promote(
            between=lambda: (
                self.artifacts / "packages" / "athanor-system.txt"
            ).write_text("bash-5.3-1.fc43.x86_64 cc\n")
        )
        self.assertEqual(r.returncode, 1)
        self.assertIn("differs from the plan", r.stderr)
        self.assertEqual(copies, [])

    def test_a_dwell_or_keys_other_than_the_plans_are_refused_at_apply(self):
        keys = self.dir / "keys"
        keys.mkdir()
        (keys / KEY.name).write_bytes(KEY.read_bytes())
        for apply_env in (
            {"PROMOTE_DWELL_HOURS": "0"},
            {"PROMOTE_KEYS_DIR": str(keys)},
        ):
            with self.subTest(apply_env=apply_env):
                self.published()
                r, copies = self.promote(apply_env=apply_env)
                self.assertEqual(r.returncode, 1)
                self.assertIn("differs from the plan", r.stderr)
                self.assertEqual(copies, [])

    def test_a_run_tag_moved_after_the_plan_is_refused_at_apply(self):
        self.published()
        r, copies = self.promote(
            between=lambda: self.set_tag(f"{REG}/athanor-system:412", digest(7))
        )
        self.assertEqual(r.returncode, 1)
        self.assertIn("moved", r.stderr)
        self.assertEqual(copies, [])

    def test_the_dwell_holds_a_fresh_run(self):
        now = datetime.datetime.now(datetime.timezone.utc).strftime(
            "%Y-%m-%dT%H:%M:%SZ"
        )
        self.published(finished=now)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("dwell", r.stderr)
        self.assertEqual(copies, [])

    def test_the_dwell_counts_from_the_newest_evidence(self):
        now = datetime.datetime.now(datetime.timezone.utc).strftime(
            "%Y-%m-%dT%H:%M:%SZ"
        )
        self.published()
        self.evidence("signature", "athanor-system", digest(1), finished=now)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("dwell", r.stderr)
        self.assertEqual(copies, [])

    def test_a_promoted_image_without_its_package_set_is_refused(self):
        self.published()
        (self.artifacts / "packages" / "athanor-system.txt").unlink()
        r, copies = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("package set", r.stderr)
        self.assertEqual(copies, [])

    def test_a_run_tag_moved_after_the_digests_were_recorded_is_refused(self):
        self.published()
        self.set_tag(f"{REG}/athanor-system:412", digest(7))
        r, copies = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("moved", r.stderr)
        self.assertEqual(copies, [])

    def test_a_digests_file_of_another_run_is_refused(self):
        self.published()
        path = self.artifacts / "image-digests.txt"
        path.write_text(path.read_text().replace(" 412 ", " 411 "))
        r, _ = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("411", r.stderr)

    def test_an_older_or_equal_build_is_refused_and_nothing_moves(self):
        for run_created in ("2026-09-01T10:00:00Z", "2026-09-10T10:00:00Z"):
            with self.subTest(run_created=run_created):
                self.published(run_created=run_created)
                r, copies = self.promote()
                self.assertEqual(r.returncode, 1)
                self.assertIn("not newer than the current stable", r.stderr)
                self.assertEqual(copies, [])

    def test_an_image_already_stable_at_the_digest_is_skipped(self):
        # After a partial apply: athanor-system moved, the nvidia copy failed. Planning again
        # moves only nvidia and leaves stable-previous of athanor-system where it was.
        self.published()
        self.set_tag(f"{REG}/athanor-system:stable", digest(1))
        r, copies = self.promote("--hardware-override", "athanor-system-nvidia")
        self.assertEqual(r.returncode, 0, r.stderr)
        nvidia = f"{REG}/athanor-system-nvidia"
        self.assertEqual(
            copies,
            [
                (f"{nvidia}@{digest(5)}", f"{nvidia}:stable-previous"),
                (f"{nvidia}@{digest(2)}", f"{nvidia}:stable-{self.record()['day']}"),
                (f"{nvidia}@{digest(2)}", f"{nvidia}:stable"),
            ],
        )
        reasons = {s["name"]: s["reason"] for s in self.record()["skipped"]}
        self.assertEqual(reasons["athanor-system"], f"already stable at {digest(1)}")

    def test_a_run_already_promoted_has_nothing_to_promote(self):
        self.published()
        r, _ = self.promote()
        self.assertEqual(r.returncode, 0, r.stderr)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("nothing to promote", r.stderr)
        self.assertEqual(copies, [])

    def test_a_stable_without_a_build_time_is_refused(self):
        # An older run against a stable whose build time cannot be read: the UT5 check must
        # refuse, not compare against an empty value.
        self.published(run_created="2026-09-01T10:00:00Z")
        fx = json.loads((self.dir / "registry.json").read_text())
        fx["configs"][f"{REG}/athanor-system@{digest(4)}"] = {}
        self.registry(fx)
        r, copies = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("no org.opencontainers.image.created label", r.stderr)
        self.assertEqual(copies, [])

    def test_the_record_names_the_registry(self):
        self.published()
        self.promote()
        self.assertEqual(self.record()["registry"], REG)

    def test_the_record_names_the_keys_and_the_dwell(self):
        self.published()
        self.promote()
        record = self.record()
        self.assertEqual(record["dwell_hours"], 24)
        self.assertEqual(
            record["keys"][KEY.name], hashlib.sha256(KEY.read_bytes()).hexdigest()
        )

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

    def test_a_run_id_is_a_number(self):
        self.published()
        r, copies = self.promote(run="412; true")
        self.assertEqual(r.returncode, 2)
        self.assertEqual(copies, [])

    def test_stable_is_read_back(self):
        self.published(moves=False)
        r, _ = self.promote()
        self.assertEqual(r.returncode, 1)
        self.assertIn("stable points at", r.stderr)

    def test_the_apply_needs_the_plan_it_signed(self):
        self.published()
        r, copies = self.promote(plan=False)
        self.assertEqual(r.returncode, 1)
        self.assertIn("promotion-plan.sh", r.stderr)
        self.assertEqual(copies, [])

        def tamper():
            record = self.record()
            record["overrides"] = ["athanor-system-nvidia"]
            # Laid out as the plan writes it, so only the content differs.
            (self.artifacts / "promotion.json").write_text(
                json.dumps(record, sort_keys=True, indent=2) + "\n"
            )

        r, copies = self.promote(between=tamper)
        self.assertEqual(r.returncode, 1)
        self.assertIn("differs from the plan", r.stderr)
        self.assertEqual(copies, [])

    def test_the_apply_tags_the_day_of_the_plan(self):
        self.published()

        def next_day():
            record = self.record()
            record["day"] = "20260916"
            (self.artifacts / "promotion.json").write_text(
                json.dumps(record, sort_keys=True, indent=2) + "\n"
            )

        r, copies = self.promote(between=next_day)
        # An approval given after midnight UTC: the apply recomputes the record with the day of
        # the plan, so the record still equals the plan and the tag is the plan's day.
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(
            (
                f"{REG}/athanor-system@{digest(1)}",
                f"{REG}/athanor-system:stable-20260916",
            ),
            copies,
        )

    def test_the_plan_moves_nothing(self):
        self.published()
        env = {**self.env, "REGISTRY": REG, "PROMOTE_ARTIFACTS": str(self.artifacts)}
        p = subprocess.run(
            ["bash", str(PLAN), "--hardware-override", "athanor-system-nvidia", "412"],
            capture_output=True,
            text=True,
            env=env,
        )
        self.assertEqual(p.returncode, 0, p.stderr)
        log = self.dir / "calls.log"
        calls = (
            [json.loads(line) for line in log.read_text().splitlines()]
            if log.exists()
            else []
        )
        self.assertEqual(
            [c for c in calls if c[:2] == ["skopeo", "copy"] and "--policy" not in c],
            [],
        )
        self.assertTrue((self.artifacts / "promotion.json").is_file())

    def test_promote_has_no_plan_mode(self):
        self.published()
        r, copies = self.promote("--plan", plan=False)
        self.assertEqual(r.returncode, 2)
        self.assertEqual(copies, [])

    def test_the_record_hashes_the_evidence_it_promoted_on(self):
        self.published()
        self.promote()
        path = self.artifacts / "evidence" / "iso-acceptance.athanor-system.json"
        self.assertEqual(
            self.record()["evidence"]["evidence/iso-acceptance.athanor-system.json"],
            hashlib.sha256(path.read_bytes()).hexdigest(),
        )


if __name__ == "__main__":
    unittest.main()
