"""check_elf_hardening.py fails a /usr/bin/athanor-* executable that is not a PIE, lacks full
RELRO or immediate binding, or was built without frame pointers, and build_spec.sh runs it on
every RPM it builds (#1350; python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import importlib.util
import io
import os
import pathlib
import shutil
import struct
import subprocess
import tempfile
import unittest
from unittest import mock

SCRIPTS = pathlib.Path(__file__).resolve().parents[1]
_spec = importlib.util.spec_from_file_location(
    "check_elf_hardening", SCRIPTS / "check_elf_hardening.py"
)
check = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(check)

# A CIE as compilers emit it: version 1, augmentation "zR", code alignment 1, data alignment -8,
# return address in r16, FDE pointers pc-relative sdata4 (0x1b), CFA at rsp+8, ra at cfa-8.
CIE_BODY = b"\x00\x00\x00\x00\x01zR\x00\x01\x78\x10\x01\x1b\x0c\x07\x08\x90\x01\x00\x00"
# The prologue `push %rbp; mov %rsp,%rbp`: CFA offset 16, rbp saved at cfa-16, CFA on rbp.
WITH_FRAME_POINTER = b"\x41\x0e\x10\x86\x02\x43\x0d\x06"
# A function that only moves rsp.
WITHOUT_FRAME_POINTER = b"\x41\x0e\x10\x00"


def eh_frame(fdes):
    """An .eh_frame of one CIE and one FDE per entry of fdes, True for a frame pointer."""
    out = struct.pack("<I", len(CIE_BODY)) + CIE_BODY
    for frame_pointer in fdes:
        body = struct.pack("<I", len(out) + 4)  # back to the CIE, from this field
        body += (
            struct.pack("<iI", 0, 16) + b"\x00"
        )  # pc_begin, pc_range, no augmentation data
        body += WITH_FRAME_POINTER if frame_pointer else WITHOUT_FRAME_POINTER
        body += b"\x00" * (-len(body) % 4)
        out += struct.pack("<I", len(body)) + body
    return out + b"\x00" * 4


def elf(
    e_type=3, interp=True, relro=True, flags=0x8, flags_1=0, fdes=(True,) * 9 + (False,)
):
    """The bytes of a minimal x86-64 ELF file with the given properties."""
    interp_bytes = b"/lib64/ld-linux-x86-64.so.2\x00"
    dynamic = b""
    if flags:
        dynamic += struct.pack("<qQ", 30, flags)
    if flags_1:
        dynamic += struct.pack("<qQ", 0x6FFFFFFB, flags_1)
    dynamic += struct.pack("<qQ", 0, 0)
    frames = eh_frame(fdes) if fdes is not None else b""
    names = b"\x00.eh_frame\x00.shstrtab\x00"
    segments = [(3, interp_bytes)] if interp else []
    segments.append((2, dynamic))
    if relro:
        segments.append((0x6474E552, b""))
    blobs_at = 64 + 56 * len(segments)
    payload, phdrs, at = b"", b"", blobs_at
    for p_type, blob in segments:
        phdrs += struct.pack(
            "<IIQQQQQQ", p_type, 4, at, at, at, len(blob), len(blob), 8
        )
        payload += blob
        at += len(blob)
    frames_at, names_at = at, at + len(frames)
    payload += frames + names
    shoff = blobs_at + len(payload)
    shdrs = b"\x00" * 64
    if fdes is not None:
        shdrs += struct.pack(
            "<IIQQQQIIQQ", 1, 1, 2, 0, frames_at, len(frames), 0, 0, 8, 0
        )
    shdrs += struct.pack("<IIQQQQIIQQ", 11, 3, 0, 0, names_at, len(names), 0, 0, 1, 0)
    count = len(shdrs) // 64
    header = b"\x7fELF\x02\x01\x01" + b"\x00" * 9
    header += struct.pack(
        "<HHIQQQIHHHHHH",
        e_type,
        62,
        1,
        0,
        64,
        shoff,
        0,
        64,
        56,
        len(segments),
        64,
        count,
        count - 1,
    )
    return header + phdrs + payload + shdrs


class ProblemsTest(unittest.TestCase):
    def test_hardened_executable_passes(self):
        self.assertEqual(check.problems(elf()), [])

    def test_df_1_now_alone_is_immediate_binding(self):
        self.assertEqual(check.problems(elf(flags=0, flags_1=0x1)), [])

    def test_not_pie(self):
        found = check.problems(elf(e_type=2))
        self.assertEqual(len(found), 1)
        self.assertIn("position-independent", found[0])

    def test_shared_object_without_interpreter_is_not_an_executable(self):
        found = check.problems(elf(interp=False))
        self.assertEqual(len(found), 1)
        self.assertIn("PT_INTERP", found[0])

    def test_no_relro(self):
        found = check.problems(elf(relro=False))
        self.assertEqual(len(found), 1)
        self.assertIn("PT_GNU_RELRO", found[0])

    def test_lazy_binding(self):
        found = check.problems(elf(flags=0))
        self.assertEqual(len(found), 1)
        self.assertIn("immediate binding", found[0])

    def test_frame_pointers_below_the_share(self):
        found = check.problems(elf(fdes=(True,) * 4 + (False,) * 6))
        self.assertEqual(len(found), 1)
        self.assertIn("frame pointers in 4 of 10", found[0])

    def test_frame_pointers_at_the_share(self):
        self.assertEqual(check.problems(elf(fdes=(True,) * 5 + (False,) * 5)), [])

    def test_no_eh_frame(self):
        found = check.problems(elf(fdes=None))
        self.assertEqual(
            found, ["no .eh_frame section to read the frame pointers from"]
        )

    def test_not_elf(self):
        with self.assertRaises(check.ElfError):
            check.problems(b"#!/bin/sh\n")

    def test_unknown_call_frame_instruction_is_an_error_not_a_pass(self):
        frames = eh_frame((True,)).replace(
            WITH_FRAME_POINTER, b"\x3f" + WITH_FRAME_POINTER[1:]
        )
        with self.assertRaises(check.ElfError):
            check.frame_pointer_share(frames)


@unittest.skipUnless(shutil.which("cpio"), "cpio is not on PATH")
class MainTest(unittest.TestCase):
    """main() on an RPM, through the real cpio: rpm2cpio is a stand-in on PATH that prints the
    cpio archive the test passes as the RPM."""

    def setUp(self):
        self.dir = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        fake = bin_dir / "rpm2cpio"
        fake.write_text('#!/bin/sh\nexec cat "$1"\n')
        fake.chmod(0o755)
        patcher = mock.patch.dict(
            os.environ, {"PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}"}
        )
        patcher.start()
        self.addCleanup(patcher.stop)

    def rpm(self, files):
        """A cpio archive of files, {path under the root: bytes}, standing for an RPM."""
        tree = self.dir / "tree"
        for name, data in files.items():
            (tree / name).parent.mkdir(parents=True, exist_ok=True)
            (tree / name).write_bytes(data)
        listing = "".join(f"./{name}\n" for name in files)
        archive = subprocess.run(
            ["cpio", "-o", "-H", "newc", "--quiet"],
            cwd=tree,
            input=listing.encode(),
            check=True,
            capture_output=True,
        ).stdout
        out = self.dir / "package.rpm"
        out.write_bytes(archive)
        return str(out)

    def test_an_rpm_without_athanor_executables_passes(self):
        rpm = self.rpm({"usr/bin/matugen": elf(e_type=2), "usr/share/doc/x": b"x"})
        self.assertEqual(check.main([rpm]), 0)

    def test_a_weak_athanor_executable_fails_and_a_script_is_skipped(self):
        rpm = self.rpm(
            {
                "usr/bin/athanor-bar": elf(relro=False),
                "usr/bin/athanor-session": b"#!/bin/sh\n",
            }
        )
        with mock.patch("sys.stderr", new_callable=io.StringIO) as err:
            self.assertEqual(check.main([rpm]), 1)
        self.assertIn("/usr/bin/athanor-bar: no PT_GNU_RELRO", err.getvalue())
        self.assertNotIn("athanor-session", err.getvalue())


@unittest.skipUnless(shutil.which("rustc"), "rustc is not on PATH")
class BuiltBothWaysTest(unittest.TestCase):
    """A program rustc links both ways. The frame-pointer share of so small a program is the
    standard library's, which differs between toolchains, so only the link properties are
    asserted here; the share's threshold is calibrated on real binaries (the module's text)."""

    def build(self, *flags):
        out = pathlib.Path(
            self.dir.name, f"prog{len(list(pathlib.Path(self.dir.name).iterdir()))}"
        )
        subprocess.run(
            [
                "rustc",
                "-O",
                "-C",
                "strip=symbols",
                *flags,
                "-o",
                str(out),
                str(self.source),
            ],
            check=True,
            capture_output=True,
        )
        return out.read_bytes()

    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.source = pathlib.Path(self.dir.name, "prog.rs")
        self.source.write_text(
            'fn main() { println!("{}", std::env::args().count()); }\n'
        )

    def test_hardened_build_passes_its_link_checks(self):
        found = check.problems(
            self.build(
                "-C",
                "relocation-model=pic",
                "-C",
                "force-frame-pointers=yes",
                "-C",
                "link-arg=-Wl,-z,relro,-z,now",
            )
        )
        self.assertEqual([f for f in found if "frame pointers" not in f], [])

    def test_weak_build_fails_them(self):
        found = check.problems(
            self.build(
                "-C",
                "relocation-model=static",
                "-C",
                "link-arg=-Wl,-z,norelro",
                "-C",
                "link-arg=-Wl,-z,lazy",
            )
        )
        self.assertTrue(any("position-independent" in f for f in found), found)
        self.assertTrue(any("PT_GNU_RELRO" in f for f in found), found)
        self.assertTrue(any("immediate binding" in f for f in found), found)


if __name__ == "__main__":
    unittest.main()
