"""Unit test of forge/specs/azoth/sign-kernel.sh (python3 -B -m unittest discover -s forge/specs/azoth/tests -v).

Every key here is a throwaway one generated in a temporary directory. The vmlinuz cases need
sbsigntools and a readable vmlinuz of the host (Fedora installs one under /usr/lib/modules):
they skip where either is missing, so on the GitHub runner the real signature is proved by the
sign and publish jobs of NVIDIA kmod instead.
"""

import glob
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "sign-kernel.sh"
KVER = "6.18.38-1.azoth.fc43.x86_64"
HOST_VMLINUZ = sorted(
    p for p in glob.glob("/usr/lib/modules/*/vmlinuz") if os.access(p, os.R_OK)
)


def cert(directory, cn):
    """A throwaway self-signed certificate and its private key, as (key, cert) paths."""
    key, crt = directory / f"{cn}.priv", directory / f"{cn}.crt"
    subprocess.run(
        [
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
        ],
        check=True,
        capture_output=True,
    )
    return key, crt


class SignKernel(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(
            tempfile.mkdtemp(dir="/var/tmp" if os.access("/var/tmp", os.W_OK) else None)
        )
        self.addCleanup(shutil.rmtree, self.tmp)
        self.bin = self.tmp / "bin"
        self.bin.mkdir()

    def stub(self, name, body):
        path = self.bin / name
        path.write_text("#!/usr/bin/env bash\nset -euo pipefail\n" + body)
        path.chmod(0o755)

    def run_script(self, *args, env=None):
        environment = dict(
            os.environ, PATH=f"{self.bin}:{os.environ['PATH']}", **(env or {})
        )
        return subprocess.run(
            ["bash", SCRIPT, *map(str, args)],
            capture_output=True,
            text=True,
            env=environment,
        )

    @staticmethod
    def archive(path, files):
        """A newc cpio archive with the ./ names of an RPM payload: the rpm2cpio stub is cat."""
        data = b""

        def entry(name, body, mode):
            nonlocal data
            name = name.encode() + b"\0"
            fields = [0, mode, 0, 0, 1, 0, len(body), 0, 0, 0, 0, len(name), 0]
            data += b"070701" + b"".join(b"%08X" % f for f in fields) + name
            data += b"\0" * (-len(data) % 4) + body
            data += b"\0" * (-len(data) % 4)

        for name, content in files.items():
            body = content if isinstance(content, bytes) else content.encode()
            entry(f"./{name}", body, 0o100644)
        entry("TRAILER!!!", b"", 0)
        pathlib.Path(path).write_bytes(data)

    def rpms(self, hash_line):
        self.stub("rpm2cpio", 'cat "$1"\n')
        (self.tmp / "kernel").mkdir()
        (self.tmp / "devel").mkdir()
        self.archive(
            self.tmp / f"kernel/kernel-core-{KVER}.rpm",
            {f"lib/modules/{KVER}/vmlinuz": "MZ kernel"},
        )
        self.archive(
            self.tmp / f"devel/kernel-devel-{KVER}.rpm",
            {f"usr/src/kernels/{KVER}/.config": f"CONFIG_X=y\n{hash_line}\n"},
        )

    def test_prepare_extracts_vmlinuz_kver_and_the_module_hash(self):
        self.rpms('CONFIG_MODULE_SIG_HASH="sha512"')
        out = self.tmp / "out"
        result = self.run_script(
            "prepare",
            "--kernel",
            self.tmp / "kernel",
            "--devel",
            self.tmp / "devel",
            "--out",
            out,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((out / "vmlinuz").read_text(), "MZ kernel")
        self.assertEqual((out / "kver").read_text(), f"{KVER}\n")
        self.assertEqual((out / "module-sig-hash").read_text(), "sha512\n")

    def test_prepare_refuses_a_hash_outside_the_kernel_list(self):
        self.rpms('CONFIG_MODULE_SIG_HASH="md5; touch /tmp/x"')
        result = self.run_script(
            "prepare",
            "--kernel",
            self.tmp / "kernel",
            "--devel",
            self.tmp / "devel",
            "--out",
            self.tmp / "out",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("CONFIG_MODULE_SIG_HASH", result.stderr)
        self.assertFalse((self.tmp / "out").exists())

    def test_prepare_refuses_two_kernel_core_rpms(self):
        self.rpms('CONFIG_MODULE_SIG_HASH="sha512"')
        shutil.copy(
            self.tmp / f"kernel/kernel-core-{KVER}.rpm",
            self.tmp / "kernel/kernel-core-other.rpm",
        )
        result = self.run_script(
            "prepare",
            "--kernel",
            self.tmp / "kernel",
            "--devel",
            self.tmp / "devel",
            "--out",
            self.tmp / "out",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("found 2", result.stderr)

    def modinfo(self, signer="Athanor test modules"):
        """modinfo: the vermagic a module names in its first line (else KVER's), and SIGNER."""
        self.stub(
            "modinfo",
            f'if [[ $2 == vermagic ]]; then v=$(head -n 1 "$3"); [[ $v == vermagic=* ]] && echo "${{v#vermagic=}}" || echo "{KVER} SMP preempt mod_unload"; '
            f'else echo "{signer}"; fi\n',
        )

    def nvidia_tree(self, root, branches=("open", "legacy")):
        """The tree nvidia.sh build writes under --out, one module set per branch."""
        for branch in branches:
            nvidia = root / branch / "lib/modules" / KVER / "extra/nvidia"
            nvidia.mkdir(parents=True)
            for name in ("nvidia", "nvidia-drm", "nvidia-modeset", "nvidia-uvm"):
                (nvidia / f"{name}.ko").write_text("ko")
            (root / branch / "kver").write_text(f"{KVER}\n")
            (root / branch / "version").write_text("580.95.05\n")
            (root / f"{branch}-build.log").write_text("log\n")
        return root

    def check_modules(self, root):
        return self.run_script("check-modules", "--kver", KVER, "--dir", root)

    def test_check_modules_accepts_the_tree_of_nvidia_sh(self):
        self.modinfo()
        result = self.check_modules(self.nvidia_tree(self.tmp / "out"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("8 modules of", result.stdout)

    def test_check_modules_refuses_anything_outside_the_allow_list(self):
        self.modinfo()
        nvidia = f"open/lib/modules/{KVER}/extra/nvidia"
        cases = {
            "another module": (f"{nvidia}/evil.ko", "ko"),
            "a module of another kernel": ("open/lib/modules/6.1.0-1.fc43.x86_64/extra/nvidia/nvidia.ko", "ko"),
            "a module outside extra/nvidia": (f"open/lib/modules/{KVER}/kernel/nvidia.ko", "ko"),
            "a script beside the modules": ("open/run.sh", "echo"),
            "a third branch": (f"beta/lib/modules/{KVER}/extra/nvidia/nvidia.ko", "ko"),
            "a vermagic of another kernel": (f"{nvidia}/nvidia-peermem.ko", "vermagic=6.1.0-1.fc43.x86_64 SMP\n"),
            "a kver of another kernel": ("legacy/kver", "6.1.0-1.fc43.x86_64\n"),
        }
        for case, (path, content) in cases.items():
            with self.subTest(case):
                root = self.nvidia_tree(pathlib.Path(tempfile.mkdtemp(dir=self.tmp)))
                (root / path).parent.mkdir(parents=True, exist_ok=True)
                (root / path).write_text(content)
                result = self.check_modules(root)
                self.assertNotEqual(result.returncode, 0, case)
                self.assertIn("sign-kernel:", result.stderr)

    def test_check_modules_refuses_a_symlink_and_a_branch_without_nvidia_ko(self):
        self.modinfo()
        nvidia = pathlib.Path(f"open/lib/modules/{KVER}/extra/nvidia")
        root = self.nvidia_tree(self.tmp / "link")
        (root / nvidia / "nvidia-uvm.ko").unlink()
        (root / nvidia / "nvidia-uvm.ko").symlink_to("/etc/passwd")
        result = self.check_modules(root)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("is a symlink", result.stderr)
        root = self.nvidia_tree(self.tmp / "missing")
        (root / nvidia / "nvidia.ko").unlink()
        result = self.check_modules(root)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("nvidia.ko is missing", result.stderr)

    def modules(self, signer, hash_name="sha512", tree=None):
        key, crt = cert(self.tmp, "Athanor test modules")
        signed = tree or self.nvidia_tree(self.tmp / "signed", branches=("open",))
        ko = signed / "open/lib/modules" / KVER / "extra/nvidia/nvidia.ko"
        (self.tmp / "hash").write_text(f"{hash_name}\n")
        self.stub("sign-file", 'printf "%s|%s|%s" "$1" "${2##*/}" "${3##*/}" >> "$4"\n')
        self.modinfo(signer)
        result = self.run_script(
            "modules",
            "--key",
            key,
            "--cert",
            crt,
            "--hash",
            self.tmp / "hash",
            "--kver",
            KVER,
            "--dir",
            signed,
            env={"SIGN_FILE": str(self.bin / "sign-file")},
        )
        return result, ko

    def test_modules_signs_every_module_with_the_hash_and_checks_the_signer(self):
        result, ko = self.modules("Athanor test modules")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            ko.read_text(),
            "kosha512|Athanor test modules.priv|Athanor test modules.crt",
        )

    def test_modules_fails_when_modinfo_names_another_signer(self):
        result, _ = self.modules("Someone else")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            'signer "Someone else", expected "Athanor test modules"', result.stderr
        )

    def test_modules_signs_nothing_outside_the_allow_list(self):
        tree = self.nvidia_tree(self.tmp / "signed", branches=("open",))
        (tree / "open/payload.ko").write_text("ko")
        result, ko = self.modules("Athanor test modules", tree=tree)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("open/payload.ko", result.stderr)
        self.assertEqual(ko.read_text(), "ko")
        self.assertEqual((tree / "open/payload.ko").read_text(), "ko")

    def test_modules_refuses_a_hash_outside_the_kernel_list(self):
        result, ko = self.modules("Athanor test modules", hash_name="md5")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(ko.read_text(), "ko")

    def test_modules_fails_without_a_module(self):
        key, crt = cert(self.tmp, "Athanor test modules")
        (self.tmp / "hash").write_text("sha512\n")
        (self.tmp / "empty").mkdir()
        result = self.run_script(
            "modules",
            "--key",
            key,
            "--cert",
            crt,
            "--hash",
            self.tmp / "hash",
            "--kver",
            KVER,
            "--dir",
            self.tmp / "empty",
            env={"SIGN_FILE": "/bin/false"},
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no module", result.stderr)

    @unittest.skipUnless(
        shutil.which("sbsign") and HOST_VMLINUZ,
        "needs sbsigntools and a readable host vmlinuz",
    )
    def test_vmlinuz_replaces_every_signature_with_the_project_one(self):
        key, crt = cert(self.tmp, "Athanor test Secure Boot")
        unsigned = self.tmp / "unsigned"
        unsigned.mkdir()
        shutil.copy(HOST_VMLINUZ[-1], unsigned / "vmlinuz")
        (unsigned / "kver").write_text(f"{KVER}\n")
        signed = self.tmp / "signed"
        result = self.run_script(
            "vmlinuz", "--key", key, "--cert", crt, "--in", unsigned, "--out", signed
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        listing = subprocess.run(
            ["sbverify", "--list", signed / "vmlinuz"], capture_output=True, text=True
        ).stdout
        self.assertEqual(
            listing.count("\nsignature ") + listing.startswith("signature "), 1, listing
        )
        self.assertIn("CN=Athanor test Secure Boot", listing)
        self.assertEqual((signed / "kver").read_text(), f"{KVER}\n")
        self.stub("rpm2cpio", 'cat "$1"\n')
        rpm = pathlib.Path(HOST_VMLINUZ[-1]).read_bytes()
        kernel = self.tmp / "kernel"
        kernel.mkdir()
        self.archive(kernel / f"kernel-core-{KVER}.rpm", {f"lib/modules/{KVER}/vmlinuz": rpm})
        verified = self.run_script("verify", "--cert", crt, "--dir", signed, "--kernel", kernel)
        self.assertEqual(verified.returncode, 0, verified.stderr)

        _, other = cert(self.tmp, "Another key")
        refused = self.run_script("verify", "--cert", other, "--dir", signed, "--kernel", kernel)
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("does not verify", refused.stderr)

        # A vmlinuz signed with the right key that is not the RPM's: one byte in the middle.
        middle = len(rpm) // 2
        self.archive(
            kernel / f"kernel-core-{KVER}.rpm",
            {f"lib/modules/{KVER}/vmlinuz": rpm[:middle] + bytes([rpm[middle] ^ 1]) + rpm[middle + 1 :]},
        )
        refused = self.run_script("verify", "--cert", crt, "--dir", signed, "--kernel", kernel)
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("without its signature is not", refused.stderr)

        (signed / "kver").write_text("6.1.0-1.fc43.x86_64\n")
        self.archive(kernel / f"kernel-core-{KVER}.rpm", {f"lib/modules/{KVER}/vmlinuz": rpm})
        refused = self.run_script("verify", "--cert", crt, "--dir", signed, "--kernel", kernel)
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn("the kernel-core RPM is", refused.stderr)

    @unittest.skipUnless(
        shutil.which("sbverify") and HOST_VMLINUZ,
        "needs sbsigntools and a readable host vmlinuz",
    )
    def test_verify_refuses_a_vmlinuz_with_a_foreign_signature(self):
        _, crt = cert(self.tmp, "Athanor test Secure Boot")
        shutil.copy(HOST_VMLINUZ[-1], self.tmp / "vmlinuz")
        result = self.run_script("verify", "--cert", crt, "--dir", self.tmp, "--kernel", self.tmp)
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
