"""athanor-uki-enroll binds a LUKS2 volume to a PIN and the value of PCR 7
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

systemd-cryptenroll is a stand-in that records each call; cryptsetup is a stand-in that
prints the LUKS2 header JSON the test gives it, or fails when there is none; mokutil is a
stand-in that prints the Secure Boot state the test gives it. rpm-ostree, bootc, dracut
and grubby record any call, which the script must not make.
"""

import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "SOURCES/usr/bin/athanor-uki-enroll"

CRYPTENROLL = """#!/bin/bash
echo "$*" >> "$WORK/calls"
[[ ! -e $WORK/fail-recovery || $1 != --recovery-key ]]
"""

MOKUTIL = """#!/bin/bash
[[ $1 == --sb-state && -e $WORK/sb-state ]] || exit 1
cat "$WORK/sb-state"
"""

# Commands that change how the system boots.
RECORD = """#!/bin/bash
echo "$(basename "$0") $*" >> "$WORK/calls"
"""

CRYPTSETUP = """#!/bin/bash
[[ $1 == luksDump && $2 == --dump-json-metadata ]] || exit 3
cat "$WORK/header"
"""

WITH_RECOVERY = '{"keyslots":{},"tokens":{\n  "0":{\n    "type":"systemd-recovery",\n    "keyslots":["1"]\n  }\n}}'
# systemd-cryptenroll --wipe-slot=recovery (or cryptsetup luksKillSlot) leaves the token
# behind with no keyslot; cryptsetup prints it this way.
ORPHAN_RECOVERY = '{"keyslots":{},"tokens":{\n    "0":{\n      "type":"systemd-recovery",\n      "keyslots":[]\n    }\n  }}'
WITHOUT_RECOVERY = '{"keyslots":{},"tokens":{\n  "0":{\n    "type":"systemd-tpm2",\n    "keyslots":["1"]\n  }\n}}'

# The empty --tpm2-pcrlock= turns off systemd-cryptenroll's own pick-up of pcrlock.json.
TPM2_CALL = (
    "--tpm2-device=auto --tpm2-with-pin=yes --tpm2-pcrs=7 --tpm2-pcrlock="
    " --wipe-slot=tpm2 /dev/vda3"
)

SECURE_BOOT_ON = "SecureBoot enabled\n"


def run(header, args=("/dev/vda3",), fail_recovery=False, sb_state=SECURE_BOOT_ON):
    """Runs the script with `header` as the volume's LUKS2 JSON (None: not LUKS2) and
    `sb_state` as mokutil's output (None: mokutil fails). Returns the finished process
    and the recorded calls."""
    work = Path(tempfile.mkdtemp())
    stubs = work / "bin"
    stubs.mkdir()
    for name, body in (
        ("systemd-cryptenroll", CRYPTENROLL),
        ("cryptsetup", CRYPTSETUP),
        ("mokutil", MOKUTIL),
        ("rpm-ostree", RECORD),
        ("bootc", RECORD),
        ("dracut", RECORD),
        ("grubby", RECORD),
    ):
        (stubs / name).write_text(body)
        (stubs / name).chmod(0o755)
    if header is not None:
        (work / "header").write_text(header)
    if fail_recovery:
        (work / "fail-recovery").touch()
    if sb_state is not None:
        (work / "sb-state").write_text(sb_state)
    result = subprocess.run(
        ["/bin/bash", str(SCRIPT), *args],
        env={"PATH": f"{stubs}:/usr/bin:/bin", "WORK": str(work)},
        capture_output=True,
        text=True,
    )
    calls = (
        (work / "calls").read_text().splitlines() if (work / "calls").exists() else []
    )
    return result, calls


def enroll(header, **kwargs):
    """Returns the exit code and the recorded calls."""
    result, calls = run(header, **kwargs)
    return result.returncode, calls


class UkiEnroll(unittest.TestCase):
    def test_binds_the_value_of_pcr_7_only(self):
        self.assertEqual(enroll(WITH_RECOVERY), (0, [TPM2_CALL]))

    def test_every_tpm2_keyslot_it_writes_needs_a_pin(self):
        tpm2 = [c for c in enroll(WITH_RECOVERY)[1] if "--tpm2-device" in c]
        self.assertEqual(len(tpm2), 1)
        self.assertIn("--tpm2-with-pin=yes", tpm2[0].split())

    def test_no_pcrlock_policy_and_no_pcrs_that_updates_change(self):
        tpm2 = [c for c in enroll(WITH_RECOVERY)[1] if "--tpm2-device" in c]
        self.assertEqual(len(tpm2), 1)
        options = dict(
            o.split("=", 1) for o in tpm2[0].split() if o.startswith("--") and "=" in o
        )
        self.assertEqual(options.get("--tpm2-pcrs"), "7")
        self.assertEqual(options.get("--tpm2-pcrlock"), "")
        self.assertNotIn("--tpm2-public-key", options)

    def test_enrols_a_recovery_key_before_the_tpm_when_there_is_none(self):
        self.assertEqual(
            enroll(WITHOUT_RECOVERY), (0, ["--recovery-key /dev/vda3", TPM2_CALL])
        )

    def test_a_recovery_token_without_a_keyslot_is_no_recovery_key(self):
        self.assertEqual(
            enroll(ORPHAN_RECOVERY), (0, ["--recovery-key /dev/vda3", TPM2_CALL])
        )

    def test_a_failed_recovery_key_enrolment_stops_before_the_tpm(self):
        code, calls = enroll(WITHOUT_RECOVERY, fail_recovery=True)
        self.assertNotEqual(code, 0)
        self.assertEqual(calls, ["--recovery-key /dev/vda3"])

    def test_refuses_a_volume_that_is_not_luks2(self):
        code, calls = enroll(None)
        self.assertNotEqual(code, 0)
        self.assertEqual(calls, [])

    def test_refuses_when_secure_boot_does_not_verify_the_boot_chain(self):
        for state in (
            "SecureBoot disabled\n",
            "SecureBoot disabled\nPlatform is in Setup Mode\n",
            "SecureBoot enabled\nSecureBoot validation is disabled in shim\n",
            None,
        ):
            with self.subTest(state=state):
                code, calls = enroll(WITHOUT_RECOVERY, sb_state=state)
                self.assertNotEqual(code, 0)
                self.assertEqual(calls, [])

    def test_leaves_the_boot_configuration_alone_and_says_what_boot_asks(self):
        # systemd-cryptsetup tries the volume's LUKS2 tokens before the passphrase, and
        # the generic initramfs carries the TPM2 token plugin: nothing to configure.
        result, calls = run(WITH_RECOVERY)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(calls, [TPM2_CALL])
        self.assertNotIn("Enrollment complete", result.stdout)
        self.assertIn("PIN", result.stdout.splitlines()[-1])

    def test_needs_exactly_one_device(self):
        self.assertEqual(enroll(WITH_RECOVERY, args=()), (2, []))
        self.assertEqual(enroll(WITH_RECOVERY, args=("/dev/a", "/dev/b")), (2, []))


if __name__ == "__main__":
    unittest.main()
