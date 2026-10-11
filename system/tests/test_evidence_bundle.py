"""system/evidence-bundle.sh: the signed evidence bundle of a promotion (ADR-0103 D23)."""

import json
import pathlib
import shutil
import subprocess
import unittest

from test_kernel_artifacts import Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "evidence-bundle.sh"
IDENTITY = "https://github.com/ars-regia/athanor/.github/workflows/promote-stable.yml@refs/heads/iso-v0"


class EvidenceBundle(Tool):
    def setUp(self):
        super().setUp()
        self.registry({"signer": IDENTITY})
        a = self.artifacts
        (a / "evidence").mkdir(parents=True)
        (a / "packages").mkdir()
        (a / "promotion.json").write_text(json.dumps({"run_id": 412}))
        (a / "image-digests.txt").write_text(
            "r/athanor-system 412 sha256:" + "a" * 64 + "\n"
        )
        (a / "evidence" / "iso-acceptance.athanor-system.json").write_text("{}")
        (a / "packages" / "athanor-system.txt").write_text(
            "bash-5.2-1.fc43.x86_64 bb\n"
        )
        self.bundle = self.dir / "bundle"

    def run_script(self, *args, **env):
        return subprocess.run(
            ["bash", str(SCRIPT), *args],
            capture_output=True,
            text=True,
            env={**self.env, **env},
        )

    def created(self):
        r = self.run_script("create", str(self.artifacts), str(self.bundle))
        self.assertEqual(r.returncode, 0, r.stderr)
        copy = self.dir / "copy"
        shutil.copytree(self.bundle, copy)
        return copy

    def test_create_sums_every_file_and_signs_the_sums(self):
        self.created()
        listed = sorted(
            line.split(maxsplit=1)[1].strip()
            for line in (self.bundle / "bundle.sha256").read_text().splitlines()
        )
        self.assertEqual(
            listed,
            sorted(
                [
                    "evidence/iso-acceptance.athanor-system.json",
                    "image-digests.txt",
                    "packages/athanor-system.txt",
                    "promotion.json",
                ]
            ),
        )
        self.assertTrue((self.bundle / "bundle.sha256.sigstore.json").is_file())

    def test_create_needs_the_plan(self):
        (self.artifacts / "promotion.json").unlink()
        r = self.run_script("create", str(self.artifacts), str(self.bundle))
        self.assertEqual(r.returncode, 1)
        self.assertIn("promotion-plan.sh", r.stderr)
        self.assertFalse(self.bundle.exists())

    def test_create_refuses_a_symlink(self):
        (self.artifacts / "evidence" / "link.json").symlink_to(
            "iso-acceptance.athanor-system.json"
        )
        r = self.run_script("create", str(self.artifacts), str(self.bundle))
        self.assertEqual(r.returncode, 1)
        self.assertIn("evidence/link.json", r.stderr)
        self.assertFalse((self.bundle / "bundle.sha256").exists())

    def test_create_refuses_a_backslash_in_a_directory_name(self):
        odd = self.artifacts / "evidence" / "a\\b"
        odd.mkdir()
        (odd / "f.json").write_text("{}")
        r = self.run_script("create", str(self.artifacts), str(self.bundle))
        self.assertEqual(r.returncode, 1)
        self.assertFalse((self.bundle / "bundle.sha256").exists())

    def test_an_intact_copy_verifies(self):
        copy = self.created()
        r = self.run_script("verify-dir", str(copy), str(self.artifacts))
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_a_changed_file_is_refused(self):
        copy = self.created()
        (copy / "evidence" / "iso-acceptance.athanor-system.json").write_text(
            '{"verdict": "pass"}'
        )
        r = self.run_script("verify-dir", str(copy), str(self.artifacts))
        self.assertNotEqual(r.returncode, 0)

    def test_an_unlisted_file_is_refused(self):
        copy = self.created()
        (copy / "evidence" / "iso-acceptance.extra.json").write_text("{}")
        r = self.run_script("verify-dir", str(copy), str(self.artifacts))
        self.assertEqual(r.returncode, 1)
        self.assertIn("do not list", r.stderr)

    def test_a_missing_file_is_refused(self):
        copy = self.created()
        (copy / "packages" / "athanor-system.txt").unlink()
        r = self.run_script("verify-dir", str(copy), str(self.artifacts))
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("athanor-system.txt", r.stderr)

    def test_another_signer_is_refused(self):
        self.run_script(
            "create",
            str(self.artifacts),
            str(self.bundle),
            FAKE_SIGNER="https://github.com/someone/athanor/.github/workflows/x.yml@refs/heads/main",
        )
        r = self.run_script("verify-dir", str(self.bundle), str(self.artifacts))
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("identities", r.stderr)

    def test_a_copy_of_another_plan_is_refused(self):
        copy = self.created()
        (self.artifacts / "promotion.json").write_text(json.dumps({"run_id": 413}))
        r = self.run_script("verify-dir", str(copy), str(self.artifacts))
        self.assertEqual(r.returncode, 1)
        self.assertIn("promotion.json", r.stderr)

    def test_the_summary_names_what_the_approval_covers(self):
        (self.artifacts / "promotion.json").write_text(json.dumps({
            "run_id": 412, "images": [{"name": "athanor-system", "digest": "sha256:" + "a" * 64, "previous_stable": None}],
            "skipped": [{"name": "athanor-system-nvidia-legacy", "reason": "not promoted before 0.9"}],
            "overrides": ["athanor-system-nvidia"]}))
        (self.artifacts / "evidence" / "iso-acceptance.athanor-system.json").write_text(json.dumps({
            "gate": "iso-acceptance", "image": "athanor-system", "verdict": "pass", "finished_at": "2026-10-09T10:00:00Z"}))
        r = self.run_script("summary", str(self.artifacts))
        self.assertEqual(r.returncode, 0, r.stderr)
        for text in ("build run 412", "sha256:" + "a" * 64, "Hardware override: athanor-system-nvidia",
                     "| iso-acceptance | athanor-system | pass |", "athanor-system-nvidia-legacy"):
            self.assertIn(text, r.stdout)

    def test_the_summary_refuses_a_missing_field(self):
        # A plan without overrides would read "Hardware override: none" in the approval.
        plan = {"run_id": 412, "images": [], "skipped": [], "overrides": []}
        evidence = {"gate": "iso-acceptance", "image": "athanor-system", "verdict": "pass", "finished_at": "t"}
        for name, document, field in (("promotion.json", plan, "overrides"),
                                      ("evidence/iso-acceptance.athanor-system.json", evidence, "verdict")):
            with self.subTest(field=field):
                (self.artifacts / "promotion.json").write_text(json.dumps(plan))
                (self.artifacts / "evidence" / "iso-acceptance.athanor-system.json").write_text(json.dumps(evidence))
                (self.artifacts / name).write_text(json.dumps({k: v for k, v in document.items() if k != field}))
                r = self.run_script("summary", str(self.artifacts))
                self.assertNotEqual(r.returncode, 0)
                self.assertIn(field, r.stderr)


if __name__ == "__main__":
    unittest.main()
