"""Unit tests of system/shared-layers.sh, the acceptance of UD40 (doc_update_delivery.md): every
variant starts with every layer of the system image it was built FROM, checked in local storage
before the push. A podman stub answers `podman image inspect --format '{{json .RootFS.Layers}}'
REF` with the rootfs.diff_ids of fixture files (python3 -B -m unittest discover -s system/tests -v).

The fixtures under fixtures/shared-layers are the `skopeo inspect --config` output of the three
images run 37384733899 published, trimmed to the platform, labels and rootfs. That run built the
system stage once per variant, so the variants share only the 97 layers of the base image; its
system stage ends at layer 116."""

import json
import pathlib
import subprocess
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "shared-layers.sh"
FIXTURES = pathlib.Path(__file__).resolve().parent / "fixtures" / "shared-layers"
RUN = "37384733899"
REGISTRY = "ghcr.io/ars-regia"
SYSTEM_ID = "a" * 64
NAMES = ("athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy")
SYSTEM_STAGE_LAYERS = 116
BASE_LAYERS = 97


def fixture(name):
    return json.loads((FIXTURES / f"{name}-{RUN}.json").read_text())


class SharedLayers(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        self.configs = self.dir / "configs"
        self.configs.mkdir()
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        # The stub serves layer lists only: any other podman use is a failure of the script. A
        # config without diff_ids prints null, as podman does for an image without RootFS.Layers.
        (bin_dir / "podman").write_text(
            textwrap.dedent(f"""\
            #!/bin/bash
            [[ $# -eq 5 && $1 == image && $2 == inspect && $3 == --format && $4 == '{{{{json .RootFS.Layers}}}}' ]] || {{ echo "unexpected podman $*" >&2; exit 64; }}
            echo "$5" >> {self.dir}/podman.refs
            file={self.configs}/$(printf '%s' "$5" | tr '/:' '__').json
            [[ -f $file ]] || {{ echo "image not known: $5" >&2; exit 125; }}
            jq -c '.rootfs.diff_ids' "$file"
            """)
        )
        (bin_dir / "podman").chmod(0o755)
        self.env = {"PATH": f"{bin_dir}:/usr/bin:/bin"}

    def tearDown(self):
        self.tmp.cleanup()

    def serve(self, ref, config):
        (self.configs / (ref.replace("/", "_").replace(":", "_") + ".json")).write_text(
            json.dumps(config)
        )

    def serve_system(self, diff_ids):
        config = fixture("athanor-system")
        config["rootfs"]["diff_ids"] = diff_ids
        self.serve(SYSTEM_ID, config)

    def serve_variant(self, name, config):
        self.serve(f"{REGISTRY}/{name}:{RUN}", config)

    def check(self, system=f"sha256:{SYSTEM_ID}"):
        return subprocess.run(
            [
                "bash",
                str(SCRIPT),
                "--system",
                system,
                "--registry",
                REGISTRY,
                "--tag",
                RUN,
            ],
            capture_output=True,
            text=True,
            env=self.env,
        )

    def system_stage(self):
        return fixture("athanor-system")["rootfs"]["diff_ids"][:SYSTEM_STAGE_LAYERS]

    def test_variants_built_from_the_system_image_pass(self):
        system = self.system_stage()
        self.serve_system(system)
        for name in NAMES:
            # Each variant keeps its own top layers from the run, over the shared system image.
            config = fixture(name)
            own = config["rootfs"]["diff_ids"][SYSTEM_STAGE_LAYERS:]
            config["rootfs"]["diff_ids"] = system + own
            self.serve_variant(name, config)
        r = self.check()
        self.assertEqual(r.returncode, 0, r.stderr)
        for name in NAMES:
            self.assertRegex(
                r.stdout, rf"{name}\b.*{SYSTEM_STAGE_LAYERS} of {SYSTEM_STAGE_LAYERS}"
            )
        refs = (self.dir / "podman.refs").read_text().splitlines()
        self.assertEqual(refs, [SYSTEM_ID] + [f"{REGISTRY}/{n}:{RUN}" for n in NAMES])

    def test_variants_that_rebuilt_the_system_stage_fail(self):
        # Run 37384733899 as published: the NVIDIA variants rebuilt the system stage.
        self.serve_system(self.system_stage())
        for name in NAMES:
            self.serve_variant(name, fixture(name))
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertNotIn("athanor-system:", r.stderr)
        for name in NAMES[1:]:
            message = f"{name}: layer {BASE_LAYERS + 1} of {SYSTEM_STAGE_LAYERS} differs"
            self.assertIn(message, r.stderr)
            # The job summary is the report on stdout: the failure is there as well.
            self.assertIn(f"**failed**: {message}", r.stdout)

    def test_a_variant_with_fewer_layers_than_the_system_image_fails(self):
        system = self.system_stage()
        self.serve_system(system)
        for name in NAMES:
            config = fixture(name)
            config["rootfs"]["diff_ids"] = (
                system[:-1] if name == "athanor-system-nvidia" else system
            )
            self.serve_variant(name, config)
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn(
            f"athanor-system-nvidia: layer {SYSTEM_STAGE_LAYERS} of {SYSTEM_STAGE_LAYERS} differs",
            r.stderr,
        )

    def test_an_unreadable_variant_fails(self):
        self.serve_system(self.system_stage())
        r = self.check()
        self.assertEqual(r.returncode, 1)
        for name in NAMES:
            message = f"cannot read the configuration of {REGISTRY}/{name}:{RUN} from local storage"
            self.assertIn(message, r.stderr)
            self.assertIn(message, r.stdout)

    def test_a_variant_without_diff_ids_fails_with_its_own_message(self):
        system = self.system_stage()
        self.serve_system(system)
        for name in NAMES:
            config = fixture(name)
            config["rootfs"]["diff_ids"] = system
            if name == "athanor-system-nvidia":
                del config["rootfs"]["diff_ids"]
            self.serve_variant(name, config)
        r = self.check()
        self.assertEqual(r.returncode, 1)
        message = f"{REGISTRY}/athanor-system-nvidia:{RUN}: the image has no RootFS.Layers array"
        self.assertIn(message, r.stderr)
        self.assertIn(message, r.stdout)
        # The other two are still checked and reported.
        self.assertRegex(r.stdout, r"athanor-system-nvidia-legacy`: 116 of 116")

    def test_a_system_image_without_diff_ids_fails_with_its_own_message(self):
        config = fixture("athanor-system")
        del config["rootfs"]["diff_ids"]
        self.serve(SYSTEM_ID, config)
        r = self.check()
        self.assertEqual(r.returncode, 1)
        message = f"{SYSTEM_ID}: the image has no RootFS.Layers array"
        self.assertIn(message, r.stderr)
        self.assertIn(message, r.stdout)

    def test_a_system_image_without_layers_is_refused(self):
        self.serve_system([])
        r = self.check()
        self.assertEqual(r.returncode, 1)
        self.assertIn("no layers", r.stderr)
        self.assertIn("**failed**", r.stdout)

    def test_the_system_image_is_an_image_id(self):
        r = self.check(system="localhost/athanor-system:latest")
        self.assertEqual(r.returncode, 2)
        self.assertFalse((self.dir / "podman.refs").exists())


if __name__ == "__main__":
    unittest.main()
