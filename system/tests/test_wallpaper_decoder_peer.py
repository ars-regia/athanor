"""The kernel premise of the decoder's admission by cgroup (doc_session_daemons.md SD17).

This test does not fail on the code of #1222: it documents the rule the fix relies on. The
regression check that fails on the old code is `scripts/verify.py peer-exe`.

athanor-wallpaper-decode@.service sets namespacing properties, which under the user manager put
it in a user namespace of its own. The kernel allows a ptrace-read (readlink /proc/<pid>/exe)
to a reader in the target's user namespace or with CAP_SYS_PTRACE in it, so a decoder in its own
cannot read the executable of a peer in a sibling's, while /proc/<pid>/cgroup stays readable. That is why the decoder admits its peer
by cgroup alone (athanor_unit::peer::admit_user_unit_cgroup). The test starts a sandboxed peer
and a reader with the shipped unit's own properties against a real user manager. It skips where
there is none, as in CI containers, and where the manager cannot give a unit a namespace, as on
Ubuntu, whose AppArmor restricts unprivileged user namespaces: the unit then exits with status
226 before it runs (python3 -B -m unittest discover -s system/tests -v).
"""

import pathlib
import subprocess
import unittest
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[2]
UNIT = ROOT / "forge/specs/athanor-wallpaper/athanor-wallpaper-1.0.0/data/athanor-wallpaper-decode@.service"
# The properties that give a unit namespaces; the rest of the unit does not change what is read.
NAMESPACING = {"ProtectHome", "ProtectSystem", "PrivateNetwork", "PrivateIPC", "TemporaryFileSystem", "InaccessiblePaths"}


def namespacing_properties():
    props, in_service = [], False
    for line in UNIT.read_text().splitlines():
        line = line.strip()
        if line.startswith("["):
            in_service = line == "[Service]"
        elif in_service and line.partition("=")[0] in NAMESPACING:
            props += ["-p", line]
    return props


def user_manager_runs():
    try:
        done = subprocess.run(["systemctl", "--user", "is-system-running"], capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return False
    return done.stdout.strip() in ("running", "degraded")


def namespaced_units_start():
    """Whether the user manager can start a unit with the shipped unit's namespacing properties
    at all: `true` under them, so a failure here is the host's, not the premise's."""
    try:
        done = subprocess.run(["systemd-run", "--user", "--pipe", "--wait", "--collect", *namespacing_properties(), "true"], capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        return False
    return done.returncode == 0


def run(*args, **kwargs):
    return subprocess.run(args, capture_output=True, text=True, timeout=60, **kwargs)


@unittest.skipUnless(user_manager_runs(), "no user manager: systemctl --user is-system-running fails")
@unittest.skipUnless(user_manager_runs() and namespaced_units_start(), "the user manager cannot start a unit with the decoder's namespacing properties here")
class WallpaperDecoderKernelPremiseTest(unittest.TestCase):
    def test_a_namespaced_reader_reads_the_peers_cgroup_and_not_its_exe(self):
        props = namespacing_properties()
        self.assertTrue({p.partition("=")[0] for p in props[1::2]} >= NAMESPACING, props)
        name = f"athanor-test-peer-{uuid.uuid4().hex[:8]}"
        started = run("systemd-run", "--user", f"--unit={name}", "-p", "PrivateIPC=yes", "sleep", "60")
        self.addCleanup(run, "systemctl", "--user", "stop", f"{name}.service")
        self.assertEqual(started.returncode, 0, started.stderr)
        pid = run("systemctl", "--user", "show", "-p", "MainPID", "--value", f"{name}.service").stdout.strip()
        self.assertTrue(pid.isdigit() and pid != "0", pid)
        script = 'cat "/proc/$1/cgroup" >/dev/null; echo "cgroup=$?"; readlink "/proc/$1/exe" >/dev/null; echo "exe=$?"'
        reader = run("systemd-run", "--user", "--pipe", "--wait", "--collect", *props, "sh", "-c", script, "sh", pid)
        self.assertEqual(reader.returncode, 0, reader.stderr)
        result = dict(line.split("=") for line in reader.stdout.split() if "=" in line)
        self.assertEqual(result.get("cgroup"), "0", reader.stdout + reader.stderr)
        self.assertNotEqual(result.get("exe"), "0", "the peer's exe was readable; SD17's premise no longer holds")


if __name__ == "__main__":
    unittest.main()
