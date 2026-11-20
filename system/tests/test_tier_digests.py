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
# What `latest` names beside the branch tiers of a landing pull request.
LATEST = {f"tier{n}": "sha256:" + "abcd"[n] * 64 for n in range(4)}
WORKFLOW = (
    "https://github.com/ars-regia/athanor/.github/workflows/call-system-image.yml"
)
TRUSTED = f"{WORKFLOW}@refs/heads/iso-v0"
RULES = "/repos/ars-regia/athanor/rules/branches/rebase/fedora-45"
# The rules GitHub returns for a branch a ruleset protects: what rebase/fedora-45 carried on
# 2026-10-22, less the parameters the check does not read.
PROTECTED = [
    {"type": "non_fast_forward"},
    {"type": "required_status_checks", "parameters": {}},
    {"type": "pull_request", "parameters": {"require_code_owner_review": True}},
]


class TierDigests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        for tool in ("skopeo", "cosign", "gh"):
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
                "GITHUB_HEAD_REF",
                "TIER_TRUSTED_REFS",
                "GITHUB_ACTIONS",
                "GH_TOKEN",
                "GITHUB_TOKEN",
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

    def publish(
        self, registry, signers=None, transient=(), tag="latest", api=None, **digests
    ):
        """Tag each tier :TAG at its digest, signed by TRUSTED unless SIGNERS names another
        identity (None: no signature) for that tier; TRANSIENT tiers fail cosign verify with a
        Rekor outage. API is what `gh api` answers per path; any other path is a 404."""
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
                    "api": api or {},
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

    def test_signed_asks_for_the_workflow_it_is_given(self):
        # forge/scripts/verify_image.sh asks for a package image built by call-dag-compile.yml
        # (issue #1176): the same branches and issuer, another workflow, and no third one.
        # The builder image is signed by call-build-builder.yml (issue #1205), under the same rules.
        dag = WORKFLOW.replace("call-system-image.yml", "call-dag-compile.yml")
        builder = WORKFLOW.replace("call-system-image.yml", "call-build-builder.yml")
        for signer, workflow, verdict in (
            (f"{dag}@refs/heads/iso-v0", "call-dag-compile.yml", "signed"),
            (f"{WORKFLOW}@refs/heads/iso-v0", "call-dag-compile.yml", "unsigned"),
            (f"{dag}@refs/heads/feature", "call-dag-compile.yml", "unsigned"),
            (f"{dag}@refs/heads/iso-v0", "call-system-image.yml", "unsigned"),
            (f"{builder}@refs/heads/iso-v0", "call-build-builder.yml", "signed"),
            (f"{builder}@refs/heads/main", "call-build-builder.yml", "signed"),
            (f"{dag}@refs/heads/iso-v0", "call-build-builder.yml", "unsigned"),
            (f"{builder}@refs/heads/feature", "call-build-builder.yml", "unsigned"),
            (f"{builder}@refs/pull/7/merge", "call-build-builder.yml", "unsigned"),
            (f"{builder}@refs/heads/rebase/fedora-45", "call-build-builder.yml", "unsigned"),
            (f"{builder}@refs/heads/iso-v0", "call-dag-compile.yml", "unsigned"),
        ):
            with self.subTest(signer=signer, workflow=workflow):
                self.publish("ghcr.io/ars-regia", signers={"tier0": signer}, **DIGESTS)
                r = self.resolve(
                    "signed", f"ghcr.io/ars-regia/athanor-forge-tier0-repo@{DIGESTS['tier0']}",
                    workflow,
                )
                self.assertEqual((r.returncode, r.stdout), (0, verdict + "\n"), r.stderr)
        r = self.resolve("signed", f"ghcr.io/ars-regia/athanor-forge-tier0-repo@{DIGESTS['tier0']}",
                         "release.yml")
        self.assertEqual(r.returncode, 2)

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

    def into_rebase_branch(self, rules):
        """A pull request into rebase/fedora-45, whose tiers that branch's runs signed; RULES is
        what the API answers for the branch (None: the call fails)."""
        self.env["GITHUB_REF"] = "refs/pull/645/merge"
        self.env["GITHUB_BASE_REF"] = "rebase/fedora-45"
        base = f"{WORKFLOW}@refs/heads/rebase/fedora-45"
        self.publish(
            "ghcr.io/ars-regia",
            tag="branch-rebase-fedora-45",
            signers={f"tier{n}": base for n in range(4)},
            api=None if rules is None else {RULES: rules},
            **DIGESTS,
        )
        return self.resolve("resolve")

    def test_a_pull_request_into_a_protected_rebase_branch_builds_on_its_tiers(self):
        r = self.into_rebase_branch(PROTECTED)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads(self.out.read_text())["tier2"], DIGESTS["tier2"])

    def test_an_unprotected_base_branch_is_not_trusted(self):
        """Maintainer decision of 2026-10-22: anyone with push access can create rebase/*, so
        only a branch a ruleset protects is trusted as the base of a pull request."""
        for name, rules in (
            ("no rules", []),
            ("no pull_request rule", [r for r in PROTECTED if r["type"] != "pull_request"]),
            ("no code owner review", [
                *(r for r in PROTECTED if r["type"] != "pull_request"),
                {"type": "pull_request", "parameters": {"require_code_owner_review": False}},
            ]),
            ("force pushes allowed", [r for r in PROTECTED if r["type"] != "non_fast_forward"]),
        ):
            with self.subTest(name=name):
                r = self.into_rebase_branch(rules)
                self.assertEqual(r.returncode, 1, r.stderr)
                self.assertIn("'rebase/fedora-45' is not protected by a ruleset", r.stderr)
                self.assertIn("tier0 tier1 tier2 tier3: not signed by a trusted run", r.stderr)
                self.assertFalse(self.out.exists())

    def test_rules_that_cannot_be_read_are_an_error(self):
        for name, rules in (("404", None), ("not a list", {"message": "Bad credentials"})):
            with self.subTest(name=name):
                r = self.into_rebase_branch(rules)
                self.assertEqual(r.returncode, 1, r.stderr)
                self.assertIn("cannot read the rules of the base branch 'rebase/fedora-45'", r.stderr)
                self.assertNotIn("not signed by a trusted run", r.stderr)
                self.assertNotIn("not protected", r.stderr)
                self.assertFalse(self.out.exists())

    def test_a_base_branch_name_outside_the_branch_alphabet_dies(self):
        """A name that would widen the identity regex never reaches the trust list or the API."""
        for base in ("rebase/x|.*", "rebase/(x)", "rebase/x y", "rebase/$x"):
            with self.subTest(base=base):
                self.env["GITHUB_REF"] = "refs/pull/7/merge"
                self.env["GITHUB_BASE_REF"] = base
                self.publish(
                    "ghcr.io/ars-regia",
                    signers={f"tier{n}": f"{WORKFLOW}@refs/heads/{base}" for n in range(4)},
                    api={f"/repos/ars-regia/athanor/rules/branches/{base}": PROTECTED},
                    **DIGESTS,
                )
                (self.dir / "calls.log").unlink(missing_ok=True)
                r = self.resolve("resolve")
                self.assertEqual(r.returncode, 1, r.stderr)
                self.assertIn(f"the base branch '{base}' is not a branch name", r.stderr)
                log = self.dir / "calls.log"
                tools = [json.loads(ln)[0] for ln in log.read_text().splitlines()] if log.exists() else []
                self.assertEqual(tools, [])
                self.assertFalse(self.out.exists())

    def test_iso_v0_and_main_as_base_need_no_api_call(self):
        for base in ("iso-v0", "main"):
            with self.subTest(base=base):
                self.env["GITHUB_REF"] = "refs/pull/7/merge"
                self.env["GITHUB_BASE_REF"] = base
                self.publish("ghcr.io/ars-regia", **DIGESTS)
                r = self.resolve("resolve")
                self.assertEqual(r.returncode, 0, r.stderr)
                calls = [json.loads(ln) for ln in (self.dir / "calls.log").read_text().splitlines()]
                self.assertNotIn("gh", [c[0] for c in calls])

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

    def landing(self, head="rebase/fedora-45", base="iso-v0", rules=PROTECTED, branch_tag=True):
        """A pull request from HEAD into BASE (ADR-0125's landing pull request by default): the
        head branch's tiers, signed by its runs, under branch-<head>, beside other tiers under
        `latest` signed by iso-v0. RULES is what the API answers for the head branch (None: the
        call fails); BRANCH_TAG False leaves branch-<head> unpublished."""
        self.env["GITHUB_REF"] = "refs/pull/1317/merge"
        self.env["GITHUB_BASE_REF"] = base
        self.env["GITHUB_HEAD_REF"] = head
        self.publish(
            "ghcr.io/ars-regia",
            tag="branch-rebase-fedora-45",
            signers={f"tier{n}": f"{WORKFLOW}@refs/heads/rebase/fedora-45" for n in range(4)},
            api=None if rules is None else {RULES: rules},
            **DIGESTS,
        )
        fixture = json.loads(self.fixture.read_text())
        if not branch_tag:
            fixture["tags"] = {}
        for tier, digest in LATEST.items():
            fixture["tags"][f"ghcr.io/ars-regia/athanor-forge-{tier}-repo:latest"] = digest
            fixture["signatures"][f"ghcr.io/ars-regia/athanor-forge-{tier}-repo@{digest}"] = TRUSTED
        self.fixture.write_text(json.dumps(fixture))
        return self.resolve("resolve")

    def calls(self):
        log = self.dir / "calls.log"
        return [json.loads(ln) for ln in log.read_text().splitlines()] if log.exists() else []

    def test_a_landing_pull_request_builds_on_the_head_branch_s_tiers(self):
        """Issue #1321: a rebase/* branch into iso-v0 or main builds on its own tiers, not on the
        old release's `latest`."""
        for base in ("iso-v0", "main"):
            with self.subTest(base=base):
                r = self.landing(base=base)
                self.assertEqual(r.returncode, 0, r.stderr)
                out = json.loads(self.out.read_text())
                self.assertEqual({k: v for k, v in out.items() if k.startswith("tier")}, DIGESTS)
                self.assertEqual(out["registry"], "ghcr.io/ars-regia")
                self.assertNotIn(":latest", json.dumps(self.calls()))
                self.assertIn(["gh", "api", RULES], self.calls())

    def test_an_ordinary_pull_request_into_iso_v0_still_reads_latest(self):
        for head in ("fix/tiers", "fix/c++", "feature/rebase/x", "rebased"):
            with self.subTest(head=head):
                r = self.landing(head=head)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertEqual(
                    {k: v for k, v in json.loads(self.out.read_text()).items() if k.startswith("tier")},
                    LATEST,
                )
                self.assertNotIn("branch-", json.dumps(self.calls()))
                self.assertNotIn("gh", [c[0] for c in self.calls()])

    def test_a_rebase_head_into_another_branch_reads_the_base_as_before(self):
        self.env["GITHUB_HEAD_REF"] = "rebase/fedora-46"
        r = self.into_rebase_branch(PROTECTED)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads(self.out.read_text())["tier2"], DIGESTS["tier2"])

    def test_a_landing_pull_request_never_falls_back_to_latest(self):
        for name, kw, message in (
            ("no branch tag", {"branch_tag": False},
             "ghcr.io/ars-regia/athanor-forge-tier0-repo:branch-rebase-fedora-45 cannot be read"),
            ("unprotected", {"rules": []}, "the head branch 'rebase/fedora-45' is not protected by a ruleset"),
            ("rules unreadable", {"rules": None}, "cannot read the rules of the head branch 'rebase/fedora-45'"),
        ):
            with self.subTest(name=name):
                (self.dir / "calls.log").unlink(missing_ok=True)
                self.out.unlink(missing_ok=True)
                r = self.landing(**kw)
                self.assertEqual(r.returncode, 1, r.stderr)
                self.assertIn(message, r.stderr)
                self.assertNotIn(":latest", json.dumps(self.calls()))
                self.assertFalse(self.out.exists())

    def test_signed_and_probe_on_a_landing_pull_request_trust_exactly_the_head(self):
        """The head is added to the identity regex as one anchored branch name: a sibling, a
        longer name or another case is a foreign identity."""
        self.env["GITHUB_REF"] = "refs/pull/1317/merge"
        self.env["GITHUB_BASE_REF"] = "iso-v0"
        self.env["GITHUB_HEAD_REF"] = "rebase/fedora-45"
        for identity, verdict in (
            (f"{WORKFLOW}@refs/heads/rebase/fedora-45", "signed"),
            (TRUSTED, "signed"),
            (f"{WORKFLOW}@refs/heads/rebase/fedora-455", "unsigned"),
            (f"{WORKFLOW}@refs/heads/rebase/fedora-45x", "unsigned"),
            (f"{WORKFLOW}@refs/heads/rebase/Fedora-45", "unsigned"),
            (f"{WORKFLOW}@refs/heads/rebase/fedoraX45", "unsigned"),
        ):
            for mode in ("signed", "probe"):
                with self.subTest(identity=identity, mode=mode):
                    self.publish(
                        "ghcr.io/ars-regia",
                        signers={"tier0": identity},
                        api={RULES: PROTECTED},
                        **DIGESTS,
                    )
                    r = self.resolve(mode, f"ghcr.io/ars-regia/athanor-forge-tier0-repo@{DIGESTS['tier0']}")
                    self.assertEqual((r.returncode, r.stdout), (0, verdict + "\n"), r.stderr)

    def test_a_dot_in_the_head_matches_only_a_dot(self):
        """The head's dots are escaped in the identity regex: rebase/f.45 does not trust rebase/fX45."""
        self.env["GITHUB_REF"] = "refs/pull/1317/merge"
        self.env["GITHUB_BASE_REF"] = "iso-v0"
        self.env["GITHUB_HEAD_REF"] = "rebase/f.45"
        for identity, verdict in (
            (f"{WORKFLOW}@refs/heads/rebase/f.45", "signed"),
            (f"{WORKFLOW}@refs/heads/rebase/fX45", "unsigned"),
        ):
            with self.subTest(identity=identity):
                self.publish(
                    "ghcr.io/ars-regia",
                    signers={"tier0": identity},
                    api={"/repos/ars-regia/athanor/rules/branches/rebase/f.45": PROTECTED},
                    **DIGESTS,
                )
                r = self.resolve("signed", f"ghcr.io/ars-regia/athanor-forge-tier0-repo@{DIGESTS['tier0']}")
                self.assertEqual((r.returncode, r.stdout), (0, verdict + "\n"), r.stderr)

    def test_a_landing_pull_request_with_partial_rules_is_not_trusted(self):
        for name, rules in (
            ("no code owner review", [
                *(r for r in PROTECTED if r["type"] != "pull_request"),
                {"type": "pull_request", "parameters": {"require_code_owner_review": False}},
            ]),
            ("pull_request without the parameter", [
                *(r for r in PROTECTED if r["type"] != "pull_request"),
                {"type": "pull_request", "parameters": {}},
            ]),
            ("force pushes allowed", [r for r in PROTECTED if r["type"] != "non_fast_forward"]),
        ):
            with self.subTest(name=name):
                (self.dir / "calls.log").unlink(missing_ok=True)
                self.out.unlink(missing_ok=True)
                r = self.landing(rules=rules)
                self.assertEqual(r.returncode, 1, r.stderr)
                self.assertIn("the head branch 'rebase/fedora-45' is not protected by a ruleset", r.stderr)
                self.assertIn("tier0 tier1 tier2 tier3: not signed by a trusted run", r.stderr)
                self.assertNotIn(":latest", json.dumps(self.calls()))
                self.assertFalse(self.out.exists())

    def test_a_head_branch_name_outside_the_branch_alphabet_dies(self):
        for head in ("rebase/x|.*", "rebase/(x)", "rebase/x y", "rebase/$x"):
            with self.subTest(head=head):
                (self.dir / "calls.log").unlink(missing_ok=True)
                r = self.landing(head=head)
                self.assertEqual(r.returncode, 1, r.stderr)
                self.assertIn(f"the head branch '{head}' is not a branch name", r.stderr)
                self.assertEqual(self.calls(), [])
                self.assertFalse(self.out.exists())

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


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL", "TIER_TRUSTED_REFS",
             "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN", "GITHUB_HEAD_REF", "GITHUB_REPOSITORY_OWNER")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    for name in TRUST_ENV:
        _os.environ.pop(name, None)


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
