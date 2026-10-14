"""Runs the installer's own scripts: the %pre that refuses a machine without UEFI or
x86-64-v3 and the %post that requests the MOK enrolment (system/athanor-install.ks,
doc_kernel_profile.md D3, D14 and section 4).

Each block is taken from the kickstart as it ships and run with bash, with the paths it
touches moved into a temporary directory and mokutil, chvt, fgconsole, mountpoint and
systemctl replaced by recording stubs. scripts/verify.py installer checks that the ISO's
kickstart and the acceptance test's carry the same blocks.

python3 -B -m unittest discover -s system/tests
"""

import hashlib
import importlib.util
import json
import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

SECTIONS = verify.kickstart_sections(
    verify.kickstart_text(ROOT, "system/athanor-install.ks")
)
PRE = next(s for s in SECTIONS if s.startswith("%pre"))
MOK = next(s for s in SECTIONS if "mokutil --import" in s)
ALPHABET = "bcdefghjknprstuvx"
SECURE_BOOT_VARIABLE = "SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c"

# Records every call, answers --test-key from a state file, and stands in for the hash with
# a digest of the password, which is enough to tell which password a request was made for.
MOKUTIL = textwrap.dedent(
    """\
    #!/usr/bin/env python3
    import hashlib, json, os, sys
    state_path = os.environ["MOK_STATE"]
    state = json.load(open(state_path))
    args = sys.argv[1:]
    with open(os.environ["MOK_LOG"], "a") as log:
        log.write(" ".join(args) + "\\n")
    if args[0] == "--test-key":
        if state.get("broken"):
            print("Failed to read efivars", file=sys.stderr)
            sys.exit(255)
        if state.get("enrolled"):
            print(f"{args[1]} is already enrolled")
        elif state.get("request"):
            print(f"{args[1]} is already in the enrollment request")
        else:
            print(f"{args[1]} is not enrolled")
            sys.exit(1)
    elif args[0] == "--revoke-import":
        state["request"] = None
    elif args[0] == "--generate-hash":
        first, second = sys.stdin.readline(), sys.stdin.readline()
        print("input password: ")
        print("input password again: ")
        if first != second:
            sys.exit(1)
        print("$6$stub$" + hashlib.sha256(first.strip().encode()).hexdigest())
    elif args[0] == "--timeout":
        state["timeout"] = args[1]
    elif args[0] == "--import":
        assert args[2:] == ["--hash-file", os.environ["MOK_DEV"] + "/stdin"], args
        # As mokutil does: a certificate already in the request is skipped, and the
        # request keeps the password it was made with.
        if state.get("request"):
            print(f"SKIP: {args[1]} is already in the enrollment request")
        else:
            state["request"] = sys.stdin.read()
    else:
        sys.exit(f"unexpected mokutil call {args}")
    json.dump(state, open(state_path, "w"))
    """
)


class Block:
    """One kickstart section turned into a script whose paths live under a temporary root."""

    def __init__(self, section, tmp, cpu_levels="x86-64-v3 (supported, searched)"):
        self.tmp = tmp
        self.dev = tmp / "dev"
        self.dev.mkdir()
        (tmp / "efi" / "efivars").mkdir(parents=True)
        (tmp / "efi" / "efivars" / SECURE_BOOT_VARIABLE).write_bytes(b"\x06\x00\x00\x00\x00")
        (tmp / "esp").mkdir()
        (tmp / "active").write_text("tty0 ttyS0\n")
        (tmp / "cert.der").write_bytes(b"certificate")
        # Letters outside the alphabet are interleaved, so tr has something to throw away.
        (self.dev / "urandom").write_bytes(b"AbqcLdw3emf!ghyjzk9np rsatuvxbcdefghjk")
        (tmp / "ld.so").write_text(
            f"#!/bin/sh\necho 'Subdirectories of glibc-hwcaps directories'\necho '  {cpu_levels}'\n"
        )
        bin_dir = tmp / "bin"
        bin_dir.mkdir()
        for name in ("chvt", "systemctl", "mountpoint"):
            (bin_dir / name).write_text(
                f'#!/bin/sh\necho "{name} $*" >> "{tmp}/calls"\nexit "${{{name.upper()}_STATUS:-0}}"\n'
            )
        (bin_dir / "fgconsole").write_text("#!/bin/sh\necho 6\n")
        (bin_dir / "mokutil").write_text(MOKUTIL)
        for f in [tmp / "ld.so", *bin_dir.iterdir()]:
            f.chmod(0o755)
        self.state({})
        body = "\n".join(section.splitlines()[1:-1]) + "\n"
        for old, new in (
            ("/sys/firmware/efi", tmp / "efi"),
            ("/sys/class/tty/console/active", tmp / "active"),
            ("/lib64/ld-linux-x86-64.so.2", tmp / "ld.so"),
            ("/boot/efi", tmp / "esp"),
            ("/usr/share/athanor/secureboot/athanor-secureboot.der", tmp / "cert.der"),
            ("/dev/", f"{self.dev}/"),
        ):
            body = body.replace(old, str(new))
        self.script = tmp / "block.sh"
        self.script.write_text(body)
        self.env = {
            **os.environ,
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "MOK_STATE": str(tmp / "mok.json"),
            "MOK_LOG": str(tmp / "mok.log"),
            "MOK_DEV": str(self.dev),
        }

    def state(self, value=None):
        path = self.tmp / "mok.json"
        if value is not None:
            path.write_text(json.dumps(value))
        return json.loads(path.read_text())

    def run(self, **env):
        return subprocess.run(
            ["bash", str(self.script)],
            env={**self.env, **env},
            capture_output=True,
            text=True,
            timeout=30,
        )

    def read(self, name):
        path = self.tmp / name
        return path.read_text() if path.exists() else ""


class MachineCheck(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.tmp = pathlib.Path(self.dir.name)

    def tearDown(self):
        self.dir.cleanup()

    def test_a_uefi_v3_machine_passes_quietly(self):
        block = Block(PRE, self.tmp)
        proc = block.run()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(block.read("calls"), "")
        self.assertEqual(block.read("dev/tty8"), "")

    def test_bios_boot_is_refused_on_screen_and_serial_then_powered_off(self):
        block = Block(PRE, self.tmp)
        shutil.rmtree(self.tmp / "efi")
        proc = block.run()
        self.assertEqual(proc.returncode, 1)
        for console in ("dev/tty8", "dev/ttyS0"):
            self.assertIn(
                "Athanor cannot be installed on this computer.", block.read(console)
            )
            self.assertIn("legacy BIOS mode", block.read(console))
        # tty0 is whichever terminal is in front: terminal 8 already carries the message.
        self.assertFalse((block.dev / "tty0").exists())
        self.assertEqual(block.read("calls"), "chvt 8\nsystemctl poweroff\n")

    def test_a_processor_without_v3_is_refused(self):
        block = Block(
            PRE, self.tmp, cpu_levels="x86-64-v3\n  x86-64-v2 (supported, searched)"
        )
        proc = block.run()
        self.assertEqual(proc.returncode, 1)
        self.assertIn("does not support x86-64-v3", block.read("dev/tty8"))
        self.assertIn("systemctl poweroff", block.read("calls"))

    def test_the_refusal_stands_when_the_power_off_does_not_happen(self):
        block = Block(PRE, self.tmp)
        shutil.rmtree(self.tmp / "efi")
        self.assertEqual(block.run(SYSTEMCTL_STATUS="1").returncode, 1)


class MokRequest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.tmp = pathlib.Path(self.dir.name)
        self.block = Block(MOK, self.tmp)

    def tearDown(self):
        self.dir.cleanup()

    def shown_password(self):
        found = re.findall(r"One-time password: ([a-z ]+)", self.block.read("dev/tty8"))
        self.assertEqual(len(found), 1, self.block.read("dev/tty8"))
        return found[0].replace(" ", "")

    def test_a_new_request_under_a_password_shown_only_on_the_screen(self):
        proc = self.block.run()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        password = self.shown_password()
        self.assertEqual(len(password), 16)
        self.assertTrue(set(password) <= set(ALPHABET), password)
        self.assertEqual(password, "bcdefghjknprstuv")
        self.assertIn(
            f"One-time password: {password[:4]} {password[4:8]}",
            self.block.read("dev/ttyS0"),
        )
        state = self.block.state()
        self.assertEqual(
            state["request"], "$6$stub$" + hashlib.sha256(password.encode()).hexdigest()
        )
        self.assertEqual(state["timeout"], "-1")
        # Anaconda logs what the script prints, and the calls are what ps would show.
        for where in (
            proc.stdout,
            proc.stderr,
            self.block.read("mok.log"),
            self.block.read("calls"),
        ):
            self.assertNotIn(password, where)
        self.assertEqual(
            (self.tmp / "esp/EFI/athanor/athanor-secureboot.der").read_bytes(),
            b"certificate",
        )
        self.assertEqual(
            self.block.read("calls"),
            "mountpoint -q " + str(self.tmp / "esp") + "\nchvt 8\nchvt 6\n",
        )

    def test_an_enrolled_certificate_needs_no_request(self):
        self.block.state({"enrolled": True})
        proc = self.block.run()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("already trusted", proc.stdout)
        self.assertNotIn("--import", self.block.read("mok.log"))
        self.assertEqual(self.block.read("dev/tty8"), "")

    def test_firmware_without_secure_boot_gets_no_request(self):
        # mokutil refuses every request there: "This system doesn't support Secure Boot".
        (self.tmp / "efi" / "efivars" / SECURE_BOOT_VARIABLE).unlink()
        proc = self.block.run()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("this firmware has no Secure Boot", proc.stdout)
        self.assertEqual(self.block.read("mok.log"), "")
        self.assertEqual(self.block.read("dev/tty8"), "")

    def test_an_image_without_the_certificate_says_so_and_installs(self):
        # An image installed from :stable before its first promotion that ships the DER.
        (self.tmp / "cert.der").unlink()
        proc = self.block.run()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("predates the Secure Boot key enrolment", proc.stdout)
        for console in ("dev/tty8", "dev/ttyS0"):
            self.assertIn("predates the Secure Boot key enrolment", self.block.read(console))
            self.assertNotIn("One-time password", self.block.read(console))
        self.assertEqual(self.block.read("mok.log"), "")
        self.assertEqual(self.block.read("calls"), "chvt 8\nchvt 6\n")
        self.assertFalse((self.tmp / "esp/EFI/athanor").exists())

    def test_a_request_left_by_an_earlier_install_is_replaced(self):
        self.block.state({"request": "$6$old$lost"})
        proc = self.block.run()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertIn("--revoke-import", self.block.read("mok.log"))
        password = self.shown_password()
        self.assertEqual(
            self.block.state()["request"],
            "$6$stub$" + hashlib.sha256(password.encode()).hexdigest(),
        )

    def test_an_unreadable_state_stops_the_install(self):
        self.block.state({"broken": True})
        proc = self.block.run()
        self.assertEqual(proc.returncode, 1)
        self.assertIn("exited 255", proc.stderr)
        self.assertNotIn("--import", self.block.read("mok.log"))

    def test_no_esp_stops_the_install_before_the_password_is_shown(self):
        proc = self.block.run(MOUNTPOINT_STATUS="1")
        self.assertEqual(proc.returncode, 1)
        self.assertIn(
            "/boot/efi is not mounted",
            proc.stderr.replace(str(self.tmp / "esp"), "/boot/efi"),
        )
        self.assertEqual(self.block.read("dev/tty8"), "")

    def test_too_little_randomness_stops_the_install(self):
        (self.block.dev / "urandom").write_bytes(b"bcdefgh")
        proc = self.block.run()
        self.assertEqual(proc.returncode, 1)
        self.assertIn("could not draw a one-time password", proc.stderr)


if __name__ == "__main__":
    unittest.main()
