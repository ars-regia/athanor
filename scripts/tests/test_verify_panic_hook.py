"""Unit tests of the panic-hook check of `verify.py`: every program of a shipped crate, src/main.rs,
src/bin/ or a [[bin]] path, starts `main` with the hook installer
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

HOOKED = "fn main() -> ExitCode {\n    athanor_unit::panic::install();\n    run()\n}\n"
LATE = "fn main() {\n    journal::init();\n    athanor_unit::panic::install();\n}\n"
COMMENTED = "fn main() {\n    // athanor_unit::panic::install();\n    run();\n}\n"


class PanicHookTest(unittest.TestCase):
    def problems(self, crates, exempt=""):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            members = ", ".join(f'"{name}"' for name in crates)
            (root / "Cargo.toml").write_text(f"[workspace]\nmembers = [{members}]\n")
            (root / "experimental").mkdir()
            (root / "experimental" / "EXEMPT").write_text(exempt)
            for name, (manifest, files) in crates.items():
                (root / name).mkdir()
                (root / name / "Cargo.toml").write_text(f'[package]\nname = "{name}"\n{manifest}')
                for path, text in files.items():
                    (root / name / path).parent.mkdir(parents=True, exist_ok=True)
                    (root / name / path).write_text(text)
            return sorted(p.split(":")[0] for p in verify.panic_hook_problems(root))

    def test_every_program_of_a_shipped_crate_starts_with_the_hook(self):
        crates = {
            "ok": ("", {"src/main.rs": HOOKED, "src/bin/two.rs": HOOKED}),
            "lib": ("", {"src/lib.rs": "pub fn f() {}\n"}),
            "late": ("", {"src/main.rs": LATE}),
            "comment": ("", {"src/main.rs": COMMENTED}),
            "bins": ('[[bin]]\nname = "t"\npath = "src/tool/main.rs"\n', {"src/tool/main.rs": "fn main() {}\n"}),
            "nested": ("", {"src/bin/three/main.rs": "fn main() {}\n"}),
            "missing": ('[[bin]]\nname = "m"\npath = "src/gone.rs"\n', {}),
        }
        self.assertEqual(self.problems(crates), [
            "bins/src/tool/main.rs", "comment/src/main.rs", "late/src/main.rs",
            "missing/src/gone.rs", "nested/src/bin/three/main.rs",
        ])

    def test_an_exempt_or_pending_crate_is_not_checked(self):
        pending = sorted(verify.PANIC_HOOK_PENDING)[0]
        crates = {"exp": ("", {"src/main.rs": "fn main() {}\n"}),
                  pending: ("", {"src/main.rs": "fn main() {}\n"})}
        self.assertEqual(self.problems(crates, exempt="# c\nexp\n"), [])


if __name__ == "__main__":
    unittest.main()
