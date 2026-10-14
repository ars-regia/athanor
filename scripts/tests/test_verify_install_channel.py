"""verify.py install-channel: installed systems follow :stable, signed (doc_update_delivery.md UD2)."""

import importlib.util
import pathlib
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class InstallChannelTest(unittest.TestCase):
    def setUp(self):
        self.root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        for name in verify.INSTALL_KICKSTARTS:
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(ROOT / name, self.root / name)

    def edit(self, name, old, new):
        path = self.root / name
        text = path.read_text()
        self.assertIn(old, text)
        path.write_text(text.replace(old, new))

    def test_the_repository_passes(self):
        self.assertEqual(verify.install_channel_problems(self.root), [])

    def test_latest_fails(self):
        self.edit("system/athanor-install.ks", "athanor-system:stable", "athanor-system:latest")
        self.assertIn(":latest", " ".join(verify.install_channel_problems(self.root)))

    def test_a_switch_without_the_policy_fails(self):
        (self.root / "other.ks").write_text("%post\nbootc switch --transport registry r:stable\n%end\n")
        self.assertIn("other.ks:2: bootc switch without", " ".join(verify.install_channel_problems(self.root)))

    def test_a_missing_stable_post_fails(self):
        self.edit("forge/test/iso/collaudo.ks", " --enforce-container-sigpolicy", "")
        problems = " ".join(verify.install_channel_problems(self.root))
        self.assertIn("forge/test/iso/collaudo.ks: needs one %post", problems)

    def test_diverging_posts_fail(self):
        self.edit("system/disk_config/iso.toml", "    exit 1\nfi\nreference", "    exit 2\nfi\nreference")
        self.assertIn("differs between", " ".join(verify.install_channel_problems(self.root)))

    def test_comments_are_not_read(self):
        (self.root / "other.ks").write_text("# this kickstart once followed :latest\n")
        self.assertEqual(verify.install_channel_problems(self.root), [])


class StablePostTest(unittest.TestCase):
    """Runs the %post of the shipped kickstart with /sysroot moved under a temporary directory
    and a bootc that records its arguments."""

    def run_post(self, *references):
        root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        text = (ROOT / "system/athanor-install.ks").read_text()
        block = next(b for b in verify.KICKSTART_POST.findall(text) if verify.STABLE_SWITCH in b)
        body = block.split("\n", 1)[1].rsplit("\n", 1)[0].replace("/sysroot/", f"{root}/sysroot/")
        for i, reference in enumerate(references):
            deploy = root / "sysroot/ostree/deploy/default/deploy"
            deploy.mkdir(parents=True, exist_ok=True)
            (deploy / f"{i}.0.origin").write_text(f"[origin]\ncontainer-image-reference={reference}\n")
        (root / "bin").mkdir()
        (root / "bin/bootc").write_text(f'#!/bin/sh\nprintf "%s\\n" "$*" > {root}/args\n')
        (root / "bin/bootc").chmod(0o755)
        env = {"PATH": f"{root}/bin:/usr/bin:/bin"}
        done = subprocess.run(["sh", "-c", body], env=env, capture_output=True, text=True, check=False)
        args = (root / "args").read_text().strip() if (root / "args").exists() else None
        return done.returncode, args

    def test_the_builder_reference_becomes_stable_signed(self):
        switch = "switch --mutate-in-place --enforce-container-sigpolicy --transport registry"
        for reference in ("ostree-unverified-registry:ghcr.io/acme/athanor-system:38546669761",
                          "ostree-unverified-image:docker://ghcr.io/acme/athanor-system:38546669761",
                          "ostree-unverified-registry:ghcr.io/acme/athanor-system@sha256:" + "a" * 64,
                          "ostree-unverified-registry:localhost:5000/athanor-system:stable"):
            with self.subTest(reference=reference):
                code, args = self.run_post(reference)
                self.assertEqual(code, 0)
                repository = reference.split(":", 1)[1].removeprefix("docker://")
                repository = repository.split("@")[0].rsplit(":", 1)[0]
                self.assertEqual(args, f"{switch} {repository}:stable")

    def test_no_origin_or_two_fail(self):
        self.assertEqual(self.run_post(), (1, None))
        self.assertEqual(self.run_post("ostree-unverified-registry:r/a:1", "ostree-unverified-registry:r/a:2"), (1, None))

    def test_an_unknown_reference_fails(self):
        self.assertEqual(self.run_post("ostree-unverified-image:oci:/run/install/repo/container"), (1, None))


if __name__ == "__main__":
    unittest.main()
