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

    def test_the_repository_passes(self):
        self.assertEqual(verify.check_trusted_refs().problems, [])


if __name__ == "__main__":
    unittest.main()
