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
    match = re.compile(rf"\n    {re.escape(name)}\)\n(.*?)\n        ;;\n", re.DOTALL).search(text, start)
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
        # The bar's case serves each popover's, bar-calendar among them, the surface's name in $2.
        for label, surface in (("bar | bar-*", "$2"), ("launcher", "launcher"), ("greeter", "greeter")):
            body = case(label, "atspi")
            name = f"atspi-{surface}${{3:+-$3}}.json"
            self.assertIn(f'ATSPI_TRANSCRIPT="/out/{name}"', body, surface)
            self.assertIn(f"golden/atspi/{name}", body, surface)
            self.assertIn("atspi_compare.py", body, surface)


class PopoverScenes(unittest.TestCase):
    """The popovers' AT-SPI and Orca runs open the popover of the bar surface they name, in the
    scene capture_bar draws it in, and every popover shell-surfaces.yml runs has its walk."""

    def test_atspi_and_orca_open_the_popover_of_bar_scene(self):
        for command in ("atspi", "orca"):
            label = "bar | bar-*" if command == "atspi" else "bar-*"
            body = case(label, command)
            self.assertIn('bar_scene "$2"', body, command)
            self.assertIn('ATHANOR_BAR_OPEN="$open"', body, command)
            self.assertIn("${scene_args[*]}", body, command)
        self.assertIn('orca_walk.py /out/$transcript ${walk[*]}', case("bar-*", "orca"))

    def test_capture_bar_takes_its_scene_from_bar_scene(self):
        text = RIG.read_text(encoding="utf-8")
        body = text[text.index("\ncapture_bar() {"):]
        body = body[: body.index("\n}\n")]
        self.assertIn('bar_scene "$surface"', body)

    def test_every_popover_of_the_workflow_has_a_walk_and_its_references(self):
        workflow = (SHELL.parent.parent.parent / ".github/workflows/shell-surfaces.yml").read_text(encoding="utf-8")
        matrix = re.search(r"\n        popover:\s*\[([^\]]*)\]", workflow)
        self.assertIsNotNone(matrix)
        run = {name.strip() for name in matrix.group(1).split(",")} - {""}
        self.assertTrue(run)
        walks = case("bar-*", "orca")
        for surface in run:
            self.assertRegex(walks, rf"\n        {surface}\) walk=\(", surface)
            for suffix in ("", "-it"):
                self.assertTrue((SHELL / f"golden/atspi/atspi-{surface}{suffix}.json").is_file(), surface)
                self.assertTrue((SHELL / f"golden/orca/orca-{surface}{suffix}.txt").is_file(), surface)

    def test_an_unknown_bar_surface_is_refused(self):
        text = RIG.read_text(encoding="utf-8")
        scene = text[text.index("\nbar_scene() {"):]
        script = scene[: scene.index("\n}\n") + 3] + 'bar_scene "$1"\n'
        result = subprocess.run(["bash", "-c", script, "-", "bar-nothing"], capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 2)
        self.assertIn("bar-nothing", result.stderr)


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


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL",
             "TIER_TRUSTED_REFS", "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
