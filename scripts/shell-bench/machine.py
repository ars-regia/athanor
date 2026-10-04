"""The reference machine of the shell standard over SSH (doc_shell_standard.md, ST9).

Commands run through the user's SSH configuration (a host alias, key only). Helpers are
streamed to the machine's own python3 on standard input, as scripts/devvm does in the VM,
so nothing is installed there.
"""

import json
import pathlib
import shlex
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
UNITS = ("athanor-bar", "athanor-dock", "athanor-shelld")


class Machine:
    def __init__(self, host, display="wayland-1"):
        self.host = host
        self.display = display

    def run(self, command, *, stdin=None, check=True, timeout=600):
        result = subprocess.run(
            ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", self.host, command],
            input=stdin,
            capture_output=True,
            timeout=timeout,
            check=False,
        )
        if check and result.returncode != 0:
            raise RuntimeError(
                f"{command!r} failed on {self.host} with {result.returncode}: "
                f"{result.stderr.decode(errors='replace').strip()}"
            )
        return result.stdout

    def session(self, command, **kwargs):
        """Runs COMMAND as the session user, with the session's compositor and bus."""
        prefix = (
            "export XDG_RUNTIME_DIR=/run/user/$(id -u) "
            f"WAYLAND_DISPLAY={shlex.quote(self.display)} "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus; "
        )
        return self.run(prefix + command, **kwargs)

    def helper(self, path, *args, root=False, session=False, **kwargs):
        command = ("sudo -n " if root else "") + "python3 - " + " ".join(
            shlex.quote(str(arg)) for arg in args
        )
        runner = self.session if session else self.run
        return runner(command, stdin=pathlib.Path(path).read_bytes(), **kwargs)

    def wall(self):
        return int(self.run("date +%s"))

    def journal(self, since, units=UNITS):
        flags = " ".join(f"-u {unit}" for unit in units)
        out = self.session(f"journalctl --user {flags} --since @{since} -o json --no-pager")
        return [json.loads(line) for line in out.splitlines() if line.strip()]

    def screenshot(self):
        return self.session("grim -t ppm -")

    def systemctl(self, *args):
        return self.session("systemctl --user " + " ".join(shlex.quote(a) for a in args))

    def main_pid(self, unit):
        return int(self.systemctl("show", "-p", "MainPID", "--value", unit))
