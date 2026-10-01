"""Unit test of the two bump groups of bump.py (python3 -B -m unittest discover -s forge/specs/azoth/tests -v)."""

import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

AZOTH = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(AZOTH))
import bump  # noqa: E402

OLD, NEW = "sha256:" + "a" * 64, "sha256:" + "b" * 64
FEDORA = "registry.fedoraproject.org/fedora:43"
ATOMIC = "quay.io/fedora-ostree-desktops/base-atomic:43"
PINS = """KERNEL_CHANNEL=stable
FEDORA_KERNEL_NVR=7.2.5-100.fc43
CACHYOS_RELEASE=cachyos-7.2.5-1
CACHYOS_CONFIG_COMMIT={c}
CACHYOS_PATCHES_COMMIT={p}
NVIDIA_OPEN_VERSION=615.71.09
NVIDIA_OPEN_COMMIT={o}
NVIDIA_LEGACY_VERSION=580.178.04
""".format(c="c" * 40, p="1" * 40, o="0" * 40)


class BumpGroups(unittest.TestCase):
    """A fake checkout: the open lock was republished at its pin (a system change), the legacy
    pin moves to a packaged release (a kernel change), the kernel pair moves to 7.2.8 and
    every base image has a new digest."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        self.pins = root / "pins.env"
        self.pins.write_text(PINS)
        self.kernel_md = root / "KERNEL.md"
        self.kernel_md.write_text("<!-- pins:begin -->\nold\n<!-- pins:end -->\n")
        self.kernel_cfs = []
        for name in ("builder", "boot", "nvidia"):
            cf = root / f"{name}.Containerfile"
            cf.write_text(f"FROM {FEDORA}@{OLD}\n")
            self.kernel_cfs.append(cf)
        self.system_cf = root / "system.Containerfile"
        self.system_cf.write_text(
            f"FROM {FEDORA}@{OLD} AS nvidia-rpms\nFROM {ATOMIC}@{OLD} AS system\n"
        )
        self.generated = []
        patches = {
            "PINS": self.pins,
            "KERNEL_MD": self.kernel_md,
            "GROUP_CONTAINERFILES": {
                "kernel": self.kernel_cfs,
                "system": [self.system_cf],
            },
            "lock_py": self.lock_py,
            "kernel_pair": lambda pins, notes: (
                "7.2.8",
                "7.2.8-100.fc43",
                "cachyos-7.2.8-1",
                "2026-09-30T00:00:00Z",
            ),
            "head_commit": lambda repo, path, until=None: (
                "c" * 40 if until else "2" * 40
            ),
            "maintained_series": lambda: {"7.2", "6.18"},
            "nvidia_open": lambda current: (current, "0" * 40),
            "image_digest": lambda image, tag: NEW,
        }
        for name, value in patches.items():
            patcher = mock.patch.object(bump, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def lock_py(self, command, branch, *rest, allowed=(0,)):
        code, stdout = 0, ""
        if command == "latest":
            stdout = "580.190.01\n"
        elif command == "verify" and branch == "open":
            code = bump.LOCK_STALE
        elif command == "generate":
            self.generated.append((branch, rest[-1]))
        return subprocess.CompletedProcess([], code, stdout=stdout, stderr="")

    def snapshot(self):
        return {
            p: p.read_text()
            for p in [self.pins, self.kernel_md, *self.kernel_cfs, self.system_cf]
        }

    def apply(self, group):
        before = self.snapshot()
        result = bump.compute(group)
        bump.apply(result)
        after = self.snapshot()
        return result, {p for p in before if before[p] != after[p]}

    def test_kernel_group_touches_only_its_files(self):
        result, touched = self.apply("kernel")
        self.assertEqual(touched, {self.pins, self.kernel_md, *self.kernel_cfs})
        self.assertIn("FEDORA_KERNEL_NVR=7.2.8-100.fc43", self.pins.read_text())
        self.assertEqual(result["locks"], {"legacy": "580.190.01"})
        self.assertEqual(self.generated, [("legacy", "580.190.01")])

    def test_system_group_touches_only_its_files(self):
        result, touched = self.apply("system")
        self.assertEqual(touched, {self.system_cf})
        self.assertEqual(self.system_cf.read_text().count(NEW), 2)
        self.assertEqual(result["new"], {})
        self.assertEqual(result["locks"], {"open": "615.71.09"})
        self.assertEqual(self.generated, [("open", "615.71.09")])

    def test_nvidia_pins_alone_leave_the_kernel_base_images_for_the_next_run(self):
        pair = ("7.2.5", "7.2.5-100.fc43", "cachyos-7.2.5-1", "2026-09-30T00:00:00Z")
        with (
            mock.patch.object(bump, "kernel_pair", lambda pins, notes: pair),
            mock.patch.object(bump, "head_commit", lambda repo, path, until=None: "c" * 40 if until else "1" * 40),
        ):
            result, touched = self.apply("kernel")
        self.assertEqual(result["new"], {"NVIDIA_LEGACY_VERSION": "580.190.01"})
        self.assertEqual(result["images"], {})
        self.assertEqual(touched, {self.pins, self.kernel_md})

    def test_a_ref_at_two_digests_across_groups_is_not_an_error(self):
        self.apply("kernel")
        # fedora:43 is now at NEW in the kernel group and still at OLD in the system group.
        result = bump.compute("system")
        self.assertEqual(result["images"][FEDORA], {"old": OLD, "new": NEW})

    def test_a_ref_at_two_digests_within_a_group_is_an_error(self):
        self.kernel_cfs[0].write_text(f"FROM {FEDORA}@{NEW}\n")
        with self.assertRaises(SystemExit):
            bump.compute("kernel")

    def test_the_command_line_requires_a_group(self):
        for argv in (["apply"], ["check", "--group"], ["apply", "--group", "all"]):
            with (
                self.subTest(argv=argv),
                mock.patch.object(sys, "argv", ["bump.py", *argv]),
            ):
                with self.assertRaises(SystemExit) as raised:
                    bump.main()
                self.assertIn("--group kernel|system", str(raised.exception.code))


if __name__ == "__main__":
    unittest.main()
