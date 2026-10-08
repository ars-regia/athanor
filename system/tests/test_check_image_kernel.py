"""Unit tests of system/check-image-kernel.sh (python3 -B -m unittest discover -s system/tests -v).

podman is a stub that serves one vmlinuz as the image's; every key is a throwaway one. The cases
sign a vmlinuz of the host with sbsign, so they skip where sbsigntools or a readable host
vmlinuz is missing.
"""

import glob
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SCRIPT = ROOT / "system" / "check-image-kernel.sh"
CN = "Athanor Secure Boot Signing Key"
HOST_VMLINUZ = sorted(
    p for p in glob.glob("/usr/lib/modules/*/vmlinuz") if os.access(p, os.R_OK)
)
PODMAN = """#!/bin/sh
case "$*" in
*/bin/sh*) [ -n "$FAKE_KERNEL_PATH" ] && echo "$FAKE_KERNEL_PATH" ;;
*/bin/cat*) cat "$FAKE_VMLINUZ" ;;
esac
"""


def run(*args, check=True):
    return subprocess.run(
        [str(a) for a in args], check=check, capture_output=True, text=True
    )


@unittest.skipUnless(
    shutil.which("sbsign") and shutil.which("sbverify") and HOST_VMLINUZ,
    "needs sbsigntools and a host vmlinuz",
)
class CheckImageKernel(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = pathlib.Path(self._tmp.name)
        (self.tmp / "bin").mkdir()
        stub = self.tmp / "bin" / "podman"
        stub.write_text(PODMAN)
        stub.chmod(0o755)

    def identity(self, cn):
        """(key, DER certificate) of a throwaway identity."""
        key, crt, der = (self.tmp / f"{cn}{ext}" for ext in (".priv", ".crt", ".der"))
        run(
            "openssl",
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            f"/CN={cn}/",
            "-keyout",
            key,
            "-out",
            crt,
        )
        run("openssl", "x509", "-in", crt, "-outform", "DER", "-out", der)
        return key, crt, der

    def unsigned(self):
        """The host vmlinuz without the distribution's signature."""
        out = self.tmp / "unsigned"
        shutil.copy(HOST_VMLINUZ[-1], out)
        run("sbattach", "--remove", out, check=False)
        return out

    def sign(self, src, identity, out):
        run("sbsign", "--key", identity[0], "--cert", identity[1], "--output", out, src)
        return out

    def check(self, vmlinuz, cert, kernel_path="/usr/lib/modules/6.18/vmlinuz"):
        env = {
            **os.environ,
            "PATH": f"{self.tmp / 'bin'}:{os.environ['PATH']}",
            "SECUREBOOT_CERT": str(cert),
            "FAKE_VMLINUZ": str(vmlinuz),
            "FAKE_KERNEL_PATH": kernel_path,
        }
        return subprocess.run(
            ["bash", str(SCRIPT), "image"], env=env, capture_output=True, text=True
        )

    def test_accepts_one_signature_by_the_project_certificate(self):
        project = self.identity(CN)
        signed = self.sign(self.unsigned(), project, self.tmp / "signed")
        result = self.check(signed, project[2])
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_refuses_a_signature_by_another_certificate(self):
        project, other = self.identity(CN), self.identity("Red Hat Test Certificate")
        signed = self.sign(self.unsigned(), other, self.tmp / "signed")
        result = self.check(signed, project[2])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not signed by", result.stderr)

    def test_refuses_the_project_signature_next_to_another(self):
        project, other = self.identity(CN), self.identity("Red Hat Test Certificate")
        first = self.sign(self.unsigned(), other, self.tmp / "first")
        both = self.tmp / "both"
        run(
            "sbsign",
            "--addsignature",
            "--key",
            project[0],
            "--cert",
            project[1],
            "--output",
            both,
            first,
        )
        result = self.check(both, project[2])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("2 signatures", result.stderr)

    def test_refuses_an_unsigned_vmlinuz(self):
        result = self.check(self.unsigned(), self.identity(CN)[2])
        self.assertNotEqual(result.returncode, 0)

    def test_refuses_an_image_without_exactly_one_vmlinuz(self):
        project = self.identity(CN)
        signed = self.sign(self.unsigned(), project, self.tmp / "signed")
        result = self.check(signed, project[2], kernel_path="")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exactly one", result.stderr)


if __name__ == "__main__":
    unittest.main()
