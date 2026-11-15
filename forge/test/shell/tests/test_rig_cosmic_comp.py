"""Tests of `rig.sh cosmic-comp-version`: the rig runs exactly the cosmic-comp that
forge/specs/cosmic-comp names, built here as the forge builds it when the published rig holds
another. rig.sh and the forge's scripts run in a copy of the tree, against a podman and a skopeo
that record their arguments and answer as the test says
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import json
import os
import shutil
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
COPIED = (
    "forge/test/shell/rig.sh",
    "forge/test/shell/spec_nevr.awk",
    "forge/test/shell/rig-image.digest",
    "forge/scripts/run_spec_build.sh",
    "forge/scripts/retry.sh",
    "forge/scripts/check_idempotency.sh",
    "forge/scripts/registry_probe.sh",
    "system/Containerfile",
)
PUBLISHED = "1.11.0-2.fc43.athanor1"
SPEC = "1.11.0-2.fc43.athanor2"
DERIVED_ID = "d" * 64
BASE_ID = "b" * 64

# The podman of the tests: `run ... rpm -q` answers the Version-Release of the image it names,
# `run ... build_spec.sh build` leaves the RPM the test names in the checkout's forge/RPMS, and
# `build` writes an image id to --iidfile, `image inspect` answers the id the test gives the
# image and `image exists` fails for the images the test lists as absent. Every call is logged
# as a JSON list.
PODMAN = textwrap.dedent(
    """\
    #!/usr/bin/env python3
    import json, pathlib, sys
    tmp = pathlib.Path(__file__).resolve().parents[1]
    conf = json.loads((tmp / "conf.json").read_text())
    args = sys.argv[1:]
    with open(tmp / "podman.log", "a") as log:
        log.write(json.dumps(args) + "\\n")
    if args[0] == "volume" and args[1] == "create":
        print("home")
    elif args[:2] == ["image", "exists"]:
        sys.exit(1 if args[2] in conf["absent"] else 0)
    elif args[:2] == ["image", "inspect"]:
        print("sha256:" + conf["ids"][args[-1]])
    elif args[0] == "build":
        iid = args[args.index("--iidfile") + 1]
        pathlib.Path(iid).write_text("sha256:" + conf["derived_id"])
        context = pathlib.Path(args[-1])
        (tmp / "Containerfile").write_text((context / "Containerfile").read_text())
    elif args[0] == "run" and "rpm" in args and "-q" in args:
        image = args[args.index("rpm") - 1]
        if image not in conf["nevr"]:
            print("package cosmic-comp is not installed")
            sys.exit(1)
        print(conf["nevr"][image], end="")
    elif args[0] == "run" and "scripts/build_spec.sh" in args and args[-2] == "build":
        workspace = next(a for a in args if a.endswith(":/workspace")).rsplit(":", 1)[0]
        rpms = pathlib.Path(workspace, "forge", "RPMS")
        rpms.mkdir(parents=True, exist_ok=True)
        if conf["rpm"]:
            (rpms / conf["rpm"]).write_text("rpm")
    """
)
# The skopeo of registry_probe.sh: the builder is published unless the test says otherwise.
SKOPEO = textwrap.dedent(
    """\
    #!/bin/sh
    [ -e "$(dirname "$0")/../builder-absent" ] || exit 0
    echo "manifest unknown" >&2
    exit 1
    """
)


def spec(release):
    return textwrap.dedent(
        f"""\
        %global fedora_release 2.fc43
        Name:           cosmic-comp
        Version:        1.11.0
        Release:        %{{fedora_release}}.{release}
        """
    )


class Rig:
    """A copy of the tree rig.sh and the forge's build path read, in a git repository."""

    def __init__(
        self,
        tmp,
        release="athanor2",
        rpm=f"cosmic-comp-{SPEC}.x86_64.rpm",
        derived_nevr=SPEC,
        builder=True,
    ):
        self.tmp = tmp
        self.tree = tmp / "tree"
        for name in COPIED:
            (self.tree / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / name, self.tree / name)
        (self.tree / "forge/builder").mkdir(parents=True)
        (self.tree / "forge/builder/Containerfile").write_text("FROM scratch\n")
        self.set_release(release)
        subprocess.run(["git", "init", "-q", str(self.tree)], check=True)
        self.base = (
            "ghcr.io/ars-regia/athanor-shell-rig@"
            + (self.tree / "forge/test/shell/rig-image.digest").read_text().strip()
        )
        bin_dir = tmp / "bin"
        bin_dir.mkdir()
        for name, body in (("podman", PODMAN), ("skopeo", SKOPEO)):
            (bin_dir / name).write_text(body)
            (bin_dir / name).chmod(0o755)
        if not builder:
            (tmp / "builder-absent").write_text("")
        (tmp / "conf.json").write_text(
            json.dumps(
                {
                    "derived_id": DERIVED_ID,
                    "ids": {self.base: BASE_ID},
                    "absent": [],
                    "rpm": rpm,
                    "nevr": {
                        self.base: PUBLISHED,
                        **({DERIVED_ID: derived_nevr} if derived_nevr else {}),
                    },
                }
            )
        )
        self.out = tmp / "out"

    def configure(self, **values):
        path = self.tmp / "conf.json"
        conf = json.loads(path.read_text())
        conf.update(values)
        path.write_text(json.dumps(conf))

    def set_release(self, release):
        path = self.tree / "forge/specs/cosmic-comp/cosmic-comp.spec"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(spec(release))

    def run(self, *args):
        env = {k: v for k, v in os.environ.items() if not k.startswith("ATHANOR_")}
        env.update(
            PATH=f"{self.tmp / 'bin'}:{env['PATH']}",
            ATHANOR_RIG_OUT=str(self.out),
            HOME=str(self.tmp),
            RETRY_ATTEMPTS="1",
        )
        return subprocess.run(
            ["bash", str(self.tree / "forge/test/shell/rig.sh"), *args],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )

    def calls(self):
        log = self.tmp / "podman.log"
        if not log.exists():
            return []
        return [json.loads(line) for line in log.read_text().splitlines()]

    def built(self):
        """Whether the spec was built: a container ran build_spec.sh."""
        return any("scripts/build_spec.sh" in call for call in self.calls())


class CosmicCompVersion(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def test_the_spec_the_rig_holds_builds_nothing(self):
        rig = Rig(self.tmp, release="athanor1")
        result = rig.run("cosmic-comp-version")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"the rig runs cosmic-comp {PUBLISHED}", result.stdout)
        self.assertFalse(rig.built())
        self.assertNotIn("build", [call[0] for call in rig.calls()])
        self.assertFalse((rig.out / "cosmic-comp-spec/rig").exists())

    def test_another_spec_is_built_installed_and_compared_again(self):
        rig = Rig(self.tmp)
        result = rig.run("cosmic-comp-version")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"the rig runs cosmic-comp {SPEC}, built here", result.stdout)
        # The follow-up that stops the rebuilds names TIER2_DIGEST.
        self.assertIn("TIER2_DIGEST", result.stderr)
        calls = rig.calls()
        # The forge's path: fetch with network, then build without, in the checkout's builder by
        # its content hash, never latest.
        stages = [call for call in calls if "scripts/build_spec.sh" in call]
        self.assertEqual([call[-2] for call in stages], ["fetch", "build"])
        self.assertIn("--network=none", stages[1])
        builder = stages[0][-5]
        self.assertRegex(builder, r"^ghcr\.io/ars-regia/athanor-builder:[0-9a-f]{64}$")
        self.assertIn(["pull", "-q", builder], calls)
        # Derived from the pinned rig with rpm -U, without network; nothing is pushed.
        builds = [call for call in calls if call[0] == "build"]
        self.assertEqual(len(builds), 1)
        self.assertIn("--network=none", builds[0])
        containerfile = (self.tmp / "Containerfile").read_text()
        self.assertTrue(containerfile.startswith(f"FROM {rig.base}\n"), containerfile)
        self.assertIn("rpm -U --oldpackage /tmp/cosmic-comp.rpm", containerfile)
        self.assertNotIn("COPY", containerfile)
        self.assertFalse([call for call in calls if call[0] == "push"])
        # The strict comparison reads the derived rig.
        queries = [call for call in calls if call[0] == "run" and "rpm" in call]
        self.assertEqual(
            [call[call.index("rpm") - 1] for call in queries], [rig.base, DERIVED_ID]
        )
        self.assertEqual(
            (rig.out / "cosmic-comp-spec/rig").read_text().split(),
            [BASE_ID, SPEC, DERIVED_ID],
        )

    def test_later_recipes_run_the_derived_rig_while_the_spec_names_its_build(self):
        rig = Rig(self.tmp)
        self.assertEqual(rig.run("cosmic-comp-version").returncode, 0)
        self.assertEqual(rig.run("probe-sandbox").returncode, 0)
        self.assertEqual(
            rig.calls()[-1][rig.calls()[-1].index("bwrap") - 1], DERIVED_ID
        )
        # A spec that names yet another build falls back to the pinned rig.
        rig.set_release("athanor3")
        self.assertEqual(rig.run("probe-sandbox").returncode, 0)
        self.assertEqual(rig.calls()[-1][rig.calls()[-1].index("bwrap") - 1], rig.base)

    def test_a_rig_that_holds_the_spec_again_forgets_the_derived_rig(self):
        rig = Rig(self.tmp)
        self.assertEqual(rig.run("cosmic-comp-version").returncode, 0)
        rig.set_release("athanor1")
        result = rig.run("cosmic-comp-version")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((rig.out / "cosmic-comp-spec/rig").exists())

    def test_a_derived_rig_of_another_base_is_not_run(self):
        rig = Rig(self.tmp)
        (rig.out / "cosmic-comp-spec").mkdir(parents=True)
        (rig.out / "cosmic-comp-spec/rig").write_text(f"{'e' * 64} {SPEC} {DERIVED_ID}\n")
        self.assertEqual(rig.run("probe-sandbox").returncode, 0)
        self.assertEqual(rig.calls()[-1][rig.calls()[-1].index("bwrap") - 1], rig.base)

    def test_a_pinned_rig_rebuilt_under_its_name_drops_the_derived_rig(self):
        rig = Rig(self.tmp)
        self.assertEqual(rig.run("cosmic-comp-version").returncode, 0)
        rig.configure(ids={rig.base: "f" * 64})
        self.assertEqual(rig.run("probe-sandbox").returncode, 0)
        self.assertEqual(rig.calls()[-1][rig.calls()[-1].index("bwrap") - 1], rig.base)

    def test_a_derived_rig_no_longer_present_is_not_run(self):
        rig = Rig(self.tmp)
        self.assertEqual(rig.run("cosmic-comp-version").returncode, 0)
        rig.configure(absent=[DERIVED_ID])
        self.assertEqual(rig.run("probe-sandbox").returncode, 0)
        self.assertEqual(rig.calls()[-1][rig.calls()[-1].index("bwrap") - 1], rig.base)

    def test_a_leftover_rpm_is_not_installed_when_the_build_leaves_none(self):
        rig = Rig(self.tmp, rpm="")
        leftover = rig.tree / f"forge/RPMS/cosmic-comp-{SPEC}.x86_64.rpm"
        leftover.parent.mkdir(parents=True)
        leftover.write_text("old")
        result = rig.run("cosmic-comp-version")
        self.assertEqual(result.returncode, 1)
        self.assertIn(f"produced no cosmic-comp-{SPEC}.x86_64.rpm", result.stderr)
        self.assertNotIn("build", [call[0] for call in rig.calls()])

    def test_a_rig_without_cosmic_comp_fails_with_a_message(self):
        rig = Rig(self.tmp)
        conf = json.loads((self.tmp / "conf.json").read_text())
        conf["nevr"] = {}
        (self.tmp / "conf.json").write_text(json.dumps(conf))
        result = rig.run("cosmic-comp-version")
        self.assertEqual(result.returncode, 1)
        self.assertIn("has no cosmic-comp package", result.stderr)
        self.assertFalse(rig.built())

    def test_a_build_without_the_spec_nevr_fails(self):
        rig = Rig(self.tmp, rpm=f"cosmic-comp-{PUBLISHED}.x86_64.rpm")
        result = rig.run("cosmic-comp-version")
        self.assertEqual(result.returncode, 1)
        self.assertIn(f"produced no cosmic-comp-{SPEC}.x86_64.rpm", result.stderr)
        self.assertNotIn("build", [call[0] for call in rig.calls()])
        self.assertFalse((rig.out / "cosmic-comp-spec/rig").exists())

    def test_a_derived_rig_that_runs_another_build_fails(self):
        rig = Rig(self.tmp, derived_nevr=PUBLISHED)
        result = rig.run("cosmic-comp-version")
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            f"runs cosmic-comp {PUBLISHED}, not the {SPEC} the spec names",
            result.stderr,
        )

    def test_a_derived_rig_without_cosmic_comp_fails_with_a_message(self):
        rig = Rig(self.tmp, derived_nevr=None)
        result = rig.run("cosmic-comp-version")
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "the rig built from forge/specs/cosmic-comp has no cosmic-comp package",
            result.stderr,
        )

    def test_an_unpublished_builder_fails_before_any_build(self):
        rig = Rig(self.tmp, builder=False)
        result = rig.run("cosmic-comp-version")
        self.assertEqual(result.returncode, 1)
        self.assertIn("is not published", result.stderr)
        self.assertFalse(rig.built())

    def test_an_unreadable_spec_fails_before_the_rig_runs(self):
        rig = Rig(self.tmp)
        (rig.tree / "forge/specs/cosmic-comp/cosmic-comp.spec").write_text(
            "Name: cosmic-comp\n"
        )
        result = rig.run("cosmic-comp-version")
        self.assertEqual(result.returncode, 1)
        self.assertIn("cannot read the Version-Release", result.stderr)
        self.assertEqual(rig.calls(), [])


if __name__ == "__main__":
    unittest.main()
