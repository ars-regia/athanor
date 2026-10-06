"""Unit test of forge/specs/azoth/signer/run.sh with a stand-in podman, curl and
system/kernel-artifacts.sh (python3 -B -m unittest discover -s forge/specs/azoth/tests -v): the
signer runs by digest, without network and without the checkout; what the sign job signs is
derived and verified in that job before any key exists; the keys never outlive the step."""

import hashlib
import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import unittest

REPO = pathlib.Path(__file__).resolve().parents[4]
DIGEST = "sha256:" + "a" * 64
KERNEL = "sha256:" + "1" * 64
DEVEL = "sha256:" + "2" * 64
EVIL = "sha256:" + "e" * 64
REG = "ghcr.io/ars-regia"
KVER = "6.18.38-1.azoth.fc43.x86_64"
COSIGN = b"cosign v3 stand-in\n"

# Records each call; for `run`, whether every mounted key file is readable right then, and the
# files `prepare` would write into the directory mounted on /out. The tree mounted on /modules
# goes through the real allow-list of sign-kernel.sh, as in the signer image.
PODMAN = f"""#!/usr/bin/env bash
set -euo pipefail
echo "podman $*" >> "$PODMAN_LOG"
if [[ $1 == run ]]; then
  for arg in "$@"; do
    case $arg in
      /*:/run/keys/*) [[ -s ${{arg%%:*}} ]] && echo "key readable ${{arg%%:*}}" >> "$PODMAN_LOG" ;;
      /*:/out) out=${{arg%:/out}} ;;
      /*:/modules | /*:/modules:ro)
        modules=${{arg%:/modules*}}
        bash "$SIGN_KERNEL_SH" check-modules --kver {KVER} --dir "$modules" >> "$PODMAN_LOG" ;;
    esac
  done
  if [[ " $* " == *" prepare "* ]]; then
    printf 'MZ\\n' > "$out/vmlinuz"; echo {KVER} > "$out/kver"; echo sha512 > "$out/module-sig-hash"
  fi
fi
"""

CURL = """#!/usr/bin/env bash
set -euo pipefail
echo "curl $*" >> "$PODMAN_LOG"
while [[ $1 != -o ]]; do shift; done
printf 'cosign v3 stand-in\\n' > "$2"
"""

# system/kernel-artifacts.sh: `resolve` writes RESOLVED (the registry's answer) into
# $KERNEL_ARTIFACTS_DIR and records which cosign it was given; `get` reads that file.
ARTIFACTS = f"""#!/usr/bin/env bash
set -euo pipefail
file=${{KERNEL_ARTIFACTS_DIR:-$PWD/kernel-artifacts}}/kernel-artifacts.env
case $1 in
  registry) echo {REG} ;;
  resolve)
    echo "resolve ${{*:2}} cosign=$(command -v cosign)" >> "$PODMAN_LOG"
    mkdir -p "${{file%/*}}"; printf '%b' "$RESOLVED" > "$file" ;;
  get) value=$(sed -n "s/^$2=//p" "$file"); [[ -n $value ]]; echo "$value" ;;
  *) exit 2 ;;
esac
"""


class SignerRun(unittest.TestCase):
    def setUp(self):
        self.root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        for path in ("forge/specs/azoth/signer/run.sh", "forge/scripts/retry.sh"):
            (self.root / path).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(REPO / path, self.root / path)
        self.write("system/kernel-artifacts.sh", ARTIFACTS)
        self.write(
            "forge/specs/azoth/signer/cosign.pin",
            f"COSIGN_VERSION=v3.1.3\nCOSIGN_SHA256={hashlib.sha256(COSIGN).hexdigest()}\n",
        )
        # A poisoned file where the artifacts job's download would put it: never read.
        self.write(
            "kernel-artifacts/kernel-artifacts.env",
            f"state=modules-missing\nregistry=evil.example\nkernel_digest={EVIL}\n",
        )
        bin_dir = self.root / "bin"
        # modinfo of the signer image: every module stand-in is of KVER.
        modinfo = f'#!/usr/bin/env bash\necho "{KVER} SMP preempt mod_unload"\n'
        for name, body in (("podman", PODMAN), ("curl", CURL), ("modinfo", modinfo)):
            self.write(f"bin/{name}", body).chmod(0o755)
        self.log = self.root / "podman.log"
        self.env = {
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "HOME": str(self.root),
            "PODMAN_LOG": str(self.log),
            "RETRY_ATTEMPTS": "1",
            "TMPDIR": str(self.root),
            "SIGN_KERNEL_SH": str(REPO / "forge/specs/azoth/sign-kernel.sh"),
            "RESOLVED": f"state=modules-missing\\nregistry={REG}\\nkernel_digest={KERNEL}\\ndevel_digest={DEVEL}\\n",
        }

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
        return target

    def digest(self, value=DIGEST):
        self.write("forge/specs/azoth/signer/image.digest", value + "\n")

    def modules(self):
        for branch in ("open", "legacy"):
            self.write(f"out/{branch}/lib/modules/{KVER}/extra/nvidia/nvidia.ko", "ko")

    def run_script(self, stage, **env):
        return subprocess.run(
            ["bash", "forge/specs/azoth/signer/run.sh", stage],
            cwd=self.root,
            capture_output=True,
            text=True,
            env={**self.env, **env},
        )

    def calls(self):
        return self.log.read_text().splitlines() if self.log.exists() else []

    def runs(self):
        return [c for c in self.calls() if c.startswith("podman run ")]

    def assert_confined(self, run):
        """Offline, by digest, the baked script, and no writable mount but the named outputs."""
        self.assertIn(" --network=none ", run)
        self.assertIn(" --pull=never ", run)
        self.assertIn(
            f" {REG}/azoth-signer@{DIGEST} bash /usr/local/bin/sign-kernel.sh ", run
        )
        for mount in re.findall(r"-v (\S+)", run):
            source, target, *mode = mount.split(":")
            self.assertNotEqual(
                pathlib.Path(source), self.root, "the checkout is never mounted"
            )
            if mode != ["ro"]:
                self.assertIn(target, ("/out", "/modules"), mount)
                self.assertIn(
                    source.removeprefix(f"{self.root}/"),
                    ("kernel-unsigned", "kernel-signed", "out", "mok"),
                    mount,
                )

    def test_without_a_committed_digest_nothing_runs(self):
        r = self.run_script("verify")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("signer/image.digest is missing", r.stderr)
        self.assertEqual(self.calls(), [])

    def test_a_malformed_digest_fails(self):
        self.digest("latest")
        r = self.run_script("verify")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("not a sha256 digest", r.stderr)
        self.assertEqual(self.calls(), [])

    def test_inputs_derives_the_kernel_from_the_registry_and_checks_the_modules(self):
        self.digest()
        self.modules()
        r = self.run_script("inputs", KERNEL_DIGEST=KERNEL)
        self.assertEqual(r.returncode, 0, r.stderr)
        calls = self.calls()
        self.assertIn(
            "https://github.com/sigstore/cosign/releases/download/v3.1.3/cosign-linux-amd64",
            calls[1],
        )
        resolve = next(c for c in calls if c.startswith("resolve "))
        self.assertIn(f"--expect-kernel-digest {KERNEL} ", resolve)
        self.assertRegex(resolve, r"cosign=\S+/bin/cosign$")
        self.assertNotIn(
            str(self.root / "bin"), resolve, "the fetched cosign, not one from PATH"
        )
        self.assertIn(f"podman create {REG}/azoth@{KERNEL} /none", calls)
        self.assertIn(f"podman create {REG}/azoth-devel@{DEVEL} /none", calls)
        runs = self.runs()
        self.assertEqual(len(runs), 2)
        for run in runs:
            self.assert_confined(run)
        self.assertIn(
            " prepare --kernel /in/kernel --devel /in/devel --out /out", runs[0]
        )
        self.assertIn(f"-v {self.root}/out:/modules:ro ", runs[1])
        self.assertIn(f" check-modules --kver {KVER} --dir /modules", runs[1])
        self.assertEqual((self.root / "kernel-unsigned/kver").read_text(), KVER + "\n")

    def test_a_poisoned_digest_fails_before_any_image_is_used(self):
        """KERNEL_DIGEST and the downloaded file both name EVIL: the registry, resolved and
        verified here, does not, so nothing is extracted and the sign step finds no input."""
        self.digest()
        self.modules()
        r = self.run_script("inputs", KERNEL_DIGEST=EVIL)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn(f"the artifacts job reported {EVIL}", r.stderr)
        self.assertEqual(self.runs(), [])
        self.assertFalse(any("create" in c for c in self.calls()))
        self.assertFalse((self.root / "kernel-unsigned").exists())
        r = self.run_script(
            "sign",
            MODULE_SIGNING_KEY="module secret",
            SECUREBOOT_SIGNING_KEY="secure boot secret",
        )
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("run.sh inputs derives it", r.stderr)
        self.assertFalse(any(c.startswith("key readable") for c in self.calls()))

    def test_prepare_signs_a_copy_the_allow_list_admits_and_ships_its_certificate(self):
        """The certificate of the test MOK stays outside the tree the signer checks and signs,
        and joins mok/ only afterwards, for the boot job."""
        profile = "forge/specs/azoth/keys/profiles/secureboot.cnf"
        (self.root / profile).parent.mkdir(parents=True)
        shutil.copy(REPO / profile, self.root / profile)
        self.digest()
        self.modules()
        self.write("out/open-rm.log", "log")
        r = self.run_script("prepare", KERNEL_DIGEST=KERNEL)
        self.assertEqual(r.returncode, 0, r.stderr)
        runs = self.runs()
        self.assertEqual(len(runs), 3)
        for run in runs:
            self.assert_confined(run)
        self.assertIn(f"-v {self.root}/mok:/modules ", runs[2])
        (cert,) = re.findall(r"-v (\S+):/run/certs/test-mok.pem:ro ", runs[2])
        self.assertFalse(
            pathlib.Path(cert).is_relative_to(self.root / "mok"), "outside the checked tree"
        )
        self.assertEqual(
            sum("nothing else" in c for c in self.calls()), 2, "out/ and mok/ both admitted"
        )
        self.assertIn("BEGIN CERTIFICATE", (self.root / "mok/test-mok.pem").read_text())

    def test_inputs_refuses_a_kernel_the_registry_does_not_verify(self):
        self.digest()
        r = self.run_script(
            "inputs",
            KERNEL_DIGEST=KERNEL,
            RESOLVED=f"state=kernel-missing\\nregistry={REG}\\n",
        )
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("nothing to sign", r.stderr)
        self.assertEqual(self.runs(), [])

    def test_inputs_refuses_a_cosign_of_another_hash(self):
        self.digest()
        self.write(
            "forge/specs/azoth/signer/cosign.pin",
            f"COSIGN_VERSION=v3.1.3\nCOSIGN_SHA256={'0' * 64}\n",
        )
        r = self.run_script("inputs", KERNEL_DIGEST=KERNEL)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("does not have the sha256", r.stderr)
        self.assertFalse(any(c.startswith("resolve ") for c in self.calls()))

    def test_inputs_needs_the_digest_and_never_reuses_a_downloaded_kernel(self):
        self.digest()
        r = self.run_script("inputs")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("KERNEL_DIGEST is ''", r.stderr)
        self.write("kernel-unsigned/vmlinuz", "MZ")
        r = self.run_script("inputs", KERNEL_DIGEST=KERNEL)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("never downloaded", r.stderr)
        self.assertEqual(self.runs(), [])

    def test_sign_without_both_keys_runs_nothing(self):
        self.digest()
        r = self.run_script("sign", MODULE_SIGNING_KEY="module secret")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("SECUREBOOT_SIGNING_KEY", r.stderr)
        self.assertEqual(self.calls(), [])

    def test_sign_runs_the_pinned_signer_offline_and_drops_the_keys(self):
        self.digest()
        self.modules()
        for name, text in (
            ("vmlinuz", "MZ"),
            ("kver", KVER),
            ("module-sig-hash", "sha512"),
        ):
            self.write(f"kernel-unsigned/{name}", text + "\n")
        r = self.run_script(
            "sign",
            MODULE_SIGNING_KEY="module secret",
            SECUREBOOT_SIGNING_KEY="secure boot secret",
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertNotIn("secret", r.stdout + r.stderr)
        calls = self.calls()
        self.assertEqual(calls[0], f"podman pull {REG}/azoth-signer@{DIGEST}")
        self.assertFalse(
            any(c.startswith("resolve ") for c in calls),
            "the sign step resolves nothing",
        )
        runs = self.runs()
        self.assertEqual(len(runs), 2)
        for run in runs:
            self.assert_confined(run)
        self.assertIn(
            f" modules --key /run/keys/module --cert /run/certs/module.pem --hash /in/module-sig-hash --kver {KVER} ",
            runs[0],
        )
        self.assertIn(" vmlinuz --key /run/keys/secureboot ", runs[1])
        keys = [c.split()[-1] for c in calls if c.startswith("key readable ")]
        self.assertEqual(len(keys), 2)
        for key in keys:
            self.assertFalse(pathlib.Path(key).exists(), f"{key} survived the step")

    def test_verify_compares_with_the_kernel_the_publish_job_resolved(self):
        self.digest()
        self.write(
            "kernel-artifacts/kernel-artifacts.env",
            f"registry={REG}\nkernel_digest={KERNEL}\n",
        )
        r = self.run_script("verify")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(f"podman create {REG}/azoth@{KERNEL} /none", self.calls())
        (run,) = self.runs()
        self.assert_confined(run)
        self.assertIn(f"-v {self.root}/kernel-signed:/signed:ro ", run)
        self.assertTrue(
            run.endswith(
                " verify --cert /run/certs/secureboot.pem --dir /signed --kernel /in/kernel"
            ),
            run,
        )


class SignerImageInputs(unittest.TestCase):
    """The tag publish.sh computes covers every file the image is made of, and a change to any
    of them starts .github/workflows/azoth-signer.yml."""

    def test_the_tag_hashes_every_input_and_the_workflow_watches_them(self):
        azoth = REPO / "forge/specs/azoth"
        (line,) = [l for l in (azoth / "signer/publish.sh").read_text().splitlines() if l.startswith("INPUTS=(")]
        inputs = set(line[len("INPUTS=(") : -1].split())
        copied = set(re.findall(r"^COPY (\S+) ", (azoth / "signer/Containerfile").read_text(), re.M))
        self.assertEqual(copied, {"lock.sh", "signer/toolchain.lock", "sign-kernel.sh"})
        self.assertEqual(inputs, copied | {"signer/Containerfile"})
        workflow = (REPO / ".github/workflows/azoth-signer.yml").read_text()
        for path in inputs:
            self.assertIn(f"      - forge/specs/azoth/{path}\n", workflow)


if __name__ == "__main__":
    unittest.main()
