"""Unit tests of the COSMIC boundary check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

COSMIC_CONFIG = 'const THEME: &str = "com.system76.CosmicTheme.Mode";\n'


def problems(files):
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        for name, text in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text)
        return verify.boundary_problems(root)


class BoundaryTest(unittest.TestCase):
    def test_the_compositor_client_may_depend_on_cosmic_and_read_its_configuration(
        self,
    ):
        self.assertEqual(
            problems(
                {
                    "system/athanor-compositor-client/Cargo.toml": '[dependencies]\ncosmic-config = "1"\n',
                    "system/athanor-compositor-client/src/theme.rs": COSMIC_CONFIG,
                }
            ),
            [],
        )

    def test_any_other_crate_that_depends_on_cosmic_is_reported(self):
        found = problems(
            {
                "forge/specs/athanor-bar/Cargo.toml": (
                    '[dependencies]\nlibcosmic = { git = "x" }\n'
                    "[target.'cfg(unix)'.dev-dependencies]\n"
                    'cosmic-text = "0.14"\n'
                ),
                "system/athanor-style/Cargo.toml": '[workspace.dependencies]\ncosmic-theme = "1"\n',
            }
        )
        self.assertEqual(len(found), 3, found)
        self.assertTrue(
            all("only athanor-compositor-client" in problem for problem in found)
        )

    def test_a_renamed_dependency_is_found_by_its_crate_name(self):
        found = problems(
            {
                "system/athanor-dock/Cargo.toml": '[dependencies]\ntheme = { package = "cosmic-theme", version = "1" }\n'
            }
        )
        self.assertEqual(len(found), 1, found)
        self.assertIn("cosmic-theme", found[0])

    def test_a_similar_name_is_not_cosmic(self):
        self.assertEqual(
            problems(
                {
                    "system/athanor-dock/Cargo.toml": '[dependencies]\ncosmic = "1"\nmy-cosmic-thing = "1"\n'
                }
            ),
            [],
        )

    def test_rust_code_naming_a_cosmic_configuration_is_reported_with_its_line(self):
        found = problems(
            {
                "system/athanor-style/src/theme.rs": "// com.system76 in a comment\n"
                + COSMIC_CONFIG
            }
        )
        self.assertEqual(len(found), 1, found)
        self.assertTrue(
            found[0].startswith("system/athanor-style/src/theme.rs:2 "), found
        )

    def test_a_cosmic_desktop_id_is_not_a_configuration(self):
        code = 'const OLD: &str = "com.system76.CosmicTerm.desktop";\n'
        self.assertEqual(problems({"system/athanor-layout/src/favorites.rs": code}), [])
        both = 'const X: [&str; 2] = ["com.system76.CosmicFiles.desktop", "com.system76.CosmicPanel"];\n'
        self.assertEqual(len(problems({"system/athanor-layout/src/favorites.rs": both})), 1)

    def test_only_the_theme_tool_and_the_compositor_client_may_name_cosmic(self):
        allowed = [
            "forge/tools/calmo-cosmic-theme/src/main.rs",
            "system/athanor-compositor-client/src/theme.rs",
        ]
        for path in allowed:
            with self.subTest(path):
                self.assertEqual(problems({path: COSMIC_CONFIG}), [])
        refused = [
            "forge/specs/athanor-layout-translator/athanor-layout-translator-1.0.0/src/main.rs",
            "system/athanor-layout/src/cosmic.rs",
            "system/athanor-layout/src/apply.rs",
            "forge/specs/athanor-bar/athanor-bar-1.0.0/src/main.rs",
        ]
        for path in refused:
            with self.subTest(path):
                self.assertEqual(len(problems({path: COSMIC_CONFIG})), 1)

    def test_a_manifest_that_does_not_parse_is_a_problem(self):
        found = problems({"system/athanor-dock/Cargo.toml": "[dependencies\n"})
        self.assertEqual(len(found), 1, found)
        self.assertIn("not valid TOML", found[0])


if __name__ == "__main__":
    unittest.main()
