"""call-kernel.yml mirrors the check jobs of kernel-build.yml until PB12 makes one call the other."""

import pathlib
import unittest

import yaml

WORKFLOWS = pathlib.Path(__file__).resolve().parents[3] / ".github" / "workflows"
MIRRORED = ("inputs", "build", "boot", "kmod")


def jobs(name):
    return yaml.safe_load((WORKFLOWS / name).read_text())["jobs"]


def mirrored_jobs(name):
    """The mirrored jobs of NAME; kernel-build.yml's inputs also needs its lint job."""
    found = {job: dict(jobs(name)[job]) for job in MIRRORED}
    if found["inputs"].get("needs") == ["lint"]:
        del found["inputs"]["needs"]
    return found


class MirrorTest(unittest.TestCase):
    def test_the_check_jobs_are_identical(self):
        kernel_build = mirrored_jobs("kernel-build.yml")
        call_kernel = mirrored_jobs("call-kernel.yml")
        for job in MIRRORED:
            with self.subTest(job=job):
                self.assertEqual(call_kernel[job], kernel_build[job])

    def test_the_mirror_publishes_and_signs_nothing(self):
        self.assertEqual(set(jobs("call-kernel.yml")), {*MIRRORED, "verdict"})

    def test_the_signer_identity_comes_from_kernel_artifacts(self):
        for name in ("kernel-build.yml", "call-kernel.yml"):
            with self.subTest(workflow=name):
                text = (WORKFLOWS / name).read_text()
                self.assertNotIn('--certificate-identity-regexp "^${GITHUB_SERVER_URL}', text)


if __name__ == "__main__":
    unittest.main()
