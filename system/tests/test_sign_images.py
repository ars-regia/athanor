"""Unit tests of system/image-digests.sh and system/sign-images.sh with a skopeo stub that
keeps a registry in a directory (python3 -B -m unittest discover -s system/tests -v)."""

import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import textwrap
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SIGN = ROOT / "system" / "sign-images.sh"
DIGESTS = ROOT / "system" / "image-digests.sh"
A_KEY = ROOT / "forge/specs/athanor-update/athanor-update-1.0.0/tests/vectors/made/a.pub"
NAMES = ["athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy"]
REG = "registry.example/owner"
SECRET = "-----BEGIN ENCRYPTED SIGSTORE PRIVATE KEY-----\nnot-a-real-key\n-----END ENCRYPTED SIGSTORE PRIVATE KEY-----\n"

STUB = textwrap.dedent("""\
    #!/usr/bin/env python3
    # A skopeo that knows tags.json (ref -> digest) and records what it signs in signed.json.
    # It signs only a copy of repo@digest onto itself, of a manifest the registry holds.
    import json, os, pathlib, stat, sys
    state = pathlib.Path(os.environ["STUB_STATE"])
    args = sys.argv[1:]
    with open(state / "calls.log", "a") as log:
        log.write(json.dumps({"args": args, "env": sorted(k for k in os.environ if k.startswith("COSIGN_"))}) + "\\n")
    tags = json.loads((state / "tags.json").read_text())
    signed = json.loads((state / "signed.json").read_text())
    if args[0] == "inspect":
        ref = args[-1].removeprefix("docker://")
        if ref not in tags:
            sys.exit("manifest unknown")
        print(tags[ref])
    elif "--sign-by-sigstore-private-key" in args:
        # containers/image writes a sigstore attachment only where registries.d enables it.
        conf = pathlib.Path(args[args.index("--registries.d") + 1]) if "--registries.d" in args else None
        if conf is None or "use-sigstore-attachments: true" not in "".join(f.read_text() for f in conf.glob("*.yaml")):
            sys.exit("writing signatures: writing sigstore attachments is disabled by configuration")
        key = pathlib.Path(args[args.index("--sign-by-sigstore-private-key") + 1])
        phrase = pathlib.Path(args[args.index("--sign-passphrase-file") + 1])
        for secret in (key, phrase):
            mode = stat.S_IMODE(secret.stat().st_mode)
            parent = stat.S_IMODE(secret.parent.stat().st_mode)
            assert mode == 0o600 and parent == 0o700, (secret, oct(mode), oct(parent))
        (state / "key.seen").write_text(key.read_text())
        (state / "passphrase.seen").write_text(phrase.read_text())
        assert args[-2] == args[-1] and "@sha256:" in args[-1], f"signed by tag: {args[-2:]}"
        repository, digest = args[-1].removeprefix("docker://").split("@")
        if not any(ref.startswith(f"{repository}:") and d == digest for ref, d in tags.items()):
            sys.exit("manifest unknown")
        signed.append(digest)
        (state / "signed.json").write_text(json.dumps(signed))
    elif "--policy" in args:
        policy = json.loads(pathlib.Path(args[args.index("--policy") + 1]).read_text())
        assert policy["default"] == [{"type": "reject"}]
        digest = args[-2].split("@")[1]
        if digest not in signed or os.environ.get("STUB_REFUSE"):
            sys.exit("Source image rejected: A signature was required, but no signature exists")
        pull = pathlib.Path(args[-1].removeprefix("dir:"))
        if os.environ.get("STUB_IN_CONTAINER"):
            assert str(pull) == "/var/tmp/verified", f"the container pulls outside its own /var/tmp: {pull}"
        else:
            assert not pull.is_relative_to(os.environ["XDG_RUNTIME_DIR"]), f"a whole image pulled into the memory-backed key directory: {pull}"
            pull.mkdir()
    else:
        sys.exit(f"stub skopeo: unsupported {args}")
    """)


PODMAN = textwrap.dedent("""\
    #!/usr/bin/env python3
    # A podman that records the container it would start and runs its command on the host,
    # after checking that every mount is read-only and that no key file sits beside one.
    import json, os, pathlib, sys
    state = pathlib.Path(os.environ["STUB_STATE"])
    args = sys.argv[1:]
    assert args[:2] == ["run", "--rm"], args
    mounts = [args[i + 1] for i, a in enumerate(args) if a == "-v"]
    for mount in mounts:
        assert mount.endswith(":ro"), mount
        host = pathlib.Path(mount.split(":")[0])
        for d in (host, *host.parents):
            assert not (d / "key").exists() and not (d / "passphrase").exists(), f"key file reachable from {mount}"
    image = next(a for a in args if "/athanor-builder:" in a)
    with open(state / "podman.log", "a") as log:
        log.write(json.dumps({"mounts": mounts, "image": image, "args": args}) + "\\n")
    cmd = args[args.index(image) + 1:]
    os.execvpe(cmd[0], cmd, {**os.environ, "STUB_IN_CONTAINER": "1"})
    """)
BUILDER = "ab" * 32


class SignImages(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        (self.dir / "bin").mkdir()
        stub = self.dir / "bin" / "skopeo"
        stub.write_text(STUB)
        stub.chmod(0o755)
        podman = self.dir / "bin" / "podman"
        podman.write_text(PODMAN)
        podman.chmod(0o755)
        self.state = self.dir / "state"
        self.state.mkdir()
        self.tags = {f"{REG}/{name}:412": "sha256:" + f"{i + 1}" * 64 for i, name in enumerate(NAMES)}
        (self.state / "tags.json").write_text(json.dumps(self.tags))
        (self.state / "signed.json").write_text("[]")
        (self.dir / "keys").mkdir()
        shutil.copy(A_KEY, self.dir / "keys" / "athanor-image-1.pub")
        (self.dir / "runtime").mkdir(mode=0o700)
        (self.dir / "tmp").mkdir()
        self.env = {"PATH": f"{self.dir / 'bin'}:{os.environ['PATH']}", "STUB_STATE": str(self.state), "RETRY_ATTEMPTS": "1",
                    "SIGN_KEYS_DIR": str(self.dir / "keys"), "XDG_RUNTIME_DIR": str(self.dir / "runtime"), "TMPDIR": str(self.dir / "tmp"),
                    "COSIGN_PRIVATE_KEY": SECRET, "COSIGN_PASSWORD": "correct horse"}
        self.file = self.dir / "artifacts" / "image-digests.txt"

    def tearDown(self):
        self.tmp.cleanup()

    def digests(self):
        return subprocess.run(["bash", str(DIGESTS), "--registry", REG, "--tag", "412", "--out", str(self.file)], capture_output=True, text=True, env=self.env)

    def sign(self, **env):
        return subprocess.run(["bash", str(SIGN), "--registry", REG, str(self.file)], capture_output=True, text=True, env={**self.env, **env})

    def calls(self):
        return [json.loads(line) for line in (self.state / "calls.log").read_text().splitlines()]

    def test_the_build_job_records_three_digests(self):
        r = self.digests()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.file.read_text().splitlines(), [f"{REG}/{name} 412 {self.tags[f'{REG}/{name}:412']}" for name in NAMES])

    def test_three_images_are_signed_then_pulled_through_the_rendered_policy(self):
        self.digests()
        r = self.sign()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads((self.state / "signed.json").read_text()), list(self.tags.values()))
        verified = [c["args"] for c in self.calls() if "--policy" in c["args"]]
        self.assertEqual([a[-2] for a in verified], [f"docker://{REG}/{name}@{self.tags[f'{REG}/{name}:412']}" for name in NAMES])
        self.assertTrue(all("--registries.d" in a for a in verified))
        self.assertEqual(r.stdout.count("signed and verified with the shipped policy"), 3)
        self.assertEqual(list((self.dir / "tmp").iterdir()), [], "the verification pulls are removed")

    def test_the_key_reaches_skopeo_as_a_private_file_and_nowhere_else(self):
        self.digests()
        r = self.sign()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.state / "key.seen").read_text(), SECRET)
        self.assertEqual((self.state / "passphrase.seen").read_text(), "correct horse")
        for call in self.calls()[3:]:  # the calls of sign-images.sh
            self.assertEqual(call["env"], [], "COSIGN_* must not be in the environment of skopeo")
            self.assertNotIn("not-a-real-key", " ".join(call["args"]))
            self.assertNotIn("correct horse", " ".join(call["args"]))
        self.assertEqual(list((self.dir / "runtime").iterdir()), [], "the private directory is removed on exit")
        self.assertNotIn("not-a-real-key", r.stdout + r.stderr)

    def test_without_the_key_the_job_fails_before_touching_the_registry(self):
        self.digests()
        before = len(self.calls())
        r = self.sign(COSIGN_PRIVATE_KEY="")
        self.assertEqual(r.returncode, 2)
        self.assertIn("COSIGN_PRIVATE_KEY is not available", r.stderr)
        self.assertEqual(len(self.calls()), before)

    def test_the_recorded_digest_is_signed_even_after_its_tag_moved(self):
        """Every reference of the job is repo@digest: a tag that moves between the build and
        the signature changes nothing that is signed, and no tag is ever read."""
        self.digests()
        before = len(self.calls())
        recorded = list(self.tags.values())
        self.tags[f"{REG}/athanor-system:old"] = self.tags[f"{REG}/athanor-system:412"]
        self.tags[f"{REG}/athanor-system:412"] = "sha256:" + "9" * 64
        (self.state / "tags.json").write_text(json.dumps(self.tags))
        r = self.sign()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads((self.state / "signed.json").read_text()), recorded)
        for call in self.calls()[before:]:
            self.assertNotIn("inspect", call["args"])
            self.assertFalse(any(":412" in a for a in call["args"]), call["args"])

    def test_with_the_builder_the_verification_runs_in_its_image_without_the_key(self):
        self.digests()
        r = self.sign(SIGN_VERIFY_BUILDER=BUILDER)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads((self.state / "signed.json").read_text()), list(self.tags.values()))
        runs = [json.loads(line) for line in (self.state / "podman.log").read_text().splitlines()]
        self.assertEqual(len(runs), 3)
        for run in runs:
            self.assertEqual(run["image"], f"{REG}/athanor-builder:{BUILDER}")
            self.assertEqual(len(run["mounts"]), 2, "only the rendered policy and the public keys")
            self.assertNotIn("-e", run["args"])
            self.assertIn("--cap-drop=all", run["args"])
            self.assertNotIn("--sign-by-sigstore-private-key", run["args"])
        self.assertTrue(all("--policy" not in c["args"] for c in self.calls() if "--sign-by-sigstore-private-key" in c["args"]))
        self.assertEqual(r.stdout.count("signed and verified with the shipped policy"), 3)

    def test_a_builder_that_is_not_a_content_hash_is_refused_before_signing(self):
        self.digests()
        for value in ("latest", BUILDER[:-1], f"{BUILDER} --privileged"):
            with self.subTest(value=value):
                r = self.sign(SIGN_VERIFY_BUILDER=value)
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertEqual(json.loads((self.state / "signed.json").read_text()), [])
                self.assertFalse((self.state / "podman.log").exists())

    def test_a_signature_a_machine_would_not_accept_fails_the_job(self):
        self.digests()
        r = self.sign(STUB_REFUSE="1")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("A signature was required", r.stderr)

    def test_a_repository_outside_the_shipped_set_is_refused_before_signing(self):
        for line in (f"{REG}/other 412 sha256:{'1' * 64}", f"ghcr.io/elsewhere/athanor-system 412 sha256:{'1' * 64}"):
            with self.subTest(line=line):
                self.digests()
                lines = self.file.read_text().splitlines()
                self.file.write_text("\n".join([line] + lines[1:]) + "\n")
                (self.state / "calls.log").unlink()
                r = self.sign()
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertIn("not a shipped repository", r.stderr)
                self.assertFalse((self.state / "calls.log").exists())

    def test_a_missing_or_repeated_repository_is_refused_before_signing(self):
        self.digests()
        lines = self.file.read_text().splitlines()
        for content, message in (([lines[0], lines[1]], "2 of the 3"), ([lines[0], lines[0], lines[1]], "twice")):
            with self.subTest(message=message):
                self.file.write_text("\n".join(content) + "\n")
                (self.state / "calls.log").unlink(missing_ok=True)
                r = self.sign()
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertIn(message, r.stderr)
                self.assertFalse((self.state / "calls.log").exists())


if __name__ == "__main__":
    unittest.main()
