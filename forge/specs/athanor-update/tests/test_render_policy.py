"""Unit tests of the policy renderer
(python3 -B -m unittest discover -s forge/specs/athanor-update/tests -v)."""

import json
import pathlib
import shutil
import subprocess
import tempfile
import unittest

PACKAGE = pathlib.Path(__file__).resolve().parents[1]
RENDER = PACKAGE / "SOURCES/usr/libexec/athanor-update/render-policy"
VECTORS = PACKAGE / "athanor-update-1.0.0/tests/vectors"
REPOSITORIES = ["athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy"]
OPEN_TRANSPORTS = ["docker-archive", "oci", "oci-archive", "dir", "containers-storage", "docker-daemon"]
ACCEPT = [{"type": "insecureAcceptAnything"}]


class Render(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name).resolve()
        self.keys = self.dir / "keys"
        self.keys.mkdir()
        shutil.copy(VECTORS / "made/b.pub", self.keys / "athanor-image-2.pub")
        shutil.copy(VECTORS / "made/a.pub", self.keys / "athanor-image-1.pub")

    def tearDown(self):
        self.tmp.cleanup()

    def render(self, *extra, registry="registry.example/owner"):
        return subprocess.run(["bash", str(RENDER), "--registry", registry, "--keys-dir", str(self.keys), "--out", str(self.dir / "out"), *extra],
                              capture_output=True, text=True)

    def test_default_rejects_users_lose_nothing_and_our_three_repositories_are_strict(self):
        r = self.render()
        self.assertEqual(r.returncode, 0, r.stderr)
        policy = json.loads((self.dir / "out/policy.json").read_text())
        self.assertEqual(policy["default"], [{"type": "reject"}])
        self.assertEqual(policy["transports"]["docker"][""], ACCEPT)
        for transport in OPEN_TRANSPORTS:
            self.assertEqual(policy["transports"][transport], {"": ACCEPT}, transport)
        keys = [str(self.keys / "athanor-image-1.pub"), str(self.keys / "athanor-image-2.pub")]
        for name in REPOSITORIES:
            self.assertEqual(policy["transports"]["docker"][f"registry.example/owner/{name}"],
                             [{"type": "sigstoreSigned", "keyPaths": keys, "signedIdentity": {"type": "matchRepository"}}])
        self.assertEqual(len(policy["transports"]["docker"]), 4)

    def test_the_attachments_policy_opens_only_our_repositories(self):
        self.render()
        policy = json.loads((self.dir / "out/attachments-policy.json").read_text())
        self.assertEqual(policy["default"], [{"type": "reject"}])
        self.assertEqual(policy["transports"]["docker"], {f"registry.example/owner/{name}": ACCEPT for name in REPOSITORIES})

    def test_registries_d_declares_the_three_repositories_and_no_wider_scope(self):
        self.render()
        text = (self.dir / "out/registries.d/athanor.yaml").read_text()
        scopes = [line.strip().rstrip(":") for line in text.splitlines() if line.startswith("  ") and not line.startswith("    ")]
        self.assertEqual(scopes, [f"registry.example/owner/{name}" for name in REPOSITORIES])
        self.assertEqual(text.count("use-sigstore-attachments: true"), 3)
        self.assertNotIn("default-docker", text)

    def test_link_etc_makes_both_links(self):
        self.render("--link-etc", str(self.dir / "etc"))
        for link, target in (("policy.json", "policy.json"), ("registries.d/athanor.yaml", "registries.d/athanor.yaml")):
            path = self.dir / "etc/containers" / link
            self.assertTrue(path.is_symlink())
            self.assertEqual(path.resolve(), self.dir / "out" / target)

    def test_no_key_a_non_key_and_a_registry_that_needs_quoting_are_refused(self):
        self.assertEqual(self.render(registry='evil"/x').returncode, 2)
        (self.keys / "athanor-image-3.pub").write_text("not a key\n")
        self.assertEqual(self.render().returncode, 2)
        for key in self.keys.iterdir():
            key.unlink()
        r = self.render()
        self.assertEqual(r.returncode, 2)
        self.assertIn("no *.pub", r.stderr)

    @unittest.skipUnless(shutil.which("skopeo"), "skopeo is not installed")
    def test_containers_image_loads_the_rendered_policy(self):
        self.render()
        r = subprocess.run(["skopeo", "copy", "--policy", str(self.dir / "out/policy.json"), f"dir:{VECTORS / 'real'}", f"dir:{self.dir / 'copy'}"],
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)


if __name__ == "__main__":
    unittest.main()
