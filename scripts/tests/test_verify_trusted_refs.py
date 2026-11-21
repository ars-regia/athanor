"""The `trusted-refs` check of scripts/verify.py (issue #1346): the default of every trusted-ref
variable of the signing scripts names iso-v0 and nothing else."""

import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


def scripts(default="iso-v0"):
    return {path: f"x=${{{var}:-{default}}}\n" for path, var in verify.TRUSTED_REF_DEFAULTS}


class TrustedRefs(unittest.TestCase):
    def test_iso_v0_alone_passes(self):
        self.assertEqual(verify.trusted_ref_problems(scripts()), [])

    def test_a_retired_or_rebase_branch_fails(self):
        for default in ("iso-v0 main", "main", "iso-v0 rebase/*", ""):
            with self.subTest(default=default):
                self.assertEqual(len(verify.trusted_ref_problems(scripts(default))), 3)

    def test_a_missing_default_or_file_fails(self):
        s = scripts()
        s["system/tag-images.sh"] = "no default here\n"
        del s["system/tier-digests.sh"]
        self.assertEqual(len(verify.trusted_ref_problems(s)), 2)

    def test_main_in_a_trigger_condition_or_case_fails(self):
        files = {path: "on:\n  push:\n    branches: [iso-v0]\n" for path in verify.MAIN_BRANCH_FILES}
        self.assertEqual(verify.main_branch_problems(files), [])
        for line in (
            "    branches: [iso-v0, main]",
            "    branches: [main]",
            "      - main",
            "      - 'main'",
            "    if: github.ref_name == 'main'",
            "    if: github.ref == 'refs/heads/iso-v0' || github.ref == 'refs/heads/main'",
            "    refs/heads/iso-v0 | refs/heads/main) TAG=latest ;;",
        ):
            with self.subTest(line=line):
                bad = dict(files, **{verify.MAIN_BRANCH_FILES[0]: line + "\n"})
                self.assertEqual(len(verify.main_branch_problems(bad)), 1)

    def test_a_comment_or_a_longer_name_is_not_main(self):
        text = "# refs/heads/main\n    branches: [main-evil]\n    if: x == 'refs/heads/main-evil'\n"
        files = {path: text for path in verify.MAIN_BRANCH_FILES}
        self.assertEqual(verify.main_branch_problems(files), [])

    def test_the_repository_passes(self):
        self.assertEqual(verify.check_trusted_refs().problems, [])


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL", "TIER_TRUSTED_REFS",
             "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN", "GITHUB_HEAD_REF", "GITHUB_REPOSITORY_OWNER")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    for name in TRUST_ENV:
        _os.environ.pop(name, None)


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
