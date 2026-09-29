"""Unit tests of the update and trust assertions of scripts/verify.py shipped
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import shutil
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify", ROOT / "scripts" / "verify.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

TEMPLATES = "forge/specs/athanor-update/SOURCES/usr/share/athanor/containers/templates"


class UpdateTrust(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        shutil.copytree(ROOT / "forge/specs/athanor-update", self.root / "forge/specs/athanor-update",
                        ignore=shutil.ignore_patterns("target", "vectors"))

    def tearDown(self):
        self.tmp.cleanup()

    def edit(self, relative, old, new):
        path = self.root / relative
        text = path.read_text()
        self.assertIn(old, text)
        path.write_text(text.replace(old, new))

    def problems(self):
        return verify.update_trust_problems(self.root)

    def test_the_package_as_committed_has_no_problem(self):
        self.assertEqual(self.problems(), [])
        self.assertEqual(verify.update_trust_problems(), [])

    def test_a_permissive_default_is_reported(self):
        self.edit(f"{TEMPLATES}/policy.json.in", '"default": [{"type": "reject"}]', '"default": [{"type": "insecureAcceptAnything"}]')
        self.assertTrue(any("`default` must be reject" in p for p in self.problems()))

    def test_a_closed_user_transport_is_reported(self):
        self.edit(f"{TEMPLATES}/policy.json.in", '    "oci": {"": [{"type": "insecureAcceptAnything"}]},\n', "")
        self.assertTrue(any("transport oci must stay open" in p for p in self.problems()))

    def test_a_repository_without_match_repository_is_reported(self):
        self.edit(f"{TEMPLATES}/policy.json.in", '"@REGISTRY@/athanor-system-nvidia": [{"type": "sigstoreSigned", "keyPaths": [@KEY_PATHS@], "signedIdentity": {"type": "matchRepository"}}]',
                  '"@REGISTRY@/athanor-system-nvidia": [{"type": "sigstoreSigned", "keyPaths": [@KEY_PATHS@]}]')
        self.assertTrue(any("athanor-system-nvidia must be sigstoreSigned" in p for p in self.problems()))

    def test_a_missing_registries_entry_is_reported(self):
        self.edit(f"{TEMPLATES}/athanor.yaml.in", "  @REGISTRY@/athanor-system-nvidia-legacy:\n    use-sigstore-attachments: true\n", "")
        self.assertTrue(any("athanor.yaml.in" in p for p in self.problems()))

    def test_a_file_the_spec_does_not_list_is_reported(self):
        self.edit("forge/specs/athanor-update/athanor-update.spec", "/usr/share/polkit-1/actions/os.athanor.update.policy\n%doc", "%doc")
        self.assertTrue(any("%files does not list /usr/share/polkit-1/actions/os.athanor.update.policy" in p for p in self.problems()))

    def test_the_wiring_this_package_replaces_must_be_gone(self):
        preset = self.root / "forge/specs/athanor-system-config/SOURCES/usr/lib/systemd/system-preset/80-athanor-system.preset"
        preset.parent.mkdir(parents=True)
        preset.write_text("enable greetd.service\nenable bootc-fetch-apply.timer\n")
        override = self.root / "forge/specs/athanor-base-config/SOURCES/usr/lib/systemd/system/bootc-fetch-apply-updates.service.d/override.conf"
        override.parent.mkdir(parents=True)
        override.write_text("[Service]\nExecStart=\nExecStart=/usr/bin/bootc upgrade --stage --quiet\n")
        found = self.problems()
        self.assertTrue(any("bootc-fetch-apply.timer" in p for p in found))
        self.assertTrue(any("--stage" in p for p in found))

    def test_a_literal_registry_owner_is_reported(self):
        self.edit(f"{TEMPLATES}/athanor.yaml.in", "  @REGISTRY@/athanor-system:\n", "  ghcr.io/hr-mes/athanor-system:\n")
        self.assertTrue(any("literal ghcr.io/hr-mes" in p for p in self.problems()))

    def test_a_package_may_build_more_than_one_crate(self):
        built = verify.crates_built_by_specs()
        self.assertEqual(built.get("athanor-update"), "athanor-update")
        self.assertEqual(built.get("athanor-update-notify"), "athanor-update")

    def test_the_retired_secure_boot_daemon_must_stay_gone(self):
        self.assertEqual([p for p in verify.update_trust_problems() if "SecureBoot" in p or "secure-boot" in p], [])
        daemon = self.root / "forge/specs/athanor-secure-boot/athanor-secure-boot-1.0.0/src/main.rs"
        daemon.parent.mkdir(parents=True)
        daemon.write_text('#[interface(name = "org.athanor.SecureBoot")]\n')
        spec = self.root / "forge/specs/athanor-secure-boot/athanor-secure-boot.spec"
        spec.write_text("%files\n/usr/lib/systemd/system/athanor-secure-boot.service\n/usr/lib/systemd/system/athanor-tpm-luks-seal.service\n")
        found = self.problems()
        self.assertTrue(any("org.athanor.SecureBoot" in p for p in found))
        self.assertTrue(any("athanor-secure-boot.service" in p for p in found))

    def test_the_tpm_files_the_image_reads_must_stay(self):
        spec = self.root / "forge/specs/athanor-secure-boot/athanor-secure-boot.spec"
        spec.parent.mkdir(parents=True)
        spec.write_text("%files\n")
        self.assertTrue(any("athanor-tpm-luks-seal.sh" in p for p in self.problems()))

    def test_the_image_build_renders_the_policy_and_links_etc(self):
        self.assertEqual(verify.image_policy_problems(), [])
        system = self.root / "system"
        (system / "keys").mkdir(parents=True)
        (system / "Containerfile").write_text("FROM scratch\nRUN systemctl preset-all\n")
        found = verify.image_policy_problems(self.root)
        self.assertTrue(any("render-policy" in p for p in found))
        self.assertTrue(any("system/keys" in p for p in found))
        (system / "keys/athanor-image-1.pub").write_text("-----BEGIN PUBLIC KEY-----\n")
        (system / "keys/leak.key").write_text("x")
        (system / "Containerfile").write_text(
            "ARG IMAGE_REGISTRY\nCOPY system/keys/ /usr/share/athanor/keys/\n"
            'RUN /usr/libexec/athanor-update/render-policy --registry "${IMAGE_REGISTRY}" --keys-dir /usr/share/athanor/keys '
            "--out /usr/share/athanor/containers --link-etc /etc\nRUN systemctl preset-all\n")
        self.assertEqual(verify.image_policy_problems(self.root), ["system/keys/leak.key: only *.pub files belong under system/keys"])


if __name__ == "__main__":
    unittest.main()
