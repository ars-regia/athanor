"""The trust state bind of athanor-greeter-client
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

The script runs unchanged but for three paths and, where a case needs a directory that
counts as root's, the owner it expects: a test cannot create a directory owned by uid 0,
so the expected owner becomes the test's own uid. bwrap and xdg-dbus-proxy are stand-ins:
the proxy creates its socket and reports ready, bwrap records its arguments and the
directory its descriptor 5 refers to.
"""

import os
import shutil
import socket
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = (
    Path(__file__).resolve().parents[1] / "SOURCES/usr/libexec/athanor-greeter-client"
)

PROXY = """#!/bin/sh
python3 -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$3"
printf x >&3
"""

BWRAP = """#!/bin/sh
printf '%s\\n' "$@" > "$WORK/args"
if [ -e /proc/self/fd/5 ]; then
    readlink /proc/self/fd/5 > "$WORK/fd5"
else
    : > "$WORK/fd5"
fi
"""


class GreeterClientTrustBind(unittest.TestCase):
    def setUp(self):
        self.work = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.work)
        self.trust = self.work / "athanor-update"

    def bind(self, owner_is_me=True):
        """Runs the client; returns the directory bound for the seal, or None."""
        # A short directory of its own per run: socket paths have a length limit.
        work = Path(tempfile.mkdtemp(dir="/tmp"))
        self.addCleanup(shutil.rmtree, work)
        script = SCRIPT.read_text()
        for old, new in (
            ("/run/athanor-update", str(self.trust)),
            ("/run/dbus/system_bus_socket", str(work / "system_bus_socket")),
        ):
            self.assertIn(old, script)
            script = script.replace(old, new)
        if owner_is_me:
            old = '[ "$trust_owner" = 0 ]'
            self.assertEqual(script.count(old), 1)
            script = script.replace(old, f'[ "$trust_owner" = {os.getuid()} ]')
        (work / "client").write_text(script)
        stubs = work / "bin"
        stubs.mkdir()
        for name, body in (("xdg-dbus-proxy", PROXY), ("bwrap", BWRAP)):
            (stubs / name).write_text(body)
            (stubs / name).chmod(0o755)
        runtime = work / "runtime"
        runtime.mkdir()
        for path in (
            runtime / "wayland-1",
            work / "greetd.sock",
            work / "system_bus_socket",
        ):
            listener = socket.socket(socket.AF_UNIX)
            listener.bind(str(path))
            self.addCleanup(listener.close)
        result = subprocess.run(
            ["/bin/sh", str(work / "client")],
            env={
                "PATH": f"{stubs}:/usr/bin:/bin",
                "WORK": str(work),
                "XDG_RUNTIME_DIR": str(runtime),
                "WAYLAND_DISPLAY": "wayland-1",
                "GREETD_SOCK": str(work / "greetd.sock"),
                "HOME": str(work / "home"),
            },
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        args = (work / "args").read_text().splitlines()
        self.assertEqual(
            args[-1], "/usr/bin/athanor-greeter-ui", "the greeter did not start"
        )
        if "--ro-bind-fd" not in args:
            self.assertNotIn(str(self.trust), args)
            self.assertIn("the seal shows no trust state", result.stderr)
            self.assertEqual((work / "fd5").read_text(), "", "descriptor 5 left open")
            return None
        at = args.index("--ro-bind-fd")
        self.assertEqual(args[at + 1 : at + 3], ["5", str(self.trust)])
        return (work / "fd5").read_text().strip()

    def test_root_directory_nobody_else_writes_is_bound(self):
        self.trust.mkdir(mode=0o755)
        self.trust.chmod(0o755)
        self.assertEqual(self.bind(), str(self.trust))

    def test_group_or_world_writable_directory_is_not_bound(self):
        self.trust.mkdir()
        for mode in (0o775, 0o757, 0o1777):
            with self.subTest(mode=oct(mode)):
                self.trust.chmod(mode)
                self.assertIsNone(self.bind())

    @unittest.skipIf(os.getuid() == 0, "the directory would be root's")
    def test_directory_of_another_uid_is_not_bound(self):
        self.trust.mkdir(mode=0o755)
        self.trust.chmod(0o755)
        self.assertIsNone(self.bind(owner_is_me=False))

    def test_symlink_is_not_bound(self):
        target = self.work / "elsewhere"
        target.mkdir(mode=0o755)
        target.chmod(0o755)
        self.trust.symlink_to(target)
        self.assertIsNone(self.bind())

    def test_missing_directory_is_not_bound_and_the_greeter_starts(self):
        self.assertIsNone(self.bind())


if __name__ == "__main__":
    unittest.main()
