"""Tests of rig.sh's opt-in compiler cache, ATHANOR_RIG_SCCACHE, through a podman that only
records its arguments (python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

SHELL = Path(__file__).resolve().parent.parent
RIG = SHELL / "rig.sh"
CACHE_VARS = ("ATHANOR_RIG_SCCACHE", "ATHANOR_RIG_SCCACHE_SIZE")


def cargo_recipes():
    """Every case of rig.sh whose podman run mounts the cargo registry: the build stage's."""
    text = RIG.read_text(encoding="utf-8")
    names = []
    for match in re.finditer(r"^([a-z-]+)\)\n(.*?)\n    ;;$", text, re.DOTALL | re.MULTILINE):
        if "athanor-cargo-registry" in match.group(2):
            names.append(match.group(1))
    return names


def runs(args, **env_extra):
    """The podman command lines of `rig.sh <args>`, and the result, with env_extra set."""
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        (tmp / "path").mkdir()
        podman = tmp / "path" / "podman"
        podman.write_text(f'#!/bin/sh\necho "$*" >> {tmp}/runs\n')
        podman.chmod(0o755)
        env = {k: v for k, v in os.environ.items() if k not in CACHE_VARS}
        env.update(
            PATH=f"{tmp / 'path'}:{env['PATH']}", ATHANOR_RIG_OUT=str(tmp / "out")
        )
        env.update({k: v.replace("{tmp}", str(tmp)) for k, v in env_extra.items()})
        result = subprocess.run(
            ["bash", str(RIG), *args],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )
        log = tmp / "runs"
        lines = log.read_text(encoding="utf-8").splitlines() if log.exists() else []
        cache_made = (tmp / "cache").is_dir()
    return lines, result, cache_made


class CompilerCache(unittest.TestCase):
    def test_the_recipes_are_found(self):
        recipes = cargo_recipes()
        self.assertIn("build-compositor", recipes)
        self.assertIn("build-toolkit", recipes)
        self.assertIn("cargo", recipes)

    def test_unset_no_cargo_run_has_a_wrapper(self):
        for recipe in cargo_recipes():
            lines, _, _ = runs([recipe, "build"] if recipe == "cargo" else [recipe])
            cargo = [line for line in lines if "athanor-cargo-registry" in line]
            self.assertTrue(cargo, recipe)
            for line in cargo:
                self.assertNotIn(":/sccache", line, recipe)  # the checkout's path may hold the word
                self.assertNotIn("RUSTC_WRAPPER", line, recipe)
                self.assertNotIn("CARGO_INCREMENTAL", line, recipe)

    def test_set_every_cargo_run_goes_through_the_cache(self):
        for recipe in cargo_recipes():
            lines, _, made = runs(
                [recipe, "build"] if recipe == "cargo" else [recipe],
                ATHANOR_RIG_SCCACHE="{tmp}/cache",
            )
            self.assertTrue(made, recipe)
            cargo = [line for line in lines if "athanor-cargo-registry" in line]
            self.assertTrue(cargo, recipe)
            for line in cargo:
                self.assertRegex(line, r"-v \S+/cache:/sccache ", recipe)
                for option in (
                    "RUSTC_WRAPPER=sccache",
                    "SCCACHE_DIR=/sccache",
                    "SCCACHE_CACHE_SIZE=20G",
                    "CARGO_INCREMENTAL=0",
                ):
                    self.assertIn(f"-e {option} ", line, recipe)

    def test_the_size_can_be_set(self):
        lines, _, _ = runs(
            ["build-compositor"],
            ATHANOR_RIG_SCCACHE="{tmp}/cache",
            ATHANOR_RIG_SCCACHE_SIZE="5G",
        )
        self.assertIn("-e SCCACHE_CACHE_SIZE=5G ", lines[0])

    def test_a_relative_path_is_refused(self):
        # podman would take it for the name of a volume.
        lines, result, _ = runs(
            ["build-compositor"], ATHANOR_RIG_SCCACHE="athanor-sccache"
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("absolute path", result.stderr)
        self.assertEqual(lines, [])


if __name__ == "__main__":
    unittest.main()
