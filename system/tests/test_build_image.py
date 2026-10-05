"""Unit tests of the kernel artifacts in system/build-image.sh, with a podman stub that records
its arguments and keeps a copy of the signed-kernel build context
(python3 -B -m unittest discover -s system/tests -v)."""

import hashlib
import os
import pathlib
import subprocess
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
BUILD = ROOT / "system" / "build-image.sh"
NVR = subprocess.run(
    ["bash", str(ROOT / "forge/specs/azoth/nvr.sh")],
    capture_output=True,
    text=True,
    check=True,
).stdout.strip()
KERNEL = "sha256:" + "1" * 64
OPEN = "sha256:" + "3" * 64
LEGACY = "sha256:" + "4" * 64
PROJECT_CERT = ROOT / "forge/specs/azoth/keys/secureboot" / "athanor-secureboot.pem"


class BuildImage(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        (bin_dir / "podman").write_text(
            textwrap.dedent(f"""\
            #!/bin/bash
            printf '%s\\n' "$@" > {self.dir}/podman.args
            for a; do
              case $a in signed-kernel=*) cp -r "${{a#signed-kernel=}}" {self.dir}/context ;; esac
            done
            """)
        )
        (bin_dir / "podman").chmod(0o755)
        self.artifacts = self.dir / "artifacts"
        self.artifacts.mkdir()
        # A kernel signed earlier, as the sign-only job hands it over: no throwaway key is generated.
        self.signed = self.dir / "signed"
        self.signed.mkdir()
        (self.signed / "vmlinuz").write_bytes(b"MZ signed vmlinuz")
        self.env = dict(
            os.environ,
            PATH=f"{bin_dir}:{os.environ['PATH']}",
            KERNEL_ARTIFACTS_DIR=str(self.artifacts),
        )

    def tearDown(self):
        self.tmp.cleanup()

    def artifacts_file(self, **values):
        lines = {
            "state": "ready",
            "nvr": NVR,
            "registry": "ghcr.io/hr-mes",
            "kernel_digest": KERNEL,
            "nvidia_open_digest": OPEN,
            "nvidia_legacy_digest": LEGACY,
            **values,
        }
        (self.artifacts / "kernel-artifacts.env").write_text(
            "".join(f"{k}={v}\n" for k, v in lines.items() if v is not None)
        )

    def build(self, gpu, *extra):
        r = subprocess.run(
            [
                "bash",
                str(BUILD),
                "--gpu",
                gpu,
                "--registry",
                "localhost",
                "--tag",
                "check",
                "--signed-kernel",
                str(self.signed),
                *extra,
            ],
            capture_output=True,
            text=True,
            env=self.env,
        )
        args = (
            (self.dir / "podman.args").read_text().splitlines()
            if (self.dir / "podman.args").exists()
            else []
        )
        return r, args

    def test_every_image_carries_its_own_version_and_build_time(self):
        self.artifacts_file()
        self.env["SOURCE_DATE_EPOCH"] = "1789466400"  # 2026-09-15T10:00:00Z
        r, args = self.build("none", "--serial", "412")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("org.opencontainers.image.version=43.20260915.412", args)
        self.assertIn("org.opencontainers.image.created=2026-09-15T10:00:00Z", args)
        self.assertIn("IMAGE_REGISTRY=localhost", args)

    def test_a_local_build_has_serial_zero_and_a_serial_is_a_number(self):
        self.artifacts_file()
        self.env["SOURCE_DATE_EPOCH"] = "1789466400"
        r, args = self.build("none")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("org.opencontainers.image.version=43.20260915.0", args)
        r, _ = self.build("none", "--serial", "v2")
        self.assertEqual(r.returncode, 2)

    def test_nvidia_builds_from_the_open_module_digest(self):
        self.artifacts_file()
        r, args = self.build("nvidia")
        self.assertEqual(r.returncode, 0, r.stderr)
        for expected in (
            f"AZOTH_NVR={NVR}",
            "KERNEL_REGISTRY=ghcr.io/hr-mes",
            f"NVIDIA_OPEN_DIGEST={OPEN}",
            f"io.athanor.azoth.digest={KERNEL}",
            f"io.athanor.azoth-nvidia.digest={OPEN}",
        ):
            self.assertIn(expected, args)
        self.assertNotIn(f"NVIDIA_LEGACY_DIGEST={LEGACY}", args)

    def test_default_image_builds_while_modules_are_missing(self):
        self.artifacts_file(
            state="modules-missing", nvidia_open_digest=None, nvidia_legacy_digest=None
        )
        r, args = self.build("none")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse(any(a.startswith("NVIDIA_") for a in args))
        # A regression check, not just a smoke test: the default build still reads its own
        # digest plumbing from the file (a pre-digest build-image.sh would pass the two
        # assertions above without ever calling kernel-artifacts.sh at all).
        for expected in (
            f"AZOTH_NVR={NVR}",
            "KERNEL_REGISTRY=ghcr.io/hr-mes",
            f"io.athanor.azoth.digest={KERNEL}",
        ):
            self.assertIn(expected, args)

    def test_variant_without_its_module_digest_is_refused(self):
        self.artifacts_file(state="modules-missing", nvidia_legacy_digest=None)
        r, args = self.build("nvidia-legacy")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("nvidia_legacy_digest", r.stderr)
        self.assertEqual(args, [])

    def test_missing_kernel_is_refused(self):
        self.artifacts_file(
            state="kernel-missing",
            kernel_digest=None,
            nvidia_open_digest=None,
            nvidia_legacy_digest=None,
        )
        r, args = self.build("none")
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(args, [])

    def test_artifacts_of_other_pins_are_refused(self):
        self.artifacts_file(nvr="7.0.0-100.azoth.fc43")
        r, args = self.build("none")
        self.assertEqual(r.returncode, 2)
        self.assertIn("resolve again", r.stderr)
        self.assertEqual(args, [])

    def test_the_signed_kernel_is_checked_against_the_committed_certificate(self):
        # The certificate staged for the final stage is the project's, never one that came
        # with the signed file, and the build argument pins the file the step installs.
        self.artifacts_file()
        (self.signed / "certificate.pem").write_text("a certificate the signer chose\n")
        r, args = self.build("none")
        self.assertEqual(r.returncode, 0, r.stderr)
        context = self.dir / "context"
        self.assertEqual((context / "vmlinuz").read_bytes(), b"MZ signed vmlinuz")
        self.assertEqual(
            (context / "certificate.pem").read_bytes(), PROJECT_CERT.read_bytes()
        )
        digest = hashlib.sha256(b"MZ signed vmlinuz").hexdigest()
        self.assertIn(f"SIGNED_VMLINUZ_SHA256={digest}", args)
        self.assertFalse(
            any(a.startswith("io.athanor.kernel-signing-key=") for a in args)
        )
        self.assertFalse(
            any(a.startswith("--secret") for a in args),
            "the image build must hold no key",
        )

    def test_push_requires_a_kernel_signed_in_the_sign_only_job(self):
        self.artifacts_file()
        r = subprocess.run(
            [
                "bash",
                str(BUILD),
                "--gpu",
                "none",
                "--registry",
                "localhost",
                "--tag",
                "check",
                "--push",
            ],
            capture_output=True,
            text=True,
            env=dict(self.env, SECUREBOOT_SIGNING_KEY="ignored"),
        )
        self.assertEqual(r.returncode, 2)
        self.assertIn("--push requires --signed-kernel", r.stderr)
        self.assertFalse((self.dir / "podman.args").exists())


if __name__ == "__main__":
    unittest.main()
