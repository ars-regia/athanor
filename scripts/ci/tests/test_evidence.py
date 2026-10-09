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
        verdict.write_text(json.dumps({"pass": True, "checks": {}}))
        self.write("--verdict-json", str(verdict))
        self.assertEqual(self.check().returncode, 0)
        verdict.unlink()
        self.write("--verdict-json", str(verdict))
        self.assertIn("verdict is fail", self.check().stderr)

    def test_bad_arguments_are_refused_at_write(self):
        for kwargs in ({"gate": "hardware"}, {"digest": "sha256:abc"}, {"run_id": "12a"}, {"run_id": "\u00b2"}, {"image": "../x"}):
            with self.subTest(**kwargs):
                self.assertEqual(self.write(**kwargs).returncode, 2)


if __name__ == "__main__":
    unittest.main()
