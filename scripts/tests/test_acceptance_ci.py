"""Unit tests of the CI mode of scripts/devvm/acceptance/run.sh (python3 -B -m unittest
discover -s scripts/tests -t scripts/tests -p test_acceptance_ci.py).

`run.sh --ci` runs the upgrade and rollback stages of doc_update_delivery.md UD18 on the
VM of forge/test/iso/run_iso_test.sh. Only what it decides before reaching the guest is
tested here: the guest itself needs QEMU, Secure Boot firmware and a candidate image.
systemctl, gh, ssh and df are stubs on PATH; HOME is a temporary directory.
"""

import json
import os
import pathlib
import re
import subprocess
import tempfile
import unittest

RUN = pathlib.Path(__file__).resolve().parents[1] / "devvm" / "acceptance" / "run.sh"
CHECK_RS = (
    pathlib.Path(__file__).resolve().parents[2]
    / "forge/specs/athanor-update/athanor-update-1.0.0/src/check.rs"
)
DIGEST = "sha256:" + "a" * 64
FROM = "sha256:" + "b" * 64
# What athanor-update's check logs (forge/specs/athanor-update/.../src/check.rs).
FEATURE_LINE = "the release attestation does not verify as the security class; the candidate is of the feature class"
SECURITY_LINE = "security update unlocked for the next shutdown"
# A guest that answers from files: the client's state, the check's journal, the boot and its
# digest. A reboot boots the staged digest under a new boot id.
GUEST = """g=@GUEST@
case "$*" in
  *state.json*) cat "$g/state.json" ;;
  *journalctl*) cat "$g/journal" ;;
  *"bootc status"*) printf '{"status":{"booted":{"image":{"imageDigest":"%s","image":{"image":"ghcr.io/owner/athanor-system:stable","signature":"containerPolicy"}}}}}' "$(cat "$g/booted")" ;;
  *boot_id*) cat "$g/boot" ;;
  *"date +%s"*) date +%s ;;
  *ActiveState*) echo inactive ;;
  *loginctl*) cat "$g/sessions" ;;
  *"systemctl reboot"*) echo 2 > "$g/boot"; cp "$g/staged" "$g/booted" ;;
  *tee*) cat > /dev/null ;;
  *"grep -qaF"*) exit "$(cat "$g/reader")" ;;
esac
"""


class CiMode(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        self.bin = self.dir / "bin"
        self.bin.mkdir()
        self.asked = self.dir / "gh-asked"
        # The host's runner unit is active, and one of its runners is busy.
        self.stub("systemctl", "exit 0")
        self.stub("gh", f"touch {self.asked}; echo 1")
        # The guest never answers: nothing in these tests may need it.
        self.stub("ssh", "exit 255")
        self.stub("df", "echo Avail; echo 100")
        (self.dir / "home").mkdir()
        (self.dir / "out").mkdir()
        self.env = {
            key: value
            for key, value in os.environ.items()
            if key not in ("CI", "GITHUB_ACTIONS")
        }
        self.env.update(
            PATH=f"{self.bin}:{os.environ['PATH']}",
            HOME=str(self.dir / "home"),
        )
        self.ci = {
            "CI": "true",
            "ATHANOR_UPDATE_TO": DIGEST,
            "ATHANOR_UPDATE_REPOSITORY": "ghcr.io/owner/athanor-system",
            "ATHANOR_UPDATE_MIRROR": "10.0.2.2:5000/owner/athanor-system",
            "ACC_CI_OUT": str(self.dir / "out"),
            "SSH_PORT": "2222",
            "ACC_SSH_KEY": str(self.dir / "id"),
            "ATHANOR_UPDATE_CLASS": "feature",
        }

    def tearDown(self):
        self.tmp.cleanup()

    def stub(self, name, body):
        path = self.bin / name
        path.write_text(f"#!/bin/sh\n{body}\n")
        path.chmod(0o755)

    def run_sh(self, *args, env=None, timeout=60):
        return subprocess.run(
            ["bash", str(RUN), *args],
            capture_output=True,
            text=True,
            env={**self.env, **(env or {})},
            timeout=timeout,
            check=False,
        )

    def test_ci_mode_refuses_outside_ci(self):
        result = self.run_sh(
            "--ci", env={k: v for k, v in self.ci.items() if k != "CI"}
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("CI", result.stderr)

    def test_ci_mode_names_a_missing_variable(self):
        for missing in self.ci:
            if missing == "CI":
                continue
            env = {k: v for k, v in self.ci.items() if k != missing}
            result = self.run_sh("--ci", env=env)
            self.assertNotEqual(result.returncode, 0, missing)
            self.assertIn(missing, result.stderr, missing)

    def test_ci_mode_rejects_a_bad_digest(self):
        result = self.run_sh("--ci", env={**self.ci, "ATHANOR_UPDATE_TO": "stable"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ATHANOR_UPDATE_TO", result.stderr)

    def test_ci_mode_rejects_a_dev_stage(self):
        result = self.run_sh("--ci", "install", env=self.ci)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown stage", result.stderr)

    def test_ci_mode_does_not_ask_github(self):
        # An answering guest with nothing to say: the seed stage stops at the digest.
        self.stub("ssh", "exit 0")
        result = self.run_sh("--ci", env=self.ci)
        self.assertFalse(
            self.asked.exists(), "run.sh --ci asked GitHub about the runners"
        )
        self.assertIn("== seed", result.stdout, result.stderr)
        self.assertNotEqual(result.returncode, 0)

    def guest(self, update, journal, serial_session=True, reader=0):
        """A guest whose check settled on UPDATE with the candidate downloaded and logged
        JOURNAL. With SERIAL_SESSION a login on ttyS0 stops the stage before Apply(). READER is
        the exit status of the grep for the attestation reader: 1 for a client that predates
        it (#784), 2 for a client that cannot be read."""
        g = self.dir / "guest"
        g.mkdir(exist_ok=True)
        state = {
            "update": update,
            "downloaded": {"digest": DIGEST},
            "last_error": "none",
            "verified": {"reason": "signature"},
        }
        (g / "state.json").write_text(json.dumps(state))
        (g / "journal").write_text(journal + "\n")
        (g / "booted").write_text(FROM)
        (g / "staged").write_text(DIGEST)
        (g / "boot").write_text("1")
        sessions = [{"session": "3", "tty": "ttyS0"}] if serial_session else []
        (g / "sessions").write_text(json.dumps(sessions))
        (g / "reader").write_text(f"{reader}\n")
        self.stub("ssh", GUEST.replace("@GUEST@", str(g)))

    def test_upgrade_reads_the_class_the_runner_verified(self):
        self.guest("downloaded", FEATURE_LINE)
        result = self.run_sh("--ci", "upgrade", env=self.ci)
        self.assertIn(
            "PASS  UD18 3: the guest reads the candidate as the feature class",
            result.stdout,
            result.stderr,
        )
        self.assertIn("ttyS0", result.stderr)

    def test_upgrade_refuses_a_class_other_than_the_verified_one(self):
        self.guest("downloaded", FEATURE_LINE)
        result = self.run_sh(
            "--ci", "upgrade", env={**self.ci, "ATHANOR_UPDATE_CLASS": "security"}
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "the guest settled the candidate as the feature class", result.stderr
        )
        self.assertNotIn("PASS  UD18 4", result.stdout)

    def test_upgrade_refuses_a_journal_with_no_class(self):
        self.guest("downloaded", "nothing about the class")
        result = self.run_sh("--ci", "upgrade", env=self.ci)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("names no class", result.stderr)

    def test_upgrade_boots_a_security_candidate_unlocked_for_the_shutdown(self):
        # UT13: with automatic updates on, the default, the check unlocks a security candidate
        # itself, and Apply() refuses one that is unlocked already: the shutdown applies it.
        self.guest("will-apply-at-next-shutdown", SECURITY_LINE, serial_session=False)
        security = {**self.ci, "ATHANOR_UPDATE_CLASS": "security"}
        result = self.run_sh("--ci", "upgrade", env=security)
        self.assertIn(
            "PASS  UD18 4: the security candidate boots at the next shutdown",
            result.stdout,
            result.stderr,
        )
        # The stage passed: the next one starts, and stops on what this guest cannot answer.
        self.assertIn("== check", result.stdout)

    def test_upgrade_holds_each_class_to_its_state(self):
        for cls, line, update in (
            ("security", SECURITY_LINE, "downloaded"),
            ("feature", FEATURE_LINE, "will-apply-at-next-shutdown"),
        ):
            with self.subTest(cls):
                self.guest(update, line)
                result = self.run_sh(
                    "--ci", "upgrade", env={**self.ci, "ATHANOR_UPDATE_CLASS": cls}
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    f"the check did not settle the {cls} candidate", result.stderr
                )

    def test_upgrade_goes_on_through_apply_with_a_client_that_predates_the_reader(self):
        self.guest("downloaded", "", reader=1)
        result = self.run_sh("--ci", "upgrade", env=self.ci)
        self.assertIn("NOTE  UD18 3: the booted client predates", result.stdout, result.stderr)
        self.assertNotIn("PASS  UD18 3", result.stdout)
        self.assertIn("PASS  UD18 4", result.stdout, result.stderr)
        # The stage reached Apply(), which the login on ttyS0 stops.
        self.assertIn("ttyS0", result.stderr)

    def test_upgrade_refuses_a_security_candidate_with_a_client_that_predates_the_reader(self):
        self.guest("downloaded", "", reader=1)
        security = {**self.ci, "ATHANOR_UPDATE_CLASS": "security"}
        result = self.run_sh("--ci", "upgrade", env=security)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("would go untested", result.stderr)
        self.assertNotIn("PASS  UD18 4", result.stdout)

    def test_upgrade_fails_when_the_booted_client_cannot_be_read(self):
        self.guest("downloaded", FEATURE_LINE, reader=2)
        result = self.run_sh("--ci", "upgrade", env=self.ci)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("the booted client could not be read: grep exited 2", result.stderr)
        self.assertNotIn("NOTE  UD18 3", result.stdout)

    def test_check_requires_the_reader_in_the_candidates_client(self):
        # The candidate is the :stable the next run upgrades from (#784).
        for reader, expected in ((0, "PASS  UD18 5: the candidate's client reads release attestations"),
                                 (1, "FAIL  UD18 5: the candidate's client has no attestation reader")):
            with self.subTest(reader=reader):
                self.guest("none", "", reader=reader)
                result = self.run_sh("--ci", "check", env=self.ci)
                self.assertIn(expected, result.stdout + result.stderr)

    def test_the_reader_is_found_by_a_message_of_the_client(self):
        marker = re.search(r'^ATTESTATION_READER="([^"]+)"$', RUN.read_text(), re.MULTILINE)
        self.assertIsNotNone(marker, "run.sh no longer looks for the attestation reader")
        # A tracing message of candidate_class: rewording it must reword the marker too.
        self.assertRegex(
            CHECK_RS.read_text(),
            r"tracing::warn!\([^;]*\"[^\"]*" + re.escape(marker.group(1)),
        )

    def test_ci_mode_rejects_an_unknown_class(self):
        result = self.run_sh("--ci", env={**self.ci, "ATHANOR_UPDATE_CLASS": "urgent"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ATHANOR_UPDATE_CLASS", result.stderr)

    def test_dev_mode_refuses_beside_a_busy_runner(self):
        result = self.run_sh()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("busy", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
