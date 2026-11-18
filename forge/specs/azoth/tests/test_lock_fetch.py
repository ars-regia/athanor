"""Unit test of lock.sh's fetch: a koji transfer that fails must stop the install and name
the file (python3 -B -m unittest discover -s forge/specs/azoth/tests -v)."""

import pathlib
import subprocess
import tempfile
import unittest

LOCK_SH = pathlib.Path(__file__).resolve().parents[1] / "lock.sh"

# A curl that records its arguments in $ARGV_FILE and writes every output of the config as a
# non-empty file, except the one ending in $MISSING (absent) or $EMPTY (empty), then exits with
# $CODE: parallel curl reports only the last transfer's status, so 0 can hide a failure.
FAKE_CURL = """#!/usr/bin/env bash
printf '%s\\n' "$*" > "$ARGV_FILE"
conf=
while (($#)); do
    [[ $1 == --config ]] && conf=$2
    shift
done
while read -r key _ value; do
    [[ $key == output ]] || continue
    out=${value//\\"/}
    if [[ $out == *"$MISSING" ]]; then
        continue
    elif [[ $out == *"$EMPTY" ]]; then
        : > "$out"
    else
        echo data > "$out"
    fi
done < "$conf"
exit "$CODE"
"""


class LockFetch(unittest.TestCase):
    def run_fetch(self, code=0, missing="-", empty="-"):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            (tmp / "bin").mkdir()
            curl = tmp / "bin" / "curl"
            curl.write_text(FAKE_CURL)
            curl.chmod(0o755)
            (tmp / "dl").mkdir()
            # The functions only: the script's own case would run a subcommand.
            script = (
                "set -euo pipefail; KOJI=https://koji.invalid; "
                f"eval \"$(sed -n '/^die()/,/^}}/p;/^splice()/,/^}}/p;/^fetch()/,/^}}/p' {LOCK_SH})\"; "
                f"fetch {tmp / 'dl'} KEY a.x86_64.rpm:a/1/1.fc43 dwz.x86_64.rpm:dwz/1/1.fc43"
            )
            run_env = {
                "PATH": f"{tmp / 'bin'}:/usr/bin:/bin",
                "CODE": str(code),
                "MISSING": missing,
                "EMPTY": empty,
                "ARGV_FILE": str(tmp / "argv"),
            }
            r = subprocess.run(
                ["bash", "-c", script], capture_output=True, text=True, env=run_env
            )
            r.argv = (tmp / "argv").read_text()
            return r

    def test_a_missing_file_with_exit_zero_is_named(self):
        r = self.run_fetch(missing="dwz.x86_64.rpm.unsigned")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("dwz.x86_64.rpm.unsigned", r.stderr)

    def test_a_failed_transfer_stops_the_fetch_with_its_status(self):
        # curl names the failure itself on stderr; the script must not go on to splice.
        r = self.run_fetch(code=35, missing="dwz.x86_64.rpm.sig")
        self.assertEqual(r.returncode, 35)

    def test_an_empty_file_is_named(self):
        r = self.run_fetch(empty="dwz.x86_64.rpm.sig")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("dwz.x86_64.rpm.sig", r.stderr)

    def test_curl_is_asked_to_retry_every_error_and_fail_early(self):
        r = self.run_fetch(missing="a.x86_64.rpm.sig")
        for flag in ("--retry-all-errors", "--fail-early", "--retry-delay"):
            self.assertIn(flag, r.argv)


if __name__ == "__main__":
    unittest.main()
