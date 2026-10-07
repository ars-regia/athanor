"""athanor-uki-enroll binds a LUKS2 volume to PCR 7 and the pcrlock policy
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

systemd-cryptenroll is a stand-in that records each call; cryptsetup is a stand-in that
prints the LUKS2 header JSON the test gives it, or fails when there is none. The script
runs as a copy whose policy path points into the test's directory.
"""

import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "SOURCES/usr/bin/athanor-uki-enroll"
POLICY = "/var/lib/systemd/pcrlock.json"

CRYPTENROLL = """#!/bin/bash
echo "$*" >> "$WORK/calls"
[[ ! -e $WORK/fail-recovery || $1 != --recovery-key ]]
"""

CRYPTSETUP = """#!/bin/bash
[[ $1 == luksDump && $2 == --dump-json-metadata ]] || exit 3
cat "$WORK/header"
"""

WITH_RECOVERY = '{"keyslots":{},"tokens":{\n  "0":{\n    "type":"systemd-recovery",\n    "keyslots":["1"]\n  }\n}}'
WITHOUT_RECOVERY = '{"keyslots":{},"tokens":{\n  "0":{\n    "type":"systemd-tpm2",\n    "keyslots":["1"]\n  }\n}}'

TPM2_CALL = "--tpm2-device=auto --tpm2-pcrs=7 --tpm2-pcrlock={policy} --wipe-slot=tpm2 /dev/vda3"


def enroll(header, args=("/dev/vda3",), policy=True, fail_recovery=False):
    """Runs the script with `header` as the volume's LUKS2 JSON (None: not LUKS2).
    Returns the exit code, the systemd-cryptenroll calls and the policy path."""
    work = Path(tempfile.mkdtemp())
    stubs = work / "bin"
    stubs.mkdir()
    for name, body in (
        ("systemd-cryptenroll", CRYPTENROLL),
        ("cryptsetup", CRYPTSETUP),
    ):
        (stubs / name).write_text(body)
        (stubs / name).chmod(0o755)
    if header is not None:
        (work / "header").write_text(header)
    if fail_recovery:
        (work / "fail-recovery").touch()
    policy_path = work / "pcrlock.json"
    if policy:
        policy_path.write_text("{}")
    script = work / "athanor-uki-enroll"
    text = SCRIPT.read_text()
    assert text.count(f"policy={POLICY}\n") == 1
    script.write_text(text.replace(f"policy={POLICY}\n", f"policy={policy_path}\n"))
    result = subprocess.run(
        ["/bin/bash", str(script), *args],
        env={"PATH": f"{stubs}:/usr/bin:/bin", "WORK": str(work)},
        capture_output=True,
        text=True,
    )
    calls = (
        (work / "calls").read_text().splitlines() if (work / "calls").exists() else []
    )
    return result.returncode, calls, str(policy_path)


class UkiEnroll(unittest.TestCase):
    def test_binds_pcr_7_and_the_pcrlock_policy_only(self):
        code, calls, policy = enroll(WITH_RECOVERY)
        self.assertEqual(code, 0)
        self.assertEqual(calls, [TPM2_CALL.format(policy=policy)])

    def test_enrols_a_recovery_key_before_the_tpm_when_there_is_none(self):
        code, calls, policy = enroll(WITHOUT_RECOVERY)
        self.assertEqual(code, 0)
        self.assertEqual(
            calls, ["--recovery-key /dev/vda3", TPM2_CALL.format(policy=policy)]
        )

    def test_a_failed_recovery_key_enrolment_stops_before_the_tpm(self):
        code, calls, _ = enroll(WITHOUT_RECOVERY, fail_recovery=True)
        self.assertNotEqual(code, 0)
        self.assertEqual(calls, ["--recovery-key /dev/vda3"])

    def test_refuses_without_a_pcrlock_policy(self):
        self.assertEqual(enroll(WITH_RECOVERY, policy=False)[:2], (1, []))

    def test_refuses_a_volume_that_is_not_luks2(self):
        code, calls, _ = enroll(None)
        self.assertNotEqual(code, 0)
        self.assertEqual(calls, [])

    def test_needs_exactly_one_device(self):
        self.assertEqual(enroll(WITH_RECOVERY, args=())[:2], (2, []))
        self.assertEqual(enroll(WITH_RECOVERY, args=("/dev/a", "/dev/b"))[:2], (2, []))


if __name__ == "__main__":
    unittest.main()
