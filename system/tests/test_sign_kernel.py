"""Unit tests of system/sign-kernel.sh, with a podman stub that plays the sign container, and of
the image-identity check in system/scripts/install_signed_kernel.py
(python3 -B -m unittest discover -s system/tests -v)."""

import json
import os
import pathlib
import stat
import struct
import subprocess
import sys
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SIGN = ROOT / "system" / "sign-kernel.sh"
PROJECT_CERT = ROOT / "forge/specs/azoth/keys/secureboot" / "athanor-secureboot.pem"
NVR = subprocess.run(
    ["bash", str(ROOT / "forge/specs/azoth/nvr.sh")],
    capture_output=True,
    text=True,
    check=True,
).stdout.strip()
SECRET = "-----BEGIN PRIVATE KEY-----\nnot-a-real-key\n-----END PRIVATE KEY-----"

sys.path.insert(0, str(ROOT / "system" / "scripts"))
from install_signed_kernel import signed_content  # noqa: E402

# A podman that records every call with the signing variables it can see, and on `run` does
# what sign.sh would: reads the mounted key and writes vmlinuz and kver to the /out mount.
STUB = textwrap.dedent("""\
    #!/usr/bin/env python3
    import json, os, pathlib, stat, sys
    state = pathlib.Path(os.environ["STUB_STATE"])
    args = sys.argv[1:]
    with open(state / "calls.log", "a") as log:
        log.write(json.dumps({"args": args, "env": sorted(k for k in os.environ if k.startswith("SECUREBOOT"))}) + "\\n")
    if args[0] == "create":
        print("ctr")
    elif args[0] == "cp":
        pathlib.Path(args[2], "kernel-core-" + os.environ["STUB_KVER"] + ".rpm").write_text("rpm")
    elif args[0] == "run":
        mounts = dict(reversed(a.split(":")[:2]) for a in args if a.count(":") >= 1 and a.startswith("/"))
        key = pathlib.Path(mounts["/run/sign/key"])
        (state / "key.mode").write_text(oct(stat.S_IMODE(key.stat().st_mode)) + " " + oct(stat.S_IMODE(key.parent.stat().st_mode)))
        (state / "key.seen").write_text(key.read_text())
        (state / "cert.seen").write_text(mounts["/run/sign/cert.pem"])
        out = pathlib.Path(mounts["/out"])
        (out / "vmlinuz").write_text("signed")
        (out / "kver").write_text(os.environ["STUB_KVER"] + "\\n")
    """)


class SignKernel(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        (self.dir / "bin").mkdir()
        stub = self.dir / "bin" / "podman"
        stub.write_text(STUB)
        stub.chmod(0o755)
        self.state = self.dir / "state"
        self.state.mkdir()
        (self.dir / "runtime").mkdir(mode=0o700)
        artifacts = self.dir / "artifacts"
        artifacts.mkdir()
        (artifacts / "kernel-artifacts.env").write_text(
            f"state=ready\nnvr={NVR}\nregistry=registry.example/owner\nkernel_digest=sha256:{'1' * 64}\n"
        )
        self.out = self.dir / "out"
        self.env = {
            k: v for k, v in os.environ.items() if not k.startswith("SECUREBOOT")
        }
        self.env.update(
            PATH=f"{self.dir / 'bin'}:{os.environ['PATH']}",
            STUB_STATE=str(self.state),
            STUB_KVER=f"{NVR}.x86_64",
            KERNEL_ARTIFACTS_DIR=str(artifacts),
            XDG_RUNTIME_DIR=str(self.dir / "runtime"),
        )

    def tearDown(self):
        self.tmp.cleanup()

    def run_sign(self, *extra, **env):
        return subprocess.run(
            ["bash", str(SIGN), "--out", str(self.out), *extra],
            capture_output=True,
            text=True,
            env=dict(self.env, **env),
        )

    def calls(self):
        log = self.state / "calls.log"
        return (
            [json.loads(line) for line in log.read_text().splitlines()]
            if log.exists()
            else []
        )

    def test_release_without_the_key_is_refused_before_anything_runs(self):
        r = self.run_sign()
        self.assertEqual(r.returncode, 2)
        self.assertIn("signing environment", r.stderr)
        self.assertEqual(self.calls(), [])

    def test_the_key_reaches_the_container_as_a_private_read_only_file_only(self):
        r = self.run_sign(SECUREBOOT_SIGNING_KEY=SECRET)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.state / "key.seen").read_text(), SECRET + "\n")
        self.assertEqual((self.state / "key.mode").read_text(), "0o600 0o700")
        self.assertEqual((self.state / "cert.seen").read_text(), str(PROJECT_CERT))
        run = next(c["args"] for c in self.calls() if c["args"][0] == "run")
        self.assertIn("--network=none", run)
        self.assertTrue(any(a.endswith(":/run/sign/key:ro") for a in run), run)
        for call in self.calls():
            self.assertEqual(
                call["env"], [], "the key variable reached a child process"
            )
            self.assertFalse(
                any("not-a-real-key" in a for a in call["args"]),
                "the key is on a command line",
            )
        self.assertEqual(
            list((self.dir / "runtime").iterdir()),
            [],
            "the key directory outlived the run",
        )
        self.assertEqual((self.out / "vmlinuz").read_text(), "signed")

    def test_the_kernel_is_the_one_the_artifacts_name(self):
        r = self.run_sign(
            SECUREBOOT_SIGNING_KEY=SECRET, STUB_KVER="7.0.0-100.azoth.fc43.x86_64"
        )
        self.assertEqual(r.returncode, 1)
        self.assertIn("the kernel artifacts name", r.stderr)

    def test_throwaway_signs_with_a_key_of_its_own_and_keeps_its_certificate(self):
        r = self.run_sign("--throwaway", SECUREBOOT_SIGNING_KEY=SECRET)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("must not be published", r.stdout)
        self.assertIn("PRIVATE KEY", (self.state / "key.seen").read_text())
        self.assertNotIn("not-a-real-key", (self.state / "key.seen").read_text())
        cert = self.out / "throwaway-certificate.pem"
        self.assertEqual((self.state / "cert.seen").read_text(), str(cert))
        self.assertIn("BEGIN CERTIFICATE", cert.read_text())


def pe_image(body: bytes, checksum: int = 0, table: bytes = b"") -> bytes:
    """A PE32+ image: DOS header, PE header, optional header with sixteen data directories,
    then the body; with a table, the body padded to 8 bytes and a certificate table after it."""
    header = bytearray(0x40 + 4 + 20 + 112 + 16 * 8)
    header[:2] = b"MZ"
    struct.pack_into("<I", header, 0x3C, 0x40)
    header[0x40:0x44] = b"PE\0\0"
    optional = 0x40 + 24
    struct.pack_into("<H", header, optional, 0x20B)
    struct.pack_into("<I", header, optional + 64, checksum)
    image = bytes(header) + body
    if table:
        image += b"\0" * (-len(image) % 8)
        struct.pack_into("<II", header, optional + 112 + 4 * 8, len(image), len(table))
        image = bytes(header) + image[len(header) :] + table
    return image


class SignedContent(unittest.TestCase):
    BODY = b"kernel text" * 5

    def test_a_signature_leaves_the_signed_content_unchanged(self):
        unsigned = pe_image(self.BODY)
        test_signed = pe_image(
            self.BODY, checksum=0x1234, table=b"pesign test certificate"
        )
        project_signed = pe_image(
            self.BODY, checksum=0x5678, table=b"athanor certificate, longer"
        )
        self.assertEqual(signed_content(unsigned), signed_content(test_signed))
        self.assertEqual(signed_content(test_signed), signed_content(project_signed))

    def test_another_kernel_is_told_apart(self):
        other = pe_image(
            self.BODY.replace(b"text", b"tExt", 1), table=b"athanor certificate"
        )
        self.assertNotEqual(signed_content(pe_image(self.BODY)), signed_content(other))

    def test_a_file_that_is_not_a_pe_image_is_refused(self):
        with self.assertRaises(ValueError):
            signed_content(b"ELF not a PE image")


if __name__ == "__main__":
    unittest.main()
