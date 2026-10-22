"""Unit tests of forge/scripts/publish_tiers.sh with stubs of skopeo, buildah and createrepo_c
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import hashlib
import json
import os
import pathlib
import subprocess
import tempfile
import textwrap
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "publish_tiers.sh"
REGISTRY = (
    "ghcr.io/owner"  # the default host and the placeholder owner of scripts/verify.py
)
DIRS = {f"athanor-forge-tier{n}-repo": f"repo-cache/repo-tier{n}" for n in range(4)}
DIRS["athanor-forge-rolling-repo"] = "repo-cache/repo"
WORKFLOW = "https://github.com/ars-regia/athanor/.github/workflows/call-system-image.yml"
TRUSTED = f"{WORKFLOW}@refs/heads/iso-v0"


def digest(name, generation):
    return "sha256:" + hashlib.sha256(f"{name}:{generation}".encode()).hexdigest()


class PublishTiers(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        self.forge = self.dir / "forge"
        self.published = (
            self.dir / "published"
        )  # <name>.json: what skopeo inspect answers
        self.published.mkdir()
        self.signatures = self.dir / "signatures"  # sha256(ref): the signer's identity
        self.signatures.mkdir()
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        stubs = {
            "skopeo": f"""\
                printf '%s\\n' "$2" >> {self.dir}/skopeo.log
                name=${{2#docker://{REGISTRY}/}}
                name=${{name%:*}}
                [[ $1 == inspect && -f {self.published}/$name.json ]] || exit 1
                cat {self.published}/$name.json
                """,
            # push --digestfile FILE IMAGE writes the digest of the new image; FAIL_PUSH names an
            # image whose push fails.
            "buildah": f"""\
                printf '%s\\n' "$*" >> {self.dir}/buildah.log
                case $1 in
                  from) echo ctr ;;
                  push)
                    [[ $4 != "${{FAIL_PUSH:-}}" ]] || exit 1
                    name=${{4#{REGISTRY}/}}
                    printf 'sha256:%s' "$(printf '%s' "${{name%:*}}:new" | sha256sum | cut -d' ' -f1)" > "$3" ;;
                esac
                """,
            "createrepo_c": "mkdir -p repodata\n",
            # verify answers from the signatures directory with cosign v3's messages; sign
            # records the reference. COSIGN_ERROR names a reference whose verification fails on
            # a transport error, FAIL_SIGN one whose signature fails.
            "cosign": f"""\
                printf '%s\\n' "$*" >> {self.dir}/cosign.log
                ref=${{*: -1}}
                case $1 in
                  sign)
                    [[ $ref != "${{FAIL_SIGN:-}}" ]] || exit 1
                    printf '%s\\n' "$ref" >> {self.dir}/signed.log ;;
                  verify)
                    if [[ $ref == "${{COSIGN_ERROR:-}}" ]]; then
                      echo 'Error: getting trusted root: GET https://tuf-repo-cdn.sigstore.dev/timestamp.json: 502 Bad Gateway' >&2
                      exit 1
                    fi
                    f={self.signatures}/$(printf '%s' "$ref" | sha256sum | cut -d' ' -f1)
                    [[ -f $f ]] || {{ echo 'Error: no signatures found' >&2; exit 10; }}
                    identity=$(< "$f")
                    if ! [[ $identity =~ $3 ]]; then
                      echo "Error: no matching signatures: failed to verify certificate identity: no matching CertificateIdentity found, last error: expected SAN value to match regex \\"$3\\", got \\"$identity\\"" >&2
                      exit 1
                    fi ;;
                esac
                """,
        }
        for tool, body in stubs.items():
            (bin_dir / tool).write_text(
                "#!/bin/bash\nset -euo pipefail\n" + textwrap.dedent(body)
            )
            (bin_dir / tool).chmod(0o755)
        for name, rel in DIRS.items():
            (self.forge / rel / name).mkdir(parents=True)
            (self.forge / rel / name / f"{name}-1.rpm").write_text(name)
        self.out = self.dir / "out" / "tier-digests.json"
        self.env = {
            k: v
            for k, v in os.environ.items()
            if k
            not in (
                "REGISTRY_HOST",
                "GITHUB_REPOSITORY_OWNER",
                "GITHUB_REPOSITORY",
                "GITHUB_SERVER_URL",
                "TIER_TRUSTED_REFS",
                "FAIL_PUSH",
                "GITHUB_REF",
            )
        }
        self.env["RETRY_ATTEMPTS"] = "1"
        self.env["PATH"] = f"{bin_dir}:{os.environ['PATH']}"
        self.env["GITHUB_REPOSITORY_OWNER"] = "owner"
        self.env["GITHUB_REF"] = "refs/heads/iso-v0"

    def tearDown(self):
        self.tmp.cleanup()

    def content_hash(self, name):
        """The tier.content.sha256 label of the RPMs under DIRS[name], as the script computes it."""
        rpms = sorted((self.forge / DIRS[name]).rglob("*.rpm"))
        lines = "".join(
            f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(self.forge)}\n"
            for p in rpms
        )
        return hashlib.sha256(lines.encode()).hexdigest()

    def sign(self, ref, identity):
        key = hashlib.sha256(ref.encode()).hexdigest()
        (self.signatures / key).write_text(identity)

    def signed(self):
        log = self.dir / "signed.log"
        return log.read_text().split() if log.exists() else []

    def publish_current(self, *names, signer=TRUSTED):
        """Publish NAMES with the RPM content they have now, at generation "old", signed by
        SIGNER (None: unsigned)."""
        for name in names:
            if signer is not None:
                self.sign(f"{REGISTRY}/{name}@{digest(name, 'old')}", signer)
            (self.published / f"{name}.json").write_text(
                json.dumps(
                    {
                        "Digest": digest(name, "old"),
                        "Labels": {"tier.content.sha256": self.content_hash(name)},
                    }
                )
            )

    def run_script(self):
        return subprocess.run(
            ["bash", str(SCRIPT), str(self.out)],
            cwd=self.forge,
            capture_output=True,
            text=True,
            env=self.env,
        )

    def pushed(self):
        log = self.dir / "buildah.log"
        return (
            [
                line.split()[-1]
                for line in log.read_text().splitlines()
                if line.startswith("push ")
            ]
            if log.exists()
            else []
        )

    def test_unchanged_tiers_are_not_pushed_and_keep_the_published_digest(self):
        self.publish_current(*DIRS)
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.pushed(), [])
        expected = {
            "registry": REGISTRY,
            **{
                f"tier{n}": digest(f"athanor-forge-tier{n}-repo", "old")
                for n in range(4)
            },
        }
        self.assertEqual(json.loads(self.out.read_text()), expected)

    def test_a_changed_or_new_tier_is_pushed_and_recorded_by_the_digest_of_the_push(
        self,
    ):
        self.publish_current(
            "athanor-forge-tier0-repo",
            "athanor-forge-tier2-repo",
            "athanor-forge-tier3-repo",
        )
        (self.forge / DIRS["athanor-forge-tier3-repo"] / "extra.rpm").write_text(
            "new"
        )  # tier1 never published
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertCountEqual(
            self.pushed(),
            [
                f"{REGISTRY}/athanor-forge-{t}-repo:latest"
                for t in ("tier1", "tier3", "rolling")
            ],
        )
        tiers = json.loads(self.out.read_text())
        self.assertEqual(tiers["tier0"], digest("athanor-forge-tier0-repo", "old"))
        self.assertEqual(tiers["tier1"], digest("athanor-forge-tier1-repo", "new"))
        self.assertEqual(tiers["tier2"], digest("athanor-forge-tier2-repo", "old"))
        self.assertEqual(tiers["tier3"], digest("athanor-forge-tier3-repo", "new"))

    def test_the_registry_follows_the_variables_in_lower_case(self):
        self.env.update(
            REGISTRY_HOST="Registry.Example", GITHUB_REPOSITORY_OWNER="HR-MES"
        )
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            json.loads(self.out.read_text())["registry"], "registry.example/hr-mes"
        )
        self.assertIn(
            "registry.example/hr-mes/athanor-forge-tier0-repo:latest",
            (self.dir / "buildah.log").read_text(),
        )

    def test_a_failed_push_fails_the_run_and_writes_no_file(self):
        self.publish_current(
            "athanor-forge-tier0-repo",
            "athanor-forge-tier1-repo",
            "athanor-forge-tier3-repo",
        )
        self.env["FAIL_PUSH"] = f"{REGISTRY}/athanor-forge-tier2-repo:latest"
        r = self.run_script()
        self.assertNotEqual(r.returncode, 0)
        self.assertFalse(self.out.exists())

    def test_every_pushed_digest_is_signed_and_a_reused_one_is_not_signed_again(self):
        self.publish_current("athanor-forge-tier0-repo", "athanor-forge-tier2-repo")
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertCountEqual(
            self.signed(),
            [
                f"{REGISTRY}/athanor-forge-{t}-repo@{digest(f'athanor-forge-{t}-repo', 'new')}"
                for t in ("tier1", "tier3", "rolling")
            ],
        )

    def test_an_unsigned_or_foreign_signed_tier_is_pushed_again_and_signed(self):
        for signer in (
            None,
            f"{WORKFLOW}@refs/heads/feature",
            "https://github.com/fork/athanor/.github/workflows/call-system-image.yml@refs/heads/iso-v0",
        ):
            with self.subTest(signer=signer):
                for log in ("buildah.log", "signed.log"):
                    (self.dir / log).unlink(missing_ok=True)
                self.publish_current(*DIRS)
                self.publish_current("athanor-forge-tier2-repo", signer=signer)
                key = f"{REGISTRY}/athanor-forge-tier2-repo@{digest('athanor-forge-tier2-repo', 'old')}"
                if signer is None:
                    (self.signatures / hashlib.sha256(key.encode()).hexdigest()).unlink()
                r = self.run_script()
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertIn("not signed by a trusted run: pushed again", r.stdout)
                self.assertEqual(self.pushed(), [f"{REGISTRY}/athanor-forge-tier2-repo:latest"])
                new = digest("athanor-forge-tier2-repo", "new")
                self.assertEqual(self.signed(), [f"{REGISTRY}/athanor-forge-tier2-repo@{new}"])
                self.assertEqual(json.loads(self.out.read_text())["tier2"], new)

    def test_a_branch_run_reuses_what_its_branch_or_a_trusted_one_signed(self):
        self.env["GITHUB_REF"] = "refs/heads/feature/x"
        self.publish_current(*DIRS, signer=f"{WORKFLOW}@refs/heads/feature/x")
        self.publish_current("athanor-forge-tier1-repo", signer=f"{WORKFLOW}@refs/heads/main")
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.pushed(), [])
        self.assertEqual(self.signed(), [])

    def test_a_tier_another_rebase_branch_signed_is_pushed_again_signed(self):
        self.env["GITHUB_REF"] = "refs/heads/iso-v0"
        self.publish_current(*DIRS)
        self.publish_current(
            "athanor-forge-tier2-repo", signer=f"{WORKFLOW}@refs/heads/rebase/x"
        )
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.pushed(), [f"{REGISTRY}/athanor-forge-tier2-repo:latest"])

    def test_a_signature_check_error_fails_the_run_signed(self):
        self.publish_current(*DIRS)
        self.env["COSIGN_ERROR"] = (
            f"{REGISTRY}/athanor-forge-tier1-repo@{digest('athanor-forge-tier1-repo', 'old')}"
        )
        r = self.run_script()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("502 Bad Gateway", r.stderr)
        self.assertFalse(self.out.exists())

    def test_a_failed_signature_fails_the_run_signed(self):
        self.env["FAIL_SIGN"] = (
            f"{REGISTRY}/athanor-forge-tier3-repo@{digest('athanor-forge-tier3-repo', 'new')}"
        )
        r = self.run_script()
        self.assertNotEqual(r.returncode, 0)
        self.assertFalse(self.out.exists())

    def test_a_usage_error_without_out(self):
        r = subprocess.run(
            ["bash", str(SCRIPT)],
            cwd=self.forge,
            capture_output=True,
            text=True,
            env=self.env,
        )
        self.assertEqual(r.returncode, 2)

    def assert_tag(self, ref, tag):
        """A run on REF reads and pushes :TAG only, for all five images."""
        self.env["GITHUB_REF"] = ref
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(self.pushed()), 5)
        reads = (self.dir / "skopeo.log").read_text().split()
        self.assertEqual(len(reads), 5)
        for reference in reads + self.pushed():
            self.assertTrue(reference.endswith(f":{tag}"), reference)
        for log in ("skopeo.log", "buildah.log"):
            (self.dir / log).unlink()

    def test_the_default_branches_push_latest(self):
        for ref in ("refs/heads/iso-v0", "refs/heads/main"):
            with self.subTest(ref=ref):
                self.assert_tag(ref, "latest")

    def test_another_branch_pushes_a_branch_tag(self):
        self.assert_tag("refs/heads/rebase/fedora-45", "branch-rebase-fedora-45")

    def test_a_pull_request_ref_pushes_a_branch_tag(self):
        self.assert_tag("refs/pull/618/merge", "branch-pull-618-merge")

    def test_an_unset_or_empty_ref_fails_closed(self):
        for ref in (None, ""):
            with self.subTest(ref=ref):
                self.env.pop("GITHUB_REF", None)
                if ref is not None:
                    self.env["GITHUB_REF"] = ref
                r = self.run_script()
                self.assertEqual(r.returncode, 2)
                self.assertEqual(self.pushed(), [])
                self.assertFalse(self.out.exists())


if __name__ == "__main__":
    unittest.main()
