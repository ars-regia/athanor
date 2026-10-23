"""system/github-release.sh: the GitHub Release of a promotion (doc_pipeline.md PL42, ADR-0131),
against the offline gh of fake_registry.py (python3 -B -m unittest discover -s system/tests)."""

import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import unittest

from test_kernel_artifacts import Tool

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "system" / "github-release.sh"
BUNDLE = ROOT / "system" / "evidence-bundle.sh"
REPO = "ars-regia/athanor"
API = f"/repos/{REPO}"
IDENTITY = (
    f"https://github.com/{REPO}/.github/workflows/promote-stable.yml@refs/heads/iso-v0"
)
REVISION = "a1" * 20
HEAD = "b2" * 20
OTHER = "c3" * 20
VERSION = "43.20261024.5123"
PROMOTION_RUN = "9001"
EMAIL = "maintainer@example.org"
ASSET = f"evidence-412-{PROMOTION_RUN}.tar"
OTHER_BUILD_ASSET = "evidence-411-8000.tar"
NOTES = '# Athanor 0.9.0 "surface"\n\nScreenshot and screen share are absent.\n'
TITLE = f'Athanor 0.9 "surface" ({VERSION})'
WRITES = ("create", "upload", "edit")


def sha256(data):
    return hashlib.sha256(data).hexdigest()


class GithubRelease(Tool):
    def setUp(self):
        super().setUp()
        a = self.artifacts
        (a / "evidence").mkdir(parents=True)
        (a / "packages").mkdir()
        (a / "promotion.json").write_text(
            json.dumps(
                {
                    "run_id": 412,
                    "version": VERSION,
                    "revision": REVISION,
                    "trigger": f"https://github.com/{REPO}/actions/runs/{PROMOTION_RUN}",
                    "images": [
                        {
                            "name": "athanor-system",
                            "digest": "sha256:" + "a" * 64,
                            "previous_stable": None,
                        }
                    ],
                    "skipped": [],
                    "overrides": [],
                    "builder_content_hash": "b" * 64,
                    "iso": "sha256:" + "c" * 64,
                    "iso_system": "sha256:" + "a" * 64,
                },
                sort_keys=True,
                indent=2,
            )
        )
        (a / "image-digests.txt").write_text(
            "r/athanor-system 412 sha256:" + "a" * 64 + "\n"
        )
        (a / "evidence" / "iso-acceptance.athanor-system.json").write_text(
            json.dumps(
                {
                    "gate": "iso-acceptance",
                    "image": "athanor-system",
                    "verdict": "pass",
                    "finished_at": "2026-10-23T10:00:00Z",
                }
            )
        )
        (a / "packages" / "athanor-system.txt").write_text(
            "bash-5.2-1.fc43.x86_64 bb\n"
        )
        self.notes_dir = self.dir / "releases"
        self.notes_dir.mkdir()
        (self.notes_dir / "0.9.0.md").write_text(NOTES)
        self.env.update(
            RELEASE_NOTES_DIR=str(self.notes_dir),
            MAINTAINER_TAG_EMAIL=EMAIL,
            GH_TOKEN="token",
            RUN_ID="412",
            GITHUB_RUN_ID=PROMOTION_RUN,
        )
        self.fx = {
            "signer": IDENTITY,
            "git_tags": {},
            "releases": {},
            "api": {
                f"{API}/branches/iso-v0": {"commit": {"sha": HEAD}},
                f"{API}/compare/{REVISION}...{HEAD}": {"status": "ahead"},
            },
        }

    # The fixture and the calls

    def save(self):
        self.registry(self.fx)

    def load(self):
        self.fx = json.loads((self.dir / "registry.json").read_text())

    def tag(
        self,
        name="v0.9.0",
        commit=REVISION,
        annotated=True,
        verified=True,
        email=EMAIL,
        message='Athanor 0.9 "surface"\n\nThe 0.9 release.\n',
    ):
        self.fx["git_tags"][name] = {
            "sha": sha256(name.encode())[:40] if annotated else commit,
            "type": "tag" if annotated else "commit",
            "commit": commit,
            "message": message,
            "tagger_email": email,
            "verified": verified,
        }

    def release(self, tag, draft=False, title=VERSION, target=REVISION, assets=None, body=None, latest=False):
        """A Release already at TAG; its body defaults to the notes the plan approves for TAG."""
        self.fx["releases"][tag] = {
            "draft": draft,
            "latest": latest,
            "target": target,
            "title": title,
            "body": self.approved_notes(tag) if body is None else body,
            "assets": assets or {},
        }

    def first_promotion(self, run="8999"):
        """A genuine earlier promotion of build run 412, by promotion run RUN, published at promoted-412."""
        promotion = self.artifacts / "promotion.json"
        record = json.loads(promotion.read_text())
        trigger = record["trigger"]
        record["trigger"] = f"https://github.com/{REPO}/actions/runs/{run}"
        promotion.write_text(json.dumps(record, sort_keys=True, indent=2))
        r = self.plan()
        self.assertEqual(r.returncode, 0, r.stderr)
        r = self.publish(self.bundle(f"promotion-{run}"), GITHUB_RUN_ID=run)
        self.assertEqual(r.returncode, 0, r.stderr)
        record["trigger"] = trigger
        promotion.write_text(json.dumps(record, sort_keys=True, indent=2))

    def tar_of(self, directory):
        return subprocess.run(["tar", "-cf", "-", "-C", str(directory), "."], capture_output=True, check=True).stdout

    def approved_notes(self, tag):
        notes = self.promotion_notes(tag)
        return NOTES + "\n" + notes if tag.startswith("v") else notes

    def blob(self, data):
        store = self.dir / "release-assets"
        store.mkdir(exist_ok=True)
        (store / sha256(data)).write_bytes(data)
        return sha256(data)

    def asset_bytes(self, tag, name=ASSET):
        return (
            self.dir / "release-assets" / self.fx["releases"][tag]["assets"][name]
        ).read_bytes()

    def run_script(self, *args, **env):
        self.save()
        (self.dir / "calls.log").unlink(missing_ok=True)
        r = subprocess.run(
            ["bash", str(SCRIPT), *args],
            capture_output=True,
            text=True,
            env={**self.env, **env},
            cwd=self.dir,
        )
        self.load()
        return r

    def calls(self):
        log = self.dir / "calls.log"
        return (
            [json.loads(line) for line in log.read_text().splitlines()]
            if log.exists()
            else []
        )

    def writes(self):
        return [
            c for c in self.calls() if c[:2] == ["gh", "release"] and c[2] in WRITES
        ]

    def plan(self, **env):
        return self.run_script("plan", str(self.artifacts), **env)

    def choice(self):
        return json.loads((self.artifacts / "release.json").read_text())

    def promotion_notes(self, tag, directory=None):
        r = subprocess.run(
            [
                "bash",
                str(BUNDLE),
                "summary",
                "--promotion",
                str(directory or self.artifacts),
                tag,
            ],
            capture_output=True,
            text=True,
            env={**self.env, "LC_ALL": "C"},
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout

    def bundle(self, name="bundle"):
        """The plan's bundle, as evidence-bundle.sh create writes it and the job downloads it."""
        path = self.dir / name
        r = subprocess.run(
            ["bash", str(BUNDLE), "create", str(self.artifacts), str(path)],
            capture_output=True,
            text=True,
            env=self.env,
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        return path

    def planned(self):
        r = self.plan()
        self.assertEqual(r.returncode, 0, r.stderr)
        return self.bundle()

    def publish(self, bundle, **env):
        return self.run_script("publish", str(bundle), **env)

    def refused(self, r, *texts):
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        for text in texts:
            self.assertIn(text, r.stderr)

    def nothing_published(self):
        self.assertEqual(self.writes(), [])


class Plan(GithubRelease):
    def test_plan_names_the_promotion_tag_and_the_version_without_a_version_tag(self):
        r = self.plan()
        self.assertEqual(r.returncode, 0, r.stderr)
        notes = self.promotion_notes("promoted-412")
        self.assertIn("gh release download promoted-412", notes)
        self.assertEqual(
            self.choice(),
            {
                "tag": "promoted-412",
                "title": VERSION,
                "latest": False,
                "notes_sha256": sha256(notes.encode()),
            },
        )

    def test_plan_names_a_maintainer_version_tag_with_its_notes_and_latest(self):
        self.tag()
        r = self.plan()
        self.assertEqual(r.returncode, 0, r.stderr)
        notes = NOTES + "\n" + self.promotion_notes("v0.9.0")
        self.assertEqual(
            self.choice(),
            {
                "tag": "v0.9.0",
                "title": TITLE,
                "latest": True,
                "notes_sha256": sha256(notes.encode()),
                "fallback": {
                    "tag": "promoted-412",
                    "title": VERSION,
                    "latest": False,
                    "notes_sha256": sha256(
                        self.promotion_notes("promoted-412").encode()
                    ),
                },
            },
        )

    def test_plan_refuses_a_version_tag_without_maintainer_tag_email_and_names_the_variable(
        self,
    ):
        self.tag()
        self.refused(
            self.plan(MAINTAINER_TAG_EMAIL=""), "MAINTAINER_TAG_EMAIL", "v0.9.0"
        )
        self.assertFalse((self.artifacts / "release.json").exists())

    def test_plan_without_a_version_tag_needs_no_maintainer_tag_email(self):
        self.tag("v0.8.0", commit=OTHER)
        r = self.plan(MAINTAINER_TAG_EMAIL="")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.choice()["tag"], "promoted-412")

    def test_plan_does_not_see_a_draft(self):
        # The plan's token cannot be relied on to list drafts, so a draft never steers the choice;
        # publish meets it through the approved fallback.
        self.tag()
        self.release(
            "v0.9.0", draft=True, title=TITLE, assets={OTHER_BUILD_ASSET: "0" * 64}
        )
        r = self.plan()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.choice()["tag"], "v0.9.0")
        self.assertEqual(self.choice()["fallback"]["tag"], "promoted-412")

    def test_plan_refuses_a_revision_not_on_the_release_branch(self):
        self.fx["api"][f"{API}/compare/{REVISION}...{HEAD}"] = {"status": "diverged"}
        self.refused(self.plan(), REVISION, "iso-v0", "diverged")

    def test_plan_refuses_a_lightweight_version_tag_and_names_it(self):
        self.tag(annotated=False)
        self.refused(self.plan(), "v0.9.0", "lightweight")

    def test_plan_refuses_an_unverified_version_tag_and_names_it(self):
        self.tag(verified=False)
        self.refused(self.plan(), "v0.9.0", "verification.verified")

    def test_plan_refuses_a_version_tag_of_another_tagger_and_names_it(self):
        self.tag(email="someone@example.org")
        self.refused(self.plan(), "v0.9.0", "tagger.email", "MAINTAINER_TAG_EMAIL")

    def test_plan_refuses_two_version_tags_at_the_commit(self):
        self.tag("v0.9.0")
        self.tag("v0.9.1")
        self.refused(self.plan(), "v0.9.0", "v0.9.1")

    def test_plan_ignores_a_version_tag_at_another_commit(self):
        self.tag("v0.8.0", commit=OTHER)
        self.tag("v0.7.0", commit=OTHER, annotated=False)
        r = self.plan()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.choice()["tag"], "promoted-412")

    def test_plan_refuses_a_version_tag_without_its_notes_file(self):
        self.tag()
        (self.notes_dir / "0.9.0.md").unlink()
        self.refused(self.plan(), "v0.9.0", "0.9.0.md")

    def test_plan_refuses_notes_still_to_be_completed(self):
        (self.notes_dir / "0.9.0.md").write_text(NOTES + "\nTO BE COMPLETED BEFORE THE v0.9.0 TAG.\n")
        # A promoted-* Release takes its notes from the summary alone: the file never reaches it.
        r = self.plan()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertNotIn("TO BE COMPLETED", self.promotion_notes("promoted-412"))
        self.tag()
        self.refused(self.plan(), "v0.9.0", "0.9.0.md", "TO BE COMPLETED BEFORE THE v0.9.0 TAG.")

    def test_plan_refuses_notes_with_carriage_returns(self):
        self.tag()
        (self.notes_dir / "0.9.0.md").write_text(NOTES.replace("\n", "\r\n"))
        self.refused(self.plan(), "0.9.0.md", "carriage return")

    def test_plan_refuses_notes_whose_first_heading_names_another_version(self):
        self.tag()
        for first in (
            "# Athanor 0.9.1",
            "## Athanor 0.9.0",
            "Athanor 0.9.0",
            "# Athanor 10.9.0",
            "# Athanor 0.9.0.1",
        ):
            with self.subTest(first=first):
                (self.notes_dir / "0.9.0.md").write_text(first + "\n\nText.\n")
                self.refused(self.plan(), "v0.9.0", "0.9.0.md")

    def test_plan_sends_another_build_of_a_tagged_commit_to_its_promotion_tag(self):
        self.tag()
        self.release("v0.9.0", title=TITLE, assets={OTHER_BUILD_ASSET: "0" * 64})
        r = self.plan()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            self.choice(),
            {
                "tag": "promoted-412",
                "title": VERSION,
                "latest": False,
                "notes_sha256": sha256(self.promotion_notes("promoted-412").encode()),
            },
        )

    def test_a_gh_error_other_than_not_found_is_not_taken_for_absent(self):
        for broken, tagged in (
            ("git/matching-refs", False),
            ("release view", True),
            ("git/ref/tags/promoted-412", False),
        ):
            with self.subTest(broken=broken):
                self.fx["git_tags"] = {}
                if tagged:
                    self.tag()
                self.fx["gh_errors"] = [broken]
                self.refused(
                    self.plan(), broken.split("/")[-1].split(" ")[0], "HTTP 502"
                )
                self.assertFalse((self.artifacts / "release.json").exists())


class Publish(GithubRelease):
    def test_publish_refuses_a_bundle_of_another_build_run(self):
        bundle = self.planned()
        self.refused(self.publish(bundle, RUN_ID="413"), "413")
        self.nothing_published()

    def test_publish_refuses_a_bundle_of_another_promotion_run(self):
        bundle = self.planned()
        self.refused(self.publish(bundle, GITHUB_RUN_ID="9002"), "9002")
        self.nothing_published()

    def test_publish_refuses_a_version_tag_moved_since_the_plan_and_names_the_field(
        self,
    ):
        self.tag()
        bundle = self.planned()
        self.tag(commit=OTHER)
        self.refused(self.publish(bundle), "release.json tag", "v0.9.0", OTHER)
        self.nothing_published()

    def test_publish_refuses_a_version_tag_no_longer_verified_and_names_the_field(self):
        self.tag()
        bundle = self.planned()
        self.tag(verified=False)
        self.refused(
            self.publish(bundle), "release.json tag", "v0.9.0", "verification.verified"
        )
        self.nothing_published()

    def test_publish_refuses_notes_whose_hash_differs_from_release_json(self):
        self.tag()
        bundle = self.planned()
        (self.notes_dir / "0.9.0.md").write_text(
            NOTES + "Changed after the approval.\n"
        )
        self.refused(self.publish(bundle), "notes_sha256", "v0.9.0")
        self.nothing_published()

    def test_a_stray_draft_publishes_nothing_and_names_the_recovery(self):
        bundle = self.planned()
        self.release("promoted-412", draft=True, title="Something else")
        self.refused(
            self.publish(bundle),
            "promoted-412",
            "Something else",
            f"gh release delete promoted-412 -R {REPO}",
            "--cleanup-tag",
            f"gh run rerun {PROMOTION_RUN}",
        )
        self.nothing_published()

    def test_a_version_draft_holding_another_builds_asset_sends_the_bundle_to_the_fallback(
        self,
    ):
        self.tag()
        bundle = self.planned()
        self.release(
            "v0.9.0", draft=True, title=TITLE, assets={OTHER_BUILD_ASSET: "0" * 64}
        )
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            self.fx["releases"]["v0.9.0"]["assets"], {OTHER_BUILD_ASSET: "0" * 64}
        )
        self.assertTrue(self.fx["releases"]["v0.9.0"]["draft"])
        fallback = self.fx["releases"]["promoted-412"]
        self.assertEqual(
            (fallback["draft"], fallback["title"], fallback["latest"]),
            (False, VERSION, False),
        )
        self.assertIn(ASSET, fallback["assets"])

    def test_a_published_version_release_of_another_build_sends_the_bundle_to_the_fallback(
        self,
    ):
        self.tag()
        bundle = self.planned()
        self.release("v0.9.0", title=TITLE, assets={OTHER_BUILD_ASSET: "0" * 64})
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            self.fx["releases"]["v0.9.0"]["assets"], {OTHER_BUILD_ASSET: "0" * 64}
        )
        self.assertIn(ASSET, self.fx["releases"]["promoted-412"]["assets"])

    def test_the_fallback_is_named_in_the_summary_with_the_other_run(self):
        self.tag()
        bundle = self.planned()
        self.release(
            "v0.9.0", draft=True, title=TITLE, assets={OTHER_BUILD_ASSET: "0" * 64}
        )
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertRegex(r.stdout, r"v0\.9\.0.*build run 411.*promoted-412")

    def test_a_promotion_tag_choice_has_no_fallback(self):
        bundle = self.planned()
        self.assertNotIn("fallback", self.choice())
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(list(self.fx["releases"]), ["promoted-412"])
        self.assertFalse([c for c in self.calls() if "v0.9.0" in c])

    def test_an_unverified_version_tag_with_another_builds_draft_fails_closed_and_names_the_tag(
        self,
    ):
        self.tag()
        bundle = self.planned()
        self.tag(verified=False)
        self.release(
            "v0.9.0", draft=True, title=TITLE, assets={OTHER_BUILD_ASSET: "0" * 64}
        )
        self.refused(self.publish(bundle), "v0.9.0")
        self.nothing_published()

    def test_the_fallback_notes_name_the_promotion_tag(self):
        self.tag()
        bundle = self.planned()
        self.release("v0.9.0", title=TITLE, assets={OTHER_BUILD_ASSET: "0" * 64})
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        body = self.fx["releases"]["promoted-412"]["body"]
        self.assertEqual(body, self.promotion_notes("promoted-412", bundle))
        self.assertIn(f"gh release download promoted-412 -R {REPO} -p 'evidence-412-*.tar'", body)
        self.assertNotIn("v0.9.0", body)
        self.assertNotIn(NOTES, body)

    def test_trees_with_different_modes_pack_to_identical_bytes(self):
        bundle = self.planned()
        other = self.dir / "other-umask"
        shutil.copytree(bundle, other)
        for path in [other, *other.rglob("*")]:
            os.chmod(path, 0o700 if path.is_dir() else 0o600)
            os.utime(path, (1_000_000, 1_000_000))
        shas = []
        for tree in (bundle, other):
            self.fx["releases"], self.fx["git_tags"] = {}, {}
            r = self.publish(tree)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertGreater(len(self.asset_bytes("promoted-412")), 0)
            shas.append(self.fx["releases"]["promoted-412"]["assets"][ASSET])
        self.assertEqual(shas[0], shas[1])

    def test_the_release_is_created_as_a_draft_then_published_with_its_latest_flag(
        self,
    ):
        self.tag()
        bundle = self.planned()
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        writes = self.writes()
        self.assertEqual([c[2] for c in writes], ["create", "edit"])
        self.assertIn("--draft", writes[0])
        self.assertEqual(writes[0][writes[0].index("--target") + 1], REVISION)
        self.assertEqual(writes[0][writes[0].index("--title") + 1], TITLE)
        self.assertIn("--draft=false", writes[1])
        self.assertIn("--latest", writes[1])
        release = self.fx["releases"]["v0.9.0"]
        self.assertEqual((release["draft"], release["latest"]), (False, True))
        self.assertEqual(
            release["body"], NOTES + "\n" + self.promotion_notes("v0.9.0", bundle)
        )

    def test_a_promotion_release_is_published_with_latest_false(self):
        bundle = self.planned()
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("--latest=false", self.writes()[-1])
        release = self.fx["releases"]["promoted-412"]
        self.assertEqual(
            (release["draft"], release["latest"], release["title"]),
            (False, False, VERSION),
        )
        self.assertEqual(self.fx["git_tags"]["promoted-412"]["commit"], REVISION)

    def test_a_published_release_the_job_did_not_create_is_refused(self):
        self.tag()
        bundle = self.planned()
        self.release("v0.9.0", title="Forged title", body="forged notes")
        self.refused(self.publish(bundle), "v0.9.0", "title", "body", f"gh release delete v0.9.0 -R {REPO}")
        self.nothing_published()
        self.assertEqual(self.fx["releases"]["v0.9.0"]["assets"], {})
        self.assertEqual(self.fx["releases"]["v0.9.0"]["body"], "forged notes")

    def test_a_draft_with_the_approved_title_and_a_forged_body_is_refused(self):
        bundle = self.planned()
        self.release("promoted-412", draft=True, body="forged notes")
        self.refused(self.publish(bundle), "promoted-412", "body", f"gh release delete promoted-412 -R {REPO}")
        self.nothing_published()
        self.assertTrue(self.fx["releases"]["promoted-412"]["draft"])

    def test_an_existing_release_must_match_the_approved_one_field_by_field(self):
        self.tag()
        bundle = self.planned()
        for field, change in (("title", {"title": "Athanor 0.9"}), ("target", {"target": OTHER}),
                              ("prerelease", {"prerelease": True}), ("latest", {"latest": False}),
                              ("body", {"body": NOTES})):
            with self.subTest(field=field):
                self.release("v0.9.0", title=TITLE, latest=True)
                self.fx["releases"]["v0.9.0"].update(change)
                self.refused(self.publish(bundle), "v0.9.0", field)
                self.nothing_published()
        self.release("v0.9.0", title=TITLE, latest=True, body=self.approved_notes("v0.9.0").replace("\n", "\r\n").rstrip())
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual([c[2] for c in self.writes()], ["upload"])

    def test_the_read_back_checks_the_title_and_the_body(self):
        bundle = self.planned()
        self.fx["release_bodies_lost"] = True
        self.refused(self.publish(bundle), "promoted-412", "body")

    def test_a_failed_view_of_the_version_release_fails_the_job(self):
        self.tag()
        bundle = self.planned()
        self.fx["gh_errors_once"] = ["release view v0.9.0"]
        self.refused(self.publish(bundle), "release view v0.9.0", "HTTP 502")
        self.nothing_published()
        self.assertEqual(self.fx["releases"], {})

    def test_a_forged_body_beside_a_valid_looking_earlier_asset_is_refused(self):
        bundle = self.planned()
        self.release("promoted-412", body="forged notes", assets={"evidence-412-8999.tar": self.blob(b"x")})
        self.refused(self.publish(bundle), "promoted-412", "body", f"gh release delete promoted-412 -R {REPO}")
        self.nothing_published()
    def test_only_a_strictly_named_asset_is_another_builds_evidence(self):
        self.tag()
        bundle = self.planned()
        self.release("v0.9.0", title=TITLE, latest=True, assets={"evidence-411-x.tar": self.blob(b"x")})
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertNotIn("promoted-412", self.fx["releases"])
        self.assertIn(ASSET, self.fx["releases"]["v0.9.0"]["assets"])
    def test_a_genuine_earlier_bundle_on_a_release_with_forged_notes_is_refused(self):
        self.first_promotion()
        bundle = self.planned()
        self.fx["releases"]["promoted-412"]["body"] = "forged notes"
        self.refused(self.publish(bundle), "promoted-412", "body")
        self.nothing_published()

    def test_a_genuine_earlier_bundle_of_another_revision_is_refused(self):
        self.first_promotion()
        bundle = self.planned()
        self.fx["releases"]["promoted-412"]["target"] = OTHER
        self.refused(self.publish(bundle), "promoted-412", "target")
        self.nothing_published()

    def test_a_draft_without_the_asset_is_completed(self):
        bundle = self.planned()
        self.release("promoted-412", draft=True)
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual([c[2] for c in self.writes()], ["upload", "edit"])
        self.assertFalse(self.fx["releases"]["promoted-412"]["draft"])

    def test_a_draft_holding_the_same_asset_is_published(self):
        bundle = self.planned()
        self.assertEqual(self.publish(bundle).returncode, 0)
        self.fx["releases"]["promoted-412"]["draft"] = True
        del self.fx["git_tags"]["promoted-412"]
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual([c[2] for c in self.writes()], ["edit"])
        self.assertFalse(self.fx["releases"]["promoted-412"]["draft"])

    def test_a_rerun_with_the_same_asset_changes_nothing(self):
        bundle = self.planned()
        self.assertEqual(self.publish(bundle).returncode, 0)
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.nothing_published()

    def test_an_existing_asset_with_other_bytes_is_refused_and_not_replaced(self):
        bundle = self.planned()
        self.assertEqual(self.publish(bundle).returncode, 0)
        local = self.fx["releases"]["promoted-412"]["assets"][ASSET]
        other = self.blob(b"other bytes")
        for draft, recovery in (
            (True, f"gh release delete promoted-412 -R {REPO}"),
            (False, "no automatic recovery"),
        ):
            with self.subTest(draft=draft):
                self.release("promoted-412", draft=draft, assets={ASSET: other})
                self.refused(self.publish(bundle), local, other, recovery)
                self.nothing_published()
                self.assertEqual(
                    self.fx["releases"]["promoted-412"]["assets"], {ASSET: other}
                )

    def test_a_second_promotion_of_the_same_build_adds_its_own_asset(self):
        self.first_promotion()
        first = "evidence-412-8999.tar"
        bundle = self.planned()
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual([c[2] for c in self.writes()], ["upload"])
        self.assertEqual(
            sorted(self.fx["releases"]["promoted-412"]["assets"]), [first, ASSET]
        )

    def test_a_promotion_tag_at_another_commit_publishes_nothing(self):
        bundle = self.planned()
        self.tag("promoted-412", commit=OTHER, annotated=False)
        self.refused(self.publish(bundle), "promoted-412", OTHER)
        self.nothing_published()

    def test_the_tar_is_byte_identical_across_runs(self):
        bundle = self.planned()
        shas = []
        for _ in range(2):
            self.fx["releases"], self.fx["git_tags"] = {}, {}
            r = self.publish(bundle)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertGreater(len(self.asset_bytes("promoted-412")), 0)
            shas.append(self.fx["releases"]["promoted-412"]["assets"][ASSET])
        self.assertEqual(shas[0], shas[1])
        listed = subprocess.run(
            ["tar", "-tf", str(self.dir / "release-assets" / shas[0])],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.split()
        for name in (
            "./promotion.json",
            "./release.json",
            "./bundle.sha256",
            "./bundle.sha256.sigstore.json",
        ):
            self.assertIn(name, listed)

    def test_the_published_release_is_read_back(self):
        bundle = self.planned()
        r = self.publish(bundle)
        self.assertEqual(r.returncode, 0, r.stderr)
        after = self.calls()[
            [c[:3] for c in self.calls()].index(["gh", "release", "edit"]) + 1 :
        ]
        self.assertIn(["gh", "release", "view"], [c[:3] for c in after])
        self.assertIn(["gh", "release", "download"], [c[:3] for c in after])
        self.assertIn(
            f"]({'https://github.com/' + REPO}/releases/tag/promoted-412)", r.stdout
        )
        self.fx["releases"], self.fx["git_tags"] = {}, {}
        self.fx["release_edits_lost"] = True
        self.refused(self.publish(bundle), "promoted-412", "draft")

    def test_no_call_uses_clobber(self):
        self.first_promotion()
        bundle = self.planned()
        self.assertEqual(self.publish(bundle).returncode, 0)
        calls = self.calls()
        self.assertTrue(
            [
                c
                for c in calls
                if c[:3] in (["gh", "release", "create"], ["gh", "release", "upload"])
            ]
        )
        self.assertFalse(
            [c for c in calls if any(a.startswith("--clobber") for a in c)]
        )


if __name__ == "__main__":
    unittest.main()
