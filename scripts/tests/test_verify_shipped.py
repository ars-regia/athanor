"""Unit tests of the COSMIC defaults wiring check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

OVERLAY = "/usr/share/athanor/cosmic-defaults"
ENV_FILE = "forge/specs/athanor-calmo/SOURCES/usr/lib/environment.d/60-athanor-cosmic-defaults.conf"
SESSION = "forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-session"
SPEC = "forge/specs/athanor-calmo/athanor-calmo.spec"
KEY = "system/athanor-style/calmo/generated/cosmic/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark"


class CosmicDefaultsTest(unittest.TestCase):
    def tree(self, tmp, **overrides):
        files = {
            KEY: "false",
            ENV_FILE: f"XDG_DATA_DIRS={OVERLAY}:${{XDG_DATA_DIRS:-/usr/local/share:/usr/share}}\n",
            SESSION: f'export XDG_DATA_DIRS="{OVERLAY}:${{XDG_DATA_DIRS:-/usr/local/share:/usr/share}}"\n',
            SPEC: ("%install\ncp -a system/athanor-style/calmo/generated/cosmic/cosmic "
                   f"%{{buildroot}}{OVERLAY}/\n%files\n{OVERLAY}\n"
                   "/usr/lib/environment.d/60-athanor-cosmic-defaults.conf\n"),
        }
        files.update(overrides)
        root = pathlib.Path(tmp)
        for name, text in files.items():
            if text is not None:
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_text(text)
        return root

    def test_a_complete_wiring_has_no_problem(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(verify.cosmic_defaults_problems(self.tree(tmp)), [])

    def test_no_generated_defaults_is_a_problem(self):
        with tempfile.TemporaryDirectory() as tmp:
            problems = verify.cosmic_defaults_problems(self.tree(tmp, **{KEY: None}))
            self.assertEqual(len(problems), 1)
            self.assertIn("derive.sh", problems[0])

    def test_the_overlay_must_come_first_for_the_user_manager(self):
        with tempfile.TemporaryDirectory() as tmp:
            late = f"XDG_DATA_DIRS=/usr/share:{OVERLAY}\n"
            problems = verify.cosmic_defaults_problems(self.tree(tmp, **{ENV_FILE: late}))
            self.assertEqual(len(problems), 1)
            self.assertIn("environment.d", problems[0])

    def test_the_compositor_needs_the_export_too(self):
        with tempfile.TemporaryDirectory() as tmp:
            problems = verify.cosmic_defaults_problems(self.tree(tmp, **{SESSION: "exec cosmic-comp\n"}))
            self.assertEqual(len(problems), 1)
            self.assertIn("athanor-session", problems[0])

    def test_the_spec_must_ship_the_overlay_and_the_environment_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            problems = verify.cosmic_defaults_problems(self.tree(tmp, **{SPEC: "%files\n"}))
            self.assertEqual(len(problems), 2)


class RemovedNamesTest(unittest.TestCase):
    def problems(self, files):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for name, text in files.items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_text(text)
            return verify.removed_name_problems(root)

    def test_a_clean_tree_has_no_problem(self):
        self.assertEqual(self.problems({
            "forge/config/packages.json": '{"upstream_core": ["nix"]}',
            "system/Containerfile": ("RUN dnf5 install -y keylime-agent \\\n    nix\n"
                                     "RUN systemctl enable tetragon.service && systemctl preset-all && systemctl disable systemd-homed.service\n"
                                     "RUN ! systemctl is-enabled --quiet keylime_agent.service\n"
                                     'RPM_LIST=$(find /x ! -name "*astro-toolchain*")\n'),
            "forge/specs/x/a.preset": "disable systemd-homed.service\n# enable keylime_agent\n",
        }), [])

    def test_a_removed_package_in_the_manifest_is_a_problem_at_any_depth(self):
        self.assertEqual(len(self.problems({
            "forge/config/packages.json": '{"upstream_desktop": ["cosmic-files"], "custom_tier0": ["antigravity"], "x": {"y": ["foot"]}}',
        })), 3)

    def test_every_install_form_is_read(self):
        for line in ("RUN dnf5 install -y \\\n    nix compiler-rt", "RUN dnf5 -y install compiler-rt",
                     "RUN dnf install -y compiler-rt", "RUN rpm-ostree install compiler-rt",
                     "RUN true && dnf5 install -y --allowerasing compiler-rt"):
            with self.subTest(line=line):
                self.assertEqual(len(self.problems({"system/Containerfile": line + "\n"})), 1)

    def test_homed_enabled_is_a_problem_in_every_place(self):
        self.assertEqual(len(self.problems({
            "system/Containerfile": "RUN authselect enable-feature with-systemd-homed\nRUN systemctl enable \\\n    tetragon.service systemd-homed.service\n",
            "system/athanor-install.ks": "services --enabled=sshd,systemd-homed\n",
            "forge/specs/x/a.preset": "enable systemd-homed-activate.service\n",
        })), 4)

    def test_disabling_is_not_a_problem(self):
        self.assertEqual(self.problems({
            "system/Containerfile": "RUN systemctl disable systemd-homed.service systemd-homed-activate.service\n",
            "forge/specs/x/a.preset": "disable systemd-homed.service\n",
        }), [])

    def test_enabling_keylime_is_a_problem(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "system").mkdir()
            (root / "system/Containerfile").write_text("RUN systemctl enable keylime_agent.service\n")
            (root / "system/x.preset").write_text("enable keylime_agent.service\n# enable keylime_agent\ndisable keylime_agent.service\n")
            self.assertEqual(len(verify.keylime_problems(root)), 2)

    def test_a_shipped_spec_must_not_require_a_removed_package(self):
        files = {
            "forge/config/packages.json": '{"custom_packages": ["desktop-ui", "shell-rs"]}',
            "forge/specs/athanor-desktop-ui/athanor-desktop-ui.spec": "Requires: nautilus foot\nRequires(post): qemu-img >= 1\n",
            "forge/specs/athanor-other/athanor-other.spec": "Requires: foot\n",
            "forge/specs/athanor-shell-rs/athanor-shell-rs.spec": "Requires: gtk4 foot\n",
        }
        found = self.problems(files)
        self.assertEqual(len(found), 2, found)
        self.assertTrue(all("desktop-ui" in p for p in found))

    def test_a_removed_document_or_a_link_to_it_is_a_problem(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for name, text in {
                "docs/architecture/doc_core_daemons.md": "x",
                "system/doc_telemetry.md": "x",
                "README.md": "[a](docs/architecture/doc_telemetry.md)",
                "system/README.md": '<a href="../docs/architecture/doc_core_daemons.md">x</a>',
            }.items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_text(text)
            self.assertEqual(len(verify.forbidden_doc_problems(root)), 4)


if __name__ == "__main__":
    unittest.main()
