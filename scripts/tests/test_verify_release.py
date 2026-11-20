"""PL10, PL54, PL56, PL57, PL60: verify.py refuses a cache step on the release path, a job of
release.yml that does not check the ref first, a request-release job that does not wait for
every job of the build or dispatches the release off iso-v0, another trigger or concurrency
group, an input interpolated into run:, and a signing job, a writer of :latest or of :stable
outside the place the spec gives it."""

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


class PolicyPullProblems(unittest.TestCase):
    def test_policy_pull_the_repository_passes(self):
        self.assertEqual(verify.policy_pull_problems(ROOT), [])

    def test_policy_pull_outside_verify_images_fails(self):
        root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        (root / "scripts/ci/tests").mkdir(parents=True)
        (root / "system").mkdir()
        (root / "scripts/ci/promote.sh").write_text(
            'skopeo --registries.d "$d" \\\n    copy --policy "$p" docker://r@d dir:x\n')
        (root / "scripts/ci/global.sh").write_text("skopeo --policy p copy docker://r@d dir:x\n")
        (root / "scripts/ci/comment.sh").write_text("# skopeo copy --policy p\n")
        (root / "scripts/ci/tests/fake.sh").write_text("skopeo copy --policy p\n")
        (root / "system/verify-images.sh").write_text("skopeo copy --policy p\n")
        self.assertEqual(verify.policy_pull_problems(root), [
            f"scripts/ci/{name}: pulls through a policy outside system/verify-images.sh, which runs it "
            "in the builder image (runner skopeo 1.13)" for name in ("global.sh", "promote.sh")])


class ReleaseCacheProblems(unittest.TestCase):
    """PL10: a job of a release-path workflow, one that signs, attests, pushes or promotes, or of a
    reusable workflow one calls, restores and saves no cache; a pull-request job may."""

    RELEASING = ("on: workflow_dispatch\njobs:\n  sign:\n    environment: signing\n    runs-on: x\n"
                 "    steps:\n      - run: true\n  build:\n    uses: ./.github/workflows/call-build.yml\n")
    CALLED = "on: workflow_call\njobs:\n  compile:\n    runs-on: x\n    steps:\n{steps}"
    PULL_REQUEST = "on: pull_request\njobs:\n  check:\n    runs-on: x\n    steps:\n{steps}"
    PIN = "0" * 40

    def problems(self, actions=(), **workflows):
        root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        (root / ".github/workflows").mkdir(parents=True)
        for name, text in workflows.items():
            (root / ".github/workflows" / name).write_text(text)
        for name, text in dict(actions).items():
            (root / ".github/actions" / name).mkdir(parents=True)
            (root / ".github/actions" / name / "action.yml").write_text(text)
        return verify.release_cache_problems(root)

    def called(self, steps, actions=()):
        """The problems of a reusable workflow with these steps, called by a signing workflow."""
        return self.problems(actions, **{"release.yml": self.RELEASING,
                                         "call-build.yml": self.CALLED.format(steps=steps)})

    @staticmethod
    def unlisted(action, job="compile", name="call-build.yml"):
        return (f"{name}: job {job} uses {action}, which is not in RELEASE_PATH_ACTIONS: add it once "
                "it is checked to restore and save no cache (PL10)")

    def test_the_repository_passes(self):
        self.assertEqual(verify.release_cache_problems(ROOT), [])

    def test_a_cache_action_in_a_called_workflow_fails(self):
        for action in ("actions/cache", "actions/cache/restore", "actions/cache/save", "Swatinem/rust-cache"):
            with self.subTest(action=action):
                steps = f"      - uses: {action}@{self.PIN}\n"
                self.assertEqual(
                    self.problems(**{"release.yml": self.RELEASING,
                                     "call-build.yml": self.CALLED.format(steps=steps)}),
                    [self.unlisted(action)])

    def test_a_cache_action_in_the_releasing_workflow_fails(self):
        text = self.RELEASING.replace("      - run: true\n", f"      - uses: actions/cache@{self.PIN}\n")
        self.assertEqual(len(self.problems(**{"release.yml": text})), 1)

    def test_a_cache_backend_on_the_actions_cache_fails(self):
        for steps in ("      - run: cargo build\n        env:\n          SCCACHE_GHA_ENABLED: 'true'\n",
                      "      - run: docker buildx build --cache-from type=gha .\n"):
            with self.subTest(steps=steps):
                self.assertEqual(
                    self.problems(**{"release.yml": self.RELEASING,
                                     "call-build.yml": self.CALLED.format(steps=steps)}),
                    ["call-build.yml: job compile gives a build a cache from outside the job (sccache "
                     "on the Actions cache, cache-from or cache-to) on the release path, which "
                     "restores and saves no cache (PL10)"])

    def test_any_cache_from_or_cache_to_fails(self):
        self.assertEqual(self.called("      - run: podman build --cache-from ghcr.io/o/cache .\n"),
                         ["call-build.yml: job compile gives a build a cache from outside the job (sccache "
                          "on the Actions cache, cache-from or cache-to) on the release path, which "
                          "restores and saves no cache (PL10)"])
        self.assertEqual(len(self.called("      - run: buildah build --cache-to ghcr.io/o/cache .\n")), 1)

    def test_an_action_outside_the_list_fails(self):
        for action in ("mozilla-actions/sccache-action", "actions/setup-go", "actions/setup-python",
                       "docker/build-push-action", "Actions/Cache", "docker://alpine"):
            with self.subTest(action=action):
                self.assertEqual(self.called(f"      - uses: {action}@{self.PIN}\n"), [self.unlisted(action)])

    def test_a_listed_action_passes_at_any_ref_and_case(self):
        steps = ("      - uses: Actions/Checkout@v4\n      - uses: sigstore/cosign-installer@main\n"
                 f"      - uses: actions/upload-artifact@{self.PIN}\n      - uses: ./.github/actions/kvm\n")
        self.assertEqual(self.called(steps), [])

    def test_a_local_composite_action_is_read_step_by_step(self):
        inner = f"runs:\n  using: composite\n  steps:\n    - uses: actions/cache/save@{self.PIN}\n"
        outer = "runs:\n  using: composite\n  steps:\n    - uses: ./.github/actions/kvm\n"
        self.assertEqual(self.called("      - uses: ./.github/actions/outer\n",
                                     {"outer": outer, "kvm": inner}),
                         [self.unlisted("./.github/actions/outer"), self.unlisted("actions/cache/save")])

    def test_a_reusable_workflow_the_check_cannot_follow_fails(self):
        for called in ("other/repo/.github/workflows/build.yml@" + self.PIN,
                       "ars-regia/athanor/.github/workflows/call-build.yml@" + self.PIN,
                       "./.github/workflows/../workflows/call-build.yml"):
            with self.subTest(called=called):
                text = self.RELEASING.replace("./.github/workflows/call-build.yml", called)
                self.assertEqual(self.problems(**{"release.yml": text}),
                                 [f"release.yml: job build calls {called}, which the check cannot follow: "
                                  "call a workflow of this repository as ./.github/workflows/<file> (PL10)"])

    def test_a_composite_action_that_is_not_yaml_is_reported(self):
        self.assertEqual(self.called("      - uses: ./.github/actions/kvm\n", {"kvm": "runs: [\n"})[0].split(":")[0],
                         "./.github/actions/kvm/action.yml")

    def test_a_workflow_level_backend_reaches_every_job(self):
        text = "env:\n  SCCACHE_GHA_ENABLED: 'true'\n" + self.CALLED.format(steps="      - run: true\n")
        self.assertEqual(len(self.problems(**{"release.yml": self.RELEASING, "call-build.yml": text})), 1)

    def test_a_pull_request_job_with_a_cache_passes(self):
        steps = f"      - uses: Swatinem/rust-cache@{self.PIN}\n      - uses: actions/cache@{self.PIN}\n"
        self.assertEqual(self.problems(**{"release.yml": self.RELEASING,
                                          "call-build.yml": self.CALLED.format(steps="      - run: true\n"),
                                          "pr.yml": self.PULL_REQUEST.format(steps=steps)}), [])

    def test_a_keyless_signer_or_a_registry_push_is_on_the_release_path(self):
        cache = f"      - uses: actions/cache@{self.PIN}\n"
        cases = {
            "sign.yml": ("on: push\njobs:\n  publish:\n    runs-on: x\n    permissions:\n"
                         "      id-token: write\n    steps:\n" + cache, "publish"),
            "push.yml": ("on: push\npermissions:\n  packages: write\njobs:\n  check:\n    runs-on: x\n"
                         "    steps:\n" + cache, "check"),
        }
        for name, (text, job) in cases.items():
            with self.subTest(name=name):
                self.assertEqual(self.problems(**{name: text}), [self.unlisted("actions/cache", job, name)])

    def test_a_job_that_narrows_the_token_leaves_the_grant_of_its_workflow(self):
        text = ("on: push\npermissions:\n  packages: write\njobs:\n  check:\n    runs-on: x\n"
                f"    permissions:\n      contents: read\n    steps:\n      - uses: actions/cache@{self.PIN}\n")
        self.assertEqual(self.problems(**{"check.yml": text}), [])

    def test_the_signer_image_and_the_kernel_build_are_on_the_release_path(self):
        path = verify.release_path_workflows(ROOT)
        for name in ("azoth-signer.yml", "kernel-build.yml", "call-dag-compile.yml", "release.yml"):
            self.assertIn(name, path)
        self.assertNotIn("pr.yml", path)

    def test_a_promotion_in_release_is_on_the_release_path(self):
        text = self.RELEASING.replace("environment: signing", "environment: release").replace(
            "      - run: true\n", f"      - uses: actions/cache/restore@{self.PIN}\n")
        self.assertEqual(len(self.problems(**{"promote.yml": text})), 1)


# The CI run's trust environment must not reach the scripts under test (#1257).
import os as _os  # noqa: E402
from unittest import mock as _mock  # noqa: E402

TRUST_ENV = ("GITHUB_BASE_REF", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_SERVER_URL", "TIER_TRUSTED_REFS",
             "GITHUB_ACTIONS", "GH_TOKEN", "GITHUB_TOKEN", "GITHUB_HEAD_REF", "GITHUB_REPOSITORY_OWNER")
_environ = _mock.patch.dict(_os.environ)


def setUpModule():
    _environ.start()
    [_os.environ.pop(name, None) for name in TRUST_ENV]


def tearDownModule():
    _environ.stop()


if __name__ == "__main__":
    unittest.main()
