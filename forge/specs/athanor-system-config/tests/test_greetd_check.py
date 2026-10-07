"""The greenboot check that greetd starts and stays up
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

systemctl is a stand-in that answers each `show` with the next line of a script of states;
sleep is a stand-in that returns at once.
"""

import subprocess
import tempfile
import unittest
from pathlib import Path

CHECK = Path(__file__).resolve().parents[1] / "SOURCES/etc/greenboot/check/required.d/10-greetd-running.sh"

SYSTEMCTL = """#!/bin/bash
n=$(cat "$WORK/calls" 2>/dev/null || echo 0)
echo $((n + 1)) > "$WORK/calls"
line=$(sed -n "$((n + 1))p" "$WORK/states")
[[ -n $line ]] || line=$(tail -n 1 "$WORK/states")
read -r state run <<< "$line"
printf 'ActiveState=%s\nInvocationID=%s\n' "$state" "$run"
"""


def check(states):
    """Runs the check against greetd going through `states`, ("state", "run") pairs; the
    last one repeats. Returns the exit code and the number of times greetd was looked at."""
    work = Path(tempfile.mkdtemp())
    (work / "states").write_text("".join(f"{state} {run}\n" for state, run in states))
    stubs = work / "bin"
    stubs.mkdir()
    for name, body in (("systemctl", SYSTEMCTL), ("sleep", "#!/bin/sh\n")):
        (stubs / name).write_text(body)
        (stubs / name).chmod(0o755)
    result = subprocess.run(
        ["/bin/bash", str(CHECK)],
        env={"PATH": f"{stubs}:/usr/bin:/bin", "WORK": str(work)},
        capture_output=True,
        text=True,
    )
    return result.returncode, int((work / "calls").read_text())


class GreetdCheck(unittest.TestCase):
    def test_a_slow_start_that_then_stays_up_passes(self):
        code, calls = check([("activating", "a")] * 60 + [("active", "a")])
        self.assertEqual((code, calls), (0, 71))

    def test_greetd_given_up_on_fails_at_once(self):
        self.assertEqual(check([("activating", "a"), ("failed", "a")]), (1, 2))

    def test_a_crash_loop_never_passes(self):
        loop = []
        for run in "abcdefghijklmnopqrstuvwxyz":
            loop += [("active", run)] * 5 + [("activating", run)]
        code, calls = check(loop)
        self.assertEqual((code, calls), (1, 120))

    def test_greetd_never_starting_fails_after_two_minutes(self):
        self.assertEqual(check([("inactive", "")]), (1, 120))


if __name__ == "__main__":
    unittest.main()
