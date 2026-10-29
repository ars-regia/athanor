"""The supervisor of the locker and the idle daemon, and athanor-session's cleanup of what a
session on the own compositor left (doc_session.md, SN15)
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

systemctl, systemd-run, systemd-cat and sleep are stand-ins that record their arguments;
the scripts' absolute paths are rewritten to copies in a scratch directory.
"""

import os
import shutil
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

SOURCES = Path(__file__).resolve().parents[1] / "SOURCES"
COMPONENTS = SOURCES / "usr/libexec/athanor/session-components"
SESSION = SOURCES / "usr/bin/athanor-session"


def anchor(test, seconds):
    """The PID of a `sleep` that is not a child of this process, so that once it ends no
    zombie answers `kill -0` for it."""
    pid = int(
        subprocess.run(
            ["/bin/sh", "-c", f"sleep {seconds} > /dev/null 2>&1 & echo $!"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
    )
    test.addCleanup(end, pid)
    return pid


def end(pid):
    try:
        os.kill(pid, 15)
    except ProcessLookupError:
        pass


class Scratch:
    """A scratch directory with stand-ins on PATH; each records its arguments in a log."""

    def __init__(self, test):
        self.work = Path(tempfile.mkdtemp())
        test.addCleanup(shutil.rmtree, self.work)
        self.bin = self.work / "bin"
        self.bin.mkdir()
        self.stub("systemd-cat", 'cat >> "$W/journal"')

    def stub(self, name, body):
        path = self.bin / name
        path.write_text(
            f'#!/bin/sh\nW="{self.work}"\necho "{name} $*" >> "$W/calls"\n{body}\n'
        )
        path.chmod(0o755)

    def calls(self):
        path = self.work / "calls"
        return path.read_text().splitlines() if path.exists() else []

    def journal(self):
        path = self.work / "journal"
        return path.read_text() if path.exists() else ""

    def env(self, **extra):
        return {"PATH": f"{self.bin}:/usr/bin:/bin", **extra}

    def components(self, call, timeout=20):
        """Runs `call` in a shell that has sourced session-components."""
        return subprocess.run(
            ["/bin/sh", "-c", f'. "{COMPONENTS}"; {call}'],
            env=self.env(),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
            timeout=timeout,
        )


class StopComponentScopes(unittest.TestCase):
    def test_only_an_active_scope_is_stopped(self):
        s = Scratch(self)
        s.stub(
            "systemctl",
            'case "$*" in *"is-active --quiet cosmic-idle.scope") exit 0 ;; *is-active*) exit 3 ;; esac',
        )
        result = s.components("stop_component_scopes")
        self.assertEqual(result.returncode, 0, result.stderr)
        stops = [c for c in s.calls() if " stop " in c]
        self.assertEqual(stops, ["systemctl --user stop cosmic-idle.scope"])

    def test_a_scope_that_stays_active_fails_the_call(self):
        s = Scratch(self)
        s.stub(
            "systemctl", 'case "$*" in *is-active*) exit 0 ;; *stop*) exit 1 ;; esac'
        )
        result = s.components("stop_component_scopes")
        self.assertEqual(result.returncode, 1)
        self.assertIn("cosmic-greeter-locker.scope did not stop", result.stderr)

    def test_a_failed_stop_of_a_scope_then_inactive_counts_as_done(self):
        s = Scratch(self)
        # Active at the first look, gone after the failed stop.
        s.stub(
            "systemctl",
            'case "$*" in *stop*) touch "$W/stopped"; exit 1 ;;'
            ' *is-active*) [ ! -e "$W/stopped" ] ;; esac',
        )
        result = s.components("stop_component_scopes")
        self.assertEqual(result.returncode, 0, result.stderr)


class Supervisor(unittest.TestCase):
    def test_with_target_it_waits_takes_the_display_and_starts_both_scopes(self):
        s = Scratch(self)
        s.stub(
            "systemctl",
            'case "$*" in'
            ' *"is-active --quiet athanor-session.target") exit 0 ;;'
            " *is-active*) exit 3 ;;"
            " *show-environment) echo LANG=C; echo WAYLAND_DISPLAY=wayland-7 ;;"
            " esac",
        )
        s.stub("systemd-run", 'echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY" >> "$W/display"')
        result = s.components(f"session_components {anchor(self, 3)} target")
        self.assertEqual(result.returncode, 0, result.stderr)
        runs = [c for c in s.calls() if c.startswith("systemd-run")]
        self.assertIn(
            "systemd-run --user --scope --collect --property=TimeoutStopSec=5s --unit=cosmic-greeter-locker.scope cosmic-greeter",
            runs,
        )
        self.assertIn(
            "systemd-run --user --scope --collect --property=TimeoutStopSec=5s --unit=cosmic-idle.scope cosmic-idle",
            runs,
        )
        self.assertEqual(
            set((s.work / "display").read_text().splitlines()),
            {"WAYLAND_DISPLAY=wayland-7"},
        )
        # The anchor gone, both scopes are stopped without waiting on a job.
        self.assertIn(
            "systemctl --user stop --no-block cosmic-greeter-locker.scope cosmic-idle.scope",
            s.calls(),
        )

    def test_with_target_and_no_display_it_logs_an_error_and_starts_nothing(self):
        s = Scratch(self)
        s.stub("systemctl", 'case "$*" in *show-environment) echo LANG=C ;; esac')
        s.stub("systemd-run", "")
        result = s.components(f"session_components {anchor(self, 10)} target")
        self.assertEqual(result.returncode, 1)
        self.assertIn("no WAYLAND_DISPLAY", s.journal())
        self.assertIn("systemd-cat -p err -t athanor-desktop", s.calls())
        self.assertFalse(any(c.startswith("systemd-run") for c in s.calls()))

    def test_an_anchor_gone_before_the_target_ends_the_wait(self):
        s = Scratch(self)
        s.stub("systemctl", "exit 3")
        s.stub("systemd-run", "")
        pid = anchor(self, 30)
        end(pid)
        result = s.components(f"session_components {pid} target")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any(c.startswith("systemd-run") for c in s.calls()))

    def test_the_fifth_short_failure_in_a_row_is_logged_once_at_err(self):
        s = Scratch(self)
        s.stub("sleep", "")
        pid = anchor(self, 30)
        # The seventh run ends the anchor, and with it the loop.
        s.stub(
            "failing",
            f'n=$(grep -c "^failing" "$W/calls"); [ "$n" -lt 7 ] || kill {pid}; exit 4',
        )
        started = time.monotonic()
        result = s.components(f"session_component {pid} cosmic-greeter failing")
        self.assertLess(time.monotonic() - started, 10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            s.journal().splitlines(),
            [
                (
                    "cosmic-greeter failed 5 times in a row, each run shorter than 60 seconds"
                    " (last status 4); retrying with backoff"
                )
            ],
        )
        delays = [c for c in s.calls() if c.startswith("sleep")]
        self.assertEqual(
            delays[:7], ["sleep 1"] * 4 + ["sleep 2", "sleep 4", "sleep 8"]
        )

    def test_on_term_it_stops_both_scopes_and_exits_143(self):
        # `loginctl terminate-session` sends TERM to the session's processes, and a scope lies
        # outside the session's own scope: the supervisor stops both before it exits.
        s = Scratch(self)
        s.stub(
            "systemctl",
            'case "$*" in'
            ' *"stop --no-block"*) : > "$W/stopped" ;;'
            ' *is-active*) [ ! -e "$W/stopped" ] ;;'
            " esac",
        )
        s.stub("systemd-run", "exec /bin/sleep 5")
        supervisor = subprocess.Popen(
            [
                "/bin/sh",
                "-c",
                f'. "{COMPONENTS}"; session_components {anchor(self, 30)}',
            ],
            env=s.env(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self.addCleanup(supervisor.kill)
        deadline = time.monotonic() + 5
        while sum(c.startswith("systemd-run") for c in s.calls()) < 2:
            self.assertLess(time.monotonic(), deadline, "both components started")
            time.sleep(0.05)
        supervisor.terminate()
        self.assertEqual(supervisor.wait(timeout=5), 143)
        self.assertIn(
            "systemctl --user stop --no-block cosmic-greeter-locker.scope cosmic-idle.scope",
            s.calls(),
        )


class SessionCleanup(unittest.TestCase):
    def run_session(self, compositor_installed):
        s = Scratch(self)
        unit = s.work / "athanor-compositor.service"
        if compositor_installed:
            unit.write_text("")
        s.stub("systemctl", 'case "$*" in *is-active*) exit 3 ;; esac')
        s.stub("athanor-usbguard-hook", "")
        text = (
            SESSION.read_text()
            .replace("/usr/libexec/athanor/session-components", str(COMPONENTS))
            .replace(
                "/usr/libexec/athanor/session-environment",
                str(SOURCES / "usr/libexec/athanor/session-environment"),
            )
            .replace("/usr/lib/systemd/user/athanor-compositor.service", str(unit))
            .replace(
                "/usr/bin/athanor-usbguard-hook", str(s.bin / "athanor-usbguard-hook")
            )
        )
        script = s.work / "athanor-session"
        script.write_text(text)
        result = subprocess.run(
            ["/bin/sh", str(script)],
            env=s.env(),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
            timeout=20,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return s.calls()

    def test_what_the_own_compositor_left_is_cleared_where_it_is_installed(self):
        calls = self.run_session(compositor_installed=True)
        self.assertIn("systemctl --user stop athanor-compositor.service", calls)
        self.assertIn("systemctl --user is-active --quiet cosmic-idle.scope", calls)
        self.assertIn(
            "systemctl --user unset-environment ATHANOR_OWN_COMPOSITOR XKB_DEFAULT_LAYOUT"
            " XKB_DEFAULT_VARIANT XKB_DEFAULT_OPTIONS",
            calls,
        )
        self.assertEqual(
            calls[-1],
            "systemd-cat -t athanor-session cosmic-comp --keep-shortcuts /usr/bin/athanor-desktop",
        )

    def test_the_compositor_unit_is_not_stopped_where_it_is_not_installed(self):
        calls = self.run_session(compositor_installed=False)
        self.assertNotIn("systemctl --user stop athanor-compositor.service", calls)
        self.assertIn(
            "systemctl --user is-active --quiet cosmic-greeter-locker.scope", calls
        )


if __name__ == "__main__":
    unittest.main()
