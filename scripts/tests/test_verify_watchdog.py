"""Unit tests of the watchdog rule of the session-units check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

PROGRAMS = {
    "athanor-idle.service": ("athanor-idle", "dbus"),
    "athanor-sessiond.service": ("athanor-sessiond", "notify"),
    "athanor-shelld.service": ("athanor-shelld", "notify"),
    "xdg-desktop-portal-athanor.service": ("xdg-desktop-portal-athanor", "dbus"),
}
SENDERS = {program: "spawn" for program, _ in PROGRAMS.values()}


def unit(program, kind, watchdog="WatchdogSec=30s\n", access="NotifyAccess=main\n"):
    return f"[Service]\nType={kind}\n{access}ExecStart=/usr/bin/{program}\n{watchdog}"


def units(**changes):
    shipped = {name: unit(*PROGRAMS[name]) for name in PROGRAMS}
    shipped.update(changes)
    return shipped


class WatchdogTest(unittest.TestCase):
    def test_the_four_daemons_that_ping_pass(self):
        self.assertEqual(verify.watchdog_problems(units(), SENDERS), [])

    def test_a_listed_daemon_without_the_watchdog_fails(self):
        shelld = unit("athanor-shelld", "notify", watchdog="")
        found = verify.watchdog_problems(
            units(**{"athanor-shelld.service": shelld}), SENDERS
        )
        self.assertEqual(len(found), 1)
        self.assertIn("athanor-shelld.service: WatchdogSec=(unset)", found[0])

    def test_a_listed_daemon_with_another_interval_fails(self):
        idle = unit("athanor-idle", "dbus", watchdog="WatchdogSec=10s\n")
        found = verify.watchdog_problems(
            units(**{"athanor-idle.service": idle}), SENDERS
        )
        self.assertEqual(len(found), 1)
        self.assertIn("WatchdogSec=10s", found[0])

    def test_a_watchdog_whose_crate_sends_no_ping_fails(self):
        found = verify.watchdog_problems(
            units(), dict(SENDERS, **{"athanor-sessiond": None})
        )
        self.assertEqual(len(found), 1)
        self.assertIn("athanor-sessiond sends no WATCHDOG=1", found[0])

    def test_a_listed_daemon_that_pings_without_the_roundtrip_fails(self):
        found = verify.watchdog_problems(
            units(), dict(SENDERS, **{"athanor-idle": "ping"})
        )
        self.assertEqual(len(found), 1)
        self.assertIn(
            "athanor-idle's main does not call athanor_unit::watchdog::spawn", found[0]
        )

    def test_a_watchdog_on_a_program_of_no_crate_fails(self):
        other = unit("someone-else", "notify")
        found = verify.watchdog_problems(units(**{"x-other.service": other}), SENDERS)
        self.assertEqual(len(found), 1)
        self.assertIn(
            "x-other.service: sets WatchdogSec= and runs someone-else", found[0]
        )

    def test_a_unit_that_refuses_notifications_fails(self):
        portal = unit(
            "xdg-desktop-portal-athanor", "dbus", access="NotifyAccess=none\n"
        )
        found = verify.watchdog_problems(
            units(**{"xdg-desktop-portal-athanor.service": portal}), SENDERS
        )
        self.assertEqual(len(found), 1)
        self.assertIn("NotifyAccess=none", found[0])

    def test_a_dbus_unit_with_the_implied_notify_access_passes(self):
        portal = unit("xdg-desktop-portal-athanor", "dbus", access="")
        found = verify.watchdog_problems(
            units(**{"xdg-desktop-portal-athanor.service": portal}), SENDERS
        )
        self.assertEqual(found, [])

    def test_a_watchdog_that_is_no_time_span_fails(self):
        other = unit("athanor-bar", "notify", watchdog="WatchdogSec=0\n")
        found = verify.watchdog_problems(
            units(**{"athanor-bar.service": other}), SENDERS
        )
        self.assertEqual(len(found), 1)
        self.assertIn("no positive time span", found[0])

    def test_a_ping_is_found_in_the_crate_and_not_in_a_comment_or_a_definition(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for name, files in {
                "pinger": {
                    "main.rs": 'fn main() { let _ = "http://x"; athanor_unit::watchdog::spawn(&c); }\n'
                },
                "quiet": {
                    "main.rs": "// athanor_unit::watchdog::spawn(&connection);\npub fn notify_watchdog() {}\n"
                },
                "tested": {
                    "main.rs": "fn main() {}\n#[cfg(test)]\nmod tests { fn t() { watchdog::spawn(&c); } }\n"
                },
                "library": {
                    "main.rs": "fn main() {}\n",
                    "lib.rs": "pub fn unused() { watchdog::spawn(&c); }\n",
                },
                "timer": {
                    "main.rs": "fn main() { athanor_unit::notify::notify_watchdog(); }\n"
                },
                "hidden": {
                    "main.rs": 'fn main() { /* watchdog::spawn(&c) */ let _ = "watchdog::spawn(&c)"; }\n'
                },
            }.items():
                crate = root / "system" / name
                (crate / "src").mkdir(parents=True)
                (crate / "Cargo.toml").write_text(f'[package]\nname = "{name}"\n')
                for file, body in files.items():
                    (crate / "src" / file).write_text(body)
            self.assertEqual(
                verify.watchdog_senders(root),
                {
                    "pinger": "spawn",
                    "quiet": None,
                    "tested": None,
                    "library": "ping",
                    "timer": "ping",
                    "hidden": None,
                },
            )


if __name__ == "__main__":
    unittest.main()
