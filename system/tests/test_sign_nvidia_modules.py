"""Unit tests of system/sign-nvidia-modules.sh and of the sign job of nvidia-kmod.yml, with a podman
stub that plays the nvidia container (python3 -B -m unittest discover -s system/tests -v)."""

import hashlib
import json
import os
import pathlib
import stat
import subprocess
import sys
import tempfile
import textwrap
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from test_sign_kernel import ARTIFACTS_STUB, NVR, RECORD, ROOT, SECRET  # noqa: E402

SIGN = ROOT / "system" / "sign-nvidia-modules.sh"
WORKFLOW = ROOT / ".github" / "workflows" / "nvidia-kmod.yml"
DEVEL = "sha256:" + "2" * 64
MODULES = [
    f"{driver}/lib/modules/k/extra/nvidia/{name}.ko"
    for driver in ("open", "legacy")
    for name in ("nvidia", "nvidia-modeset")
]
RESOLVED = (
    f"state=modules-missing\\nnvr={NVR}\\nregistry=registry.example/owner\\n"
    f"kernel_digest=sha256:{'1' * 64}\\ndevel_digest={DEVEL}\\n"
)

# A podman that records every call, and on `run` does what nvidia.sh sign would: reads the
# mounted key and marks every module under /out as signed.
STUB = (
    "#!/usr/bin/env python3\n"
    + RECORD
    + textwrap.dedent("""\
    import stat
    state = pathlib.Path(os.environ["STUB_STATE"])
    args = sys.argv[1:]
    record(args)
    if args[0] == "create":
        print("ctr")
    elif args[0] == "cp":
        pathlib.Path(args[2], "kernel-devel-x.rpm").write_text("rpm")
    elif args[0] == "run":
        mounts = dict(reversed(a.split(":")[:2]) for a in args if a.count(":") >= 1 and a.startswith("/"))
        key = pathlib.Path(mounts["/run/module.key"])
        (state / "key.mode").write_text(oct(stat.S_IMODE(key.stat().st_mode)))
        (state / "key.seen").write_text(key.read_text())
        for ko in pathlib.Path(mounts["/out"]).rglob("*.ko"):
            ko.write_text("signed")
        sys.exit(int(os.environ.get("STUB_RUN_STATUS", "0")))
    """)
)


class SignNvidiaModules(unittest.TestCase):
    """The script runs from a copy of the tree whose system/kernel-artifacts.sh is a stub."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        (bin_dir / "record.py").write_text(RECORD + "record(sys.argv[1:])\n")
        (bin_dir / "podman").write_text(STUB)
        (bin_dir / "podman").chmod(0o755)
        self.tree = self.dir / "tree"
        (self.tree / "system").mkdir(parents=True)
        (self.tree / "forge").symlink_to(ROOT / "forge")
        self.sign = self.tree / "system" / "sign-nvidia-modules.sh"
        self.sign.write_text(SIGN.read_text())
        (self.tree / "system" / "kernel-artifacts.sh").write_text(ARTIFACTS_STUB)
        self.state = self.dir / "state"
        self.state.mkdir()
        (self.dir / "runtime").mkdir(mode=0o700)
        self.modules = self.dir / "out"
        self.write_modules()
        # The artifacts of the run, as a job that could write them would leave them.
        forged = self.dir / "artifacts"
        forged.mkdir()
        (forged / "kernel-artifacts.env").write_text(
            f"state=modules-missing\nnvr={NVR}\nregistry=evil.example/x\ndevel_digest=sha256:{'6' * 64}\n"
        )
        self.env = {
            k: v for k, v in os.environ.items() if not k.startswith("MODULE_SIGNING")
        }
        self.env.update(
            PATH=f"{bin_dir}:{os.environ['PATH']}",
            STUB_BIN=str(bin_dir),
            STUB_STATE=str(self.state),
            STUB_RESOLVED=RESOLVED,
            KERNEL_ARTIFACTS_DIR=str(forged),
            XDG_RUNTIME_DIR=str(self.dir / "runtime"),
            NVIDIA_MANIFEST_OPEN=self.manifest("open"),
            NVIDIA_MANIFEST_LEGACY=self.manifest("legacy"),
        )

    def write_modules(self):
        """The unsigned modules of both branches, as the build artifacts unpack them."""
        for path in MODULES:
            ko = self.modules / path
            ko.parent.mkdir(parents=True, exist_ok=True)
            ko.write_text(f"unsigned {ko.name}")

    def manifest(self, driver):
        return "\n".join(
            f"{hashlib.sha256((self.modules / path).read_bytes()).hexdigest()}  {path}"
            for path in MODULES
            if path.startswith(driver + "/")
        )

    def tearDown(self):
        self.tmp.cleanup()

    def run_sign(self, **env):
        return subprocess.run(
            [
                "bash",
                str(self.sign),
                "--modules",
                str(self.modules),
                "--devel",
                str(self.dir / "devel"),
            ],
            capture_output=True,
            text=True,
            env=dict(self.env, **env),
        )

    def calls(self):
        log = self.state / "calls.log"
        return (
            [json.loads(line) for line in log.read_text().splitlines()]
            if log.exists()
            else []
        )

    def test_without_the_key_nothing_runs(self):
        r = self.run_sign()
        self.assertEqual(r.returncode, 2)
        self.assertIn("signing environment", r.stderr)
        self.assertEqual(self.calls(), [])

    def test_the_devel_is_resolved_here_never_read_from_the_run_artifacts(self):
        r = self.run_sign(MODULE_SIGNING_KEY=SECRET)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(
            self.calls()[0]["args"][:2], ["kernel-artifacts.sh", "resolve"]
        )
        pull = next(c["args"] for c in self.calls() if c["args"][0] == "pull")
        self.assertEqual(pull[-1], f"registry.example/owner/azoth-devel@{DEVEL}")
        for path in MODULES:
            self.assertEqual((self.modules / path).read_text(), "signed")
        self.assertTrue((self.dir / "devel" / "kernel-devel-x.rpm").exists())

    def test_an_unverified_kernel_is_refused_before_any_container(self):
        r = self.run_sign(
            MODULE_SIGNING_KEY=SECRET,
            STUB_RESOLVED=f"state=kernel-missing\\nnvr={NVR}\\nregistry=registry.example/owner\\n",
        )
        self.assertEqual(r.returncode, 1)
        self.assertIn("nothing to sign with", r.stderr)
        self.assertTrue(
            all(c["args"][0] == "kernel-artifacts.sh" for c in self.calls()),
            self.calls(),
        )

    def test_the_key_exists_only_while_the_sign_container_runs(self):
        for status in ("0", "3"):
            with self.subTest(status=status):
                (self.state / "calls.log").unlink(missing_ok=True)
                self.write_modules()
                r = self.run_sign(MODULE_SIGNING_KEY=SECRET, STUB_RUN_STATUS=status)
                self.assertEqual(r.returncode, int(status), r.stderr)
                steps = [(c["args"][0], c["key"]) for c in self.calls()]
                self.assertEqual(steps[-1], ("run", True), steps)
                self.assertTrue(all(not key for _, key in steps[:-1]), steps)
                self.assertEqual(list((self.dir / "runtime").iterdir()), [])
                for call in self.calls():
                    self.assertEqual(call["env"], [])
                    self.assertFalse(any("not-a-real-key" in a for a in call["args"]))
        self.assertEqual((self.state / "key.seen").read_text(), SECRET + "\n")
        self.assertEqual((self.state / "key.mode").read_text(), "0o600")
        run = next(c["args"] for c in self.calls() if c["args"][0] == "run")
        self.assertIn("--network=none", run)

    def assert_refused(self, message, **env):
        r = self.run_sign(MODULE_SIGNING_KEY=SECRET, **env)
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn(message, r.stderr)
        self.assertEqual(self.calls(), [], "something ran before the modules were checked")
        self.assertEqual(list((self.dir / "runtime").iterdir()), [])

    def test_a_module_the_build_did_not_report_is_not_signed(self):
        # Another artifact matching the old download pattern, merged into the same directory.
        extra = self.modules / "evil/lib/modules/k/extra/nvidia/nvidia.ko"
        extra.parent.mkdir(parents=True)
        extra.write_text("not from the build")
        self.assert_refused("not those the build reported")

    def test_a_module_replaced_after_the_build_is_not_signed(self):
        (self.modules / MODULES[0]).write_text("replaced")
        self.assert_refused("differs from the one the build reported")

    def test_a_missing_module_is_not_signed_around(self):
        (self.modules / MODULES[-1]).unlink()
        self.assert_refused("not those the build reported")

    def test_a_branch_the_build_did_not_report_stops_the_signing(self):
        self.assert_refused("NVIDIA_MANIFEST_LEGACY is empty", NVIDIA_MANIFEST_LEGACY="")

    def test_a_manifest_line_outside_its_branch_is_refused(self):
        for line in (
            f"{'0' * 64}  open/../../etc/x.ko",
            f"{'0' * 64}  legacy/lib/nvidia.ko",
            f"{'0' * 64}  open/lib/nvidia.so",
            "nvidia.ko",
        ):
            with self.subTest(line=line):
                self.assert_refused(
                    "is not the hash and path of a module",
                    NVIDIA_MANIFEST_OPEN=self.manifest("open") + "\n" + line,
                )


class SignJob(unittest.TestCase):
    """The sign job of nvidia-kmod.yml, as text: the key only from the branches that publish,
    and the kernel-devel never from the artifacts of the run."""

    def setUp(self):
        sys.path.insert(0, str(ROOT / "scripts"))
        import verify

        _, self.lines = verify.workflow_jobs(verify.read(WORKFLOW))["sign"]
        self.body = "\n".join(self.lines)

    def test_the_modules_are_signed_from_the_publishing_branches_only(self):
        self.assertIn(
            "    if: github.ref == 'refs/heads/iso-v0' || github.ref == 'refs/heads/main'",
            self.lines,
        )

    def test_the_modules_come_by_name_with_the_build_manifest(self):
        self.assertNotIn("pattern:", self.body)
        for driver in ("open", "legacy"):
            self.assertIn(f"          name: nvidia-{driver}-unsigned", self.lines)
            self.assertIn(
                f"          NVIDIA_MANIFEST_{driver.upper()}: ${{{{ needs.build.outputs.manifest-{driver} }}}}",
                self.lines,
            )
        build = (ROOT / ".github/workflows/nvidia-build.yml").read_text()
        for driver in ("open", "legacy"):
            self.assertIn(f"manifest-{driver}:", build)

    def test_the_sign_job_takes_no_kernel_from_the_run(self):
        self.assertNotIn("nvidia-kernel-artifacts", self.body)
        self.assertNotIn("kernel_digest", self.body)
        self.assertIn("bash system/sign-nvidia-modules.sh", self.body)


if __name__ == "__main__":
    unittest.main()
