"""athanor-compositor-session: the session on the own compositor, and its hand-off of the
sign-in to cosmic-comp's session (doc_session.md revision 4, SN14 and SN15)
(python3 -B -m unittest discover -s forge/specs/athanor-compositor/tests -v).

systemctl, systemd-cat, loginctl, busctl, gsettings, the compositor, the USBGuard hook and
athanor-session are stand-ins that record their arguments; the script's absolute paths are
rewritten to them. The supervisor of the locker and the idle daemon is the real one, and
finds athanor-session.target never active, so it starts nothing; the reader of the import
list and the list are the real ones, from athanor-system-config.
"""

import shutil
import signal
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

SPECS = Path(__file__).resolve().parents[2]
SCRIPT = (
    SPECS
    / "athanor-compositor/athanor-compositor-1.0.0/data/athanor-compositor-session"
)
COMPONENTS = (
    SPECS / "athanor-system-config/SOURCES/usr/libexec/athanor/session-components"
)
READER = "/usr/libexec/athanor/session-import-names"
ENVIRONMENT = "/usr/libexec/athanor/session-environment"
IMPORT_LIST = "/usr/share/athanor-system-config/session-import"
CONFIG_SOURCES = SPECS / "athanor-system-config/SOURCES"
# SN2's list, read as the reader reads it, for the shipped file has neither blanks nor CRLF.
NAMES = [
    line
    for line in (CONFIG_SOURCES / IMPORT_LIST.lstrip("/")).read_text().splitlines()
    if line and not line.startswith("#")
]
# What this session alone sets in the user manager (SN15, step 5).
OWN = [
    "ATHANOR_OWN_COMPOSITOR",
    "XKB_DEFAULT_LAYOUT",
    "XKB_DEFAULT_VARIANT",
    "XKB_DEFAULT_OPTIONS",
]

# systemctl: `start --wait` creates the marker as the unit's ExecStartPost= does when $START
# is "ok", fails at once when it is "fail", and when it is "hold" returns only once the
# compositor has been stopped after step 1's stop.
SYSTEMCTL = """
case "$*" in
    *"stop athanor-session.target athanor-compositor.service")
        [ ! -e "$W/stop-fails" ] ;;
    *"start --wait athanor-compositor.service")
        case $START in
            ok) touch "$W/run/athanor-compositor-session.started" ;;
            fail) exit 1 ;;
            hold) n=0; until [ "$(grep -c 'stop athanor-session.target' "$W/calls")" -ge 2 ] || [ "$n" -ge 100 ]; do sleep 0.1; n=$((n + 1)); done ;;
        esac ;;
    *"show athanor-compositor.service"*) echo Result=exit-code; echo ExecMainStatus=1 ;;
    *is-active*) exit 3 ;;
    *import-environment*) env > "$W/imported-env" ;;
esac
"""

# systemd-cat: with a command, runs it, as the script's re-exec does; without, journals stdin.
SYSTEMD_CAT = """
while [ $# -gt 0 ]; do
    case $1 in -t|-p) shift 2 ;; *) break ;; esac
done
if [ $# -gt 0 ]; then exec "$@"; fi
cat >> "$W/journal"
"""


class CompositorSession(unittest.TestCase):
    def setUp(self):
        self.work = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.work)
        (self.work / "run").mkdir()
        self.bin = self.work / "bin"
        self.bin.mkdir()
        self.stub("systemctl", SYSTEMCTL)
        self.stub("systemd-cat", SYSTEMD_CAT)
        self.stub("loginctl", "echo user")
        self.stub(
            "busctl",
            '[ ! -e "$W/busctl-fails" ] || exit 1\n'
            'case "$*" in *X11Layout) echo \'s "it"\' ;; *) echo \'s ""\' ;; esac',
        )
        self.stub("gsettings", 'cat "$W/magnifier" 2>/dev/null || echo false')
        self.stub(
            "athanor-compositor", 'exit "$(cat "$W/filter" 2>/dev/null || echo 0)"'
        )
        self.stub("athanor-usbguard-hook", "")
        self.stub("athanor-session", 'echo "$XDG_SESSION_TYPE" > "$W/handed-off"')
        text = SCRIPT.read_text().replace(
            "/usr/libexec/athanor/session-components", str(COMPONENTS)
        )
        for path in (READER, ENVIRONMENT, IMPORT_LIST):
            self.assertIn(path, text)
            text = text.replace(path, str(CONFIG_SOURCES / path.lstrip("/")))
        for name in ("athanor-session", "athanor-compositor", "athanor-usbguard-hook"):
            text = text.replace(f"/usr/bin/{name}", str(self.bin / name))
        self.text = text
        self.script = self.work / "athanor-compositor-session"
        self.script.write_text(text)
        self.script.chmod(0o755)

    def stub(self, name, body):
        path = self.bin / name
        path.write_text(
            f'#!/bin/sh\nW="{self.work}"\necho "{name} $*" >> "$W/calls"\n{body}\n'
        )
        path.chmod(0o755)

    def start(self, start="ok", **extra):
        env = {
            "PATH": f"{self.bin}:/usr/bin:/bin",
            "HOME": str(self.work / "home"),
            "XDG_RUNTIME_DIR": str(self.work / "run"),
            "XDG_SESSION_ID": "3",
            "START": start,
            **extra,
        }
        return subprocess.Popen(
            [str(self.script)],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )

    def run_session(self, start="ok", **extra):
        process = self.start(start, **extra)
        _, stderr = process.communicate(timeout=30)
        return process.returncode, stderr

    def calls(self):
        return (self.work / "calls").read_text().splitlines()

    def journal(self):
        path = self.work / "journal"
        return path.read_text() if path.exists() else ""

    def handed_off(self):
        return (self.work / "handed-off").exists()

    def assert_handed_off_before_start(self, reason):
        self.assertTrue(self.handed_off())
        self.assertIn(reason, self.journal())
        self.assertNotIn(
            "systemctl --user set-environment ATHANOR_OWN_COMPOSITOR=1", self.calls()
        )
        self.assertFalse(any("start --wait" in c for c in self.calls()))

    def assert_cleaned_up(self):
        # SN15 step 3 (d): the import list less XDG_CURRENT_DESKTOP, then this session's own.
        self.assertIn("XDG_CURRENT_DESKTOP", NAMES)
        removed = [n for n in NAMES if n != "XDG_CURRENT_DESKTOP"]
        self.assertIn("WAYLAND_DISPLAY", removed)
        self.assertIn(
            " ".join(["systemctl --user unset-environment", *removed, *OWN]),
            self.calls(),
        )

    def test_a_session_that_started_ends_with_the_compositor_and_cleans_up(self):
        status, stderr = self.run_session()
        self.assertEqual(status, 0, stderr)
        self.assertFalse(self.handed_off())
        calls = self.calls()
        for call in (
            "systemctl --user set-environment ATHANOR_OWN_COMPOSITOR=1",
            "systemctl --user set-environment XKB_DEFAULT_LAYOUT=it",
            "systemctl --user unset-environment XKB_DEFAULT_VARIANT",
            # SN15 step 5: the import list less the displays the compositor sets.
            " ".join(
                [
                    "systemctl --user import-environment",
                    *(n for n in NAMES if n not in ("WAYLAND_DISPLAY", "DISPLAY")),
                ]
            ),
            "athanor-usbguard-hook unlock",
        ):
            self.assertIn(call, calls)
        start = calls.index("systemctl --user start --wait athanor-compositor.service")
        stops = [
            i
            for i, c in enumerate(calls)
            if c
            == "systemctl --user stop athanor-session.target athanor-compositor.service"
        ]
        self.assertEqual(len(stops), 2, "step 1's stop and the end's")
        self.assertLess(stops[0], start)
        self.assertGreater(stops[1], start)
        self.assert_cleaned_up()

    def test_the_session_follows_the_import_list(self):
        listing = self.work / "session-import"
        listing.write_bytes(
            b"# c\r\n  # x y\r\n\tWAYLAND_DISPLAY\r\nLANG\r\n\nXDG_CURRENT_DESKTOP\n"
        )
        self.script.write_text(
            self.text.replace(
                str(CONFIG_SOURCES / IMPORT_LIST.lstrip("/")), str(listing)
            )
        )
        status, stderr = self.run_session()
        self.assertEqual(status, 0, stderr)
        calls = self.calls()
        self.assertIn(
            "systemctl --user import-environment LANG XDG_CURRENT_DESKTOP", calls
        )
        self.assertIn(
            " ".join(["systemctl --user unset-environment WAYLAND_DISPLAY LANG", *OWN]),
            calls,
        )

    def test_a_list_of_the_displays_alone_imports_nothing(self):
        listing = self.work / "session-import"
        listing.write_bytes(b"WAYLAND_DISPLAY\nDISPLAY\n")
        self.script.write_text(
            self.text.replace(
                str(CONFIG_SOURCES / IMPORT_LIST.lstrip("/")), str(listing)
            )
        )
        status, stderr = self.run_session()
        self.assertEqual(status, 0, stderr)
        self.assertFalse(any("import-environment" in c for c in self.calls()))
        self.assertTrue(any("start --wait" in c for c in self.calls()))

    def test_a_refused_import_list_starts_nothing_and_hands_nothing_off(self):
        for listing, reason in (
            (b"# only a comment\n", "names no variable"),
            (b"WAYLAND_DISPLAY\n*\n", "not a variable name"),
            (b"A B\n", "more than one word"),
            (None, "cannot read the import list"),
        ):
            with self.subTest(listing=listing):
                path = self.work / "session-import"
                path.unlink(missing_ok=True)
                (self.work / "calls").unlink(missing_ok=True)
                (self.work / "journal").unlink(missing_ok=True)
                if listing is not None:
                    path.write_bytes(listing)
                self.script.write_text(
                    self.text.replace(
                        str(CONFIG_SOURCES / IMPORT_LIST.lstrip("/")), str(path)
                    )
                )
                status, stderr = self.run_session()
                self.assertEqual(status, 1, stderr)
                self.assertIn(reason, stderr + self.journal())
                self.assertIn("the import list cannot be used", self.journal())
                self.assertFalse(self.handed_off())
                # Step 1's cleanup ran; nothing of this session was set or started.
                calls = self.calls()
                self.assertIn(
                    "systemctl --user stop athanor-session.target"
                    " athanor-compositor.service",
                    calls,
                )
                self.assertFalse(
                    any(
                        word in c
                        for c in calls
                        for word in ("set-environment", "import-environment", "start")
                    )
                )

    def test_the_user_s_language_region_and_cursor_reach_the_import(self):
        self.assertIn("LANG", NAMES)
        self.stub(
            "busctl",
            'case "$*" in\n'
            "    *FindUserById*) echo 'o \"/org/freedesktop/Accounts/User1000\"' ;;\n"
            '    *Languages) echo \'v as 2 "it_IT.UTF-8" "en_US.UTF-8"\' ;;\n'
            "    *X11Layout) echo 's \"it\"' ;;\n"
            "    *) echo 's \"\"' ;;\n"
            "esac",
        )
        self.stub(
            "gsettings",
            'case "$2 $3" in\n'
            "    *screen-magnifier-enabled) echo false ;;\n"
            "    *region) echo \"'de_DE.UTF-8'\" ;;\n"
            "    *cursor-size) echo 32 ;;\n"
            "    *cursor-theme) echo \"'Adwaita'\" ;;\n"
            "esac",
        )
        status, stderr = self.run_session()
        self.assertEqual(status, 0, stderr)
        self.assertFalse(self.handed_off())
        exported = (self.work / "imported-env").read_text().splitlines()
        for line in (
            "LANG=it_IT.UTF-8",
            "LANGUAGE=it_IT.UTF-8:en_US.UTF-8",
            "LC_TIME=de_DE.UTF-8",
            "XCURSOR_SIZE=32",
            "XCURSOR_THEME=Adwaita",
        ):
            self.assertIn(line, exported)

    def test_the_magnifier_from_the_greeter_hands_off(self):
        status, stderr = self.run_session(ATHANOR_A11Y="screen-reader,magnifier")
        self.assertEqual(status, 0, stderr)
        self.assert_handed_off_before_start(
            "the greeter hands the magnifier to the session"
        )

    def test_the_magnifier_on_in_the_settings_hands_off(self):
        (self.work / "magnifier").write_text("true\n")
        self.run_session()
        self.assert_handed_off_before_start("the magnifier is on (true)")

    def test_a_dconf_database_that_cannot_be_read_hands_off(self):
        dconf = self.work / "home/.config/dconf"
        dconf.mkdir(parents=True)
        (dconf / "user").write_text("")
        (dconf / "user").chmod(0)
        self.run_session()
        self.assert_handed_off_before_start(
            "cannot be read, so the magnifier's setting"
        )

    def test_a_colour_filter_on_or_unknown_hands_off(self):
        for code in ("10", "2"):
            with self.subTest(code=code):
                (self.work / "filter").write_text(code)
                for left in ("calls", "journal", "handed-off"):
                    (self.work / left).unlink(missing_ok=True)
                self.run_session()
                self.assert_handed_off_before_start(
                    f"athanor-compositor --screen-filter-on exited {code}"
                )

    def test_a_session_that_did_not_start_hands_off_after_cleaning_up(self):
        status, stderr = self.run_session(start="fail")
        self.assertEqual(status, 0, stderr)
        self.assertTrue(self.handed_off())
        self.assertIn(
            "the session did not start (systemctl start exited 1;"
            " Result=exit-code ExecMainStatus=1)",
            self.journal(),
        )
        self.assert_cleaned_up()
        # cosmic-comp's session gets the environment the script exported.
        self.assertEqual((self.work / "handed-off").read_text().strip(), "wayland")

    def test_a_keyboard_setting_that_cannot_be_read_hands_off_after_cleaning_up(self):
        (self.work / "busctl-fails").write_text("")
        self.run_session()
        self.assertTrue(self.handed_off())
        self.assertIn(
            "busctl get-property org.freedesktop.locale1 X11Layout failed",
            self.journal(),
        )
        self.assertFalse(any("start --wait" in c for c in self.calls()))
        self.assert_cleaned_up()

    def test_a_failed_cleanup_of_the_previous_session_ends_the_sign_in(self):
        (self.work / "stop-fails").write_text("")
        status, _ = self.run_session()
        self.assertNotEqual(status, 0)
        self.assertFalse(self.handed_off())
        self.assertFalse(any("start --wait" in c for c in self.calls()))

    def test_a_hangup_while_the_compositor_runs_ends_the_session_without_a_hand_off(
        self,
    ):
        process = self.start(start="hold")
        deadline = time.monotonic() + 10
        while not any("start --wait" in c for c in self.calls_so_far()):
            self.assertLess(time.monotonic(), deadline, "the compositor never started")
            time.sleep(0.05)
        process.send_signal(signal.SIGHUP)
        _, stderr = process.communicate(timeout=30)
        self.assertEqual(process.returncode, 129, stderr)
        self.assertFalse(self.handed_off())
        self.assert_cleaned_up()

    def test_a_supervisor_that_ignores_term_is_killed_and_the_session_still_ends(self):
        # The stand-in ignores TERM and loops, as a supervisor stuck in systemd-run would.
        stuck = self.work / "session-components"
        stuck.write_text(
            f". {COMPONENTS}\n"
            "session_components() {\n"
            f'    trap "" TERM; : > "{self.work}/stuck-ready"\n'
            "    while :; do sleep 0.1; done\n"
            "}\n"
        )
        self.script.write_text(self.text.replace(str(COMPONENTS), str(stuck)))
        process = self.start(start="hold")
        deadline = time.monotonic() + 10
        while not (
            (self.work / "stuck-ready").exists()
            and any("start --wait" in c for c in self.calls_so_far())
        ):
            self.assertLess(time.monotonic(), deadline, "the session never started")
            time.sleep(0.05)
        began = time.monotonic()
        process.send_signal(signal.SIGHUP)
        # The pipes close only when the supervisor and its sleeps are gone.
        _, stderr = process.communicate(timeout=30)
        self.assertLess(time.monotonic() - began, 15)
        self.assertEqual(process.returncode, 129, stderr)
        self.assertIn("did not end within 5 s and was killed", self.journal())
        calls = self.calls()
        scope_check = calls.index(
            "systemctl --user is-active --quiet cosmic-idle.scope",
            calls.index("systemctl --user start --wait athanor-compositor.service"),
        )
        unit_stop = (
            "systemctl --user stop athanor-session.target athanor-compositor.service"
        )
        self.assertGreater(len(calls) - calls[::-1].index(unit_stop) - 1, scope_check)
        self.assert_cleaned_up()

    def calls_so_far(self):
        path = self.work / "calls"
        return path.read_text().splitlines() if path.exists() else []


if __name__ == "__main__":
    unittest.main()
