"""vm.sh end to end against fakes of curl (the GitHub API), QEMU (its QMP socket) and
systemd-inhibit: when an idle guest is recycled and when a guest is left alone."""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

RUNNER = Path(__file__).resolve().parent.parent
RUNNERS = "/repos/owner/repo/actions/runners/7"

# Answers GitHub the way vm.sh calls it: curl -o FILE -w '%{http_code}' -X METHOD ... URL.
# GETs of the registration answer the scenario's list in order, the last one repeated;
# after a DELETE answered 204 the registration is gone and every GET answers 404.
FAKE_CURL = """\
import json, os, sys
args = sys.argv[1:]
out = args[args.index("-o") + 1]
method = args[args.index("-X") + 1]
path = args[-1].removeprefix("https://api.github.com")
sys.stdin.read()
root = os.environ["FAKE_ROOT"]
with open(os.path.join(root, "scenario.json")) as f:
    scenario = json.load(f)
with open(os.path.join(root, "calls.log"), "a") as log:
    log.write(f"{method} {path}\\n")
if path.endswith("/generate-jitconfig"):
    status, body = 201, {"runner": {"id": 7}, "encoded_jit_config": "e30="}
elif method == "DELETE":
    status = scenario["delete"].pop(0) if len(scenario["delete"]) > 1 else scenario["delete"][0]
    body = {}
    if status == 204:
        scenario["deleted"] = True
elif scenario.get("deleted"):
    status, body = 404, {}
else:
    status, body = scenario["gets"].pop(0) if len(scenario["gets"]) > 1 else scenario["gets"][0]
with open(os.path.join(root, "scenario.json"), "w") as f:
    json.dump(scenario, f)
with open(out, "w") as f:
    json.dump(body, f)
print(status, end="")
"""

# Serves QMP on the first -qmp socket, logs each command and exits on system_powerdown.
FAKE_QEMU = """\
import json, os, socket, sys
args = sys.argv[1:]
path = args[args.index("-qmp") + 1].removeprefix("unix:").split(",")[0]
server = socket.socket(socket.AF_UNIX)
server.bind(path)
server.listen()
while True:
    conn, _ = server.accept()
    stream = conn.makefile("rw")
    stream.write(json.dumps({"QMP": {}}) + "\\n")
    stream.flush()
    for line in stream:
        command = json.loads(line)["execute"]
        if command != "qmp_capabilities":
            with open(os.path.join(os.environ["FAKE_ROOT"], "calls.log"), "a") as log:
                log.write(f"QMP {command}\\n")
        stream.write(json.dumps({"return": {}}) + "\\n")
        stream.flush()
        if command == "system_powerdown":
            sys.exit(0)
    conn.close()
"""

FAKE_INHIBIT = """\
#!/bin/bash
while [[ $1 == --* ]]; do shift; done
exec "$@"
"""


@unittest.skipUnless(shutil.which("jq"), "vm.sh needs jq")
class VmSh(unittest.TestCase):
    def run_vm(self, gets, delete=(204,)):
        """Runs vm.sh until it exits; returns its output and the calls GitHub and QMP saw."""
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        script = (RUNNER / "vm.sh").read_text()
        # Fast polls, and a guest idle from the first poll: the most eager recycling.
        for constant, value in (("POLL_SECONDS", "1"), ("IDLE_RECYCLE_SECONDS", "0")):
            script, n = re.subn(
                rf"^{constant}=\d+$", f"{constant}={value}", script, flags=re.M
            )
            self.assertEqual(n, 1, constant)
        runner = root / "runner"
        fake = root / "fake"
        for directory in ("runner", "fake", "state", "runtime", "logs", "credentials"):
            (root / directory).mkdir()
        (runner / "vm.sh").write_text(script)
        (runner / "balloon.py").write_text("")
        (runner / "runner.env").write_text(
            textwrap.dedent("""\
            REPOSITORY=owner/repo
            RUNNER_LABELS=self-hosted
            CACHE_DISK_SIZE=1M
            SCRATCH_DISK_SIZE=1M
            VM_CPUS=1
            VM_MEMORY=1G
            BALLOON_FLOOR=1G
            HOST_RESERVE=1G
        """)
        )
        for name, body in (("curl", FAKE_CURL), ("qemu-system-x86_64", FAKE_QEMU)):
            (fake / name).write_text(f"#!{sys.executable}\n{body}")
        (fake / "systemd-inhibit").write_text(FAKE_INHIBIT)
        for path in fake.iterdir():
            path.chmod(0o755)
        (root / "state" / "golden.qcow2").touch()
        (root / "credentials" / "github-token").write_text("token")
        (root / "scenario.json").write_text(
            json.dumps({"gets": gets, "delete": list(delete)})
        )
        env = dict(
            os.environ,
            PATH=f"{fake}:{os.environ['PATH']}",
            FAKE_ROOT=str(root),
            STATE_DIRECTORY=str(root / "state"),
            RUNTIME_DIRECTORY=str(root / "runtime"),
            LOGS_DIRECTORY=str(root / "logs"),
            CREDENTIALS_DIRECTORY=str(root / "credentials"),
        )
        result = subprocess.run(
            ["bash", str(runner / "vm.sh")],
            env=env,
            capture_output=True,
            text=True,
            timeout=60,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = (root / "calls.log").read_text().splitlines()
        return result.stdout, calls

    def test_an_idle_guest_is_deregistered_then_powered_off(self):
        stdout, calls = self.run_vm(gets=[[200, {"busy": False}]])
        self.assertIn("registration removed; powering the guest off", stdout)
        delete = calls.index(f"DELETE {RUNNERS}")
        self.assertLess(delete, calls.index("QMP system_powerdown"))
        self.assertEqual(calls.count(f"DELETE {RUNNERS}"), 1)

    def test_a_guest_that_took_a_job_is_never_recycled(self):
        # Busy, then idle again (the job is over, GitHub has not removed it yet), then gone.
        stdout, calls = self.run_vm(
            gets=[[200, {"busy": True}], [200, {"busy": False}], [404, {}]]
        )
        self.assertNotIn(f"DELETE {RUNNERS}", calls)
        self.assertIn("job finished, registration removed by GitHub", stdout)
        self.assertEqual(
            calls.count(f"GET {RUNNERS}"), 4
        )  # three polls, then cleanup's
        self.assertEqual(calls[-2:], ["QMP system_powerdown", f"GET {RUNNERS}"])

    def test_a_job_assigned_while_recycling_keeps_its_guest(self):
        # GitHub refuses to delete a runner that has just started a job: the guest stays up
        # until the job's own end.
        stdout, calls = self.run_vm(
            gets=[[200, {"busy": False}], [200, {"busy": True}], [404, {}]],
            delete=[422],
        )
        self.assertIn(
            "idle recycle deferred, registration not removed: HTTP 422", stdout
        )
        self.assertEqual(calls.count(f"DELETE {RUNNERS}"), 1)
        self.assertIn("job finished, registration removed by GitHub", stdout)
        self.assertEqual(calls.count("QMP system_powerdown"), 1)
        self.assertLess(
            calls.index(f"DELETE {RUNNERS}"), calls.index("QMP system_powerdown")
        )


if __name__ == "__main__":
    unittest.main()
