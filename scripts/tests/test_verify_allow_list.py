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
ext_image_copy_capture_manager_v1 privileged
athanor_toplevel_actions_v1 privileged
athanor_keyboard_group_manager_v1 privileged
athanor_a11y_manager_v1 privileged
"""

SPEC = """\
Name:           {name}
%build
cargo build --release --locked -p %{{name}}
%install
install -D -m 0755 target/release/{bin} %{{buildroot}}/usr/bin/{bin}
"""

CLIENT = "system/athanor-compositor-client"


OSD = {"/usr/bin/cosmic-osd": "cosmic-osd.service"}


class AllowList(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.write(verify.GLOBALS_FILE, GLOBALS)
        self.lines("")

    def tearDown(self):
        self.tmp.cleanup()

    def fresh(self):
        """An empty fixture tree in place of the current one."""
        self.tearDown()
        self.setUp()

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
        """A path dependency that binds the toplevel list at connect, the image copy
        capture at the first `capture`, the window actions at the first action of
        `window_actions`, the keyboard group at the first `watch_keyboard_group` and the
        accessibility manager at the first `watch_accessibility`, as athanor-compositor-client
        does."""
        self.write(
            f"{CLIENT}/Cargo.toml", '[package]\nname = "athanor-compositor-client"\n'
        )
        self.write(
            f"{CLIENT}/src/lib.rs",
            "mod a11y;\nmod actions;\nmod capture;\nmod connection;\nmod keyboard_group;\npub fn connect() {}\n",
        )
        self.write(
            f"{CLIENT}/src/connection.rs",
            "use wayland_protocols::ext::foreign_toplevel_list::v1::client::"
            "ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1;\n",
        )
        self.write(
            f"{CLIENT}/src/capture.rs",
            'fn capture() { Unavailable("ext_image_copy_capture_manager_v1") }\n',
        )
        self.write(
            f"{CLIENT}/src/actions.rs",
            'fn act() { Unavailable("athanor_toplevel_actions_v1") }\n',
        )
        self.write(
            f"{CLIENT}/src/keyboard_group.rs",
            'fn set_group() { Unavailable("athanor_keyboard_group_manager_v1") }\n',
        )
        self.write(
            f"{CLIENT}/src/a11y.rs",
            "use crate::protocols::athanor_a11y::client::athanor_a11y_manager_v1::AthanorA11yManagerV1;\n",
        )

    def problems(self, upstream=None):
        """The problems of the fixture tree, which installs no executable of OUTSIDE_THE_LIST
        and has no line of UPSTREAM's unless `upstream` names them."""
        return verify.allow_list_problems(self.root, outside=set(), upstream=upstream or {})

    def exempt(self):
        """(problems, notes) of the fixture tree with the shipped OUTSIDE_THE_LIST."""
        notes = []
        return verify.allow_list_problems(
            self.root, notes, verify.OUTSIDE_THE_LIST, upstream={}
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
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
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
        # The client binds the image copy capture at the first `Client::capture` only.
        self.client()
        dependency = '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n'
        self.package(
            "athanor-dock",
            dependency,
            {"src/main.rs": "use athanor_compositor_client::connect;\nfn main() {}\n"},
        )
        self.lines("/usr/bin/athanor-dock athanor-dock.service ext_foreign_toplevel_list_v1\n")
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
        self.assertIn("ext_image_copy_capture_manager_v1", out[0])

    def test_the_window_actions_count_only_for_a_target_that_acts(self):
        # The client binds the window actions at the first action of `Client::window_actions`
        # only: the control center, which never acts, needs no grant of them.
        self.client()
        dependency = '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n'
        self.package(
            "athanor-control-center",
            dependency,
            {"src/main.rs": "use athanor_compositor_client::connect;\nfn f() { panel.close(); }\n"},
        )
        self.lines("/usr/bin/athanor-control-center athanor-control-center.service ext_foreign_toplevel_list_v1\n")
        self.assertEqual(self.problems(), [])
        for call in (
            "client.window_actions().close(id)",
            "Client::window_actions(&client).activate(id)",
            "client.window_actions ().minimize(id)",
        ):
            with self.subTest(call):
                self.package(
                    "athanor-bar",
                    dependency,
                    {"src/main.rs": f"use athanor_compositor_client::Client;\nfn f() {{ {call}; }}\n"},
                )
                self.lines(
                    "/usr/bin/athanor-control-center athanor-control-center.service ext_foreign_toplevel_list_v1\n"
                    "/usr/bin/athanor-bar athanor-bar.service ext_foreign_toplevel_list_v1\n"
                )
                out = self.problems()
                self.assertEqual(len(out), 1, out)
                self.assertIn("/usr/bin/athanor-bar", out[0])
                self.assertIn("athanor_toplevel_actions_v1", out[0])
                self.lines(
                    "/usr/bin/athanor-control-center athanor-control-center.service ext_foreign_toplevel_list_v1\n"
                    "/usr/bin/athanor-bar athanor-bar.service ext_foreign_toplevel_list_v1 athanor_toplevel_actions_v1\n"
                )
                self.assertEqual(self.problems(), [])

    def test_the_keyboard_group_counts_only_for_a_target_that_watches_it(self):
        # The client binds the keyboard group at the first `Client::watch_keyboard_group`
        # only: the control center, which never watches it, needs no grant of it.
        self.client()
        dependency = '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n'
        self.package(
            "athanor-control-center",
            dependency,
            {"src/main.rs": "use athanor_compositor_client::connect;\nfn f() { client.keyboard_group(); }\n"},
        )
        self.lines("/usr/bin/athanor-control-center athanor-control-center.service ext_foreign_toplevel_list_v1\n")
        self.assertEqual(self.problems(), [])
        for call in ("client.watch_keyboard_group()", "Client::watch_keyboard_group(&client)"):
            with self.subTest(call):
                self.package(
                    "athanor-bar",
                    dependency,
                    {"src/main.rs": f"use athanor_compositor_client::Client;\nfn f() {{ {call}; }}\n"},
                )
                self.lines(
                    "/usr/bin/athanor-control-center athanor-control-center.service ext_foreign_toplevel_list_v1\n"
                    "/usr/bin/athanor-bar athanor-bar.service ext_foreign_toplevel_list_v1\n"
                )
                out = self.problems()
                self.assertEqual(len(out), 1, out)
                self.assertIn("/usr/bin/athanor-bar", out[0])
                self.assertIn("athanor_keyboard_group_manager_v1", out[0])
                self.lines(
                    "/usr/bin/athanor-control-center athanor-control-center.service ext_foreign_toplevel_list_v1\n"
                    "/usr/bin/athanor-bar athanor-bar.service ext_foreign_toplevel_list_v1 athanor_keyboard_group_manager_v1\n"
                )
                self.assertEqual(self.problems(), [])

    def test_the_accessibility_manager_counts_only_for_a_target_that_watches_it(self):
        # athanor-compositor's accessibility manager is bound at the first
        # `Client::watch_accessibility` only: the control center needs no grant of it.
        self.client()
        dependency = '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n'
        self.package(
            "athanor-control-center",
            dependency,
            {"src/main.rs": "use athanor_compositor_client::connect;\nfn f() { client.accessibility(); }\n"},
        )
        self.lines("/usr/bin/athanor-control-center athanor-control-center.service ext_foreign_toplevel_list_v1\n")
        self.assertEqual(self.problems(), [])
        for call in ("client.watch_accessibility()", "Client::watch_accessibility(&client)"):
            with self.subTest(call):
                self.package(
                    "athanor-bar",
                    dependency,
                    {"src/main.rs": f"use athanor_compositor_client::Client;\nfn f() {{ {call}; }}\n"},
                )
                self.lines(
                    "/usr/bin/athanor-control-center athanor-control-center.service ext_foreign_toplevel_list_v1\n"
                    "/usr/bin/athanor-bar athanor-bar.service ext_foreign_toplevel_list_v1\n"
                )
                out = self.problems()
                self.assertEqual(len(out), 1, out)
                self.assertIn("/usr/bin/athanor-bar", out[0])
                self.assertIn("athanor_a11y_manager_v1", out[0])
                self.lines(
                    "/usr/bin/athanor-control-center athanor-control-center.service ext_foreign_toplevel_list_v1\n"
                    "/usr/bin/athanor-bar athanor-bar.service ext_foreign_toplevel_list_v1 athanor_a11y_manager_v1\n"
                )
                self.assertEqual(self.problems(), [])

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

    def test_a_line_grants_every_privileged_global_the_target_binds(self):
        self.client()
        dependency = '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n'
        dock = "use athanor_compositor_client::connect;\nfn f() { w.set_layer(Layer::Top); }\n"
        launcher = "use athanor_compositor_client::Client;\nfn f() { client.capture(id); }\n"
        cases = {
            # What the client binds at connect, missing from the line.
            "a missing connect name": (
                "athanor-dock", dock,
                "/usr/bin/athanor-dock athanor-dock.service zwlr_layer_shell_v1\n",
                "ext_foreign_toplevel_list_v1",
            ),
            # What the client binds at the first capture, missing from a capturing target's line.
            "a missing capture grant": (
                "athanor-launcher", launcher,
                "/usr/bin/athanor-launcher athanor-launcher.service ext_foreign_toplevel_list_v1\n",
                "ext_image_copy_capture_manager_v1",
            ),
            # A name that is no privileged or layer-gated row, as a new version of a global.
            "an unknown name": (
                "athanor-dock", dock,
                "/usr/bin/athanor-dock athanor-dock.service zwlr_layer_shell_v1 "
                "ext_foreign_toplevel_list_v1 ext_workspace_manager_v2\n",
                "ext_workspace_manager_v2",
            ),
            "a trailing comment": (
                "athanor-dock", dock,
                "/usr/bin/athanor-dock athanor-dock.service zwlr_layer_shell_v1 "
                "ext_foreign_toplevel_list_v1 #dock\n",
                "names #dock",
            ),
            "a line with no interface": (
                "athanor-dock", dock,
                "/usr/bin/athanor-dock athanor-dock.service zwlr_layer_shell_v1 "
                "ext_foreign_toplevel_list_v1\n/usr/bin/athanor-dock athanor-dock.service\n",
                "grant nothing at all",
            ),
            "a standard global": (
                "athanor-dock", dock,
                "/usr/bin/athanor-dock athanor-dock.service zwlr_layer_shell_v1 "
                "ext_foreign_toplevel_list_v1 wl_compositor\n",
                "wl_compositor",
            ),
        }
        for case, (name, main, line, missing) in cases.items():
            with self.subTest(case):
                self.fresh()
                self.client()
                self.package(name, dependency, {"src/main.rs": main})
                self.lines(line)
                out = self.problems()
                self.assertEqual(len(out), 1, out)
                self.assertIn(missing, out[0])
        # A grant beyond what the target binds is the maintainer's: it passes.
        self.fresh()
        self.client()
        self.package("athanor-dock", dependency, {"src/main.rs": dock})
        self.package("athanor-launcher", dependency, {"src/main.rs": launcher})
        self.lines(
            "/usr/bin/athanor-dock athanor-dock.service zwlr_layer_shell_v1 "
            "ext_foreign_toplevel_list_v1 ext_session_lock_manager_v1\n"
            "/usr/bin/athanor-launcher athanor-launcher.service ext_foreign_toplevel_list_v1 "
            "ext_image_copy_capture_manager_v1\n"
        )
        self.assertEqual(self.problems(), [])

    def test_the_check_never_passes_on_nothing(self):
        self.package("athanor-dock", files={"src/main.rs": "fn main() {}\n"})
        self.assertEqual(self.problems(), [])
        cases = {
            "no privileged row": (verify.GLOBALS_FILE, "zwlr_layer_shell_v1 layer-gated\n", "no privileged row"),
            "a row with a trailing comment": (
                verify.GLOBALS_FILE, GLOBALS + "zwlr_output_manager_v1 privileged # the control center\n",
                "zwlr_output_manager_v1 privileged # the control center",
            ),
            "a row of no class": (verify.GLOBALS_FILE, GLOBALS + "wl_shm everyone\n", "wl_shm everyone"),
            "a client whose connect names no privileged global": (
                f"{CLIENT}/src/connection.rs", "pub fn connect() {}\n", "binds at connect",
            ),
            "a client whose capture moved": (
                f"{CLIENT}/src/capture.rs", "\n", "binds at the first capture",
            ),
            "a client whose window actions moved": (
                f"{CLIENT}/src/actions.rs", "\n", "binds at the first window action",
            ),
            "no target": ("forge/specs/athanor-dock/athanor-dock.spec", "Name: athanor-dock\n", "no target"),
        }
        for case, (path, text, problem) in cases.items():
            with self.subTest(case):
                self.fresh()
                if path.startswith(CLIENT):
                    self.client()
                self.package("athanor-dock", files={"src/main.rs": "fn main() {}\n"})
                self.write(path, text)
                out = self.problems()
                self.assertEqual(len(out), 1, out)
                self.assertIn(problem, out[0])

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
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
        out, notes = self.exempt()
        self.assertEqual(len(out), 1, out)
        self.assertIn("/usr/bin/athanor-greeter-ui", out[0])
        self.assertIn("installed by no spec", out[0])

    def test_the_upstream_clients_are_the_locker_the_idle_daemon_and_the_osd(self):
        # doc_session.md revision 4, SN12 and SN15.
        self.assertEqual(
            verify.UPSTREAM,
            {
                "/usr/bin/cosmic-greeter": "cosmic-greeter-locker.scope",
                "/usr/bin/cosmic-idle": "cosmic-idle.scope",
                "/usr/bin/cosmic-osd": "cosmic-osd.service",
            },
        )

    def test_an_upstream_line_no_spec_installs_passes(self):
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
        self.lines("/usr/bin/cosmic-osd cosmic-osd.service zwlr_layer_shell_v1\n")
        out = self.problems()
        self.assertEqual(len(out), 1, out)
        self.assertIn("/usr/bin/cosmic-osd is installed by no spec", out[0])
        self.assertEqual(self.problems(OSD), [])

    def test_an_upstream_client_without_its_line_fails(self):
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
        out = self.problems(OSD)
        self.assertEqual(len(out), 1, out)
        self.assertIn("has no line", out[0])

    def test_an_upstream_line_still_grants_only_privileged_or_layer_gated_rows(self):
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
        self.lines("/usr/bin/cosmic-osd cosmic-osd.service wl_compositor\n")
        out = self.problems(OSD)
        self.assertEqual(len(out), 1, out)
        self.assertIn("no privileged or layer-gated row", out[0])

    def test_an_upstream_line_for_another_unit_fails(self):
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
        self.lines("/usr/bin/cosmic-osd other.service zwlr_layer_shell_v1\n")
        out = self.problems(OSD)
        self.assertEqual(len(out), 1, out)
        self.assertIn("needs one line, for cosmic-osd.service", out[0])

    def test_an_upstream_scope_the_supervisor_does_not_start_fails(self):
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
        self.lines("/usr/bin/cosmic-idle cosmic-idle.scope zwlr_layer_shell_v1\n")
        idle = {"/usr/bin/cosmic-idle": "cosmic-idle.scope"}
        out = self.problems(idle)
        self.assertEqual(len(out), 1, out)
        self.assertIn("starts no cosmic-idle.scope", out[0])
        self.write(verify.SESSION_COMPONENTS, "systemd-run --user --scope --unit=cosmic-idle.scope cosmic-idle\n")
        self.assertEqual(self.problems(idle), [])

    def test_an_upstream_scope_the_supervisor_only_mentions_fails(self):
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
        self.lines("/usr/bin/cosmic-idle cosmic-idle.scope zwlr_layer_shell_v1\n")
        idle = {"/usr/bin/cosmic-idle": "cosmic-idle.scope"}
        start = "systemd-run --user --scope --unit=cosmic-idle.scope cosmic-idle"
        for text in (
            f"# {start}\n",
            start.replace("scope cosmic", "scope.bak cosmic") + "\n",
            "echo " + start + "\n",
            "systemd-run --user --scope cosmic-idle --unit=cosmic-idle.scope\n",
            "systemd-run\n--unit=cosmic-idle.scope\n",
        ):
            self.write(verify.SESSION_COMPONENTS, text)
            out = self.problems(idle)
            self.assertEqual(len(out), 1, (text, out))
            self.assertIn("starts no cosmic-idle.scope", out[0])
        self.write(verify.SESSION_COMPONENTS, f"    {start} &\n")
        self.assertEqual(self.problems(idle), [])
        self.write(verify.SESSION_COMPONENTS, f"{start}\n")
        self.assertEqual(self.problems(idle), [])

    def test_an_upstream_client_a_spec_installs_fails(self):
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
        self.write(
            "forge/specs/cosmic-osd/cosmic-osd.spec",
            "Name: cosmic-osd\n%install\ninstall -D -m 0755 cosmic-osd %{buildroot}/usr/bin/cosmic-osd\n",
        )
        self.lines("/usr/bin/cosmic-osd cosmic-osd.service zwlr_layer_shell_v1\n")
        out = self.problems(OSD)
        self.assertEqual(len(out), 1, out)
        self.assertIn("remove it from UPSTREAM", out[0])

    def test_the_compositor_serving_the_globals_is_no_client(self):
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
        self.package(
            "athanor-compositor",
            files={
                "src/main.rs": "fn bind() {\n    ext_session_lock_manager_v1;\n    window.set_layer(Layer::Overlay);\n}\n"
            },
        )
        self.assertEqual(self.problems(), [])

    def test_another_binary_installed_as_the_compositor_is_checked(self):
        self.package("athanor-bar", files={"src/main.rs": "fn main() {\n    window.set_layer(Layer::Overlay);\n}\n"})
        self.write(
            "forge/specs/athanor-bar/athanor-bar.spec",
            "Name:           athanor-bar\n%build\ncargo build --release --locked -p %{name}\n%install\n"
            "install -D -m 0755 target/release/athanor-bar %{buildroot}/usr/bin/athanor-compositor\n",
        )
        out = self.problems()
        self.assertEqual(len(out), 1, out)
        self.assertIn("/usr/bin/athanor-compositor", out[0])

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

    def test_an_install_source_the_check_cannot_expand_fails_as_such(self):
        written = {
            "a shell variable": "for b in athanor-dock; do install -D -m 0755 target/release/$b "
            "%{buildroot}%{_bindir}/$b; done",
            "a glob": "install -D -m 0755 -t %{buildroot}%{_bindir} target/release/*",
            "an unknown macro": "install -D -m 0755 target/release/%{binary} %{buildroot}%{_bindir}/athanor-dock",
        }
        self.package("athanor-bar", files={"src/main.rs": "fn main() {}\n"})
        for case, install in written.items():
            with self.subTest(case):
                self.package("athanor-dock", files={"src/main.rs": "fn main() {}\n"})
                self.spec("athanor-dock", install)
                out = self.problems()
                self.assertEqual(len(out), 1, out)
                self.assertIn("cannot expand", out[0])
                self.assertIn("name each binary", out[0])

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
        self.client()
        dependency = '[dependencies]\nathanor-compositor-client = { path = "../../../../system/athanor-compositor-client" }\n'
        self.package(
            "athanor-dock",
            dependency,
            {"src/main.rs": "use athanor_compositor_client::connect;\nfn main() {}\n"},
        )
        self.lines("/usr/bin/athanor-dock athanor-dock.service ext_foreign_toplevel_list_v1\n")
        self.assertEqual(self.problems(), [])
        self.lines(
            "/usr/bin/athanor-dock athanor-dock.service ext_foreign_toplevel_list_v1\n"
            "/usr/bin/athanor-launcher athanor-launcher.service ext_foreign_toplevel_list_v1\n"
        )
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
                self.assertIn("ext_image_copy_capture_manager_v1", out[0])

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
