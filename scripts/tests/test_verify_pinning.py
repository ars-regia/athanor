"""Unit tests of the pinning check of scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class PinningTest(unittest.TestCase):
    def problems(self, files):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for name, text in files.items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_text(text)
            return verify.pinning_problems(root)

    def test_an_unpinned_reference_in_a_workflow_is_reported(self):
        found = self.problems(
            {".github/workflows/x.yml": "run: nix shell nixpkgs#cosign -c bash system/tier-digests.sh verify\n"}
        )
        self.assertEqual(len(found), 1)
        self.assertIn(".github/workflows/x.yml:1: nixpkgs#cosign", found[0])

    def test_a_reference_through_the_locked_flake_passes(self):
        found = self.problems(
            {
                ".github/workflows/x.yml": "run: nix shell --inputs-from . nixpkgs#cosign -c true\n",
                "forge/scripts/x.sh": 'nix run --inputs-from "$here/../.." nixpkgs#syft -- packages x\n',
            }
        )
        self.assertEqual(found, [])

    def test_a_branch_ref_is_reported_even_with_inputs_from(self):
        found = self.problems(
            {"system/x.sh": "nix shell --inputs-from . nixpkgs/nixos-unstable#cosign -c true\n"}
        )
        self.assertEqual(len(found), 1)

    def test_inputs_from_after_the_reference_does_not_pin_it(self):
        found = self.problems({"scripts/x.sh": "nix run nixpkgs#jq --inputs-from . -- .\n"})
        self.assertEqual(len(found), 1)

    def test_scripts_actions_justfiles_and_system_are_covered(self):
        found = self.problems(
            {
                "scripts/devvm/x.sh": "nix run nixpkgs#cosign -- version\n",
                ".github/actions/a/action.yml": "run: nix shell nixpkgs#skopeo -c true\n",
                "Justfile": "x:\n    nix run nixpkgs#jq\n",
                "system/x.sh": "nix shell nixpkgs#sbsigntool -c true\n",
            }
        )
        self.assertEqual(len(found), 4)

    def test_comments_tests_and_documents_are_not_scanned(self):
        found = self.problems(
            {
                "forge/scripts/x.sh": "# run under `nix shell nixpkgs#cosign -c`\n",
                "scripts/tests/test_x.py": "'nix shell nixpkgs#cosign -c true'\n",
                "scripts/devvm/README.md": "nix profile add nixpkgs#virt-viewer\n",
            }
        )
        self.assertEqual(found, [])


if __name__ == "__main__":
    unittest.main()
