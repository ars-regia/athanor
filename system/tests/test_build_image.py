"""Unit tests of the kernel artifacts in system/build-image.sh, with a podman stub that records
its arguments (python3 -B -m unittest discover -s system/tests -v)."""

import json
import os
import pathlib
import subprocess
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
BUILD = ROOT / "system" / "build-image.sh"
NVR = subprocess.run(["bash", str(ROOT / "forge/specs/azoth/nvr.sh")], capture_output=True, text=True, check=True).stdout.strip()
KERNEL = "sha256:" + "1" * 64
OPEN = "sha256:" + "3" * 64
LEGACY = "sha256:" + "4" * 64
BOOT = "sha256:" + "7" * 64  # azoth-boot, the signed vmlinuz
SYSTEM = "5" * 64  # the image ID the stub's --iidfile reports for the system stage
TIERS = {f"tier{n}": "sha256:" + "89ab"[n] * 64 for n in range(4)}  # the forge tier repositories


class BuildImage(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        (bin_dir / "podman").write_text(textwrap.dedent(f"""\
            #!/bin/bash
            printf '%s\\n' "$@" -- >> {self.dir}/podman.args
            while [[ $# -gt 0 ]]; do
                [[ $1 != --iidfile ]] || echo "sha256:{SYSTEM}" > "$2"
                shift
            done
            """))
        (bin_dir / "podman").chmod(0o755)
        self.artifacts = self.dir / "artifacts"
        self.artifacts.mkdir()
        # No signing key anywhere: the vmlinuz arrives signed, by digest (D43).
        self.env = {k: v for k, v in os.environ.items() if not k.endswith("_KEY")}
        self.env.update(PATH=f"{bin_dir}:{os.environ['PATH']}", KERNEL_ARTIFACTS_DIR=str(self.artifacts),
                        TIER_DIGESTS_DIR=str(self.dir / "tiers"))
        self.tiers_file()

    def tearDown(self):
        self.tmp.cleanup()

    def artifacts_file(self, **values):
        lines = {"state": "ready", "nvr": NVR, "registry": "ghcr.io/ars-regia", "kernel_digest": KERNEL,
                 "boot_digest": BOOT, "nvidia_open_digest": OPEN, "nvidia_legacy_digest": LEGACY, **values}
        (self.artifacts / "kernel-artifacts.env").write_text("".join(f"{k}={v}\n" for k, v in lines.items() if v is not None))

    def tiers_file(self, **values):
        """tier-digests.json as publish_tiers.sh and tier-digests.sh write it; None drops a key."""
        tiers = {"registry": "ghcr.io/ars-regia", **TIERS, **values}
        (self.dir / "tiers").mkdir(exist_ok=True)
        (self.dir / "tiers" / "tier-digests.json").write_text(json.dumps({k: v for k, v in tiers.items() if v is not None}))

    def calls(self):
        """The arguments of each podman call, in order."""
        if not (self.dir / "podman.args").exists():
            return []
        calls, call = [], []
        for line in (self.dir / "podman.args").read_text().splitlines():
            if line == "--":
                calls.append(call)
                call = []
            else:
                call.append(line)
        return calls

    def build(self, gpu, *extra):
        r = subprocess.run(["bash", str(BUILD), "--gpu", gpu, "--registry", "localhost", "--tag", "check", *extra],
                           capture_output=True, text=True, env=self.env)
        return r, [a for call in self.calls() for a in call]

    def test_the_system_stage_is_built_alone_and_reports_its_image_id(self):
        self.artifacts_file()
        iid = self.dir / "out" / "system-image.iid"
        r = subprocess.run(["bash", str(BUILD), "--system", "--registry", "localhost", "--iidfile", str(iid)],
                           capture_output=True, text=True, env=self.env)
        self.assertEqual(r.returncode, 0, r.stderr)
        [args] = self.calls()
        self.assertEqual(args[args.index("--target") + 1], "system")
        self.assertEqual(args[args.index("--iidfile") + 1], str(iid))
        self.assertEqual(iid.read_text().strip(), f"sha256:{SYSTEM}")
        for expected in (f"AZOTH_NVR={NVR}", "IMAGE_REGISTRY=localhost", "FORGE_REGISTRY=ghcr.io/ars-regia",
                         f"BOOT_DIGEST={BOOT}", "--format", "docker"):
            self.assertIn(expected, args)
        # Nothing of a variant: no GPU, no tag, no label, no secret.
        for absent in ("-t", "--label", "--secret"):
            self.assertNotIn(absent, args)
        self.assertFalse(any(a.startswith(("GPU=", "SYSTEM_IMAGE=", "NVIDIA_")) for a in args))

    def test_a_variant_builds_from_the_system_image_it_is_given(self):
        self.artifacts_file()
        given = "6" * 64
        r, _ = self.build("nvidia", "--system-image", f"sha256:{given}")
        self.assertEqual(r.returncode, 0, r.stderr)
        [args] = self.calls()
        self.assertIn(f"SYSTEM_IMAGE={given}", args)
        self.assertIn("GPU=nvidia", args)
        self.assertNotIn("--target", args)

    def test_a_variant_alone_builds_the_system_stage_first(self):
        self.artifacts_file()
        r, _ = self.build("none")
        self.assertEqual(r.returncode, 0, r.stderr)
        system, variant = self.calls()
        self.assertEqual(system[system.index("--target") + 1], "system")
        self.assertIn(f"SYSTEM_IMAGE={SYSTEM}", variant)
        self.assertIn("localhost/athanor-system:check", variant)

    def test_the_system_image_is_an_image_id(self):
        self.artifacts_file()
        r, args = self.build("none", "--system-image", "localhost/athanor-system:latest")
        self.assertEqual(r.returncode, 2)
        self.assertEqual(args, [])

    def test_every_image_carries_its_own_version_and_build_time(self):
        self.artifacts_file()
        self.env["SOURCE_DATE_EPOCH"] = "1789466400"  # 2026-09-15T10:00:00Z
        r = subprocess.run(["bash", str(BUILD), "--gpu", "none", "--registry", "localhost", "--tag", "check", "--serial", "412"],
                           capture_output=True, text=True, env=self.env)
        self.assertEqual(r.returncode, 0, r.stderr)
        args = self.calls()[-1]
        self.assertIn("org.opencontainers.image.version=43.20260915.412", args)
        self.assertIn("org.opencontainers.image.created=2026-09-15T10:00:00Z", args)
        self.assertIn("IMAGE_REGISTRY=localhost", args)

    def test_a_local_build_has_serial_zero_and_a_serial_is_a_number(self):
        self.artifacts_file()
        self.env["SOURCE_DATE_EPOCH"] = "1789466400"
        r, args = self.build("none")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("org.opencontainers.image.version=43.20260915.0", args)
        r = subprocess.run(["bash", str(BUILD), "--gpu", "none", "--registry", "localhost", "--tag", "check", "--serial", "v2"],
                           capture_output=True, text=True, env=self.env)
        self.assertEqual(r.returncode, 2)

    def test_nvidia_builds_from_the_open_module_digest(self):
        self.artifacts_file()
        r, args = self.build("nvidia")
        self.assertEqual(r.returncode, 0, r.stderr)
        for expected in (f"AZOTH_NVR={NVR}", "KERNEL_REGISTRY=ghcr.io/ars-regia", f"NVIDIA_OPEN_DIGEST={OPEN}",
                         f"io.athanor.azoth.digest={KERNEL}", f"io.athanor.azoth-nvidia.digest={OPEN}"):
            self.assertIn(expected, args)
        self.assertNotIn(f"NVIDIA_LEGACY_DIGEST={LEGACY}", args)

    def test_default_image_builds_while_modules_are_missing(self):
        self.artifacts_file(state="modules-missing", nvidia_open_digest=None, nvidia_legacy_digest=None)
        r, args = self.build("none")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse(any(a.startswith("NVIDIA_") for a in args))
        # A regression check, not just a smoke test: the default build still reads its own
        # digest plumbing from the file (a pre-digest build-image.sh would pass the two
        # assertions above without ever calling kernel-artifacts.sh at all).
        for expected in (f"AZOTH_NVR={NVR}", "KERNEL_REGISTRY=ghcr.io/ars-regia", f"io.athanor.azoth.digest={KERNEL}"):
            self.assertIn(expected, args)

    def test_every_build_takes_the_signed_vmlinuz_by_digest_and_no_secret(self):
        self.artifacts_file()
        r, _ = self.build("nvidia")
        self.assertEqual(r.returncode, 0, r.stderr)
        system, variant = self.calls()
        for args in (system, variant):
            self.assertIn(f"BOOT_DIGEST={BOOT}", args)
            self.assertNotIn("--secret", args)
            self.assertFalse(any("uki" in a for a in args), args)
        self.assertIn(f"io.athanor.azoth-boot.digest={BOOT}", variant)

    def test_every_build_takes_the_tiers_by_the_digests_of_the_file(self):
        self.artifacts_file()
        self.tiers_file(registry="registry.example/owner")
        r, _ = self.build("nvidia")
        self.assertEqual(r.returncode, 0, r.stderr)
        system, variant = self.calls()
        for args in (system, variant):
            self.assertIn("FORGE_REGISTRY=registry.example/owner", args)
            for n in range(4):
                self.assertIn(f"TIER{n}_DIGEST={TIERS[f'tier{n}']}", args)
        for n in range(4):
            self.assertIn(f"io.athanor.forge-tier{n}.digest={TIERS[f'tier{n}']}", variant)

    def test_without_valid_tier_digests_nothing_builds(self):
        self.artifacts_file()
        cases = {"no file": None, "tier missing": {"tier2": None}, "a tag, not a digest": {"tier1": "latest"},
                 "short digest": {"tier0": "sha256:abc"}, "no registry": {"registry": None},
                 "registry with a tag": {"registry": "ghcr.io/ars-regia:latest"}}
        for case, values in cases.items():
            with self.subTest(case=case):
                if values is None:
                    (self.dir / "tiers" / "tier-digests.json").unlink()
                else:
                    self.tiers_file(**values)
                r, _ = self.build("none")
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertIn("tier-digests", r.stderr)
                self.assertEqual(self.calls(), [])

    def test_push_needs_no_signing_key(self):
        self.artifacts_file()
        r, _ = self.build("none", "--push")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.calls()[-1], ["push", "localhost/athanor-system:check"])

    def test_without_the_signed_vmlinuz_nothing_builds(self):
        self.artifacts_file(state="modules-missing", boot_digest=None, nvidia_open_digest=None, nvidia_legacy_digest=None)
        for command in (["--gpu", "none", "--tag", "check"], ["--system", "--iidfile", str(self.dir / "iid")]):
            with self.subTest(command=command):
                r = subprocess.run(["bash", str(BUILD), "--registry", "localhost", *command],
                                   capture_output=True, text=True, env=self.env)
                self.assertNotEqual(r.returncode, 0)
                self.assertIn("boot_digest", r.stderr)
                self.assertEqual(self.calls(), [])

    def test_variant_without_its_module_digest_is_refused(self):
        self.artifacts_file(state="modules-missing", nvidia_legacy_digest=None)
        r, args = self.build("nvidia-legacy")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("nvidia_legacy_digest", r.stderr)
        self.assertEqual(args, [])

    def test_missing_kernel_is_refused(self):
        self.artifacts_file(state="kernel-missing", kernel_digest=None, nvidia_open_digest=None, nvidia_legacy_digest=None)
        r, args = self.build("none")
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(args, [])

    def test_artifacts_of_other_pins_are_refused(self):
        self.artifacts_file(nvr="7.0.0-100.azoth.fc43")
        r, args = self.build("none")
        self.assertEqual(r.returncode, 2)
        self.assertIn("resolve again", r.stderr)
        self.assertEqual(args, [])



class SignedKernelStage(unittest.TestCase):
    """system/Containerfile installs azoth-boot's vmlinuz only over the kernel it was signed for."""

    def setUp(self):
        lines = (ROOT / "system" / "Containerfile").read_text().replace("\\\n", " ").splitlines()
        copies = [i for i, line in enumerate(lines) if line.startswith("COPY") and "--from=signed-kernel" in line]
        self.assertEqual(len(copies), 1, "exactly one COPY of the signed vmlinuz")
        self.copy = lines[copies[0]]
        self.check = lines[copies[0] - 1]

    def test_the_vmlinuz_keeps_the_mode_of_the_rpm(self):
        self.assertIn("--chmod=0755", self.copy.split())
        self.assertTrue(self.copy.endswith(" /vmlinuz /usr/lib/modules/${AZOTH_NVR}.x86_64/vmlinuz"), self.copy)

    def test_the_kver_of_azoth_boot_is_compared_with_the_pins_before_the_copy(self):
        self.assertTrue(self.check.startswith("RUN --mount=type=bind,from=signed-kernel,source=/kver,target=/tmp/azoth-boot-kver "), self.check)
        command = self.check.split("target=/tmp/azoth-boot-kver ", 1)[1]
        with tempfile.TemporaryDirectory() as tmp:
            kver = pathlib.Path(tmp) / "kver"
            command = command.replace("/tmp/azoth-boot-kver", str(kver))
            for signed, status in ((f"{NVR}.x86_64", 0), ("7.0.0-1.azoth.fc43.x86_64", 1)):
                kver.write_text(signed + "\n")
                r = subprocess.run(["sh", "-c", command], env={"AZOTH_NVR": NVR, "PATH": os.environ["PATH"]},
                                   capture_output=True, text=True)
                self.assertEqual(r.returncode, status, r.stderr)
            self.assertIn("not of " + NVR, r.stderr)


if __name__ == "__main__":
    unittest.main()
