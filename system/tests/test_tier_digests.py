"""Unit tests of system/tier-digests.sh against the offline registry of fake_registry.py
(python3 -B -m unittest discover -s system/tests -v)."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "tier-digests.sh"
DIGESTS = {f"tier{n}": "sha256:" + "0123"[n] * 64 for n in range(4)}
WORKFLOW = (
    "https://github.com/ars-regia/athanor/.github/workflows/call-system-image.yml"
)
TRUSTED = f"{WORKFLOW}@refs/heads/iso-v0"


class TierDigests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        for tool in ("skopeo", "cosign"):
            (bin_dir / tool).symlink_to(HERE / "fake_registry.py")
        self.fixture = self.dir / "registry.json"
        self.env = {
            k: v
            for k, v in os.environ.items()
            if k
            not in (
                "REGISTRY_HOST",
                "GITHUB_REPOSITORY_OWNER",
                "GITHUB_REPOSITORY",
                "GITHUB_SERVER_URL",
                "GITHUB_REF",
                "GITHUB_BASE_REF",
                "TIER_TRUSTED_REFS",
            )
        }
        self.env.update(
            PATH=f"{bin_dir}:{os.environ['PATH']}",
            FAKE_REGISTRY=str(self.fixture),
            FAKE_LOG=str(self.dir / "calls.log"),
            TIER_DIGESTS_DIR=str(self.dir / "out"),
            RETRY_ATTEMPTS="1",
        )
        self.out = self.dir / "out" / "tier-digests.json"

    def tearDown(self):
        self.tmp.cleanup()

    def publish(self, registry, signers=None, transient=(), tag="latest", **digests):
        """Tag each tier :TAG at its digest, signed by TRUSTED unless SIGNERS names another
        identity (None: no signature) for that tier; TRANSIENT tiers fail cosign verify with a
        Rekor outage."""
        signers = signers or {}
        tags = {
            f"{registry}/athanor-forge-{tier}-repo:{tag}": d
            for tier, d in digests.items()
        }
        signatures = {
            f"{registry}/athanor-forge-{tier}-repo@{d}": signers.get(tier, TRUSTED)
            for tier, d in digests.items()
            if signers.get(tier, TRUSTED) is not None
        }
        errors = [
            f"{registry}/athanor-forge-{tier}-repo@{digests[tier]}"
            for tier in transient
        ]
        self.fixture.write_text(
            json.dumps(
                {
                    "tags": tags,
                    "signatures": signatures,
                    "signature_transient_errors": errors,
                }
            )
        )

    def resolve(self, *args):
        return subprocess.run(
            ["bash", str(SCRIPT), *args], capture_output=True, text=True, env=self.env
        )

    def signed(self, tier):
        return self.resolve(
            "signed", f"ghcr.io/ars-regia/athanor-forge-{tier}-repo@{DIGESTS[tier]}"
        )

    def test_signed_prints_the_verdict_of_one_reference(self):
        self.publish("ghcr.io/ars-regia", signers={"tier1": None}, **DIGESTS)
        for tier, verdict in (("tier0", "signed"), ("tier1", "unsigned")):
            with self.subTest(tier=tier):
                r = self.signed(tier)
                self.assertEqual(
                    (r.returncode, r.stdout), (0, verdict + "\n"), r.stderr
                )
        self.assertIn(
            "athanor-forge-tier1-repo@" + DIGESTS["tier1"] + " has no signature",
            r.stderr,
        )

    def test_a_foreign_identity_is_unsigned(self):
        for identity in (
            f"{WORKFLOW}@refs/heads/feature",
            f"{WORKFLOW}@refs/pull/7/merge",
            "https://github.com/fork/athanor/.github/workflows/call-system-image.yml@refs/heads/iso-v0",
            "https://github.com/ars-regia/athanor/.github/workflows/kernel-build.yml@refs/heads/iso-v0",
            f"{WORKFLOW}@refs/heads/iso-v0x",
            f"{WORKFLOW}@refs/heads/main-evil",
            f"{WORKFLOW}@refs/heads/rebase/fedora-45",
            f"{WORKFLOW}@refs/heads/rebase",
        ):
            with self.subTest(identity=identity):
                self.publish(
                    "ghcr.io/ars-regia", signers={"tier3": identity}, **DIGESTS
                )
                r = self.signed("tier3")
                self.assertEqual((r.returncode, r.stdout), (0, "unsigned\n"), r.stderr)
                self.assertIn(
                    f"is signed as {identity}, not by a trusted run", r.stderr
                )

    def test_the_trusted_branches_and_the_run_s_own_branch_are_signed(self):
        for identity, ref in (
            (f"{WORKFLOW}@refs/heads/main", None),
            (f"{WORKFLOW}@refs/heads/rebase/fedora-45", "refs/heads/rebase/fedora-45"),
            (f"{WORKFLOW}@refs/heads/feature/x", "refs/heads/feature/x"),
        ):
            with self.subTest(identity=identity):
                self.env.pop("GITHUB_REF", None)
                if ref:
                    self.env["GITHUB_REF"] = ref
                self.publish(
                    "ghcr.io/ars-regia", signers={"tier0": identity}, **DIGESTS
                )
                r = self.signed("tier0")
                self.assertEqual((r.returncode, r.stdout), (0, "signed\n"), r.stderr)

    def test_a_pull_request_run_does_not_trust_its_own_ref(self):
        self.env["GITHUB_REF"] = "refs/pull/7/merge"
        self.publish(
            "ghcr.io/ars-regia",
            signers={"tier0": f"{WORKFLOW}@refs/pull/7/merge"},
            **DIGESTS,
        )
        self.assertEqual(self.signed("tier0").stdout, "unsigned\n")

    def test_a_branch_whose_name_needs_escaping_trusts_only_the_default_branches(self):
        self.env["GITHUB_REF"] = "refs/heads/fix/c++"
        self.publish("ghcr.io/ars-regia", **DIGESTS)
        r = self.signed("tier0")
        self.assertEqual((r.returncode, r.stdout), (0, "signed\n"), r.stderr)
        self.assertIn("'fix/c++' is not trusted for its own tiers", r.stderr)

    def test_a_cosign_failure_is_an_error_not_a_missing_signature(self):
        self.publish("ghcr.io/ars-regia", transient=("tier2",), **DIGESTS)
        r = self.signed("tier2")
        self.assertEqual((r.returncode, r.stdout), (1, ""))
        self.assertIn("rekor lookup: 502 Bad Gateway", r.stderr)
        self.assertNotIn("has no signature", r.stderr)

    def test_signed_and_probe_take_one_reference(self):
        for args in (("signed",), ("probe",), ("signed", "a", "b")):
            with self.subTest(args=args):
                self.assertEqual(self.resolve(*args).returncode, 2)

    def cosign_stub(self, body):
        """Replace the fake cosign with a bash script of BODY."""
        stub = self.dir / "bin" / "cosign"
        stub.unlink()
        stub.write_text(f"#!/usr/bin/env bash\n{body}\n")
        stub.chmod(0o755)

    def test_a_legacy_or_key_signature_is_foreign(self):
        # What cosign v3 prints for a cosign v2 signature or one made with a key.
        empty_key = (
            "Error: no matching signatures: error verifying bundle: empty key\n"
            "main.go:74: error during command execution: no matching signatures: "
            "error verifying bundle: empty key\n"
        )
        foreign = "https://github.com/fork/athanor/.github/workflows/x.yml@refs/heads/main"
        subjects = (
            "Error: none of the expected identities matched what was in the "
            f"certificate, got subjects [{foreign}]\n"
        )
        for stderr, says in (
            (empty_key, "has a signature in the legacy format or by a key"),
            (empty_key + subjects, f"is signed as {foreign}, not by a trusted run"),
        ):
            with self.subTest(says=says):
                (self.dir / "stderr").write_text(stderr)
                self.cosign_stub(f"cat {self.dir}/stderr >&2; exit 1")
                r = self.signed("tier0")
                self.assertEqual((r.returncode, r.stdout), (0, "unsigned\n"), r.stderr)
                self.assertIn(says, r.stderr)

    def test_resolve_records_the_registry_and_the_digest_each_tier_names(self):
        self.publish("ghcr.io/ars-regia", **DIGESTS)
        r = self.resolve("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        tiers = json.loads(self.out.read_text())
        self.assertRegex(tiers.pop("resolved"), r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$")
        self.assertEqual(tiers, {"registry": "ghcr.io/ars-regia", **DIGESTS})

    def test_the_registry_follows_the_variables_in_lower_case(self):
        self.publish("registry.example/hr-mes", **DIGESTS)
        self.env.update(
            REGISTRY_HOST="registry.example", GITHUB_REPOSITORY_OWNER="HR-MES"
        )
        r = self.resolve("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            json.loads(self.out.read_text())["registry"], "registry.example/hr-mes"
        )

    def test_a_tier_that_cannot_be_read_writes_no_file(self):
        self.publish(
            "ghcr.io/ars-regia", **{k: v for k, v in DIGESTS.items() if k != "tier2"}
        )
        r = self.resolve("resolve")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("athanor-forge-tier2-repo:latest", r.stderr)
        self.assertFalse(self.out.exists())

    def test_resolve_refuses_an_unsigned_tier_signed(self):
        self.publish("ghcr.io/ars-regia", signers={"tier1": None}, **DIGESTS)
        r = self.resolve("resolve")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn(
            "athanor-forge-tier1-repo@" + DIGESTS["tier1"] + " has no signature",
            r.stderr,
        )
        self.assertIn("tier1: not signed by a trusted run", r.stderr)
        self.assertFalse(self.out.exists())

    def test_resolve_refuses_a_foreign_identity_signed(self):
        for identity in (
            f"{WORKFLOW}@refs/heads/feature",
            f"{WORKFLOW}@refs/pull/7/merge",
            "https://github.com/fork/athanor/.github/workflows/call-system-image.yml@refs/heads/iso-v0",
            "https://github.com/ars-regia/athanor/.github/workflows/kernel-build.yml@refs/heads/iso-v0",
            f"{WORKFLOW}@refs/heads/iso-v0x",
            f"{WORKFLOW}@refs/heads/main-evil",
            f"{WORKFLOW}@refs/heads/rebase/fedora-45",
            f"{WORKFLOW}@refs/heads/rebase",
        ):
            with self.subTest(identity=identity):
                self.publish(
                    "ghcr.io/ars-regia", signers={"tier3": identity}, **DIGESTS
                )
                r = self.resolve("resolve")
                self.assertEqual(r.returncode, 1, r.stderr)
                self.assertIn(
                    f"is signed as {identity}, not by a trusted run", r.stderr
                )
                self.assertFalse(self.out.exists())

    def test_resolve_accepts_the_trusted_branches_and_the_run_s_own_branch(self):
        for identity, ref in (
            (f"{WORKFLOW}@refs/heads/main", None),
            (f"{WORKFLOW}@refs/heads/rebase/fedora-45", "refs/heads/rebase/fedora-45"),
            (f"{WORKFLOW}@refs/heads/feature/x", "refs/heads/feature/x"),
        ):
            with self.subTest(identity=identity):
                self.env.pop("GITHUB_REF", None)
                if ref:
                    self.env["GITHUB_REF"] = ref
                self.publish(
                    "ghcr.io/ars-regia", signers={"tier0": identity}, **DIGESTS
                )
                r = self.resolve("resolve")
                self.assertEqual(r.returncode, 0, r.stderr)

    def test_a_pull_request_run_does_not_trust_its_own_ref_signed(self):
        self.env["GITHUB_REF"] = "refs/pull/7/merge"
        self.publish(
            "ghcr.io/ars-regia",
            signers={"tier0": f"{WORKFLOW}@refs/pull/7/merge"},
            **DIGESTS,
        )
        r = self.resolve("resolve")
        self.assertEqual(r.returncode, 1)
        self.assertIn("tier0: not signed by a trusted run", r.stderr)

    def test_a_branch_whose_name_needs_escaping_trusts_only_the_default_branches_signed(self):
        self.env["GITHUB_REF"] = "refs/heads/fix/c++"
        self.publish("ghcr.io/ars-regia", **DIGESTS)
        r = self.resolve("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("'fix/c++' is not trusted for its own tiers", r.stderr)

    def test_a_pull_request_into_a_rebase_branch_builds_on_that_branch_s_tiers_signed(self):
        self.env["GITHUB_REF"] = "refs/pull/645/merge"
        self.env["GITHUB_BASE_REF"] = "rebase/fedora-45"
        base = f"{WORKFLOW}@refs/heads/rebase/fedora-45"
        self.publish(
            "ghcr.io/ars-regia",
            tag="branch-rebase-fedora-45",
            signers={f"tier{n}": base for n in range(4)},
            **DIGESTS,
        )
        r = self.resolve("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads(self.out.read_text())["tier2"], DIGESTS["tier2"])

    def test_a_pull_request_into_iso_v0_reads_latest_and_trusts_no_other_branch_signed(self):
        self.env["GITHUB_REF"] = "refs/pull/7/merge"
        self.env["GITHUB_BASE_REF"] = "iso-v0"
        self.publish(
            "ghcr.io/ars-regia",
            signers={"tier1": f"{WORKFLOW}@refs/heads/rebase/fedora-45"},
            **DIGESTS,
        )
        r = self.resolve("resolve")
        self.assertEqual(r.returncode, 1)
        self.assertIn("tier1: not signed by a trusted run", r.stderr)

    def test_a_cosign_failure_is_an_error_not_a_missing_signature_signed(self):
        self.publish("ghcr.io/ars-regia", transient=("tier2",), **DIGESTS)
        r = self.resolve("resolve")
        self.assertEqual(r.returncode, 1)
        self.assertIn("rekor lookup: 502 Bad Gateway", r.stderr)
        self.assertNotIn("not signed by a trusted run", r.stderr)
        self.assertNotIn("has no signature", r.stderr)
        self.assertFalse(self.out.exists())

    def test_verify_checks_the_file_and_a_tampered_digest_fails_signed(self):
        self.publish("ghcr.io/ars-regia", **DIGESTS)
        self.out.parent.mkdir(parents=True)
        self.out.write_text(json.dumps({"registry": "ghcr.io/ars-regia", **DIGESTS}))
        r = self.resolve("verify")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.out.write_text(
            json.dumps(
                {
                    "registry": "ghcr.io/ars-regia",
                    **DIGESTS,
                    "tier2": "sha256:" + "f" * 64,
                }
            )
        )
        r = self.resolve("verify")
        self.assertEqual(r.returncode, 1)
        self.assertIn("tier2: not signed by a trusted run", r.stderr)

    def test_verify_refuses_a_malformed_file_signed(self):
        self.out.parent.mkdir(parents=True)
        for content in (
            {"registry": "ghcr.io/ars-regia", **DIGESTS, "tier3": "latest"},
            {"registry": "Bad Registry", **DIGESTS},
        ):
            with self.subTest(content=content):
                self.out.write_text(json.dumps(content))
                self.assertEqual(self.resolve("verify").returncode, 1)

    def test_anything_but_a_mode_is_a_usage_error(self):
        for args in ((), ("get",), ("resolve", "extra"), ("verify", "x"), ("signed",)):
            with self.subTest(args=args):
                r = self.resolve(*args)
                self.assertEqual(r.returncode, 2)
                self.assertIn("resolve", r.stderr)


if __name__ == "__main__":
    unittest.main()
