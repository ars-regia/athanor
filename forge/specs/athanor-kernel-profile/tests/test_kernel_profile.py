"""Unit tests of kernel_profile.py: composition rules, role combinations and generation.

Run: python3 -B -m unittest discover -s forge/specs/athanor-kernel-profile/tests -v
"""

import io
import json
import pathlib
import sys
import tempfile
import textwrap
import tomllib
import unittest
from contextlib import redirect_stderr, redirect_stdout

PACKAGE = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PACKAGE))
import kernel_profile as kp  # noqa: E402


def manifest(text: str) -> dict:
    return tomllib.loads(textwrap.dedent(text))


def value(settings: dict, kind: str, name: str):
    return settings[(kind, name)]["value"]


def run_tool(*args: str) -> tuple[int, str]:
    out, err = io.StringIO(), io.StringIO()
    with redirect_stdout(out), redirect_stderr(err):
        code = kp.main(list(args))
    return code, out.getvalue() + err.getvalue()


class Composition(unittest.TestCase):
    def test_base_only_manifest_has_one_combination(self) -> None:
        m = manifest(
            """
            schema = 1
            [base.sysctl]
            "kernel.kptr_restrict" = { value = "2", decision = "D47", locked = true }
            """
        )
        allowed, refused = kp.combinations(m)
        self.assertEqual(allowed, [()])
        self.assertEqual(refused, [])
        self.assertEqual(value(kp.effective(m, ()), "sysctl", "kernel.kptr_restrict"), "2")

    def test_role_overrides_an_unlocked_base_setting(self) -> None:
        m = manifest(
            """
            schema = 1
            [base.sysctl]
            "kernel.sysrq" = { value = 176, decision = "section 6" }
            [roles.mesh.sysctl]
            "kernel.sysrq" = { value = 0, decision = "section 7" }
            """
        )
        self.assertEqual(value(kp.effective(m, ()), "sysctl", "kernel.sysrq"), 176)
        mesh = kp.effective(m, ("mesh",))
        self.assertEqual(value(mesh, "sysctl", "kernel.sysrq"), 0)
        self.assertEqual(mesh[("sysctl", "kernel.sysrq")]["decision"], "section 7")

    def test_locked_base_setting_cannot_be_overridden(self) -> None:
        m = manifest(
            """
            schema = 1
            [base.sysctl]
            "kernel.kptr_restrict" = { value = "2", decision = "D47", locked = true }
            [roles.desktop.sysctl]
            "kernel.kptr_restrict" = { value = "1", decision = "test" }
            """
        )
        with self.assertRaisesRegex(kp.ProfileError, "locked"):
            kp.effective(m, ("desktop",))

    def test_roles_with_different_scalars_conflict(self) -> None:
        m = manifest(
            """
            schema = 1
            [roles.desktop.sysctl]
            "vm.swappiness" = { value = 60, decision = "test" }
            [roles.laptop.sysctl]
            "vm.swappiness" = { value = 10, decision = "test" }
            """
        )
        with self.assertRaisesRegex(kp.ProfileError, "priority"):
            kp.effective(m, ("desktop", "laptop"))

    def test_higher_priority_resolves_a_conflict(self) -> None:
        m = manifest(
            """
            schema = 1
            [roles.desktop.sysctl]
            "vm.swappiness" = { value = 60, decision = "test", priority = 10 }
            [roles.laptop.sysctl]
            "vm.swappiness" = { value = 10, decision = "test" }
            """
        )
        self.assertEqual(value(kp.effective(m, ("desktop", "laptop")), "sysctl", "vm.swappiness"), 60)

    def test_declared_ordering_resolves_numbers(self) -> None:
        m = manifest(
            """
            schema = 1
            [roles.desktop.sysctl]
            "vm.max_map_count" = { value = 65530, decision = "test", ordering = "max" }
            [roles.laptop.sysctl]
            "vm.max_map_count" = { value = 1048576, decision = "test", ordering = "max" }
            """
        )
        self.assertEqual(
            value(kp.effective(m, ("desktop", "laptop")), "sysctl", "vm.max_map_count"), 1048576
        )

    def test_lists_merge_by_union(self) -> None:
        m = manifest(
            """
            schema = 1
            [base.runtime]
            lsm = { value = ["lockdown", "yama"], decision = "section 5" }
            [roles.desktop.runtime]
            lsm = { value = ["yama", "ipe"], decision = "test" }
            """
        )
        self.assertEqual(
            value(kp.effective(m, ("desktop",)), "runtime", "lsm"), ["lockdown", "yama", "ipe"]
        )

    def test_maps_merge_recursively_and_still_conflict(self) -> None:
        merged = manifest(
            """
            schema = 1
            [roles.desktop.runtime]
            power = { value = { ac = "balanced" }, decision = "test" }
            [roles.laptop.runtime]
            power = { value = { battery = "power-saver" }, decision = "test" }
            """
        )
        self.assertEqual(
            value(kp.effective(merged, ("desktop", "laptop")), "runtime", "power"),
            {"ac": "balanced", "battery": "power-saver"},
        )
        conflicting = manifest(
            """
            schema = 1
            [roles.desktop.runtime]
            power = { value = { ac = "balanced" }, decision = "test" }
            [roles.laptop.runtime]
            power = { value = { ac = "performance" }, decision = "test" }
            """
        )
        with self.assertRaisesRegex(kp.ProfileError, "priority"):
            kp.effective(conflicting, ("desktop", "laptop"))

    def test_fragment_shared_by_two_roles_is_not_a_conflict(self) -> None:
        m = manifest(
            """
            schema = 1
            [fragments.interactive.sysctl]
            "kernel.warn_limit" = { value = 0, decision = "D19" }
            [roles.desktop]
            includes = ["interactive"]
            [roles.laptop]
            includes = ["interactive"]
            """
        )
        self.assertEqual(
            value(kp.effective(m, ("desktop", "laptop")), "sysctl", "kernel.warn_limit"), 0
        )

    def test_rejected_combinations_are_not_generated(self) -> None:
        m = manifest(
            """
            schema = 1
            [roles.desktop]
            [roles.mesh]
            [rules]
            rejected = [["desktop", "mesh"]]
            """
        )
        allowed, refused = kp.combinations(m)
        self.assertEqual(allowed, [(), ("desktop",), ("mesh",)])
        self.assertEqual(refused, [("desktop", "mesh")])
        files, _ = kp.build(m)
        self.assertEqual(sorted(files), ["base.json", "desktop.json", "mesh.json"])

    def test_malformed_manifests_fail(self) -> None:
        cases = (
            ('[base.kernel]\n"x" = { value = 1, decision = "t" }', "unknown kind"),
            ('[base.sysctl]\n"x" = { value = 1 }', "decision"),
            ('[base.sysctl]\n"x" = { value = 1, decision = "t", colour = "red" }', "unknown fields"),
            ('[roles.desktop.sysctl]\n"x" = { value = 1, decision = "t", locked = true }', "only base"),
            ('[roles.desktop]\nincludes = ["nope"]', "unknown fragment"),
            ('[rules]\nrejected = [["desktop", "ghost"]]\n[roles.desktop]', "unknown roles"),
            ('[base]\nincludes = ["x"]', "includes"),
            ('[rules]\nrejected = [[]]\n', "at least two"),
            ('[rules]\nrejected = [["desktop"]]\n[roles.desktop]', "at least two"),
            ('[roles.desktop.cmdline]\nx = { value = "1", decision = "t" }', "base only"),
            ('[base.cmdline]\nx = { value = 1, decision = "t" }', "string value"),
            ('[base.cmdline]\n"x=y" = { value = "1", decision = "t" }', "string value"),
            ('[base.cmdline]\nx = { value = "a b", decision = "t" }', "without spaces"),
            ("[base.cmdline]\nx = { value = 'a\\\\b', decision = \"t\" }", "backslashes"),
            ('[base.cmdline]\nx = { value = "", decision = "t" }', "non-empty"),
        )
        for text, message in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(kp.ProfileError, message):
                    kp.build(manifest("schema = 1\n" + text))
        with self.assertRaisesRegex(kp.ProfileError, "schema"):
            kp.build(manifest("schema = 2\n"))


class Generation(unittest.TestCase):
    def test_generate_check_and_detect_stale_files(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            tmp = pathlib.Path(name)
            source = tmp / "profile.toml"
            source.write_text(
                'schema = 1\n[base.sysctl]\n"kernel.dmesg_restrict" = { value = "1", decision = "D47" }\n'
                '[base.cmdline]\nlockdown = { value = "integrity", decision = "t" }\n'
                "[roles.desktop]\n"
            )
            out = tmp / "out"
            paths = ("--manifest", str(source), "--out", str(out), "--kargs", str(tmp / "kargs.toml"),
                     "--boot-cmdline", str(tmp / "cmdline"))
            code, text = run_tool("generate", *paths)
            self.assertEqual(code, 0, text)
            self.assertEqual(sorted(p.name for p in out.glob("*.json")), ["base.json", "desktop.json"])
            base = json.loads((out / "base.json").read_text())
            self.assertEqual(base["settings"]["sysctl"]["kernel.dmesg_restrict"]["value"], "1")

            code, text = run_tool("check", *paths)
            self.assertEqual(code, 0, text)

            (out / "desktop.json").write_text("{}\n")
            code, text = run_tool("check", *paths)
            self.assertEqual(code, 1, text)
            self.assertIn("desktop.json", text)

    def test_cmdline_copies_are_generated_and_checked(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            tmp = pathlib.Path(name)
            source = tmp / "profile.toml"
            source.write_text(
                "schema = 1\n[base.cmdline]\n"
                'vsyscall = { value = "none", decision = "t" }\n'
                '"page_alloc.shuffle" = { value = "1", decision = "t" }\n'
            )
            kargs, boot = tmp / "kargs.d" / "10.toml", tmp / "cmdline"
            paths = ("--manifest", str(source), "--out", str(tmp / "out"), "--kargs", str(kargs),
                     "--boot-cmdline", str(boot))
            code, text = run_tool("generate", *paths)
            self.assertEqual(code, 0, text)
            document = tomllib.loads(kargs.read_text())
            self.assertEqual(document["kargs"], ["page_alloc.shuffle=1", "vsyscall=none"])
            self.assertEqual(document["match-architectures"], ["x86_64"])
            self.assertEqual(boot.read_text(), "page_alloc.shuffle=1 vsyscall=none\n")

            for path in (kargs, boot):
                with self.subTest(path=path.name):
                    good = path.read_text()
                    path.write_text(good.replace("vsyscall=none", "vsyscall=emulate"))
                    code, text = run_tool("check", *paths)
                    self.assertEqual(code, 1, text)
                    self.assertIn(str(path), text)
                    path.write_text(good)

    def test_cmdline_cannot_be_empty(self) -> None:
        with self.assertRaisesRegex(kp.ProfileError, "base command line is empty"):
            kp.command_lines(manifest("schema = 1\n"), pathlib.Path("k"), pathlib.Path("c"))

    def test_repository_manifest_is_valid_and_generated(self) -> None:
        code, text = run_tool("check")
        self.assertEqual(code, 0, text)
        self.assertIn("rejected combination desktop+mesh", text)


if __name__ == "__main__":
    unittest.main()
