"""Unit tests of system/image-digests.sh, system/sign-images.sh, system/verify-images.sh and
system/tag-images.sh with a skopeo stub that keeps a registry in a directory
(python3 -B -m unittest discover -s system/tests -v)."""

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
VERIFY = ROOT / "system" / "verify-images.sh"
TAG = ROOT / "system" / "tag-images.sh"
A_KEY = (
    ROOT / "forge/specs/athanor-update/athanor-update-1.0.0/tests/vectors/made/a.pub"
)
NAMES = ["athanor-system", "athanor-system-nvidia", "athanor-system-nvidia-legacy"]
REG = "registry.example/owner"
KERNEL_LINES = [
    f"{REG}/azoth 7.2.9-100.azoth.fc43 sha256:{'a' * 64}",
    f"{REG}/azoth-devel 7.2.9-100.azoth.fc43 sha256:{'b' * 64}",
    f"{REG}/azoth-boot 7.2.9-100.azoth.fc43 sha256:{'c' * 64}",
    f"{REG}/azoth-nvidia 615-7.2.9-100.azoth.fc43 sha256:{'d' * 64}",
    f"{REG}/azoth-nvidia 580-7.2.9-100.azoth.fc43 sha256:{'e' * 64}",
    f"{REG}/azoth 7.2.9-100.azoth.fc43-microvm sha256:{'f' * 64}",
    f"{REG}/azoth-debuginfo 7.2.9-100.azoth.fc43 sha256:{'9' * 64}",
    f"{REG}/azoth-signer image.digest sha256:{'8' * 64}",
]
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
    lag_file = state / "lag.json"
    lag = json.loads(lag_file.read_text()) if lag_file.exists() else {}
    if args[0] == "inspect":
        ref = args[-1].removeprefix("docker://")
        if ref not in tags:
            sys.exit("manifest unknown")
        with open(state / "inspect.log", "a") as log:
            log.write(ref + "\\n")
        if lag.get(ref, [None, 0])[1] > 0:
            # The registry still serves the manifest the tag named before the copy.
            lag[ref][1] -= 1
            lag_file.write_text(json.dumps(lag))
            print(lag[ref][0])
        else:
            print(tags[ref])
    elif "--sign-by-sigstore-private-key" in args:
        # containers/image writes a sigstore attachment only where registries.d enables it.
        conf = pathlib.Path(args[args.index("--registries.d") + 1]) if "--registries.d" in args else None
        scope = args[-1].removeprefix("docker://").split("@")[0]
        enabled = f"  {scope}:\\n    use-sigstore-attachments: true\\n"
        if conf is None or enabled not in "".join(f.read_text() for f in conf.glob("*.yaml")):
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
    elif args[0] == "copy":
        # A tag moved onto a digest the registry holds; STUB_STALE leaves the tag where it was.
        assert "--preserve-digests" in args, args
        source, target = (a.removeprefix("docker://") for a in args[-2:])
        repository, digest = source.split("@")
        # The tag stays in the source repository.
        assert target.startswith(f"{repository}:"), args
        if not os.environ.get("STUB_STALE"):
            # STUB_LAG=N: the next N reads of the tag still return its previous digest.
            if os.environ.get("STUB_LAG"):
                lag[target] = [tags.get(target, "sha256:" + "0" * 64), int(os.environ["STUB_LAG"])]
                lag_file.write_text(json.dumps(lag))
            tags[target] = digest
            (state / "tags.json").write_text(json.dumps(tags))
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
        self.tags = {
            f"{REG}/{name}:412": "sha256:" + f"{i + 1}" * 64
            for i, name in enumerate(NAMES)
        }
        (self.state / "tags.json").write_text(json.dumps(self.tags))
        (self.state / "signed.json").write_text("[]")
        (self.dir / "keys").mkdir()
        shutil.copy(A_KEY, self.dir / "keys" / "athanor-image-1.pub")
        (self.dir / "runtime").mkdir(mode=0o700)
        (self.dir / "tmp").mkdir()
        self.env = {
            "PATH": f"{self.dir / 'bin'}:{os.environ['PATH']}",
            "STUB_STATE": str(self.state),
            "RETRY_ATTEMPTS": "1",
            "TAG_READBACK_DELAY": "0",
            "GITHUB_REF": "refs/heads/iso-v0",
            "SIGN_KEYS_DIR": str(self.dir / "keys"),
            "VERIFY_KEYS_DIR": str(self.dir / "keys"),
            "XDG_RUNTIME_DIR": str(self.dir / "runtime"),
            "TMPDIR": str(self.dir / "tmp"),
            "COSIGN_PRIVATE_KEY": SECRET,
            "COSIGN_PASSWORD": "correct horse",
        }
        self.file = self.dir / "artifacts" / "image-digests.txt"

    def tearDown(self):
        self.tmp.cleanup()

    def digests(self):
        return subprocess.run(
            [
                "bash",
                str(DIGESTS),
                "--registry",
                REG,
                "--tag",
                "412",
                "--out",
                str(self.file),
            ],
            capture_output=True,
            text=True,
            env=self.env,
        )

    def sign(self, **env):
        return subprocess.run(
            ["bash", str(SIGN), "--registry", REG, str(self.file)],
            capture_output=True,
            text=True,
            env={**self.env, **env},
        )

    def kernel_file(self, lines):
        path = self.dir / "artifacts" / "kernel-unsigned.txt"
        path.write_text("".join(f"{line}\n" for line in lines))
        tags = json.loads((self.state / "tags.json").read_text())
        for line in lines:
            repo, tag, digest = line.split()
            tags[f"{repo}:{tag}"] = digest
        (self.state / "tags.json").write_text(json.dumps(tags))
        return path

    def sign_kernel(self, path, **env):
        return subprocess.run(
            ["bash", str(SIGN), "--registry", REG, "--kernel-digests", str(path), str(self.file)],
            capture_output=True,
            text=True,
            env={**self.env, **env},
        )


    def verify(self, *args, **env):
        # The verification job holds no key.
        clean = {k: v for k, v in self.env.items() if not k.startswith("COSIGN_")}
        return subprocess.run(
            ["bash", str(VERIFY), "--registry", REG, *args, str(self.file)],
            capture_output=True,
            text=True,
            env={**clean, **env},
        )

    def tag(self, *args, **env):
        target = list(args) or [str(self.file)]
        return subprocess.run(
            ["bash", str(TAG), "--registry", REG, "--tag", "latest", *target],
            capture_output=True,
            text=True,
            env={**self.env, **env},
        )

    def calls(self):
        return [
            json.loads(line)
            for line in (self.state / "calls.log").read_text().splitlines()
        ]

    def test_the_build_job_records_three_digests(self):
        r = self.digests()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            self.file.read_text().splitlines(),
            [f"{REG}/{name} 412 {self.tags[f'{REG}/{name}:412']}" for name in NAMES],
        )

    def test_three_images_are_signed_by_digest_and_nothing_else(self):
        """The signing job signs and ends: no verification and no container beside the key."""
        self.digests()
        r = self.sign()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            json.loads((self.state / "signed.json").read_text()),
            list(self.tags.values()),
        )
        self.assertFalse(any("--policy" in c["args"] for c in self.calls()))
        self.assertFalse((self.state / "podman.log").exists())
        self.assertEqual(r.stdout.count("signed: "), 3)

    def test_the_key_reaches_skopeo_as_a_private_file_and_nowhere_else(self):
        self.digests()
        r = self.sign()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.state / "key.seen").read_text(), SECRET)
        self.assertEqual((self.state / "passphrase.seen").read_text(), "correct horse")
        for call in self.calls()[3:]:  # the calls of sign-images.sh
            self.assertEqual(
                call["env"], [], "COSIGN_* must not be in the environment of skopeo"
            )
            self.assertNotIn("not-a-real-key", " ".join(call["args"]))
            self.assertNotIn("correct horse", " ".join(call["args"]))
        self.assertEqual(
            list((self.dir / "runtime").iterdir()),
            [],
            "the private directory is removed on exit",
        )
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

    def test_the_signed_images_are_pulled_through_the_rendered_policy_without_the_key(
        self,
    ):
        self.digests()
        self.sign()
        before = len(self.calls())
        r = self.verify()
        self.assertEqual(r.returncode, 0, r.stderr)
        verified = [c for c in self.calls()[before:]]
        self.assertEqual(
            [c["args"][-2] for c in verified],
            [
                f"docker://{REG}/{name}@{self.tags[f'{REG}/{name}:412']}"
                for name in NAMES
            ],
        )
        self.assertTrue(
            all(
                "--policy" in c["args"] and "--registries.d" in c["args"]
                for c in verified
            )
        )
        self.assertTrue(all(c["env"] == [] for c in verified))
        self.assertEqual(r.stdout.count("verified with the shipped policy"), 3)
        self.assertEqual(
            list((self.dir / "tmp").iterdir()), [], "the verification pulls are removed"
        )

    def test_images_the_signing_job_did_not_sign_fail_the_verification(self):
        """A skipped or rejected signing job leaves the images unsigned: the verification job,
        which runs anyway, fails the run."""
        self.digests()
        r = self.verify()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("A signature was required", r.stderr)

    def test_a_signature_a_machine_would_not_accept_fails_the_verification(self):
        self.digests()
        self.sign()
        r = self.verify(STUB_REFUSE="1")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("A signature was required", r.stderr)

    def test_verification_writes_one_signature_evidence_per_image(self):
        self.digests()
        self.sign()
        out = self.dir / "evidence"
        run_id = self.file.read_text().split()[1]
        r = self.verify(
            "--evidence",
            str(out),
            "--build-run-id",
            run_id,
            GITHUB_SERVER_URL="https://github.com",
            GITHUB_REPOSITORY="ars-regia/athanor",
            GITHUB_RUN_ID="9",
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        for line in self.file.read_text().splitlines():
            repository, tag, digest = line.split()
            data = json.loads(
                (out / f"signature.{repository.rsplit('/', 1)[1]}.json").read_text()
            )
            self.assertEqual(
                (data["digest"], data["run_id"], data["verdict"]),
                (digest, int(tag), "pass"),
            )
            self.assertEqual(
                data["workflow_run_url"],
                "https://github.com/ars-regia/athanor/actions/runs/9",
            )

    def test_a_failed_verification_writes_no_evidence_for_that_image(self):
        self.digests()
        # No signature at all: the first image already fails the policy pull.
        out = self.dir / "evidence"
        r = self.verify(
            "--evidence",
            str(out),
            "--build-run-id",
            self.file.read_text().split()[1],
            GITHUB_SERVER_URL="https://github.com",
            GITHUB_REPOSITORY="ars-regia/athanor",
            GITHUB_RUN_ID="9",
        )
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(list(out.glob("signature.*.json")) if out.exists() else [], [])

    def test_a_digests_file_of_another_build_run_writes_no_evidence(self):
        self.digests()
        self.sign()
        out = self.dir / "evidence"
        r = self.verify(
            "--evidence",
            str(out),
            "--build-run-id",
            "1",
            GITHUB_SERVER_URL="https://github.com",
            GITHUB_REPOSITORY="ars-regia/athanor",
            GITHUB_RUN_ID="9",
        )
        self.assertEqual(r.returncode, 1)
        self.assertIn("build run 1", r.stderr)
        self.assertFalse(out.exists())

    def test_with_the_builder_the_verification_runs_in_its_image(self):
        self.digests()
        self.sign()
        r = self.verify("--builder", BUILDER)
        self.assertEqual(r.returncode, 0, r.stderr)
        runs = [
            json.loads(line)
            for line in (self.state / "podman.log").read_text().splitlines()
        ]
        self.assertEqual(len(runs), 3)
        for run in runs:
            self.assertEqual(run["image"], f"{REG}/athanor-builder:{BUILDER}")
            self.assertEqual(
                len(run["mounts"]), 2, "only the rendered policy and the public keys"
            )
            self.assertNotIn("-e", run["args"])
            self.assertIn("--cap-drop=all", run["args"])
        self.assertEqual(r.stdout.count("verified with the shipped policy"), 3)

    def test_a_builder_that_is_not_a_content_hash_is_refused_before_the_registry(self):
        self.digests()
        before = len(self.calls())
        for value in ("latest", BUILDER[:-1], f"{BUILDER} --privileged"):
            with self.subTest(value=value):
                r = self.verify("--builder", value)
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertEqual(len(self.calls()), before)
                self.assertFalse((self.state / "podman.log").exists())

    def test_latest_moves_to_the_recorded_digests_and_is_read_back(self):
        self.digests()
        recorded = {f"{REG}/{name}": self.tags[f"{REG}/{name}:412"] for name in NAMES}
        r = self.tag()
        self.assertEqual(r.returncode, 0, r.stderr)
        tags = json.loads((self.state / "tags.json").read_text())
        self.assertEqual(
            {repository: tags[f"{repository}:latest"] for repository in recorded},
            recorded,
        )
        self.assertEqual(r.stdout.count(":latest -> sha256:"), 3)

    def test_latest_moves_only_from_a_trusted_ref(self):
        self.digests()
        before = self.calls()
        for ref in ("refs/heads/feature/x", "refs/heads/iso-v0-x", "refs/tags/iso-v0", ""):
            with self.subTest(ref=ref):
                r = self.tag(GITHUB_REF=ref)
                self.assertEqual(r.returncode, 1)
                self.assertIn("not a trusted ref", r.stderr)
                self.assertEqual(self.calls(), before)
        self.assertEqual(self.tag(GITHUB_REF="refs/heads/main").returncode, 0)

    def test_a_tag_that_does_not_read_back_fails(self):
        self.digests()
        self.tags.update(
            {f"{REG}/{name}:latest": "sha256:" + "0" * 64 for name in NAMES}
        )
        (self.state / "tags.json").write_text(json.dumps(self.tags))
        r = self.tag(STUB_STALE="1")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("after the copy", r.stderr)

    def test_a_tag_the_registry_serves_late_is_read_again(self):
        """Right after the copy the registry may still serve the previous manifest: the
        read-back is tried again, a bounded number of times."""
        self.digests()
        r = self.tag(STUB_LAG="2")
        self.assertEqual(r.returncode, 0, r.stderr)
        reads = (self.state / "inspect.log").read_text().splitlines()
        for name in NAMES:
            self.assertEqual(reads.count(f"{REG}/{name}:latest"), 3)
        self.assertEqual(r.stdout.count(":latest -> sha256:"), 3)
        # Six stale reads exhaust the six attempts: the job fails instead of waiting on.
        tags = json.loads((self.state / "tags.json").read_text())
        tags.update({f"{REG}/{name}:latest": "sha256:" + "0" * 64 for name in NAMES})
        (self.state / "tags.json").write_text(json.dumps(tags))
        r = self.tag(STUB_LAG="6")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("after the copy", r.stderr)

    def test_the_iso_latest_moves_to_the_digest_of_its_run(self):
        iso = "sha256:" + "7" * 64
        self.tags[f"{REG}/athanor-iso:412"] = iso
        (self.state / "tags.json").write_text(json.dumps(self.tags))
        r = self.tag("--iso", "412")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            json.loads((self.state / "tags.json").read_text())[
                f"{REG}/athanor-iso:latest"
            ],
            iso,
        )
        for value in ("latest", "412 extra"):
            with self.subTest(value=value):
                self.assertEqual(self.tag("--iso", *value.split()).returncode, 2)

    def test_verify_and_tag_refuse_a_repository_outside_the_shipped_set(self):
        self.digests()
        lines = self.file.read_text().splitlines()
        self.file.write_text(
            "\n".join([f"{REG}/other 412 sha256:{'1' * 64}"] + lines[1:]) + "\n"
        )
        for run in (self.verify, self.tag):
            with self.subTest(script=run.__name__):
                (self.state / "calls.log").unlink(missing_ok=True)
                r = run()
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertIn("not a shipped repository", r.stderr)
                self.assertFalse((self.state / "calls.log").exists())

    def test_a_repository_outside_the_shipped_set_is_refused_before_signing(self):
        for line in (
            f"{REG}/other 412 sha256:{'1' * 64}",
            f"ghcr.io/elsewhere/athanor-system 412 sha256:{'1' * 64}",
        ):
            with self.subTest(line=line):
                self.digests()
                lines = self.file.read_text().splitlines()
                self.file.write_text("\n".join([line] + lines[1:]) + "\n")
                (self.state / "calls.log").unlink()
                r = self.sign()
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertIn("not a shipped repository", r.stderr)
                self.assertFalse((self.state / "calls.log").exists())

    def test_the_default_image_is_required_and_a_repeat_is_refused_before_signing(self):
        self.digests()
        lines = self.file.read_text().splitlines()
        for content, message in (
            ([lines[1], lines[2]], "the default image is required"),
            ([lines[0], lines[0], lines[1]], "twice"),
        ):
            with self.subTest(message=message):
                self.file.write_text("\n".join(content) + "\n")
                (self.state / "calls.log").unlink(missing_ok=True)
                r = self.sign()
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertIn(message, r.stderr)
                self.assertFalse((self.state / "calls.log").exists())

    def test_a_run_without_an_nvidia_variant_is_signed_for_the_images_it_built(self):
        self.digests()
        lines = self.file.read_text().splitlines()
        self.file.write_text("\n".join([lines[0], lines[1]]) + "\n")
        r = self.sign()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            json.loads((self.state / "signed.json").read_text()),
            list(self.tags.values())[:2],
        )

    def test_a_run_with_only_the_default_image_is_signed_for_it_alone(self):
        self.digests()
        lines = self.file.read_text().splitlines()
        self.file.write_text(lines[0] + "\n")
        r = self.sign()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            json.loads((self.state / "signed.json").read_text()),
            [self.tags[f"{REG}/athanor-system:412"]],
        )

    def test_the_digests_record_only_the_variants_that_built(self):
        variants = self.dir / "variants.txt"
        variants.write_text("athanor-system\n")
        r = subprocess.run(
            ["bash", str(DIGESTS), "--registry", REG, "--tag", "412", "--out", str(self.file), "--variants", str(variants)],
            capture_output=True,
            text=True,
            env=self.env,
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual([line.split()[0] for line in self.file.read_text().splitlines()], [f"{REG}/athanor-system"])

    def test_a_variants_file_that_is_not_a_set_with_the_default_image_is_refused(self):
        variants = self.dir / "variants.txt"
        for content, message in (
            ("athanor-system\nathanor-other\n", "athanor-other"),
            ("athanor-system\nathanor-system-nvidia\nathanor-system-nvidia\n", "twice"),
            ("athanor-system-nvidia\n", "the default image is required"),
            ("", "the default image is required"),
        ):
            with self.subTest(content=content):
                variants.write_text(content)
                r = subprocess.run(
                    ["bash", str(DIGESTS), "--registry", REG, "--tag", "412", "--out", str(self.file), "--variants", str(variants)],
                    capture_output=True,
                    text=True,
                    env=self.env,
                )
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertIn(message, r.stderr)
                self.assertFalse(self.file.exists())


    def test_the_kernel_artefacts_are_signed_after_the_images(self):
        """The stub writes an attachment only for a repository a registries.d file names."""
        self.digests()
        r = self.sign_kernel(self.kernel_file(KERNEL_LINES))
        self.assertEqual(r.returncode, 0, r.stderr)
        signed = json.loads((self.state / "signed.json").read_text())
        self.assertEqual(signed, list(self.tags.values()) + [line.split()[2] for line in KERNEL_LINES])
        self.assertEqual(r.stdout.count("signed: "), len(NAMES) + len(KERNEL_LINES))

    def test_an_empty_kernel_list_signs_the_images_alone(self):
        self.digests()
        r = self.sign_kernel(self.kernel_file([]))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads((self.state / "signed.json").read_text()), list(self.tags.values()))

    def test_the_kernel_registry_is_the_one_kernel_artifacts_uses(self):
        """KERNEL_REGISTRY moves the kernel artefacts, not the system images."""
        self.digests()
        moved = [line.replace(REG, "registry.example/kernel", 1) for line in KERNEL_LINES]
        path = self.kernel_file(moved)
        r = self.sign_kernel(path)
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertIn("not a kernel repository under", r.stderr)
        r = self.sign_kernel(path, KERNEL_REGISTRY="registry.example/kernel")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout.count("signed: "), len(NAMES) + len(KERNEL_LINES))
        # It reaches sed and the rendered registries.d: held to the pattern render-policy uses.
        r = self.sign_kernel(path, KERNEL_REGISTRY="registry.example/kernel|x")
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertIn("not a registry/owner", r.stderr)

    def test_a_kernel_line_outside_the_kernel_set_is_refused_before_signing(self):
        for lines in (
            [f"{REG}/athanor-system 412 sha256:{'d' * 64}"],
            [f"ghcr.io/elsewhere/azoth 7.2.9 sha256:{'d' * 64}"],
            [f"{REG}/azoth 7.2.9 latest"],
            [KERNEL_LINES[0], KERNEL_LINES[0]],
        ):
            with self.subTest(lines=lines):
                self.digests()
                path = self.dir / "artifacts" / "kernel-unsigned.txt"
                path.write_text("".join(f"{line}\n" for line in lines))
                (self.state / "calls.log").unlink(missing_ok=True)
                r = self.sign_kernel(path)
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertFalse((self.state / "calls.log").exists())


if __name__ == "__main__":
    unittest.main()
