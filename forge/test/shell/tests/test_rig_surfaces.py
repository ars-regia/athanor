"""Tests of rig.sh's atspi and orca cases that need no container
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

SHELL = Path(__file__).resolve().parent.parent
RIG = SHELL / "rig.sh"


def rig(*args, compositor=None):
    env = {k: v for k, v in os.environ.items() if k != "RIG_COMPOSITOR"}
    if compositor:
        env["RIG_COMPOSITOR"] = compositor
    return subprocess.run(["bash", str(RIG), *args], capture_output=True, text=True, env=env, check=False)


def case(name, after):
    """The text of rig.sh's `after` case up to its next sibling, from the `name)` label."""
    text = RIG.read_text(encoding="utf-8")
    start = text.index(f"\n{after})\n")
    match = re.compile(rf"\n    {name}\)\n(.*?)\n        ;;\n", re.DOTALL).search(text, start)
    return match.group(1)


class GreeterOnlyUnderCosmicComp(unittest.TestCase):
    def test_the_greeter_is_refused_under_the_own_compositor(self):
        for command in ("atspi", "orca"):
            for language in ([], ["it"]):
                result = rig(command, "greeter", *language, compositor="athanor")
                self.assertEqual(result.returncode, 2, (command, result.stderr))
                self.assertIn("SN12", result.stderr)
                self.assertIn(command, result.stderr)


class AtspiReferences(unittest.TestCase):
    def test_bar_launcher_and_greeter_write_and_compare_their_transcripts(self):
        for surface in ("bar", "launcher", "greeter"):
            body = case(surface, "atspi")
            name = f"atspi-{surface}${{3:+-$3}}.json"
            self.assertIn(f'ATSPI_TRANSCRIPT="/out/{name}"', body, surface)
            self.assertIn(f"golden/atspi/{name}", body, surface)
            self.assertIn("atspi_compare.py", body, surface)


class OwnCompositorScenesRunInSystemdMode(unittest.TestCase):
    """#971: a recipe that always runs under athanor-sulphur must set RIG_COMPOSITOR where
    in_rig reads it, or the container runs without --systemd=always and in-unit.sh cannot make
    grim's cgroup (mkdir /sys/fs/cgroup/rig-grim.service: Read-only file system)."""

    def scenes(self, *args):
        """The podman runs of scene.sh by `rig.sh <args>` without RIG_COMPOSITOR on the host,
        each with the RIG_COMPOSITOR podman sees, through a podman that only records."""
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "path").mkdir()
            (tmp / "out" / "bin").mkdir(parents=True)
            for name in ("athanor-sulphur", "athanor-sal-chooser", "athanor-sal-gallery"):
                (tmp / "out" / "bin" / name).write_text("")
                (tmp / "out" / "bin" / name).chmod(0o755)
            podman = tmp / "path" / "podman"
            podman.write_text(f'#!/bin/sh\necho "${{RIG_COMPOSITOR:-}}|$*" >> {tmp}/runs\n')
            podman.chmod(0o755)
            env = {k: v for k, v in os.environ.items() if k != "RIG_COMPOSITOR"}
            env.update(PATH=f"{tmp / 'path'}:{env['PATH']}", ATHANOR_RIG_OUT=str(tmp / "out"))
            subprocess.run(["bash", str(RIG), *args], capture_output=True, text=True, env=env, check=False)
            runs = (tmp / "runs").read_text(encoding="utf-8").splitlines()
        return [run for run in runs if "scene.sh" in run]

    def test_each_scene_of_an_own_compositor_recipe_runs_in_systemd_mode(self):
        for args in (
            ("surface", "toolkit-gallery"),
            ("surface", "toolkit-chooser"),
            ("atspi", "toolkit-chooser"),
            ("compositor-globals",),
        ):
            scenes = self.scenes(*args)
            self.assertTrue(scenes, args)
            for run in scenes:
                seen, _, command = run.partition("|")
                self.assertEqual(seen, "athanor", (args, command))
                self.assertIn("--systemd=always", command, args)
                self.assertIn("-e RIG_COMPOSITOR", command, args)

    def test_the_gtk_gallery_stays_under_cosmic_comp(self):
        scenes = self.scenes("surface", "gtk-gallery")
        self.assertTrue(scenes)
        for run in scenes:
            self.assertNotIn("--systemd=always", run)


if __name__ == "__main__":
    unittest.main()
