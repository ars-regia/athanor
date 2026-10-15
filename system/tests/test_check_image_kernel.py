"""Unit tests of system/check-image-kernel.sh (python3 -B -m unittest discover -s system/tests -v).

podman is a stub that serves one vmlinuz as the image's; every key is a throwaway one. The cases
sign a vmlinuz of the host with sbsign, so they skip where sbsigntools or a readable host
vmlinuz is missing.
"""

import glob
import io
import struct
import sys
import tarfile
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
*"tar -cf"*)
    [ -n "$FAKE_PODMAN_RC" ] && exit "$FAKE_PODMAN_RC"
    [ -n "$FAKE_GARBAGE" ] && { echo "not a tar"; exit 0; }
    for last; do :; done
    exec /bin/sh -c "$(printf %s "$last" | sed "s|cd / |cd $FAKE_ROOT |")" ;;
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


def fake_pe(*sections):
    """A minimal PE image whose section table names the given sections."""
    pe, opt = 0x80, 0xF0
    head = b"MZ" + bytes(0x3A) + struct.pack("<I", pe) + bytes(pe - 0x40)
    head += b"PE\0\0" + struct.pack("<HH", 0x8664, len(sections)) + bytes(12)
    head += struct.pack("<H", opt) + bytes(2) + bytes(opt)
    return head + b"".join(n.encode().ljust(8, b"\0") + bytes(32) for n in sections)


class CheckImageUki(unittest.TestCase):
    def findings(self, files):
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w") as tar:
            for name, data in files.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
        buf.seek(0)
        sys.path.insert(0, str(HERE.parent))
        try:
            import check_image_uki
        finally:
            sys.path.pop(0)
        return check_image_uki.findings(buf)

    def test_refuses_a_pe_with_a_linux_section_in_the_bootupd_payload(self):
        found = self.findings(
            {
                "usr/lib/bootupd/updates/EFI/fedora/uki.efi": fake_pe(
                    ".text", ".linux", ".initrd"
                )
            }
        )
        self.assertEqual(len(found), 1)

    def test_accepts_shim_and_grub(self):
        files = {
            "usr/lib/bootupd/updates/EFI/fedora/shimx64.efi": fake_pe(
                ".text", ".data", ".sbat"
            ),
            "usr/lib/bootupd/updates/EFI/fedora/grubx64.efi": fake_pe(
                ".text", ".mods", ".sbat"
            ),
        }
        self.assertEqual(self.findings(files), [])

    def test_refuses_an_efi_under_efi_linux_by_name(self):
        self.assertEqual(len(self.findings({"boot/efi/EFI/Linux/a.efi": b"x"})), 1)

    def test_refuses_a_uki_in_a_kernel_directory(self):
        found = self.findings(
            {"usr/lib/modules/7.0/vmlinuz-virt.efi": fake_pe(".linux")}
        )
        self.assertEqual(len(found), 1)

    def test_fails_closed_on_a_header_past_the_cap(self):
        head = bytearray(fake_pe(".text"))
        struct.pack_into("<I", head, 0x3C, 1 << 20)
        found = self.findings({"boot/x.efi": bytes(head)})
        self.assertEqual(len(found), 1)
        self.assertIn("past 64 KiB", found[0])

    def test_matches_efi_linux_in_any_case(self):
        self.assertEqual(len(self.findings({"./boot/efi/efi/linux/A.EFI": b"x"})), 1)

    def test_a_truncated_stream_exits_2(self):
        result = subprocess.run(
            [sys.executable, "-B", str(HERE.parent / "check_image_uki.py")],
            input=b"not a tar" * 100,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn(b"could not read", result.stderr)

    def test_accepts_an_empty_boot_content(self):
        self.assertEqual(self.findings({}), [])

    def script(self, files=None, **extra):
        """Run the script against a fake root holding files (path -> bytes); (result, stderr)."""
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d, "root")
            root.mkdir()
            for name, data in (files or {}).items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_bytes(data)
            stub = pathlib.Path(d, "bin", "podman")
            stub.parent.mkdir()
            stub.write_text(PODMAN)
            stub.chmod(0o755)
            env = {
                **os.environ,
                "PATH": f"{stub.parent}:{os.environ['PATH']}",
                "FAKE_ROOT": str(root),
                **extra,
            }
            return subprocess.run(
                ["bash", str(SCRIPT), "image"], env=env, capture_output=True, text=True
            )

    def test_the_script_refuses_a_uki_under_the_kernel_directory(self):
        result = self.script(
            {"usr/lib/modules/7.0/vmlinuz-virt.efi": fake_pe(".linux")}
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("carries a UKI", result.stderr)

    def test_the_script_refuses_a_uki_on_the_esp(self):
        result = self.script({"boot/efi/EFI/Linux/a.efi": fake_pe(".linux")})
        self.assertIn("carries a UKI", result.stderr)

    def test_the_script_passes_a_clean_root_to_the_kernel_check(self):
        result = self.script({"usr/lib/modules/7.0/other.efi": fake_pe(".text")})
        self.assertNotIn("UKI", result.stderr)
        self.assertNotIn("could not be read", result.stderr)

    def test_the_script_names_an_unreadable_stream(self):
        result = self.script(FAKE_GARBAGE="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("could not be read (reader status 2)", result.stderr)

    def test_the_script_names_a_failing_podman(self):
        result = self.script(FAKE_PODMAN_RC="125")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("podman failed (status 125)", result.stderr)


if __name__ == "__main__":
    unittest.main()
