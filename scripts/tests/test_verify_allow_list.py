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
        """The problems of the fixture tree, which installs no executable of OUTSIDE_THE_LIST."""
        return verify.allow_list_problems(self.root, outside=set())

    def exempt(self):
        """(problems, notes) of the fixture tree with the shipped OUTSIDE_THE_LIST."""
        notes = []
        return verify.allow_list_problems(
            self.root, notes, verify.OUTSIDE_THE_LIST
        ), notes

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
        self.write(
            f"{CLIENT}/Cargo.toml", '[package]\nname = "athanor-compositor-client"\n'
        )
        self.write(f"{CLIENT}/src/lib.rs", "mod capture;\npub fn connect() {}\n")
        self.write(
            f"{CLIENT}/src/capture.rs",
            'fn capture() { Unavailable("ext_session_lock_manager_v1") }\n',
        )
        dependency = '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n'
        self.package(
            "athanor-dock",
            dependency,
            {"src/main.rs": "use athanor_compositor_client::connect;\nfn main() {}\n"},
        )
        self.assertEqual(self.problems(), [])
        self.package(
            "athanor-launcher",
            dependency,
            {
                "src/main.rs": "use athanor_compositor_client::connect;\nasync fn f() { client.capture(id).await; }\n"
            },
        )
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

    def greeter(self):
        self.package(
            "athanor-greeter-ui",
            files={
                "src/main.rs": "fn build() {\n    window.set_layer(Layer::Overlay);\n}\n"
            },
        )

    def test_the_greeter_is_outside_the_list_and_noted(self):
        # doc_session.md SN12: named in the test as outside the list until it moves.
        self.assertIn("/usr/bin/athanor-greeter-ui", verify.OUTSIDE_THE_LIST)
        self.greeter()
        out, notes = self.exempt()
        self.assertEqual(out, [])
        self.assertEqual(len(notes), 1, notes)
        self.assertIn("/usr/bin/athanor-greeter-ui", notes[0])
        self.assertIn("src/main.rs:2", notes[0])

    def test_a_line_for_the_greeter_while_it_is_outside_the_list_fails(self):
        self.greeter()
        self.lines("/usr/bin/athanor-greeter-ui greeter.scope zwlr_layer_shell_v1\n")
        out, notes = self.exempt()
        self.assertEqual(notes, [])
        self.assertEqual(len(out), 1, out)
        self.assertIn("OUTSIDE_THE_LIST", out[0])

    def test_an_exemption_no_spec_installs_fails(self):
        out, notes = self.exempt()
        self.assertEqual(len(out), 1, out)
        self.assertIn("/usr/bin/athanor-greeter-ui", out[0])
        self.assertIn("installed by no spec", out[0])

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

    def spec(self, name, install):
        """The spec of package `name`, building it and installing with `install`."""
        self.write(
            f"forge/specs/{name}/{name}.spec",
            f"Name: {name}\n%build\ncargo build --release --locked\n%install\n{install}\n",
        )

    def test_the_top_layer_counts_however_it_is_written(self):
        written = {
            "a variable": "fn f() { let l = Layer::Top; w.set_layer(l); }\n",
            "an if-expression": "fn f() { w.set_layer(if full { Layer::Overlay } else { Layer::Bottom }); }\n",
            "an alias": "use gtk4_layer_shell::Layer as L;\nfn f() { w.set_layer(L::Top); }\n",
            "an alias in a group": "use gtk4_layer_shell::{Edge, Layer as Plane, LayerShell};\nfn f() { w.set_layer(Plane::Overlay); }\n",
            "a type alias": "type Plane = gtk4_layer_shell::Layer;\nfn f() { w.set_layer(Plane::Top); }\n",
            "a constant": "const LAYER: Layer = Layer::Overlay;\n",
            "the protocol request": "fn f() { shell.get_layer_surface(&s, None, zwlr_layer_shell_v1::Layer::Top, ns, qh, ()); }\n",
            "an imported variant": "use zwlr_layer_shell_v1::Layer::{Bottom, Overlay};\n",
        }
        for case, code in written.items():
            with self.subTest(case):
                self.package("athanor-dock", files={"src/main.rs": code})
                out = self.problems()
                self.assertEqual(len(out), 1, out)
                self.assertIn("top or overlay layer", out[0])
        self.package(
            "athanor-dock",
            files={"src/main.rs": "fn f() { w.set_layer(Layer::Bottom); }\n"},
        )
        self.assertEqual(self.problems(), [])

    def test_an_install_counts_however_it_is_written(self):
        written = {
            "into libexec": (
                "install -D -m 0755 target/release/athanor-dock %{buildroot}%{_libexecdir}/athanor-dock",
                "/usr/libexec/athanor-dock",
            ),
            "with a target directory": (
                "install -Dm755 -t %{buildroot}%{_bindir} target/release/athanor-dock",
                "/usr/bin/athanor-dock",
            ),
            "with cp into a directory": (
                "cp -p target/release/athanor-dock %{buildroot}/usr/bin/",
                "/usr/bin/athanor-dock",
            ),
            "over continued lines": (
                "install -D -m 0755 \\\n    target/release/athanor-dock \\\n"
                "    %{buildroot}%{_bindir}/athanor-dock",
                "/usr/bin/athanor-dock",
            ),
            "from a quoted source": (
                'install -D -m 0755 "target/release/athanor-dock" "%{buildroot}%{_bindir}/athanor-dock"',
                "/usr/bin/athanor-dock",
            ),
            "with a macro without braces": (
                "install -D -m 0755 target/release/athanor-dock %{buildroot}%_bindir/athanor-dock",
                "/usr/bin/athanor-dock",
            ),
            "into sbin": (
                "install -D -m 0755 target/release/athanor-dock %{buildroot}%{_sbindir}/athanor-dock",
                "/usr/sbin/athanor-dock",
            ),
            "on the line of a for loop": (
                "for _ in once; do install -D -m 0755 target/release/athanor-dock "
                "%{buildroot}%{_bindir}/athanor-dock; done",
                "/usr/bin/athanor-dock",
            ),
        }
        for case, (install, executable) in written.items():
            with self.subTest(case):
                self.package(
                    "athanor-dock",
                    files={"src/main.rs": "fn f() { w.set_layer(Layer::Top); }\n"},
                )
                self.spec("athanor-dock", install)
                self.lines("")
                out = self.problems()
                self.assertEqual(len(out), 1, out)
                self.assertIn(f"{executable}: asks for", out[0])
                self.lines(f"{executable} athanor-dock.service zwlr_layer_shell_v1\n")
                self.assertEqual(self.problems(), [])

    def test_a_binary_of_src_bin_is_a_target(self):
        self.package(
            "athanor-dock",
            files={
                "src/main.rs": "fn main() {}\n",
                "src/bin/athanor-dock-edge.rs": "fn main() { w.set_layer(Layer::Top); }\n",
                "src/bin/athanor-dock-peek/main.rs": "mod peek;\nfn main() {}\n",
                "src/bin/athanor-dock-peek/peek.rs": "fn f() { w.set_layer(Layer::Overlay); }\n",
            },
            installs=("athanor-dock", "athanor-dock-edge", "athanor-dock-peek"),
        )
        out = self.problems()
        self.assertEqual(len(out), 2, out)
        self.assertIn("/usr/bin/athanor-dock-edge: asks for", out[0])
        self.assertIn("/usr/bin/athanor-dock-peek: asks for", out[1])
        with self.subTest("the package's library"):
            self.package(
                "athanor-dock",
                files={
                    "src/lib.rs": "pub fn f() { w.set_layer(Layer::Top); }\n",
                    "src/bin/ui.rs": "fn main() { athanor_dock::f(); }\n",
                },
                installs=("ui",),
            )
            out = self.problems()
            self.assertEqual(len(out), 1, out)
            self.assertIn("/usr/bin/ui: asks for", out[0])
            self.assertIn("src/lib.rs:1", out[0])

    def test_a_binary_no_crate_of_the_tree_builds_fails_when_the_spec_carries_a_crate(
        self,
    ):
        self.package(
            "athanor-dock",
            files={"src/main.rs": "fn main() {}\n"},
            installs=("athanor-dock", "athanor-dock-gone"),
        )
        out = self.problems()
        self.assertEqual(len(out), 1, out)
        self.assertIn("athanor-dock-gone", out[0])

    def test_every_line_of_an_executable_is_checked(self):
        self.package(
            "athanor-dock",
            files={"src/main.rs": "fn f() { w.set_layer(Layer::Top); }\n"},
        )
        self.lines(
            "/usr/bin/athanor-dock athanor-dock.service zwlr_layer_shell_v1\n"
            "/usr/bin/athanor-dock athanor-dock-peek.service ext_foreign_toplevel_list_v1\n"
        )
        out = self.problems()
        self.assertEqual(len(out), 1, out)
        self.assertIn("athanor-dock-peek.service", out[0])

    def test_a_capture_counts_however_it_is_called(self):
        self.write(
            f"{CLIENT}/Cargo.toml", '[package]\nname = "athanor-compositor-client"\n'
        )
        self.write(
            f"{CLIENT}/src/lib.rs",
            "mod capture;\nuse crate::capture;\npub fn connect() {}\n",
        )
        self.write(
            f"{CLIENT}/src/capture.rs",
            'fn capture() { Unavailable("ext_session_lock_manager_v1") }\n',
        )
        dependency = '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n'
        self.package(
            "athanor-dock",
            dependency,
            {"src/main.rs": "use athanor_compositor_client::connect;\nfn main() {}\n"},
        )
        self.assertEqual(self.problems(), [])
        for call in (
            "Client::capture(&client, id)",
            "windows.map(Client::capture)",
            "client.capture (id)",
        ):
            with self.subTest(call):
                self.package(
                    "athanor-launcher",
                    dependency,
                    {
                        "src/main.rs": f"use athanor_compositor_client::Client;\nfn f() {{ {call}; }}\n"
                    },
                )
                out = self.problems()
                self.assertEqual(len(out), 1, out)
                self.assertIn("/usr/bin/athanor-launcher", out[0])
                self.assertIn("ext_session_lock_manager_v1", out[0])

    def test_a_spec_feature_of_a_dependency_enables_it(self):
        self.write(
            "system/athanor-toolkit/Cargo.toml",
            '[package]\nname = "athanor-toolkit"\n[features]\nedge = []\n',
        )
        self.write(
            "system/athanor-toolkit/src/lib.rs",
            "pub fn surface() { s.set_layer(Layer::Top); }\n",
        )
        self.package(
            "athanor-dock",
            '[dependencies]\nathanor-toolkit = { path = "../../../../system/athanor-toolkit", optional = true }\n',
            {"src/main.rs": "use athanor_toolkit::surface;\nfn main() {}\n"},
        )
        self.assertEqual(self.problems(), [])
        for flag in ("--features athanor-toolkit/edge",):
            with self.subTest(flag):
                self.write(
                    "forge/specs/athanor-dock/athanor-dock.spec",
                    f"Name: athanor-dock\n%build\ncargo build --release --locked {flag}\n%install\n"
                    "install -D -m 0755 target/release/athanor-dock %{buildroot}/usr/bin/athanor-dock\n",
                )
                self.assertEqual(len(self.problems()), 1)


if __name__ == "__main__":
    unittest.main()
