"""PL54, PL56, PL57, PL60: verify.py refuses a job of release.yml that does not check the ref
first, a request-release job that does not wait for every job of the build or dispatches the
release off iso-v0, another trigger or concurrency group, an input interpolated into run:, and a
signing job, a writer of :latest or of :stable outside the place the spec gives it."""

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


    def test_a_trigger_beside_workflow_dispatch_fails(self):
        def push(text):
            return text.replace("on:\n  workflow_dispatch:\n", "on:\n  push:\n  workflow_dispatch:\n", 1)
        self.assertEqual(self.problems(release=push),
                         ["release.yml: starts on ['push', 'workflow_dispatch'], not on workflow_dispatch alone (PL54)"])

    def test_an_input_interpolated_into_run_fails(self):
        def interpolate(text):
            return text.replace('--out "${RUNNER_TEMP}/out" |',
                                '--out "${RUNNER_TEMP}/out" --run ${{ inputs.build_run_id }} |', 1)
        self.assertEqual(self.problems(release=interpolate),
                         ["release.yml: job candidate interpolates an input into run:, which only env may carry (PL54)"])

    def test_another_release_group_fails(self):
        def regroup(text):
            return text.replace("  group: ${{ github.workflow }}\n",
                                "  group: ${{ github.workflow }}-${{ github.ref }}\n", 1)
        self.assertEqual(self.problems(release=regroup),
                         ["release.yml: concurrency is not the group ${{ github.workflow }} without "
                          "cancel-in-progress (PL57)"])

    def test_another_build_group_fails(self):
        def cancel(text):
            return text.replace("cancel-in-progress: false", "cancel-in-progress: true", 1)
        self.assertEqual(self.problems(orchestrator=cancel),
                         ["athanor-forge-orchestrator.yml: concurrency is not the group "
                          "${{ github.workflow }}-${{ github.ref }} without cancel-in-progress (PL57)"])

    def test_a_job_level_group_fails(self):
        def group(text):
            return text.replace("  candidate:\n", "  candidate:\n    concurrency: other\n", 1)
        self.assertEqual(self.problems(release=group),
                         ["release.yml: job candidate declares a concurrency group of its own (PL57)"])

    def test_a_dispatch_off_iso_v0_fails(self):
        def anywhere(text):
            return text.replace("if: github.ref == 'refs/heads/iso-v0'", "if: always()", 1)
        self.assertEqual(self.problems(orchestrator=anywhere),
                         ["athanor-forge-orchestrator.yml: request-release does not dispatch release.yml once, "
                          "on iso-v0 alone, with its run id (PL54)"])

    def test_a_signing_job_outside_release_fails(self):
        for name in ("signing", "Signing-Images"):
            with self.subTest(environment=name):
                def move(text, name=name):
                    return text.replace("environment: signing-kernel", f"environment: {name}", 1)
                self.assertEqual(self.problems(orchestrator=move),
                                 [f"athanor-forge-orchestrator.yml: job nvidia-kmod-sign runs in {name.lower()}, "
                                  "which only release.yml may use (PL56)"])

    def test_a_second_writer_of_latest_fails(self):
        def tag(text):
            return text + "      - run: bash system/tag-images.sh --registry r --tag latest d.txt\n"
        self.assertEqual(self.problems(orchestrator=tag),
                         ["athanor-forge-orchestrator.yml: job request-release moves :latest of the system "
                          "images, which only tag-latest of release.yml writes (PL35, PL56)"])

    def test_an_input_in_any_expression_form_fails(self):
        for expression in ("${{ inputs['build_run_id'] }}", "${{ format('{0}', inputs.build_run_id) }}",
                           "${{ github.event.inputs.build_run_id }}"):
            with self.subTest(expression):
                def inject(text):
                    return text.replace("        run: echo \"Dispatched by ${GITHUB_TRIGGERING_ACTOR}.\"",
                                        "        run: echo \"" + expression + "\"", 1)
                self.assertEqual(self.problems(release=inject),
                                 ["release.yml: job candidate interpolates an input into run:, which only env "
                                  "may carry (PL54)"])

    def test_a_dispatch_that_may_fail_quietly_fails(self):
        for extra in ("continue-on-error: true", "shell: sh", "timeout-minutes: 1"):
            with self.subTest(extra):
                def qualify(text):
                    return text.replace("        if: github.ref == 'refs/heads/iso-v0'\n",
                                        "        if: github.ref == 'refs/heads/iso-v0'\n        " + extra + "\n", 1)
                self.assertEqual(self.problems(orchestrator=qualify),
                                 ["athanor-forge-orchestrator.yml: request-release does not dispatch release.yml "
                                  "once, on iso-v0 alone, with its run id (PL54)"])

    def test_a_dispatch_with_another_token_fails(self):
        def token(text):
            return text.replace("GH_TOKEN: ${{ github.token }}", "GH_TOKEN: ${{ secrets.PAT }}", 1)
        self.assertEqual(self.problems(orchestrator=token),
                         ["athanor-forge-orchestrator.yml: request-release does not dispatch release.yml "
                          "once, on iso-v0 alone, with its run id (PL54)"])

    def test_a_writer_of_latest_in_another_spelling_fails(self):
        for call in ("bash system/tag-images.sh --registry r \\\n          --tag latest d.txt",
                     "bash system/tag-images.sh --registry r --tag \\\n            latest d.txt",
                     "bash system/tag-images.sh --registry r --tag\tlatest d.txt",
                     "bash system/tag-images.sh --registry r --tag=latest d.txt",
                     "bash system/tag-images.sh --registry r --tag \"latest\" d.txt",
                     "cd system && bash tag-images.sh --registry r --tag latest d.txt"):
            with self.subTest(call):
                def tag(text):
                    return text + "      - run: |\n          " + call + "\n"
                self.assertEqual(self.problems(orchestrator=tag),
                                 ["athanor-forge-orchestrator.yml: job request-release moves :latest of the "
                                  "system images, which only tag-latest of release.yml writes (PL35, PL56)"])

    def test_another_tag_named_like_latest_passes(self):
        def tag(text):
            return text + "      - run: bash system/tag-images.sh --registry r --tag latest-rc d.txt\n"
        self.assertEqual(self.problems(orchestrator=tag), [])

    def test_a_missing_release_workflow_fails(self):
        root = tree()
        self.roots = getattr(self, "roots", []) + [root]
        (root / verify.RELEASE_YML).unlink()
        self.assertEqual(verify.release_problems(root), ["release.yml: missing (PL54)"])

    def test_stable_outside_release_fails(self):
        def promote(text):
            return text + "      - run: bash system/promote.sh 412\n"
        self.assertEqual(self.problems(orchestrator=promote),
                         ["athanor-forge-orchestrator.yml: job request-release writes :stable outside the "
                          "environment release (PL60)"])


if __name__ == "__main__":
    unittest.main()
