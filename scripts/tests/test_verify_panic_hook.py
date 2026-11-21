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
CONFINE = (
    "    if let Err(err) = sandbox::ensure_single_threaded()\n"
    "        .and_then(|()| sandbox::apply_landlock_sandbox())\n"
    "        .and_then(|()| sandbox::forbid_core_dumps())\n"
    "    {\n        eprintln!(\"cannot confine: {err}\");\n        return ExitCode::FAILURE;\n    }\n"
)
HOOK = "    athanor_unit::panic::install();\n"


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

    def test_the_greeter_installs_the_hook_right_after_its_confinement(self):
        greeter = "athanor-greeter-ui"
        for main, problems in (
            ("fn main() -> ExitCode {\n" + CONFINE + HOOK + "    run()\n}\n", []),
            ("fn main() -> ExitCode {\n" + HOOK + CONFINE + "    run()\n}\n", [f"{greeter}/src/main.rs"]),
            ("fn main() -> ExitCode {\n" + CONFINE + "    setup();\n" + HOOK + "}\n", [f"{greeter}/src/main.rs"]),
        ):
            with self.subTest(main=main):
                self.assertEqual(self.problems({greeter: ("", {"src/main.rs": main})}), problems)

    def test_a_hooked_program_logs_through_the_journal_subscriber(self):
        journal = (
            "use tracing_subscriber::util::SubscriberInitExt;\n"
            "fn log() {\n    let filter = tracing_subscriber::EnvFilter::new(\"info\");\n"
            "    athanor_unit::journal::with_filter(filter, std::io::stdout).try_init();\n}\n"
        )
        crates = {
            "fmt": ("", {"src/main.rs": HOOKED + "fn log() {\n    tracing_subscriber::fmt().init();\n}\n"}),
            "imported": ("", {"src/main.rs": HOOKED, "src/log.rs": "use tracing_subscriber::{EnvFilter, fmt};\n"}),
            "renamed": ("", {"src/main.rs": HOOKED, "src/log.rs": "use tracing_subscriber as ts;\n"}),
            "global": ("", {"src/main.rs": HOOKED + "fn g(s: S) {\n    tracing::subscriber::set_global_default(s);\n}\n"}),
            "journal": ("", {"src/main.rs": HOOKED + journal}),
            "tested": ("", {"src/main.rs": HOOKED + "#[cfg(test)]\nmod tests {\n    fn t() { tracing_subscriber::fmt().init(); }\n}\n"}),
            "lib": ("", {"src/lib.rs": "pub fn f() { tracing_subscriber::fmt().init(); }\n"}),
        }
        self.assertEqual(self.problems(crates), [
            "fmt/src/main.rs", "global/src/main.rs", "imported/src/log.rs", "renamed/src/log.rs",
        ])

    def test_the_pending_set_only_shrinks(self):
        self.assertLessEqual(verify.PANIC_HOOK_PENDING, {"athanor-lock"})

    def test_an_exempt_or_pending_crate_is_not_checked(self):
        pending = sorted(verify.PANIC_HOOK_PENDING)[0]
        crates = {"exp": ("", {"src/main.rs": "fn main() {}\n"}),
                  pending: ("", {"src/main.rs": "fn main() {}\n"})}
        self.assertEqual(self.problems(crates, exempt="# c\nexp\n"), [])


if __name__ == "__main__":
    unittest.main()
