"""scripts/ci/evidence.py: the evidence files of UD4 and the check promote.sh runs."""

import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "evidence.py"
D = "sha256:" + "a" * 64
OTHER = "sha256:" + "b" * 64
URL = "https://github.com/ars-regia/athanor/actions/runs/9"


def run(*args):
    return subprocess.run([sys.executable, "-B", str(SCRIPT), *args], capture_output=True, text=True)


class Evidence(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, *verdict, gate="iso-acceptance", image="athanor-system", digest=D, run_id="412"):
        return run("write", "--gate", gate, "--image", image, "--digest", digest, "--run-id", run_id,
                   *(verdict or ("--verdict", "pass")), "--run-url", URL, "--out", str(self.dir))

    def check(self, gate="iso-acceptance", image="athanor-system", digest=D, run_id="412"):
        return run("check", "--dir", str(self.dir), "--gate", gate, "--image", image, "--digest", digest, "--run-id", run_id)

    def test_the_file_carries_the_ud4_fields_and_the_image(self):
        self.assertEqual(self.write().returncode, 0)
        data = json.loads((self.dir / "iso-acceptance.athanor-system.json").read_text())
        self.assertEqual(set(data), {"gate", "image", "digest", "run_id", "verdict", "workflow_run_url", "finished_at"})
        self.assertEqual((data["run_id"], data["verdict"], data["workflow_run_url"]), (412, "pass", URL))
        self.assertRegex(data["finished_at"], r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$")

    def test_a_passing_file_checks_and_prints_its_finish_time(self):
        self.write()
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertRegex(r.stdout.strip(), r"^\d+$")

    def test_a_missing_file_is_refused(self):
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("no iso-acceptance evidence for athanor-system", r.stderr)

    def test_a_fail_verdict_is_refused(self):
        self.write("--verdict", "fail")
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("verdict is fail", r.stderr)

    def test_evidence_for_another_digest_is_refused(self):
        self.write(digest=OTHER)
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("digest", r.stderr)

    def test_evidence_for_another_run_is_refused(self):
        self.write(run_id="411")
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("run id", r.stderr)

    def test_a_file_renamed_to_another_image_is_refused(self):
        self.write(image="athanor-system-nvidia")
        (self.dir / "iso-acceptance.athanor-system-nvidia.json").rename(self.dir / "iso-acceptance.athanor-system.json")
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("image", r.stderr)

    def test_a_malformed_file_is_refused(self):
        (self.dir / "iso-acceptance.athanor-system.json").write_text("{")
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("malformed", r.stderr)

    def test_the_verdict_json_decides_and_its_absence_is_a_fail(self):
        verdict = self.dir / "verdict.json"
        verdict.write_text(json.dumps({"pass": True, "checks": {"secure-boot": True}}))
        self.write("--verdict-json", str(verdict))
        self.assertEqual(self.check().returncode, 0)
        verdict.unlink()
        self.write("--verdict-json", str(verdict))
        self.assertIn("verdict is fail", self.check().stderr)

    def test_every_verdict_json_must_pass(self):
        good, bad = self.dir / "good.json", self.dir / "bad.json"
        good.write_text(json.dumps({"pass": True, "checks": {"secure-boot": True}}))
        bad.write_text(json.dumps({"pass": False}))
        self.write("--verdict-json", str(good), str(good))
        self.assertEqual(self.check().returncode, 0)
        self.write("--verdict-json", str(good), str(bad))
        self.assertIn("verdict is fail", self.check().stderr)

    UPGRADE_CHECKS = ("secure-boot", "upgrade-stages", "upgraded-secure-boot", "rolled-back-secure-boot")

    def upgrade_verdict(self, checks):
        path = self.dir / "upgrade.json"
        path.write_text(json.dumps({"pass": True, "checks": checks}))
        self.write("--verdict-json", str(path), gate="upgrade-acceptance")
        return json.loads((self.dir / "upgrade-acceptance.athanor-system.json").read_text())["verdict"]

    def test_upgrade_gate_passes_with_every_required_check(self):
        self.assertEqual(self.upgrade_verdict(dict.fromkeys(self.UPGRADE_CHECKS, True)), "pass")
        self.assertEqual(self.check(gate="upgrade-acceptance").returncode, 0)

    def test_upgrade_gate_fails_without_rolled_back_secure_boot(self):
        # UD18: Secure Boot must hold on the deployment GoBack() returns to, not only on the
        # candidate; a verdict.json that passes without the check does not pass the gate.
        checks = {name: True for name in self.UPGRADE_CHECKS if name != "rolled-back-secure-boot"}
        self.assertEqual(self.upgrade_verdict(checks), "fail")
        for name in self.UPGRADE_CHECKS:
            with self.subTest(false=name):
                checks = dict.fromkeys(self.UPGRADE_CHECKS, True)
                checks[name] = False
                self.assertEqual(self.upgrade_verdict(checks), "fail")

    def test_iso_gate_unchanged(self):
        path = self.dir / "sb.json"
        path.write_text(json.dumps({"pass": True, "checks": {"secure-boot": True}}))
        self.write("--verdict-json", str(path))
        data = json.loads((self.dir / "iso-acceptance.athanor-system.json").read_text())
        self.assertEqual(data["verdict"], "pass")

    def test_the_iso_acceptance_needs_the_secure_boot_case(self):
        plain, sb = self.dir / "plain.json", self.dir / "sb.json"
        plain.write_text(json.dumps({"pass": True, "checks": {"greeter": True}}))
        sb.write_text(json.dumps({"pass": True, "checks": {"greeter": True, "secure-boot": True}}))
        for files, verdict in (((plain,), "fail"), ((plain, plain), "fail"), ((plain, sb), "pass")):
            with self.subTest(files=[f.name for f in files]):
                self.write("--verdict-json", *map(str, files))
                data = json.loads((self.dir / "iso-acceptance.athanor-system.json").read_text())
                self.assertEqual(data["verdict"], verdict)
        sb.write_text(json.dumps({"pass": True, "checks": {"secure-boot": "yes"}}))
        self.write("--verdict-json", str(plain), str(sb))
        self.assertIn("verdict is fail", self.check().stderr)

    def test_bad_arguments_are_refused_at_write(self):
        for kwargs in ({"gate": "hardware"}, {"digest": "sha256:abc"}, {"run_id": "12a"}, {"run_id": "\u00b2"}, {"image": "../x"}):
            with self.subTest(**kwargs):
                self.assertEqual(self.write(**kwargs).returncode, 2)


if __name__ == "__main__":
    unittest.main()
