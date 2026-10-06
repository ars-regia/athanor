"""The D43 part of the `workflows` check of scripts/verify.py (doc_kernel_profile.md, section 12
item 1): a signing secret reaches only sign-only jobs of the protected `signing` environment."""

import importlib.util
import json
import pathlib
import tempfile
import textwrap
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

CHECKOUT = "actions/checkout@11d5960a326750d5838078e36cf38b85af677262"
DOWNLOAD = "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c"
UPLOAD = "actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02"

ENVIRONMENTS = {
    "signing": {
        "reviewers": [{"name": "maintainer", "type": "User"}],
        "secrets": ["COSIGN_PRIVATE_KEY", "SECUREBOOT_SIGNING_KEY"],
    },
    "github-pages": {"reviewers": [], "secrets": []},
}

SIGN_JOB = f"""\
  sign:
    runs-on: ubuntu-24.04
    environment: signing
    steps:
      - uses: {CHECKOUT}
      - uses: {DOWNLOAD}
        with:
          name: unsigned
      - name: Sign
        env:
          SECUREBOOT_SIGNING_KEY: ${{{{ secrets.SECUREBOOT_SIGNING_KEY }}}}
        # podman build would be a build: this comment is not one.
        run: bash forge/specs/azoth/sign-kernel.sh --out signed
      - uses: {UPLOAD}
        with:
          name: signed
"""

BUILD_JOB = """\
  build:
    runs-on: ubuntu-24.04
    steps:
      - uses: cachix/install-nix-action@ba0dd844c9180cbf77aa72a116d6fbc515d0e87b
      - run: podman build -t localhost/thing .
"""


def workflow(*jobs):
    return "name: Sign\non: push\njobs:\n" + "".join(jobs)


class SigningTest(unittest.TestCase):
    def problems(self, workflows, environments=ENVIRONMENTS):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / ".github/settings").mkdir(parents=True)
            (root / ".github/workflows").mkdir(parents=True)
            (root / ".github/settings/environments.json").write_text(
                json.dumps(environments)
            )
            for name, text in workflows.items():
                (root / ".github/workflows" / name).write_text(text)
            return verify.signing_problems(root)

    def test_a_sign_only_job_beside_a_build_job_passes(self):
        self.assertEqual(self.problems({"k.yml": workflow(BUILD_JOB, SIGN_JOB)}), [])

    def test_rule_1_a_signing_secret_outside_the_signing_environment_fails(self):
        job = SIGN_JOB.replace("    environment: signing\n", "")
        self.assertEqual(
            self.problems({"k.yml": workflow(job)}),
            [
                "k.yml: job sign reads SECUREBOOT_SIGNING_KEY without environment: signing (D43)"
            ],
        )

    def test_rule_1_the_environment_written_as_a_mapping_counts(self):
        job = SIGN_JOB.replace(
            "    environment: signing\n", "    environment:\n      name: signing\n"
        )
        self.assertEqual(self.problems({"k.yml": workflow(job)}), [])

    def test_rule_2_a_third_party_action_in_a_signing_job_fails(self):
        job = SIGN_JOB.replace(
            f"      - uses: {UPLOAD}\n",
            "      - uses: sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6\n"
            f"      - uses: {UPLOAD}\n",
        )
        self.assertEqual(
            self.problems({"k.yml": workflow(job)}),
            [
                "k.yml: signing job sign uses sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6"
                ": only checkout, download-artifact and upload-artifact pinned by SHA (D43)"
            ],
        )

    def test_rule_2_a_first_party_action_by_tag_fails(self):
        job = SIGN_JOB.replace(CHECKOUT, "actions/checkout@v4")
        self.assertEqual(
            self.problems({"k.yml": workflow(job)}),
            [
                "k.yml: signing job sign uses actions/checkout@v4"
                ": only checkout, download-artifact and upload-artifact pinned by SHA (D43)"
            ],
        )

    def test_rule_2_a_reusable_workflow_in_the_signing_environment_fails(self):
        job = textwrap.dedent("""\
              sign:
                environment: signing
                uses: ./.github/workflows/other.yml
            """)
        job = textwrap.indent(job, "  ")
        self.assertEqual(
            self.problems({"k.yml": workflow(job)}),
            [
                "k.yml: signing job sign uses ./.github/workflows/other.yml"
                ": only checkout, download-artifact and upload-artifact pinned by SHA (D43)"
            ],
        )

    def test_rule_3_a_build_in_a_signing_job_fails(self):
        for command in (
            "podman build -t localhost/signer .",
            "buildah bud -t signer .",
            "bash system/build-image.sh --gpu none --registry r --tag t",
            "nix shell nixpkgs#cosign -c true",
            "sudo apt-get install -y sbsigntool",
            "dnf5 install -y sbsigntools",
            "cargo build --release",
            "podman run --rm localhost/azoth-nvidia true",
        ):
            with self.subTest(command=command):
                job = SIGN_JOB.replace(
                    "run: bash forge/specs/azoth/sign-kernel.sh --out signed",
                    f"run: |\n          set -euo pipefail\n          {command}",
                )
                problems = self.problems({"k.yml": workflow(job)})
                self.assertEqual(len(problems), 1, problems)
                self.assertRegex(
                    problems[0], r"^k\.yml:\d+: signing job sign builds or installs: "
                )

    def test_rule_4_a_signing_secret_in_a_job_of_another_environment_fails(self):
        job = SIGN_JOB.replace("environment: signing", "environment: github-pages")
        self.assertEqual(
            self.problems({"k.yml": workflow(job)}),
            [
                "k.yml: job sign reads SECUREBOOT_SIGNING_KEY without environment: signing (D43)"
            ],
        )

    def test_rule_5_a_signing_environment_without_reviewers_fails(self):
        environments = json.loads(json.dumps(ENVIRONMENTS))
        environments["signing"]["reviewers"] = []
        self.assertEqual(
            self.problems({"k.yml": workflow(SIGN_JOB)}, environments),
            [
                ".github/settings/environments.json: the signing environment has no required reviewer (D43)"
            ],
        )

    def test_a_missing_signing_environment_fails(self):
        self.assertEqual(
            self.problems(
                {"k.yml": workflow(SIGN_JOB)},
                {"github-pages": ENVIRONMENTS["github-pages"]},
            ),
            [".github/settings/environments.json: no signing environment (D43)"],
        )


if __name__ == "__main__":
    unittest.main()
