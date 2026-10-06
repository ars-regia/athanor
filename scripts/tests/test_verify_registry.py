"""Unit tests of the registry check of scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class RegistryTest(unittest.TestCase):
    def problems(self, files):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for name, text in files.items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_text(text)
            return verify.registry_problems(root)

    def test_a_literal_owner_in_a_workflow_is_reported(self):
        found = self.problems(
            {
                ".github/workflows/x.yml": "image: ghcr.io/someone/athanor-builder:latest\n"
            }
        )
        self.assertEqual(len(found), 1)
        self.assertIn("ghcr.io/someone", found[0])

    def test_the_variables_are_not_an_owner(self):
        found = self.problems(
            {
                ".github/workflows/x.yml": "image: ${{ vars.REGISTRY_HOST || 'ghcr.io' }}/${{ github.repository_owner }}/b\n",
                "forge/scripts/x.sh": "image=${REGISTRY_HOST:-ghcr.io}/${GITHUB_REPOSITORY_OWNER:-someone}/b\n",
                "Justfile": "pull:\n    podman pull ghcr.io/${OWNER}/b\n",
            }
        )
        self.assertEqual(found, [])

    def test_scripts_justfiles_and_forge_scripts_are_covered(self):
        found = self.problems(
            {
                "scripts/devvm/x.sh": "OWNER=ghcr.io/someone\n",
                "forge/Justfile": "pull:\n    podman pull ghcr.io/someone/b\n",
                "forge/scripts/x.sh": "skopeo inspect docker://ghcr.io/someone/b:latest\n",
            }
        )
        self.assertEqual(len(found), 3, found)

    def test_the_containerfile_and_test_fixtures_are_outside(self):
        found = self.problems(
            {
                "system/Containerfile": "RUN --mount=type=bind,from=ghcr.io/someone/tier0:latest\n",
                "scripts/tests/test_x.py": 'FIXTURE = "ghcr.io/someone/athanor-system"\n',
            }
        )
        self.assertEqual(found, [])

    def test_unit_test_placeholders_are_not_owners(self):
        self.assertEqual(
            verify.literal_owners('"ghcr.io/owner/a:latest", "ghcr.io/o/a:stable"'), []
        )


if __name__ == "__main__":
    unittest.main()
