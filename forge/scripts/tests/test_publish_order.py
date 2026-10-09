"""A node's hash tag is the registry's proof that it is built, so it must be pushed after the
image is signed and attested, in the package build of call-dag-compile.yml
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[3]
WORKFLOW = ROOT / ".github/workflows/call-dag-compile.yml"
SCRIPT = ROOT / "forge/scripts/dag_package.sh"


def script_step(name):
    """The body of step_<name> in dag_package.sh, which the workflow step of that name runs."""
    match = re.search(rf"^step_{name}\(\) {{\n(.*?)^}}\n", SCRIPT.read_text(), re.S | re.M)
    if match is None:
        raise AssertionError(f"dag_package.sh has no step_{name}")
    return match.group(1)


class PublishOrderTest(unittest.TestCase):
    def test_the_hash_tag_is_pushed_after_signing_and_attesting(self):
        text = WORKFLOW.read_text()
        jobs = re.split(r"\n  (?=dag-build[a-z0-9-]*:\n)", text)[1:]
        self.assertEqual(1, len(jobs))  # one package matrix, PL48
        for job in jobs:
            steps = job.split("\n      - name: ")[1:]
            names = [step.splitlines()[0] for step in steps]
            # By name: since tag_signed_image.sh the step holds neither ":hash-" nor "push",
            # and a text match found no step at all, so the test passed on every job unchecked.
            pushes = [i for i, name in enumerate(names) if name.startswith("Publish the hash tag")]
            self.assertEqual(1, len(pushes), job.splitlines()[0])
            signing = next(i for i, name in enumerate(names) if name.startswith("Sign & Attest"))
            self.assertGreater(pushes[0], signing, job.splitlines()[0])

    def test_the_signed_digest_is_the_one_that_gets_the_hash_tag(self):
        text = WORKFLOW.read_text()
        jobs = re.split(r"\n  (?=dag-build[a-z0-9-]*:\n)", text)[1:]
        for job in jobs:
            steps = job.split("\n      - name: ")[1:]
            by_name = {step.splitlines()[0]: step for step in steps}
            publish = next((v for k, v in by_name.items() if k.startswith("Publish Micro-Container")), None)
            if publish is None:
                continue
            title = job.splitlines()[0]
            # The steps run their bodies from dag_package.sh (#138), signing stays inline.
            sbom = next(v for k, v in by_name.items() if k.startswith("Generate SBOM"))
            last = next(v for k, v in by_name.items() if k.startswith("Publish the hash tag"))
            for step, name in ((publish, "publish"), (sbom, "sbom"), (last, "tag")):
                self.assertIn(f"scripts/dag_package.sh {name}\n", step, title)
            self.assertIn("--digestfile", script_step("publish"), title)
            sign = next(v for k, v in by_name.items() if k.startswith("Sign & Attest"))
            for name, body in (("Generate SBOM", script_step("sbom")), ("Sign & Attest", sign)):
                self.assertNotIn(":latest", body, f"{title}: {name} must use the digest")
                self.assertIn("image.digest", body, f"{title}: {name}")
            self.assertIn("tag_signed_image.sh", script_step("tag"), title)
            self.assertNotIn("buildah push", script_step("tag"), title)


if __name__ == "__main__":
    unittest.main()
