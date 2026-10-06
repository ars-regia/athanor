"""Unit tests of forge/scripts/select_check_specs.py: what Spec Build Check builds for a change
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "select_check_specs.py"
spec = importlib.util.spec_from_file_location("select_check_specs", SCRIPT)
sel = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sel)

DAG = ["specs/athanor-bar", "specs/athanor-telemetry", "specs/bat", "specs/cosmic-comp"]
NO_SPEC = {"specs/athanor-gone"}


def select(*changed):
    return sel.select(list(changed), DAG, lambda d: d not in NO_SPEC)


class SelectTest(unittest.TestCase):
    def test_a_spec_change_builds_that_spec_only_in_the_published_builder(self):
        self.assertEqual(
            select(
                "forge/specs/bat/bat.spec", "forge/specs/bat/SOURCES/sources.sha256"
            ),
            (False, ["specs/bat"], {}),
        )

    def test_rpmmacros_rebuilds_every_dag_spec_and_the_changed_ones(self):
        builder, specs, _ = select(
            "forge/config/rpmmacros", "forge/specs/polkit/polkit.spec"
        )
        self.assertFalse(builder)
        self.assertEqual(
            specs,
            ["specs/athanor-bar", "specs/bat", "specs/cosmic-comp", "specs/polkit"],
        )

    def test_the_shared_build_scripts_rebuild_every_dag_spec(self):
        for script in ("build_spec.sh", "run_spec_build.sh", "fetch_sources.sh"):
            with self.subTest(script=script):
                builder, specs, _ = select(f"forge/scripts/{script}")
                self.assertFalse(builder)
                self.assertEqual(
                    specs, ["specs/athanor-bar", "specs/bat", "specs/cosmic-comp"]
                )

    def test_the_builder_inputs_build_the_builder_and_every_dag_spec(self):
        for path in ("flake.nix", "flake.lock", "forge/builder/verify_compilers.sh"):
            with self.subTest(path=path):
                builder, specs, _ = select(path)
                self.assertTrue(builder)
                self.assertEqual(
                    specs, ["specs/athanor-bar", "specs/bat", "specs/cosmic-comp"]
                )

    def test_the_kernel_the_nix_built_telemetry_and_specless_directories_are_skipped(
        self,
    ):
        builder, specs, skipped = select(
            "forge/specs/azoth/config",
            "forge/specs/athanor-telemetry/telemetry.spec",
            "forge/specs/athanor-gone/gone.spec",
        )
        self.assertEqual((builder, specs), (False, []))
        self.assertEqual(
            sorted(skipped),
            ["specs/athanor-gone", "specs/athanor-telemetry", "specs/azoth"],
        )

    def test_other_changes_build_nothing(self):
        for path in (
            "forge/scripts/build_changed_specs.sh",
            "forge/config/packages.json",
            "forge/specs/README.md",
            "flake.nix.orig",
            "system/Containerfile",
        ):
            with self.subTest(path=path):
                self.assertEqual(select(path), (False, [], {}))


if __name__ == "__main__":
    unittest.main()
