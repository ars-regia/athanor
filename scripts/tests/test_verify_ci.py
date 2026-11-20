"""The `ci` check of scripts/verify.py: doc_ci.md covers every workflow, secret and variable."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

WORKFLOW = """name: Build
on: push
jobs:
  build:
    runs-on: ubuntu-24.04
    steps:
      - run: echo "${{ vars.REGISTRY_HOST }}"
        env:
          TOKEN: ${{ secrets.BUILD_TOKEN }}
"""
DOC = """# CI
### CI1 Build
- **File:** `build.yml`. Uses `REGISTRY_HOST` and `BUILD_TOKEN`.
- The KVM action lives in `.github/actions/kvm/action.yml`; `call-*.yml` are reusable.
"""


class CiTest(unittest.TestCase):
    def problems(self, files):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for name, text in files.items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_text(text)
            return verify.ci_problems(root)

    def files(self, doc=DOC, **workflows):
        out = {verify.CI_DOC: doc, ".github/workflows/build.yml": WORKFLOW}
        out.update(
            {
                f".github/workflows/{k.replace('_', '-')}.yml": v
                for k, v in workflows.items()
            }
        )
        return out

    def test_described_workflows_pass(self):
        self.assertEqual(self.problems(self.files()), [])

    def test_missing_doc_fails(self):
        self.assertEqual(
            self.problems({".github/workflows/build.yml": WORKFLOW}),
            [f"{verify.CI_DOC}: missing"],
        )

    def test_undescribed_workflow_fails(self):
        problems = self.problems(
            self.files(nightly="name: Nightly\non: push\njobs: {}\n")
        )
        self.assertEqual(
            problems,
            [f".github/workflows/nightly.yml: not described in {verify.CI_DOC}"],
        )

    def test_doc_naming_a_missing_workflow_fails(self):
        problems = self.problems(
            self.files(doc=DOC + "- `.github/workflows/gone.yml` was removed.\n")
        )
        self.assertEqual(
            problems,
            [f"{verify.CI_DOC}: names gone.yml, which is not in .github/workflows"],
        )

    def test_unnamed_secret_fails(self):
        problems = self.problems(
            self.files(doc=DOC.replace("`BUILD_TOKEN`", "a token"))
        )
        self.assertEqual(
            problems,
            [
                f".github/workflows/build.yml:9: BUILD_TOKEN is not named in {verify.CI_DOC}"
            ],
        )

    def test_unnamed_variable_fails(self):
        problems = self.problems(
            self.files(doc=DOC.replace("`REGISTRY_HOST`", "a host"))
        )
        self.assertEqual(
            problems,
            [
                f".github/workflows/build.yml:7: REGISTRY_HOST is not named in {verify.CI_DOC}"
            ],
        )

    def test_name_must_match_whole_word(self):
        problems = self.problems(
            self.files(doc=DOC.replace("`BUILD_TOKEN`", "`BUILD_TOKEN_OLD`"))
        )
        self.assertEqual(len(problems), 1)


if __name__ == "__main__":
    unittest.main()
