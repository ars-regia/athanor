"""Unit tests of the signing-secret rule of scripts/verify.py workflows (D43)
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

SIGN_JOB = """\
  sign:
    runs-on: ubuntu-24.04
    environment: signing
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262
      - uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
        with:
          name: unsigned
      - name: Build the locked tool image
        run: podman build -t localhost/azoth-sign -f forge/specs/azoth/sign/Containerfile forge/specs/azoth
      - name: Sign
        env:
          SECUREBOOT_SIGNING_KEY: ${{ secrets.SECUREBOOT_SIGNING_KEY }}
        run: |
          set -euo pipefail
          bash system/sign-kernel.sh --out signed
      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: signed
          path: signed/
"""


class SigningSecrets(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        (self.root / ".github/workflows").mkdir(parents=True)

    def tearDown(self):
        self.tmp.cleanup()

    def problems(self, jobs, head=""):
        text = "name: test\non: push\n" + textwrap.dedent(head) + "jobs:\n" + jobs
        (self.root / ".github/workflows/test.yml").write_text(text)
        return verify.signing_secret_problems(self.root)

    def test_the_repository_workflows_have_no_problem(self):
        self.assertEqual(verify.signing_secret_problems(), [])

    def test_a_sign_only_job_passes(self):
        self.assertEqual(self.problems(SIGN_JOB), [])

    def test_the_key_outside_the_signing_environment(self):
        problems = self.problems(SIGN_JOB.replace("    environment: signing\n", ""))
        self.assertEqual(len(problems), 1)
        self.assertIn("outside the signing environment", problems[0])

    def test_the_environment_in_its_long_form(self):
        job = SIGN_JOB.replace(
            "    environment: signing\n", "    environment:\n      name: signing\n"
        )
        self.assertEqual(self.problems(job), [])

    def test_a_third_party_action_beside_the_key(self):
        job = SIGN_JOB.replace(
            "      - name: Sign\n",
            "      - uses: cachix/install-nix-action@ba0dd844c9180cbf77aa72a116d6fbc515d0e87b\n      - name: Sign\n",
        )
        problems = self.problems(job)
        self.assertEqual(len(problems), 1)
        self.assertIn("cachix/install-nix-action", problems[0])

    def test_a_build_beside_the_key(self):
        for command in (
            "bash system/build-image.sh --gpu none",
            "podman build -t x -f system/Containerfile .",
            "cargo build --release",
            "nix build .#iso",
        ):
            with self.subTest(command=command):
                problems = self.problems(
                    SIGN_JOB.replace("bash system/sign-kernel.sh --out signed", command)
                )
                self.assertEqual(len(problems), 1)
                self.assertIn("builds", problems[0])

    def test_a_job_without_signing_secrets_is_not_restricted(self):
        job = "  build:\n    runs-on: ubuntu-24.04\n    steps:\n      - uses: cachix/install-nix-action@v27\n      - run: nix build\n"
        self.assertEqual(self.problems(job), [])

    def test_a_reusable_workflow_call_passes_the_secrets_on(self):
        job = (
            "  image:\n    uses: ./.github/workflows/call-system-image.yml\n    secrets:\n"
            "      SECUREBOOT_SIGNING_KEY: ${{ secrets.SECUREBOOT_SIGNING_KEY }}\n"
        )
        self.assertEqual(self.problems(job), [])

    def test_the_key_in_the_workflow_env(self):
        problems = self.problems(
            SIGN_JOB, head="env:\n  KEY: ${{ secrets.MODULE_SIGNING_KEY }}\n"
        )
        self.assertEqual(len(problems), 1)
        self.assertIn("workflow-level env", problems[0])

    def test_all_the_secrets_at_once(self):
        problems = self.problems(
            SIGN_JOB + "      - run: echo '${{ toJSON(secrets) }}' > /dev/null\n"
        )
        self.assertTrue(any("as a whole" in p for p in problems))

    def test_secret_names_are_matched_without_regard_to_case(self):
        problems = self.problems(
            SIGN_JOB.replace(
                "secrets.SECUREBOOT_SIGNING_KEY", "secrets.secureboot_Signing_Key"
            ).replace("    environment: signing\n", "")
        )
        self.assertEqual(len(problems), 1)
        self.assertIn(
            "SECUREBOOT_SIGNING_KEY outside the signing environment", problems[0]
        )

    def called(self, called_jobs, secrets):
        (self.root / ".github/workflows/called.yml").write_text(
            "name: called\non: workflow_call\njobs:\n" + called_jobs
        )
        return self.problems(
            "  caller:\n    uses: ./.github/workflows/called.yml\n    secrets:"
            + secrets
        )

    def test_a_renamed_secret_is_followed_into_a_local_workflow(self):
        build = (
            "  build:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: make\n"
            "        env:\n          KEY: ${{ secrets.SB }}\n"
        )
        renamed = "\n      SB: ${{ secrets.SECUREBOOT_SIGNING_KEY }}\n"
        problems = self.called(build, renamed)
        self.assertTrue(problems, "the renamed key reaches a build job")
        self.assertTrue(
            all("called.yml" in p and "receives SB" in p for p in problems), problems
        )
        sign = SIGN_JOB.replace("secrets.SECUREBOOT_SIGNING_KEY", "secrets.sb")
        self.assertEqual(self.called(sign, renamed), [])
        # Another secret under the same new name carries no key.
        self.assertEqual(
            self.called(build, "\n      SB: ${{ secrets.GITHUB_TOKEN }}\n"), []
        )

    def test_secrets_to_an_external_workflow(self):
        external = (
            "  caller:\n    uses: someone/else/.github/workflows/x.yml"
            "@0123456789abcdef0123456789abcdef01234567\n"
        )
        problems = self.problems(external + "    secrets: inherit\n")
        self.assertEqual(len(problems), 1)
        self.assertIn("secrets: inherit", problems[0])
        problems = self.problems(
            external + "    secrets:\n      K: ${{ secrets.MODULE_SIGNING_KEY }}\n"
        )
        self.assertEqual(len(problems), 1)
        self.assertIn("passes MODULE_SIGNING_KEY to the external workflow", problems[0])
        self.assertEqual(
            self.problems(
                external + "    secrets:\n      T: ${{ secrets.GITHUB_TOKEN }}\n"
            ),
            [],
        )

    def test_a_comment_is_not_a_reference(self):
        job = "  build:\n    runs-on: ubuntu-24.04\n    steps:\n      # secrets.SECUREBOOT_SIGNING_KEY is not here\n      - run: make\n"
        self.assertEqual(self.problems(job), [])


if __name__ == "__main__":
    unittest.main()
