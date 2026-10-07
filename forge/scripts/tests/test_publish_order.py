"""A node's hash tag is the registry's proof that it is built, so it must be pushed after the
image is signed and attested, in every level of call-dag-compile.yml
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import pathlib
import re
import unittest

WORKFLOW = pathlib.Path(__file__).resolve().parents[3] / ".github/workflows/call-dag-compile.yml"


class PublishOrderTest(unittest.TestCase):
    def test_the_hash_tag_is_pushed_after_signing_and_attesting(self):
        text = WORKFLOW.read_text()
        jobs = re.split(r"\n  (?=dag-build-[a-z0-9-]+:\n)", text)[1:]
        self.assertEqual(4, len(jobs))
        for job in jobs:
            steps = job.split("\n      - name: ")[1:]
            names = [step.splitlines()[0] for step in steps]
            pushes = [i for i, step in enumerate(steps) if ":hash-" in step and "push" in step]
            if not pushes:
                continue  # the flatpak job publishes nothing
            self.assertEqual(1, len(pushes), job.splitlines()[0])
            signing = next(i for i, name in enumerate(names) if name.startswith("Sign & Attest"))
            self.assertGreater(pushes[0], signing, job.splitlines()[0])

    def test_the_signed_digest_is_the_one_that_gets_the_hash_tag(self):
        text = WORKFLOW.read_text()
        jobs = re.split(r"\n  (?=dag-build-[a-z0-9-]+:\n)", text)[1:]
        for job in jobs:
            steps = job.split("\n      - name: ")[1:]
            by_name = {step.splitlines()[0]: step for step in steps}
            publish = next((v for k, v in by_name.items() if k.startswith("Publish Micro-Container")), None)
            if publish is None:
                continue
            title = job.splitlines()[0]
            self.assertIn("--digestfile", publish, title)
            for name in ("Generate SBOM", "Sign & Attest"):
                step = next(v for k, v in by_name.items() if k.startswith(name))
                self.assertNotIn(":latest", step, f"{title}: {name} must use the digest")
                self.assertIn("image.digest", step, f"{title}: {name}")
            last = next(v for k, v in by_name.items() if k.startswith("Publish the hash tag"))
            self.assertIn("tag_signed_image.sh", last, title)
            self.assertNotIn("buildah push", last, title)


if __name__ == "__main__":
    unittest.main()
