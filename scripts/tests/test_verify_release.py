"""PL54: verify.py refuses a job of release.yml that does not check the ref first, and a
request-release job that does not wait for every job of the build."""

import importlib.util
import pathlib
import shutil
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

ROOT = SCRIPT.parents[1]
CHECK = "        run: '" + verify.RELEASE_REF_CHECK + "'\n"


def tree(release=lambda t: t, orchestrator=lambda t: t):
    root = pathlib.Path(tempfile.mkdtemp())
    (root / ".github/workflows").mkdir(parents=True)
    for rel, change in ((verify.RELEASE_YML, release), (verify.ORCHESTRATOR_YML, orchestrator)):
        (root / rel).write_text(change((ROOT / rel).read_text()))
    return root


class ReleaseProblems(unittest.TestCase):
    def tearDown(self):
        for root in getattr(self, "roots", []):
            shutil.rmtree(root)

    def problems(self, **changes):
        root = tree(**changes)
        self.roots = getattr(self, "roots", []) + [root]
        return verify.release_problems(root)

    def test_the_repository_passes(self):
        self.assertEqual(verify.release_problems(ROOT), [])

    def test_a_job_without_the_ref_check_fails(self):
        def drop_last(text):
            head, _, tail = text.rpartition(CHECK)
            return head + "        run: 'true'\n" + tail
        self.assertEqual(self.problems(release=drop_last),
                         ["release.yml: job request-acceptance runs a step before RELEASE_REF_CHECK"])

    def test_a_ref_check_after_another_step_fails(self):
        def move(text):
            return text.replace("      - name: 🌿 Release only from iso-v0\n" + CHECK, "", 1).replace(
                "          path: ${{ runner.temp }}/out/\n          if-no-files-found: error\n",
                "          path: ${{ runner.temp }}/out/\n          if-no-files-found: error\n"
                "      - name: late\n" + CHECK, 1)
        self.assertEqual(self.problems(release=move),
                         ["release.yml: job candidate runs a step before RELEASE_REF_CHECK"])

    def test_a_ref_check_that_may_be_skipped_or_fail_fails(self):
        def qualify(text):
            return text.replace(CHECK, CHECK + "        continue-on-error: true\n        if: false\n", 1)
        self.assertEqual(self.problems(release=qualify),
                         ["release.yml: job candidate qualifies RELEASE_REF_CHECK with continue-on-error, if"])

    def test_request_release_without_a_need_fails(self):
        def drop(text):
            return text.replace("dag-compile, system-image]", "system-image]", 1)
        self.assertEqual(self.problems(orchestrator=drop),
                         ["athanor-forge-orchestrator.yml: request-release does not need dag-compile"])

    def test_request_release_accepting_a_failure_fails(self):
        def loosen(text):
            return text.replace("""contains(fromJSON('["success", "skipped"]'), needs.dag-compile.result)""",
                                "true", 1)
        self.assertEqual(self.problems(orchestrator=loosen),
                         ["athanor-forge-orchestrator.yml: request-release does not ask a success or a skip of dag-compile"])

    def test_request_release_accepting_a_skipped_image_fails(self):
        def loosen(text):
            head, _, tail = text.partition("  request-release:")
            return head + "  request-release:" + tail.replace("needs.system-image.result == 'success'", "true", 1)
        self.assertEqual(self.problems(orchestrator=loosen),
                         ["athanor-forge-orchestrator.yml: request-release does not ask a success of system-image"])


if __name__ == "__main__":
    unittest.main()
