"""Unit tests of system/kernel-artifacts.sh against an offline registry
(python3 -B -m unittest discover -s system/tests -v)."""

import hashlib
import json
import os
import pathlib
import re
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SCRIPT = ROOT / "system" / "kernel-artifacts.sh"
PINS = dict(re.findall(r"^(\w+)=(.*)$", (ROOT / "forge/specs/azoth/pins.env").read_text(), re.M))
# A version the NVIDIA open pin never holds, so a bump built from it always moves the pin.
NVIDIA_OPEN_BUMP = f"{PINS['NVIDIA_OPEN_VERSION']}.1"
NVR = subprocess.run(["bash", str(ROOT / "forge/specs/azoth/nvr.sh")], capture_output=True, text=True, check=True).stdout.strip()
REG = "ghcr.io/ars-regia"
KERNEL = "sha256:" + "1" * 64
DEVEL = "sha256:" + "2" * 64
OTHER_KERNEL = "sha256:" + "9" * 64
OTHER_NVR = "7.2.4-99.fc43.azoth.99"  # some other NVR the pins have since moved past
OTHER_FEDORA_KERNEL_NVR = "7.2.4-99.fc43"  # a FEDORA_KERNEL_NVR pin an attestation can carry
ATTESTED_OTHER_NVR = subprocess.run(
    ["bash", str(ROOT / "forge/specs/azoth/nvr.sh"), "/dev/stdin"],
    input=f"FEDORA_KERNEL_NVR={OTHER_FEDORA_KERNEL_NVR}\n", capture_output=True, text=True, check=True,
).stdout.strip()  # the NVR nvr.sh derives from it, i.e. what attested_nvr() must report
assert ATTESTED_OTHER_NVR != NVR, "OTHER_FEDORA_KERNEL_NVR must derive an NVR other than the real one"
INPUTS = json.loads(subprocess.run(["python3", str(ROOT / "forge/specs/azoth/build-inputs.py")], capture_output=True, text=True, check=True).stdout)
MODULE = {"open": "sha256:" + "3" * 64, "legacy": "sha256:" + "4" * 64}
BOOT = "sha256:" + "5" * 64
SECUREBOOT_CERT = hashlib.sha256((ROOT / "forge/specs/azoth/keys/secureboot/athanor-secureboot.pem").read_bytes()).hexdigest()
KERNEL_BUILD = "https://github.com/ars-regia/athanor/.github/workflows/kernel-build.yml@refs/heads/iso-v0"
KMOD = "https://github.com/ars-regia/athanor/.github/workflows/nvidia-kmod.yml@refs/heads/iso-v0"


def tag(branch, kernel=KERNEL):
    return f"{NVR}-k{kernel[7:19]}-{branch}-{PINS[f'NVIDIA_{branch.upper()}_VERSION']}"


def predicate(branch, kernel=KERNEL):
    return {"driver": branch, "version": PINS[f"NVIDIA_{branch.upper()}_VERSION"], "kernel": f"{NVR}.x86_64",
            "kernel_digest": kernel, "devel_digest": DEVEL,
            "pins": {k: v for k, v in PINS.items() if k.startswith("NVIDIA_")}}


def boot_tag(kernel=KERNEL):
    return f"{NVR}-k{kernel[7:19]}"


def boot_predicate(kernel=KERNEL, cert=SECUREBOOT_CERT):
    return {"boot": "vmlinuz", "kver": f"{NVR}.x86_64", "kernel_digest": kernel, "secureboot_cert": cert}


def published(branches=("open", "legacy"), boot=True):
    """A registry holding the signed kernel of the pins, the attested modules of BRANCHES and,
    with BOOT, its attested signed vmlinuz."""
    fx = {
        "tags": {f"{REG}/azoth:{NVR}": KERNEL, f"{REG}/azoth-devel:{NVR}": DEVEL},
        "signatures": {f"{REG}/azoth@{KERNEL}": KERNEL_BUILD, f"{REG}/azoth-devel@{DEVEL}": KERNEL_BUILD},
        # The pins attestation the workflow attaches to the kernel: the inputs of the build.
        "attestations": {f"{REG}/azoth@{KERNEL}": [{"identity": KERNEL_BUILD, "predicate": INPUTS}]},
        "errors": [],
    }
    for branch in branches:
        ref = f"{REG}/azoth-nvidia@{MODULE[branch]}"
        fx["tags"][f"{REG}/azoth-nvidia:{tag(branch)}"] = MODULE[branch]
        fx["signatures"][ref] = KMOD
        fx["attestations"][ref] = [{"identity": KMOD, "predicate": predicate(branch)}]
    if boot:
        ref = f"{REG}/azoth-boot@{BOOT}"
        fx["tags"][f"{REG}/azoth-boot:{boot_tag()}"] = BOOT
        fx["signatures"][ref] = KMOD
        fx["attestations"][ref] = [{"identity": KMOD, "predicate": boot_predicate()}]
    return fx


class Tool(unittest.TestCase):
    """A temporary directory with the fakes on PATH, the artifacts directory and a git identity."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        bin_dir = self.dir / "bin"
        bin_dir.mkdir()
        for tool in ("skopeo", "cosign", "gh"):
            (bin_dir / tool).symlink_to(HERE / "fake_registry.py")
        self.artifacts = self.dir / "artifacts"
        self.env = {
            "PATH": f"{bin_dir}:{os.environ['PATH']}", "HOME": str(self.dir), "RETRY_ATTEMPTS": "1",
            "KERNEL_ARTIFACTS_DIR": str(self.artifacts), "FAKE_REGISTRY": str(self.dir / "registry.json"),
            "FAKE_LOG": str(self.dir / "calls.log"), "GITHUB_REPOSITORY_OWNER": "ars-regia",
            "GITHUB_REPOSITORY": "ars-regia/athanor", "GITHUB_SERVER_URL": "https://github.com",
            "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
        }
        self.registry({})

    def tearDown(self):
        self.tmp.cleanup()

    def registry(self, fx):
        (self.dir / "registry.json").write_text(json.dumps(fx))

    def run_script(self, *args, cwd=None, script=SCRIPT):
        return subprocess.run(["bash", str(script), *args], capture_output=True, text=True, env=self.env, cwd=cwd or self.dir)

    def state_file(self):
        path = self.artifacts / "kernel-artifacts.env"
        return dict(line.split("=", 1) for line in path.read_text().splitlines()) if path.exists() else None


class Resolve(Tool):
    def test_ready_records_every_digest_and_the_tag_form(self):
        self.registry(published())
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        got = self.state_file()
        self.assertEqual(got["state"], "ready")
        self.assertEqual((got["kernel_digest"], got["devel_digest"]), (KERNEL, DEVEL))
        self.assertEqual((got["nvidia_open_digest"], got["nvidia_legacy_digest"]), (MODULE["open"], MODULE["legacy"]))
        self.assertEqual(got["nvidia_open_tag"], f"{NVR}-k{'1' * 12}-open-{PINS['NVIDIA_OPEN_VERSION']}")

    def test_ready_records_the_signed_vmlinuz(self):
        self.registry(published())
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        got = self.state_file()
        self.assertEqual((got["state"], got["boot_tag"], got["boot_digest"]), ("ready", boot_tag(), BOOT))

    def test_missing_signed_vmlinuz_is_modules_missing(self):
        self.registry(published(boot=False))
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        got = self.state_file()
        self.assertEqual(got["state"], "modules-missing")
        self.assertEqual(got["boot_tag"], boot_tag())
        self.assertNotIn("boot_digest", got)
        self.assertEqual((got["nvidia_open_digest"], got["nvidia_legacy_digest"]), (MODULE["open"], MODULE["legacy"]))

    def test_signed_vmlinuz_that_does_not_match_is_modules_missing(self):
        for case, identity, attested in (
            ("signed by Kernel Build, not NVIDIA kmod", KERNEL_BUILD, boot_predicate()),
            ("attested for another kernel", KMOD, boot_predicate(kernel=OTHER_KERNEL)),
            ("signed with a rotated certificate", KMOD, boot_predicate(cert="0" * 64)),
            ("attested as something else", KMOD, dict(boot_predicate(), boot="initramfs")),
        ):
            with self.subTest(case):
                fx = published()
                ref = f"{REG}/azoth-boot@{BOOT}"
                fx["signatures"][ref] = identity
                fx["attestations"][ref] = [{"identity": identity, "predicate": attested}]
                self.registry(fx)
                r = self.run_script("resolve")
                self.assertEqual(r.returncode, 0, r.stderr)
                got = self.state_file()
                self.assertEqual(got["state"], "modules-missing")
                self.assertNotIn("boot_digest", got)

    def test_signed_vmlinuz_registry_error_fails(self):
        fx = published()
        fx["errors"].append(f"{REG}/azoth-boot:{boot_tag()}")
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertNotEqual(r.returncode, 0)
        self.assertIsNone(self.state_file())

    def test_missing_module_tag_is_modules_missing(self):
        self.registry(published(branches=("open",)))
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        got = self.state_file()
        self.assertEqual(got["state"], "modules-missing")
        self.assertEqual(got["nvidia_open_digest"], MODULE["open"])
        self.assertNotIn("nvidia_legacy_digest", got)
        self.assertEqual(got["nvidia_legacy_tag"], tag("legacy"))

    def test_never_published_module_package_is_modules_missing(self):
        # ghcr.io denies the bearer token for a package that does not exist yet (azoth-nvidia
        # before the first NVIDIA build under an owner), instead of answering manifest unknown.
        fx = published(branches=())
        fx["unpublished"] = [f"{REG}/azoth-nvidia"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        got = self.state_file()
        self.assertEqual(got["state"], "modules-missing")
        self.assertEqual(got["nvidia_open_tag"], tag("open"))
        self.assertNotIn("nvidia_open_digest", got)
        self.assertIn(f"{REG}/azoth-nvidia: denied", r.stderr)

    def test_never_published_kernel_package_is_kernel_missing(self):
        fx = published()
        fx["unpublished"] = [f"{REG}/azoth"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file(), {"state": "kernel-missing", "nvr": NVR, "registry": REG})

    def test_publish_refuses_a_tag_the_anonymous_resolve_could_not_see(self):
        # A private azoth-nvidia: the anonymous resolve did not see the legacy tag, the
        # logged-in publish does. It must stop before building or pushing anything.
        self.registry(published(branches=("open",)))
        self.assertEqual(self.run_script("resolve").returncode, 0)
        self.registry(published())
        r = self.run_script(str(self.dir / "signed"), script=ROOT / "forge/specs/azoth/nvidia-publish.sh")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn(f"already holds {MODULE['legacy']}", r.stderr)
        self.assertIn("refusing to overwrite", r.stderr)

    def test_absent_kernel_is_kernel_missing(self):
        fx = published()
        del fx["tags"][f"{REG}/azoth:{NVR}"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file(), {"state": "kernel-missing", "nvr": NVR, "registry": REG})

    def test_unsigned_kernel_is_kernel_missing(self):
        fx = published()
        del fx["signatures"][f"{REG}/azoth@{KERNEL}"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "kernel-missing")

    def test_absent_devel_is_kernel_missing(self):
        fx = published()
        del fx["tags"][f"{REG}/azoth-devel:{NVR}"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "kernel-missing")

    def test_unsigned_devel_is_kernel_missing(self):
        fx = published()
        del fx["signatures"][f"{REG}/azoth-devel@{DEVEL}"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "kernel-missing")

    def test_kernel_tag_transport_error_fails(self):
        fx = published()
        fx["errors"].append(f"{REG}/azoth:{NVR}")
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 1)
        self.assertIn("i/o timeout", r.stderr)
        self.assertIsNone(self.state_file())

    def test_transient_signature_error_is_not_folded_into_unsigned(self):
        fx = published()
        fx["signature_transient_errors"] = [f"{REG}/azoth@{KERNEL}"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 1)
        self.assertIn("502 Bad Gateway", r.stderr)
        self.assertIsNone(self.state_file())

    def test_registry_error_fails_and_leaves_no_file(self):
        self.registry(published())
        self.assertEqual(self.run_script("resolve").returncode, 0)
        fx = published()
        fx["errors"].append(f"{REG}/azoth-nvidia:{tag('legacy')}")
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 1)
        self.assertIn("i/o timeout", r.stderr)
        self.assertIsNone(self.state_file())

    def test_rekor_error_fails(self):
        fx = published()
        fx["errors"].append(f"{REG}/azoth-nvidia@{MODULE['open']}")
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 1)
        self.assertIn("502 Bad Gateway", r.stderr)
        self.assertIsNone(self.state_file())

    def test_unsigned_module_tag_is_modules_missing(self):
        fx = published()
        del fx["signatures"][f"{REG}/azoth-nvidia@{MODULE['legacy']}"]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "modules-missing")
        self.assertNotIn("nvidia_legacy_digest", self.state_file())

    def test_module_signed_by_another_workflow_is_modules_missing(self):
        fx = published()
        ref = f"{REG}/azoth-nvidia@{MODULE['open']}"
        fx["signatures"][ref] = KERNEL_BUILD
        fx["attestations"][ref] = [{"identity": KERNEL_BUILD, "predicate": predicate("open")}]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "modules-missing")
        self.assertNotIn("nvidia_open_digest", self.state_file())

    def test_module_signed_without_attestation_is_modules_missing(self):
        fx = published()
        ref = f"{REG}/azoth-nvidia@{MODULE['open']}"
        fx["attestations"][ref] = []
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "modules-missing")
        self.assertNotIn("nvidia_open_digest", self.state_file())

    def test_attestation_for_wrong_driver_is_modules_missing(self):
        fx = published()
        ref = f"{REG}/azoth-nvidia@{MODULE['open']}"
        wrong = predicate("open")
        wrong["driver"] = "legacy"
        fx["attestations"][ref] = [{"identity": KMOD, "predicate": wrong}]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "modules-missing")
        self.assertNotIn("nvidia_open_digest", self.state_file())

    def test_attestation_for_another_kernel_digest_is_modules_missing(self):
        fx = published()
        ref = f"{REG}/azoth-nvidia@{MODULE['open']}"
        fx["attestations"][ref] = [{"identity": KMOD, "predicate": predicate("open", kernel=OTHER_KERNEL)}]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "modules-missing")
        self.assertNotIn("nvidia_open_digest", self.state_file())

    def test_attestation_for_another_devel_digest_is_modules_missing(self):
        fx = published()
        ref = f"{REG}/azoth-nvidia@{MODULE['open']}"
        stale = predicate("open")
        stale["devel_digest"] = OTHER_KERNEL
        fx["attestations"][ref] = [{"identity": KMOD, "predicate": stale}]
        self.registry(fx)
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "modules-missing")
        self.assertNotIn("nvidia_open_digest", self.state_file())

    def test_attestation_for_other_nvidia_pins_is_modules_missing(self):
        fx = published()
        ref = f"{REG}/azoth-nvidia@{MODULE['open']}"
        stale = predicate("open")
        stale["pins"]["NVIDIA_OPEN_COMMIT"] = "0" * 40
        fx["attestations"][ref] = [{"identity": KMOD, "predicate": stale}]
        self.registry(fx)
        self.assertEqual(self.run_script("resolve").returncode, 0)
        self.assertEqual(self.state_file()["state"], "modules-missing")

    def test_republished_kernel_fails_against_the_expected_digest(self):
        self.registry(published())
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIn("republished", r.stderr)

    def test_republished_kernel_leaves_no_stale_file(self):
        self.registry(published())
        self.assertEqual(self.run_script("resolve").returncode, 0)
        self.assertIsNotNone(self.state_file())
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIsNone(self.state_file())

    def test_expect_kernel_digest_fails_when_kernel_becomes_absent(self):
        fx = published()
        del fx["tags"][f"{REG}/azoth:{NVR}"]
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIsNone(self.state_file())

    def test_expect_kernel_digest_fails_when_kernel_becomes_unsigned(self):
        fx = published()
        del fx["signatures"][f"{REG}/azoth@{KERNEL}"]
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIsNone(self.state_file())

    def test_expect_kernel_digest_reports_kernel_missing_when_the_pins_moved_to_another_nvr(self):
        # The tag of the current (new) NVR does not exist yet: normal for a kernel just bumped,
        # not the withdrawal the plain die() message would suggest.
        fx = published()
        del fx["tags"][f"{REG}/azoth:{NVR}"]
        fx["configs"] = {f"{REG}/azoth@{KERNEL}": {"org.opencontainers.image.version": OTHER_NVR}}
        # The kernel of the older NVR has no attestation of the current pins: the label decides.
        fx["attestations"] = {}
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", KERNEL)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file(), {"state": "kernel-missing", "nvr": NVR, "registry": REG})

    def test_expect_kernel_digest_fails_when_the_same_nvr_loses_its_tag(self):
        # KERNEL's own label still names the current NVR: the tag really was withdrawn under an
        # unchanged NVR, not superseded by a pin bump, so this must still die.
        fx = published()
        del fx["tags"][f"{REG}/azoth:{NVR}"]
        fx["configs"] = {f"{REG}/azoth@{KERNEL}": {"org.opencontainers.image.version": NVR}}
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIn("no longer published", r.stderr)
        self.assertIsNone(self.state_file())

    def test_expect_kernel_digest_fails_on_an_attestation_only_outage(self):
        # attestation_errors fails only cosign verify-attestation on this ref: skopeo's config
        # read would succeed cleanly (no configs entry, i.e. an ordinary "no label"). EXPECT is
        # deliberately the current, matching KERNEL: if the outage were silently swallowed and
        # read as "not moved" (a false negative from pins_moved), nothing here would ever
        # notice the difference, because $kernel == $expect already skips every other die() and
        # the fixture is otherwise fully ready -- this run would wrongly succeed with
        # state=ready instead of failing, which is the only way this test can tell a real fix
        # from a swallowed die() that merely printed the right words on its way to a silent,
        # unrelated success.
        fx = published()
        fx["attestation_errors"] = [f"{REG}/azoth@{KERNEL}"]
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", KERNEL)
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("could not verify its pins attestation", r.stderr)
        self.assertIsNone(self.state_file())

    def test_pins_moved_check_prefers_the_verified_attestation_over_the_label(self):
        # The label disagrees (it still names the current NVR, i.e. "not moved"); the
        # cosign-verified attestation is the one that must decide, and it says the pins
        # moved on, so the stale expectation is dropped and this resolves to ready.
        fx = published()
        fx["attestations"][f"{REG}/azoth@{OTHER_KERNEL}"] = [
            {"identity": KERNEL_BUILD, "predicate": {"pins": {"FEDORA_KERNEL_NVR": OTHER_FEDORA_KERNEL_NVR}}}
        ]
        fx["configs"] = {f"{REG}/azoth@{OTHER_KERNEL}": {"org.opencontainers.image.version": NVR}}
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "ready")

    def test_pins_moved_check_scans_every_attestation_entry_for_the_pin(self):
        # An unrelated verified entry sorts first: attested_nvr must still find the pin in a
        # later one instead of stopping at the first entry, the way module_verdict already
        # scans every predicate with `any(.[]; ...)`.
        fx = published()
        fx["attestations"][f"{REG}/azoth@{OTHER_KERNEL}"] = [
            {"identity": KERNEL_BUILD, "predicate": {"unrelated": "entry"}},
            {"identity": KERNEL_BUILD, "predicate": {"pins": {"FEDORA_KERNEL_NVR": OTHER_FEDORA_KERNEL_NVR}}},
        ]
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "ready")

    def test_pins_moved_check_confirms_the_nvr_when_the_matching_attestation_sorts_first(self):
        # Two verified attestations can sit on one digest, since the identity regex accepts
        # kernel-build.yml on any branch: the one confirming the current NVR must decide
        # regardless of the other, so the expectation is kept and this still dies.
        fx = published()
        fx["attestations"][f"{REG}/azoth@{OTHER_KERNEL}"] = [
            {"identity": KERNEL_BUILD, "predicate": {"pins": {"FEDORA_KERNEL_NVR": PINS["FEDORA_KERNEL_NVR"]}}},
            {"identity": KERNEL_BUILD, "predicate": {"pins": {"FEDORA_KERNEL_NVR": OTHER_FEDORA_KERNEL_NVR}}},
        ]
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIn("republished", r.stderr)
        self.assertIsNone(self.state_file())

    def test_pins_moved_check_confirms_the_nvr_when_the_matching_attestation_sorts_second(self):
        # Same as above with the two attestations in the opposite order: picking an arbitrary
        # one (the first) would let the non-matching entry contest a confirmed NVR just
        # because it sorts first, wrongly dropping the republish guard here.
        fx = published()
        fx["attestations"][f"{REG}/azoth@{OTHER_KERNEL}"] = [
            {"identity": KERNEL_BUILD, "predicate": {"pins": {"FEDORA_KERNEL_NVR": OTHER_FEDORA_KERNEL_NVR}}},
            {"identity": KERNEL_BUILD, "predicate": {"pins": {"FEDORA_KERNEL_NVR": PINS["FEDORA_KERNEL_NVR"]}}},
        ]
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIn("republished", r.stderr)
        self.assertIsNone(self.state_file())

    def test_pins_moved_check_falls_back_to_the_label_when_the_attestation_is_unverified(self):
        # The attestation exists but was not signed by kernel-build.yml: it must not count,
        # falling back to the label, which agrees with the current NVR (not moved), so this
        # still dies exactly as an unrelated, stale digest should.
        fx = published()
        fx["attestations"][f"{REG}/azoth@{OTHER_KERNEL}"] = [
            {"identity": KMOD, "predicate": {"pins": {"FEDORA_KERNEL_NVR": OTHER_FEDORA_KERNEL_NVR}}}
        ]
        fx["configs"] = {f"{REG}/azoth@{OTHER_KERNEL}": {"org.opencontainers.image.version": NVR}}
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIn("republished", r.stderr)
        self.assertIsNone(self.state_file())

    def test_pins_moved_check_keeps_the_expectation_when_the_attestation_says_it_did_not_move(self):
        # The label disagrees (it names another NVR, i.e. it looks like "moved"); the
        # cosign-verified attestation says the real current NVR, so the expectation is kept
        # and this dies exactly as an unrelated, stale digest should — the attestation must
        # override the label in the "not moved" direction too, not just the "moved" one.
        fx = published()
        fx["attestations"][f"{REG}/azoth@{OTHER_KERNEL}"] = [
            {"identity": KERNEL_BUILD, "predicate": {"pins": {"FEDORA_KERNEL_NVR": PINS["FEDORA_KERNEL_NVR"]}}}
        ]
        fx["configs"] = {f"{REG}/azoth@{OTHER_KERNEL}": {"org.opencontainers.image.version": OTHER_NVR}}
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIn("republished", r.stderr)
        self.assertIsNone(self.state_file())

    def test_pins_moved_check_fails_on_a_malformed_attested_fedora_kernel_nvr(self):
        # A signed but garbled FEDORA_KERNEL_NVR must not silently run through nvr.sh into
        # some unpredictable string that happens to differ from the current NVR, quietly
        # dropping the expectation instead of a loud, obvious failure.
        fx = published()
        fx["attestations"][f"{REG}/azoth@{OTHER_KERNEL}"] = [
            {"identity": KERNEL_BUILD, "predicate": {"pins": {"FEDORA_KERNEL_NVR": "not-a-valid-nvr"}}}
        ]
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not look like one", r.stderr)
        self.assertIsNone(self.state_file())

    def test_pins_moved_past_the_expected_digest_resolves_normally_to_ready(self):
        # The current NVR's tag exists (KERNEL, untouched, fully ready) but the caller's
        # OTHER_KERNEL was resolved for a since-superseded NVR: the expectation no longer
        # applies to this resolution at all, which proceeds exactly as an unconstrained
        # resolve would, reporting the real current digest, never the stale OTHER_KERNEL.
        fx = published()
        fx["configs"] = {f"{REG}/azoth@{OTHER_KERNEL}": {"org.opencontainers.image.version": OTHER_NVR}}
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 0, r.stderr)
        got = self.state_file()
        self.assertEqual(got["state"], "ready")
        self.assertEqual(got["kernel_digest"], KERNEL)

    def test_pins_moved_past_the_expected_digest_resolves_normally_to_modules_missing(self):
        # Same as above, but the current NVR's own modules are not both published yet: the
        # dropped expectation still falls through to an ordinary (non-ready) resolution,
        # not to a forced kernel-missing.
        fx = published(branches=("open",))
        fx["configs"] = {f"{REG}/azoth@{OTHER_KERNEL}": {"org.opencontainers.image.version": OTHER_NVR}}
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 0, r.stderr)
        got = self.state_file()
        self.assertEqual(got["state"], "modules-missing")
        self.assertEqual(got["kernel_digest"], KERNEL)

    def test_pins_moved_past_the_expected_digest_reports_kernel_missing_when_the_new_kernel_is_not_signed(self):
        # The current NVR's own kernel exists but is not signed yet, exactly like a kernel
        # bump caught mid-publish: once the stale, superseded expectation stops applying,
        # this is kernel-missing through the ordinary "not ready" path, not a die().
        fx = published()
        del fx["signatures"][f"{REG}/azoth@{KERNEL}"]
        fx["configs"] = {f"{REG}/azoth@{OTHER_KERNEL}": {"org.opencontainers.image.version": OTHER_NVR}}
        self.registry(fx)
        r = self.run_script("resolve", "--expect-kernel-digest", OTHER_KERNEL)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "kernel-missing")

    def test_require_ready(self):
        self.registry(published(branches=("open",)))
        self.assertEqual(self.run_script("require-ready").returncode, 1)
        self.registry(published())
        self.assertEqual(self.run_script("require-ready").returncode, 0)

    def test_get_and_has(self):
        self.registry(published(branches=("open",)))
        self.run_script("resolve")
        self.assertEqual(self.run_script("get", "kernel_digest").stdout.strip(), KERNEL)
        self.assertEqual(self.run_script("get", "nvidia_legacy_digest").returncode, 1)
        self.assertEqual(self.run_script("has", "nvidia_open_digest").returncode, 0)
        self.assertEqual(self.run_script("has", "nvidia_legacy_digest").returncode, 1)

    def test_registry_prints_the_default_without_a_network_call(self):
        # No fixture is loaded (setUp leaves the fake registry empty): a real network call
        # would find nothing signed and fail differently than a plain digest mismatch.
        r = self.run_script("registry")
        self.assertEqual((r.returncode, r.stdout.strip()), (0, REG))
        self.assertFalse((self.dir / "calls.log").exists(), "registry must never call skopeo or cosign")

    def test_registry_honours_kernel_registry(self):
        env = dict(self.env, KERNEL_REGISTRY="registry.example/other")
        r = subprocess.run(["bash", str(SCRIPT), "registry"], capture_output=True, text=True, env=env, cwd=self.dir)
        self.assertEqual((r.returncode, r.stdout.strip()), (0, "registry.example/other"))


class Repo(Tool):
    """A git repository with a bare origin and a shallow clone, as actions/checkout leaves it."""

    def setUp(self):
        super().setUp()
        self.origin = self.dir / "origin.git"
        self.work = self.dir / "work"
        self.git("init", "-q", "--bare", "-b", "iso-v0", str(self.origin), cwd=self.dir)
        self.git("clone", "-q", str(self.origin), str(self.work), cwd=self.dir)
        self.commit({"README.md": "x\n", "forge/specs/azoth/pins.env": "".join(f"{k}={v}\n" for k, v in PINS.items())})

    def git(self, *args, cwd=None):
        return subprocess.run(["git", *args], cwd=cwd or self.work, env=self.env, check=True, capture_output=True, text=True).stdout.strip()

    def commit(self, files):
        for rel, text in files.items():
            path = self.work / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "change")
        self.git("push", "-q", "origin", "HEAD:iso-v0")
        return self.git("rev-parse", "HEAD")

    def shallow(self):
        """A depth-1 clone of origin, like the checkout of a push event."""
        clone = self.dir / "shallow"
        self.git("clone", "-q", "--depth=1", "-b", "iso-v0", f"file://{self.origin}", str(clone), cwd=self.dir)
        return clone

    def state(self, state, **extra):
        self.artifacts.mkdir(exist_ok=True)
        lines = {"state": state, "nvr": NVR, "registry": REG, **extra}
        (self.artifacts / "kernel-artifacts.env").write_text("".join(f"{k}={v}\n" for k, v in lines.items()))

    def pin_change(self, **pins):
        text = "".join(f"{k}={pins.get(k, v)}\n" for k, v in PINS.items())
        return {"forge/specs/azoth/pins.env": text}


class Cycle(Repo):
    def cycle(self, *args, cwd=None):
        return self.run_script("cycle", *args, cwd=cwd or self.shallow())

    def test_kernel_build_paths_match_the_workflow(self):
        text = (ROOT / ".github/workflows/kernel-build.yml").read_text()
        block = re.search(r"^  push:\n(?:    .*\n)*?    paths:\n((?:      - .*\n)+)", text, re.M).group(1)
        paths = [line.strip()[2:].replace("**", "*") for line in block.splitlines()]
        script = re.search(r"^KERNEL_BUILD_PATHS=\((.*)\)$", SCRIPT.read_text(), re.M).group(1)
        self.assertEqual(paths, re.findall(r"'([^']+)'", script))

    def test_ready_builds(self):
        self.state("ready")
        before = self.git("rev-parse", "HEAD")
        after = self.commit({"system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "build")

    def test_push_starting_kernel_build_defers_a_missing_kernel(self):
        self.state("kernel-missing")
        before = self.git("rev-parse", "HEAD")
        after = self.commit({**self.pin_change(FEDORA_KERNEL_NVR="7.2.6-100.fc43"), "system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "defer")
        self.assertIn("Kernel Build", r.stdout)

    def test_push_starting_kernel_build_defers_missing_modules(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        before = self.git("rev-parse", "HEAD")
        after = self.commit({".github/workflows/kernel-build.yml": "name: x\n", "system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "defer")

    def test_push_outside_kernel_build_with_missing_kernel_fails(self):
        self.state("kernel-missing")
        before = self.git("rev-parse", "HEAD")
        after = self.commit({"system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not start Kernel Build", r.stderr)
        self.assertNotIn("cycle", self.state_file())

    def test_push_outside_kernel_build_with_missing_modules_builds(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        before = self.git("rev-parse", "HEAD")
        after = self.commit({"system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "build")

    def test_push_without_previous_commit_and_missing_kernel_fails(self):
        self.state("kernel-missing")
        after = self.commit({"system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", "0" * 40, "--after", after)
        self.assertEqual(r.returncode, 1)
        self.assertIn("previous commit", r.stderr)

    def test_dispatch_superseded_by_a_newer_push_defers(self):
        self.state("kernel-missing")
        r = self.cycle("--event", "workflow_dispatch", "--sha", "a" * 40, "--head", "b" * 40)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "defer")

    def test_dispatch_at_head_with_missing_kernel_fails(self):
        self.state("kernel-missing")
        r = self.cycle("--event", "workflow_dispatch", "--sha", "a" * 40, "--head", "a" * 40)
        self.assertEqual(r.returncode, 1)
        self.assertIn("is not published", r.stderr)

    def test_manual_dispatch_with_missing_modules_builds(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        r = self.cycle("--event", "workflow_dispatch", "--head", "a" * 40)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "build")

    def test_schedule_with_missing_kernel_fails(self):
        self.state("kernel-missing")
        self.assertEqual(self.cycle("--event", "schedule").returncode, 1)

    def test_push_without_previous_commit_and_missing_modules_builds(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        after = self.commit({"system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", "0" * 40, "--after", after)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "build")

    def test_schedule_with_missing_modules_builds(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        r = self.cycle("--event", "schedule")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["cycle"], "build")

    def test_unfetchable_base_commit_fails(self):
        self.state("kernel-missing")
        before = "f" * 40  # well-formed, but no such commit exists anywhere
        after = self.commit({"system/x": "1\n"})
        r = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertNotEqual(r.returncode, 0)

    def test_cycle_rejects_an_unknown_event(self):
        self.state("ready")
        r = self.cycle("--event", "bogus", "--before", "a" * 40, "--after", "b" * 40)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("--event", r.stderr)

    def test_ready_with_a_malformed_before_fails(self):
        # Argument shape is checked before the state is even read: state=ready must not mask
        # a caller bug.
        self.state("ready")
        r = self.cycle("--event", "push", "--before", "not-a-sha", "--after", "a" * 40)
        self.assertNotEqual(r.returncode, 0)

    def test_push_without_after_fails(self):
        self.state("ready")
        r = self.cycle("--event", "push", "--before", "a" * 40)
        self.assertNotEqual(r.returncode, 0)

    def test_dispatch_with_sha_but_empty_head_fails(self):
        self.state("kernel-missing")
        r = self.cycle("--event", "workflow_dispatch", "--sha", "a" * 40)
        self.assertNotEqual(r.returncode, 0)

    def test_dispatch_with_a_short_sha_fails(self):
        self.state("kernel-missing")
        r = self.cycle("--event", "workflow_dispatch", "--sha", "a" * 7, "--head", "a" * 7 + "0" * 33)
        self.assertNotEqual(r.returncode, 0)

    def test_cycle_write_replaces_a_stale_decision(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        before = self.git("rev-parse", "HEAD")
        after = self.commit({"system/x": "1\n"})
        r1 = self.cycle("--event", "push", "--before", before, "--after", after)
        self.assertEqual(r1.returncode, 0, r1.stderr)
        self.assertEqual(self.state_file()["cycle"], "build")

        after2 = self.commit({".github/workflows/kernel-build.yml": "name: x\n"})
        clone2 = self.dir / "shallow2"
        self.git("clone", "-q", "--depth=1", "-b", "iso-v0", f"file://{self.origin}", str(clone2), cwd=self.dir)
        r2 = self.run_script("cycle", "--event", "push", "--before", after, "--after", after2, cwd=clone2)
        self.assertEqual(r2.returncode, 0, r2.stderr)

        text = (self.artifacts / "kernel-artifacts.env").read_text()
        self.assertEqual(text.count("cycle="), 1)
        self.assertEqual(self.run_script("get", "cycle").stdout.strip(), "defer")


class CheckPlan(Repo):
    def plan(self, files):
        self.commit(files)
        return self.run_script("check-plan", "--base", "HEAD^1", "--head", "HEAD", cwd=self.work)

    def test_ready_builds_three_images_with_the_delta(self):
        self.state("ready")
        r = self.plan({"system/x": "1\n"})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.state_file()["check_gpus"], self.state_file()["check_delta"]), ("none nvidia nvidia-legacy", "true"))

    def test_kernel_pin_bump_skips_every_build(self):
        self.state("kernel-missing")
        r = self.plan({**self.pin_change(FEDORA_KERNEL_NVR="7.2.6-100.fc43"), "forge/specs/azoth/SOURCES/sources.sha256": "h\n", "forge/specs/azoth/KERNEL.md": "k\n"})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.state_file()["check_gpus"], self.state_file()["check_delta"]), ("", "false"))
        self.assertIn("warning", r.stdout)

    def test_kernel_pin_bump_mixed_with_other_changes_fails(self):
        self.state("kernel-missing")
        r = self.plan({**self.pin_change(FEDORA_KERNEL_NVR="7.2.6-100.fc43"), "system/Containerfile": "FROM x\n"})
        self.assertEqual(r.returncode, 1)
        self.assertIn("own pull request", r.stderr)

    def test_nvidia_pin_bump_builds_the_default_image(self):
        self.state("modules-missing", kernel_digest=KERNEL, boot_digest=BOOT)
        r = self.plan({**self.pin_change(NVIDIA_OPEN_VERSION=NVIDIA_OPEN_BUMP), "system/nvidia/locks/open.lock": "l\n"})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.state_file()["check_gpus"], self.state_file()["check_delta"]), ("none", "true"))

    def test_nvidia_pin_bump_without_the_signed_vmlinuz_fails(self):
        # Even the default image copies the signed vmlinuz in: without it nothing can be built.
        self.state("modules-missing", kernel_digest=KERNEL)
        r = self.plan({**self.pin_change(NVIDIA_OPEN_VERSION=NVIDIA_OPEN_BUMP), "system/nvidia/locks/open.lock": "l\n"})
        self.assertEqual(r.returncode, 1)
        self.assertIn("signed vmlinuz", r.stderr)

    def test_missing_modules_with_unchanged_pins_fail(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        r = self.plan({"system/x": "1\n"})
        self.assertEqual(r.returncode, 1)
        self.assertIn("Orchestrator", r.stderr)

    def test_nvidia_and_kernel_pins_together_with_missing_modules_fail(self):
        self.state("modules-missing", kernel_digest=KERNEL)
        r = self.plan(self.pin_change(NVIDIA_OPEN_VERSION=NVIDIA_OPEN_BUMP, CACHYOS_PATCHES_COMMIT="f" * 40))
        self.assertEqual(r.returncode, 1)

    def test_kernel_missing_with_only_nvidia_pins_moved_fails(self):
        # only_kernel_pins is true (the diff touches only pins.env, a KERNEL_PIN_FILES member),
        # but no kernel pin actually moved: that never explains a missing kernel.
        self.state("kernel-missing")
        r = self.plan(self.pin_change(NVIDIA_OPEN_VERSION=NVIDIA_OPEN_BUMP))
        self.assertEqual(r.returncode, 1)

    def test_kernel_pin_files_match_the_bump_workflow(self):
        """The kernel group of kernel-bump.yml regenerates exactly KERNEL_PIN_FILES (O7, O8):
        its bump-pins artifact plus the files of the bump-prep artifact (the two manifests and
        the package locks), so the copy cannot drift from what check-plan trusts to recognize
        a pure pin bump. system/Containerfile is the system group's, in a PR of its own."""
        script = SCRIPT.read_text()
        nvidia_line = re.search(r"^NVIDIA_PIN_FILES=\(.*\)$", script, re.M).group(0)
        kernel_line = re.search(r"^KERNEL_PIN_FILES=\(.*\)$", script, re.M).group(0)
        bash = nvidia_line + "\n" + kernel_line + "\nprintf '%s\\n' \"${KERNEL_PIN_FILES[@]}\"\n"
        out = subprocess.run(["bash", "-c", bash], capture_output=True, text=True, check=True)
        kernel_pin_files = set(out.stdout.split())

        text = (ROOT / ".github/workflows/kernel-bump.yml").read_text()
        block = re.search(r"name: bump-pins\n(?:.*\n)*?          path: \|\n((?:            .*\n)+)", text).group(1)
        check_paths = {line.strip() for line in block.splitlines()} - {"body.md"}
        block = re.search(r"name: bump-prep\n(?:.*\n)*?          path: \|\n((?:            .*\n)+)", text).group(1)
        prep_paths = {line.strip() for line in block.splitlines()} - {"prep.md", "prep.log", "prep-outcome"}
        self.assertEqual(check_paths | prep_paths, kernel_pin_files)


def signed_by(ref_name, workflow="kernel-build.yml"):
    return f"https://github.com/ars-regia/athanor/.github/workflows/{workflow}@{ref_name}"


class TrustedRefs(Tool):
    """Only the workflows of the trusted refs may publish a kernel or the NVIDIA modules. Any
    other branch can be dispatched by whoever can write to the repository, and its signature is
    as valid as the trusted one's, so the ref is part of the identity that is trusted."""

    def resolve_with(self, fx, trusted=None):
        self.registry(fx)
        if trusted is not None:
            self.env["KERNEL_TRUSTED_REFS"] = trusted
        return self.run_script("resolve")

    def kernel_signed_by(self, identity):
        fx = published()
        fx["signatures"][f"{REG}/azoth@{KERNEL}"] = identity
        fx["signatures"][f"{REG}/azoth-devel@{DEVEL}"] = identity
        fx["attestations"][f"{REG}/azoth@{KERNEL}"] = [{"identity": identity, "predicate": INPUTS}]
        return fx

    def test_iso_v0_and_main_are_trusted_by_default(self):
        for ref in ("refs/heads/iso-v0", "refs/heads/main"):
            r = self.resolve_with(self.kernel_signed_by(signed_by(ref)))
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(self.state_file()["state"], "ready", ref)

    def test_a_kernel_published_from_another_branch_is_not_ready(self):
        r = self.resolve_with(self.kernel_signed_by(signed_by("refs/heads/claude/other")))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "kernel-missing")
        # The operator is told which identity signed it, since "missing" alone would mislead.
        self.assertIn("refs/heads/claude/other", r.stderr)

    def test_a_pull_request_or_a_tag_is_not_a_trusted_ref(self):
        for ref in ("refs/pull/7/merge", "refs/tags/v1"):
            self.resolve_with(self.kernel_signed_by(signed_by(ref)))
            self.assertEqual(self.state_file()["state"], "kernel-missing", ref)

    def test_the_ref_is_matched_exactly(self):
        for ref in ("refs/heads/iso-v0-evil", "refs/heads/iso-v0/x", "refs/heads/xiso-v0", "refs/heads/iso-v01"):
            self.resolve_with(self.kernel_signed_by(signed_by(ref)))
            self.assertEqual(self.state_file()["state"], "kernel-missing", ref)

    def test_another_workflow_on_a_trusted_ref_is_not_the_kernel_build(self):
        self.resolve_with(self.kernel_signed_by(signed_by("refs/heads/iso-v0", workflow="other.yml")))
        self.assertEqual(self.state_file()["state"], "kernel-missing")

    def test_the_trusted_refs_can_be_widened_by_name(self):
        fx = self.kernel_signed_by(signed_by("refs/heads/claude/other"))
        r = self.resolve_with(fx, trusted="iso-v0 claude/other")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "ready")

    def test_a_dot_in_a_trusted_name_is_a_dot_and_not_a_wildcard(self):
        r = self.resolve_with(self.kernel_signed_by(signed_by("refs/heads/releaseX1")), trusted="iso-v0 release.1")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "kernel-missing")
        r = self.resolve_with(self.kernel_signed_by(signed_by("refs/heads/release.1")), trusted="iso-v0 release.1")
        self.assertEqual(self.state_file()["state"], "ready")

    def test_a_name_with_a_metacharacter_is_refused(self):
        for trusted in ("iso-v0;true", "iso-v0|.*", "a b(c", "iso-v0\\", "*"):
            r = self.resolve_with(published(), trusted=trusted)
            self.assertEqual(r.returncode, 1, trusted)
            self.assertIn("KERNEL_TRUSTED_REFS", r.stderr)

    def test_the_modules_follow_the_same_rule(self):
        fx = published()
        for branch in ("open", "legacy"):
            ref = f"{REG}/azoth-nvidia@{MODULE[branch]}"
            other = signed_by("refs/heads/claude/other", workflow="nvidia-kmod.yml")
            fx["signatures"][ref] = other
            fx["attestations"][ref] = [{"identity": other, "predicate": predicate(branch)}]
        r = self.resolve_with(fx)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.state_file()["state"], "modules-missing")


class KernelInputs(Tool):
    """A signed kernel is ready only when a verified attestation carries exactly the inputs of
    this checkout, as the workflow's own reuse check requires. Signed by a trusted ref is not
    enough: the NVR is the same after a patch changes, and the tag would point at the old build."""

    def with_attestation(self, predicate_or_none, identity=KERNEL_BUILD):
        fx = published()
        ref = f"{REG}/azoth@{KERNEL}"
        fx["attestations"][ref] = [] if predicate_or_none is None else [{"identity": identity, "predicate": predicate_or_none}]
        self.registry(fx)

    def test_the_inputs_of_this_checkout_make_it_ready(self):
        self.registry(published())
        self.assertEqual(self.run_script("resolve").returncode, 0)
        self.assertEqual(self.state_file()["state"], "ready")

    def test_a_kernel_with_no_attestation_is_missing(self):
        self.with_attestation(None)
        self.assertEqual(self.run_script("resolve").returncode, 0)
        self.assertEqual(self.state_file()["state"], "kernel-missing")

    def test_a_kernel_built_from_other_inputs_is_missing(self):
        for change in (lambda i: i["patches_sha256"].update({"0001-x.patch": "0" * 64}),
                       lambda i: i["pins"].update({"FEDORA_KERNEL_NVR": "7.0.0-1.fc43"}),
                       lambda i: i.update({"build_sh_sha256": "1" * 64}),
                       lambda i: i.update({"builder_base": "registry.example/other@sha256:" + "2" * 64})):
            other = json.loads(json.dumps(INPUTS))
            change(other)
            self.with_attestation(other)
            self.assertEqual(self.run_script("resolve").returncode, 0)
            self.assertEqual(self.state_file()["state"], "kernel-missing")

    def test_any_verified_entry_with_the_inputs_is_enough(self):
        fx = published()
        stale = json.loads(json.dumps(INPUTS))
        stale["build_sh_sha256"] = "1" * 64
        fx["attestations"][f"{REG}/azoth@{KERNEL}"] = [
            {"identity": KERNEL_BUILD, "predicate": stale},
            {"identity": KERNEL_BUILD, "predicate": INPUTS},
        ]
        self.registry(fx)
        self.assertEqual(self.run_script("resolve").returncode, 0)
        self.assertEqual(self.state_file()["state"], "ready")

    def test_the_key_order_of_the_predicate_does_not_matter(self):
        self.with_attestation(dict(reversed(list(INPUTS.items()))))
        self.assertEqual(self.run_script("resolve").returncode, 0)
        self.assertEqual(self.state_file()["state"], "ready")

    def test_an_attestation_of_an_untrusted_ref_does_not_count(self):
        self.with_attestation(INPUTS, identity=signed_by("refs/heads/claude/other"))
        self.assertEqual(self.run_script("resolve").returncode, 0)
        self.assertEqual(self.state_file()["state"], "kernel-missing")

    def test_a_caller_that_resolved_the_kernel_is_told_it_was_republished_with_other_inputs(self):
        other = json.loads(json.dumps(INPUTS))
        other["build_sh_sha256"] = "1" * 64
        self.with_attestation(other)
        r = self.run_script("resolve", "--expect-kernel-digest", KERNEL)
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("other inputs", r.stderr)
        self.assertIsNone(self.state_file())

    def test_a_failure_to_read_the_inputs_is_an_error_and_never_ready(self):
        bin_dir = self.dir / "bin"
        (bin_dir / "python3").write_text("#!/bin/sh\necho 'build-inputs: broken' >&2\nexit 3\n")
        (bin_dir / "python3").chmod(0o755)
        self.registry(published())
        r = self.run_script("resolve")
        self.assertEqual(r.returncode, 1)
        self.assertIn("build-inputs", r.stderr)
        self.assertIsNone(self.state_file())


if __name__ == "__main__":
    unittest.main()
