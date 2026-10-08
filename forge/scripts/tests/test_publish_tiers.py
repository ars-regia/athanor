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
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        stubs = {
            "skopeo": f"""\
                name=${{2#docker://{REGISTRY}/}}
                name=${{name%:latest}}
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
                    printf 'sha256:%s' "$(printf '%s' "${{name%:latest}}:new" | sha256sum | cut -d' ' -f1)" > "$3" ;;
                esac
                """,
            "createrepo_c": "mkdir -p repodata\n",
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
            if k not in ("REGISTRY_HOST", "GITHUB_REPOSITORY_OWNER", "FAIL_PUSH")
        }
        self.env["PATH"] = f"{bin_dir}:{os.environ['PATH']}"
        self.env["GITHUB_REPOSITORY_OWNER"] = "owner"

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

    def publish_current(self, *names):
        """Publish NAMES with the RPM content they have now, at generation "old"."""
        for name in names:
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

    def test_a_usage_error_without_out(self):
        r = subprocess.run(
            ["bash", str(SCRIPT)],
            cwd=self.forge,
            capture_output=True,
            text=True,
            env=self.env,
        )
        self.assertEqual(r.returncode, 2)


if __name__ == "__main__":
    unittest.main()
