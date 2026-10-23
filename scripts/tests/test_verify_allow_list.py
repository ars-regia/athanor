"""Unit tests of `verify.py allow-list`: a shipped binary that asks for the top or overlay
layer, or binds a privileged global, has a line in the compositor's allow-list, and a line
names only an executable a spec installs (python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

GLOBALS = """\
wl_compositor standard
zwlr_layer_shell_v1 layer-gated
ext_foreign_toplevel_list_v1 privileged
ext_session_lock_manager_v1 privileged
"""

SPEC = """\
Name:           {name}
%build
cargo build --release --locked -p %{{name}}
%install
install -D -m 0755 target/release/{bin} %{{buildroot}}/usr/bin/{bin}
"""

CLIENT = "system/athanor-compositor-client"


class AllowList(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.write(verify.GLOBALS_FILE, GLOBALS)
        self.lines("")

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, path, text):
        p = self.root / path
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)

    def lines(self, text):
        self.write(verify.ALLOW_LIST, "# a comment\n" + text)

    def package(self, name, manifest="", files=None, installs=(None,)):
        """A package `name` under forge/specs, its spec installing each of `installs` (the
        package's own binary for None), its crate holding `files`."""
        crate = f"forge/specs/{name}/{name}-1.0.0"
        self.write(
            f"{crate}/Cargo.toml",
            f'[package]\nname = "{name}"\nversion = "1.0.0"\n{manifest}',
        )
        for path, text in (files or {}).items():
            self.write(f"{crate}/{path}", text)
        text = "".join(SPEC.format(name=name, bin=b or name) for b in installs)
        self.write(f"forge/specs/{name}/{name}.spec", text)

    def client(self):
        """A path dependency that binds the toplevel list, as athanor-compositor-client does."""
        self.write(
            f"{CLIENT}/Cargo.toml", '[package]\nname = "athanor-compositor-client"\n'
        )
        self.write(
            f"{CLIENT}/src/lib.rs",
            "use wayland_protocols::ext::foreign_toplevel_list::v1::client::"
            "ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1;\npub fn connect() {}\n",
        )

    def problems(self):
        return verify.allow_list_problems(self.root)

    def test_a_top_layer_target_without_a_line_fails(self):
        self.package(
            "athanor-dock",
            files={
                "src/main.rs": "fn main() {}\n",
                "src/ui/surface.rs": "fn f() { window.set_layer(Layer::Top); }\n",
            },
        )
        out = self.problems()
        self.assertEqual(len(out), 1)
        self.assertIn("/usr/bin/athanor-dock", out[0])
        self.assertIn("src/ui/surface.rs:1", out[0])

    def test_an_overlay_target_with_its_line_passes(self):
        self.package(
            "athanor-launcher",
            files={
                "src/main.rs": "fn f() { self.window.set_layer(Layer::Overlay); }\n"
            },
        )
        self.lines(
            "/usr/bin/athanor-launcher athanor-launcher.service zwlr_layer_shell_v1\n"
        )
        self.assertEqual(self.problems(), [])

    def test_a_target_the_spec_does_not_install_is_skipped(self):
        manifest = (
            '[[bin]]\nname = "athanor-dock"\npath = "src/main.rs"\n'
            '[[bin]]\nname = "athanor-dock-toolkit"\npath = "src/toolkit/main.rs"\n'
        )
        self.package(
            "athanor-dock",
            manifest,
            {
                "src/main.rs": "fn main() {}\n",
                "src/toolkit/main.rs": "const L: Options = Options { layer: Layer::Top };\n",
            },
        )
        self.assertEqual(self.problems(), [])
        # On the tree, the toolkit dock is never a target.
        self.assertFalse(
            [p for p in verify.allow_list_problems() if "athanor-dock-toolkit" in p]
        )

    def test_files_of_another_bin_target_are_not_counted(self):
        manifest = (
            '[[bin]]\nname = "athanor-dock"\npath = "src/main.rs"\n'
            '[[bin]]\nname = "athanor-dock-toolkit"\npath = "src/toolkit/main.rs"\n'
        )
        self.package(
            "athanor-dock",
            manifest,
            {
                "src/main.rs": "fn main() {}\n",
                "src/toolkit/main.rs": "fn main() {}\n",
                "src/toolkit/edge.rs": "const L: Layer = Layer::Top; fn f() { x.set_layer(Layer::Top); }\n",
            },
            installs=("athanor-dock", "athanor-dock-toolkit"),
        )
        self.lines(
            "/usr/bin/athanor-dock-toolkit athanor-dock.service zwlr_layer_shell_v1\n"
        )
        self.assertEqual(self.problems(), [])
        self.assertFalse(
            [p for p in verify.allow_list_problems() if "src/toolkit/" in p]
        )

    def test_a_line_for_an_executable_nothing_installs_fails(self):
        self.lines("/usr/bin/grim grim.service zwlr_layer_shell_v1\n")
        out = self.problems()
        self.assertEqual(len(out), 1)
        self.assertIn("/usr/bin/grim", out[0])

    def test_a_test_module_asking_for_the_top_layer_is_ignored(self):
        self.package(
            "athanor-bar",
            files={
                "src/main.rs": "#[cfg(test)]\nmod fixture;\nfn main() {}\n"
                "#[cfg(test)]\nmod tests {\n    fn t() { w.set_layer(Layer::Top); }\n}\n",
                "src/fixture.rs": "fn t() { w.set_layer(Layer::Overlay); }\n",
                "tests/layer.rs": "fn t() { w.set_layer(Layer::Top); }\n",
                "src/doc.rs": "// w.set_layer(Layer::Top) is what the bar does, under its line.\n",
            },
        )
        self.assertEqual(self.problems(), [])

    def test_a_bind_in_a_path_dependency_counts(self):
        self.client()
        self.package(
            "athanor-dock",
            '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n',
            {
                "src/main.rs": "use athanor_compositor_client::connect;\nfn main() { connect(); }\n"
            },
        )
        out = self.problems()
        self.assertEqual(len(out), 1)
        self.assertIn("ext_foreign_toplevel_list_v1", out[0])
        self.assertIn("/usr/bin/athanor-dock", out[0])

    def test_a_disabled_optional_dependency_is_not_followed(self):
        self.write(
            "system/athanor-toolkit/Cargo.toml", '[package]\nname = "athanor-toolkit"\n'
        )
        self.write(
            "system/athanor-toolkit/src/lib.rs",
            "pub fn surface() { s.set_layer(Layer::Top); }\n",
        )
        manifest = (
            '[features]\ndefault = ["gtk"]\ngtk = []\ntoolkit = ["dep:athanor-toolkit"]\n'
            '[dependencies]\nathanor-toolkit = { path = "../../../../system/athanor-toolkit", optional = true }\n'
        )
        self.package(
            "athanor-dock",
            manifest,
            {
                "src/main.rs": '#[cfg(feature = "toolkit")]\nuse athanor_toolkit::surface;\nfn main() {}\n'
            },
        )
        self.assertEqual(self.problems(), [])
        # The same dependency, enabled by the features the package builds with, counts.
        self.package(
            "athanor-dock",
            manifest.replace('default = ["gtk"]', 'default = ["gtk", "toolkit"]'),
            {"src/main.rs": "use athanor_toolkit::surface;\nfn main() {}\n"},
        )
        self.assertEqual(len(self.problems()), 1)

    def test_a_capture_global_counts_only_for_a_target_that_captures(self):
        # A client whose only privileged name is in capture.rs, entered by `Client::capture`.
        self.write(f"{CLIENT}/Cargo.toml", '[package]\nname = "athanor-compositor-client"\n')
        self.write(f"{CLIENT}/src/lib.rs", "mod capture;\npub fn connect() {}\n")
        self.write(f"{CLIENT}/src/capture.rs", 'fn capture() { Unavailable("ext_session_lock_manager_v1") }\n')
        dependency = '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n'
        self.package("athanor-dock", dependency, {"src/main.rs": "use athanor_compositor_client::connect;\nfn main() {}\n"})
        self.assertEqual(self.problems(), [])
        self.package("athanor-launcher", dependency,
                     {"src/main.rs": "use athanor_compositor_client::connect;\nasync fn f() { client.capture(id).await; }\n"})
        out = self.problems()
        self.assertEqual(len(out), 1)
        self.assertIn("/usr/bin/athanor-launcher", out[0])
        self.assertIn("ext_session_lock_manager_v1", out[0])

    def test_a_top_layer_target_whose_line_omits_layer_shell_fails(self):
        self.package(
            "athanor-dock",
            files={"src/main.rs": "fn f() { w.set_layer(Layer::Top); }\n"},
        )
        self.lines(
            "/usr/bin/athanor-dock athanor-dock.service ext_foreign_toplevel_list_v1\n"
        )
        out = self.problems()
        self.assertEqual(len(out), 1)
        self.assertIn("zwlr_layer_shell_v1", out[0])

    def test_a_target_with_a_line_passes_whatever_its_other_interfaces(self):
        self.client()
        self.package(
            "athanor-dock",
            '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n',
            {
                "src/main.rs": "use athanor_compositor_client::connect;\nfn f() { w.set_layer(Layer::Top); }\n"
            },
        )
        # The line lacks the toplevel list the client binds: the check never derives a grant.
        self.lines(
            "/usr/bin/athanor-dock athanor-dock.service zwlr_layer_shell_v1 ext_session_lock_manager_v1\n"
        )
        self.assertEqual(self.problems(), [])

    def test_the_greeter_overlay_target_needs_its_line(self):
        self.package(
            "athanor-greeter-ui",
            files={
                "src/main.rs": "mod ui;\nfn main() {}\n",
                "src/ui.rs": "fn build() {\n    window.set_layer(Layer::Overlay);\n}\n",
            },
        )
        out = self.problems()
        self.assertEqual(len(out), 1)
        self.assertIn("/usr/bin/athanor-greeter-ui", out[0])
        self.assertIn("src/ui.rs:2", out[0])
        self.lines("/usr/bin/athanor-greeter-ui greeter.scope zwlr_layer_shell_v1\n")
        self.assertEqual(self.problems(), [])

    def test_a_target_naming_the_session_lock_needs_its_line(self):
        self.package(
            "athanor-lock",
            files={
                "src/main.rs": "use ext_session_lock_manager_v1::ExtSessionLockManagerV1;\nfn main() {}\n"
            },
        )
        out = self.problems()
        self.assertEqual(len(out), 1)
        self.assertIn("ext_session_lock_manager_v1", out[0])
        self.lines(
            "/usr/bin/athanor-lock athanor-lock.service ext_session_lock_manager_v1\n"
        )
        self.assertEqual(self.problems(), [])


if __name__ == "__main__":
    unittest.main()
