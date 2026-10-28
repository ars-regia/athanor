"""athanor-session exports the user's language, region and cursor (doc_session.md SN2)
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

The script runs unchanged but for the USBGuard hook's path; systemctl, busctl, gsettings
and systemd-cat are stand-ins. systemd-cat records the journal messages and, for the call
that starts the compositor, the environment the compositor would have been given.
"""

import subprocess
import tempfile
import unittest
from pathlib import Path

SOURCES = Path(__file__).resolve().parents[1] / "SOURCES"
SCRIPT = SOURCES / "usr/bin/athanor-session"
IMPORT_LIST = SOURCES / "usr/share/athanor-system-config/session-import"
FORMATS = (
    "LC_NUMERIC",
    "LC_TIME",
    "LC_MONETARY",
    "LC_PAPER",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
)
DEFAULTS = {
    "org.gnome.system.locale region": "''",
    "org.gnome.desktop.interface cursor-size": "24",
    "org.gnome.desktop.interface cursor-theme": "'Adwaita'",
}


class SessionEnvironment(unittest.TestCase):
    def session_environment(self, languages="as 0", settings=None, lang="en_US.UTF-8"):
        """Runs athanor-session with busctl printing `languages` (None: busctl fails) and
        gsettings printing `settings` over DEFAULTS (a value None: that key fails)."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        work = Path(tmp.name)
        hook = "/usr/bin/athanor-usbguard-hook"
        text = SCRIPT.read_text()
        self.assertIn(hook, text)
        script = work / "athanor-session"
        script.write_text(text.replace(hook, "true"))
        stubs = work / "bin"
        stubs.mkdir()

        def stub(name, body):
            (stubs / name).write_text("#!/bin/sh\n" + body)
            (stubs / name).chmod(0o755)

        stub("systemctl", "exit 0\n")
        if languages is None:
            stub("busctl", "exit 1\n")
        else:
            (work / "languages").write_text(languages + "\n")
            # FindUserById names the object; get-property must read that object.
            stub(
                "busctl",
                'case "$3" in\n'
                'call) echo \'o "/org/freedesktop/Accounts/User1000"\' ;;\n'
                f'get-property) [ "$5" = /org/freedesktop/Accounts/User1000 ] && cat "{work}/languages" ;;\n'
                "*) exit 2 ;;\n"
                "esac\n",
            )
        cases = ""
        for key, value in {**DEFAULTS, **(settings or {})}.items():
            if value is None:
                cases += f'"{key}") exit 1 ;;\n'
            else:
                (work / key.replace(" ", "_")).write_text(value + "\n")
                cases += f'"{key}") cat "{work}/{key.replace(" ", "_")}" ;;\n'
        stub("gsettings", f'case "$2 $3" in\n{cases}*) exit 2 ;;\nesac\n')
        # A call with -p is a journal message; the other one starts the compositor.
        stub(
            "systemd-cat",
            f'if [ "$1" = -p ]; then cat >> "{work}/journal"; else env > "{work}/env"; fi\n',
        )
        subprocess.run(
            ["/bin/sh", str(script)],
            env={"PATH": f"{stubs}:/usr/bin:/bin", "LANG": lang},
            check=True,
        )
        journal = work / "journal"
        self.journal = journal.read_text() if journal.exists() else ""
        return dict(
            line.split("=", 1)
            for line in (work / "env").read_text().splitlines()
            if "=" in line
        )

    def test_lang_comes_from_the_first_language_and_language_from_the_list(self):
        env = self.session_environment('as 2 "it_IT.UTF-8" "en_US.UTF-8"')
        self.assertEqual(env["LANG"], "it_IT.UTF-8")
        self.assertEqual(env["LANGUAGE"], "it_IT.UTF-8:en_US.UTF-8")

    def test_one_language_sets_no_language_list(self):
        env = self.session_environment('as 1 "de_DE.UTF-8"')
        self.assertEqual(env["LANG"], "de_DE.UTF-8")
        self.assertNotIn("LANGUAGE", env)

    def test_no_language_keeps_the_system_locale(self):
        env = self.session_environment("as 0")
        self.assertEqual(env["LANG"], "en_US.UTF-8")
        self.assertNotIn("LANGUAGE", env)
        self.assertEqual(self.journal, "")

    def test_a_value_that_fails_the_check_is_logged_and_ignored(self):
        for languages in (
            'as 1 "it_IT.UTF-8;x"',
            'as 1 "it IT"',
            'as 1 "$(id)"',
            "as 1 *",
        ):
            with self.subTest(languages=languages):
                env = self.session_environment(languages)
                self.assertEqual(env["LANG"], "en_US.UTF-8")
                self.assertIn("ignoring the language", self.journal)

    def test_an_unreadable_accounts_service_is_logged_and_the_login_goes_on(self):
        env = self.session_environment(None)
        self.assertEqual(env["LANG"], "en_US.UTF-8")
        self.assertIn("cannot read the languages", self.journal)

    def test_an_answer_that_is_not_a_list_of_strings_is_logged(self):
        env = self.session_environment('s "it_IT.UTF-8"')
        self.assertEqual(env["LANG"], "en_US.UTF-8")
        self.assertIn("cannot parse the languages", self.journal)

    def test_a_region_that_differs_from_the_language_sets_the_formats(self):
        env = self.session_environment(
            'as 1 "en_US.UTF-8"', {"org.gnome.system.locale region": "'it_IT.UTF-8'"}
        )
        for name in FORMATS:
            self.assertEqual(env[name], "it_IT.UTF-8", name)

    def test_a_region_equal_to_the_language_or_empty_sets_none(self):
        for region in ("'en_US.UTF-8'", "''"):
            with self.subTest(region=region):
                env = self.session_environment(
                    'as 1 "en_US.UTF-8"', {"org.gnome.system.locale region": region}
                )
                self.assertFalse(set(FORMATS) & env.keys())

    def test_an_unreadable_or_odd_region_sets_none(self):
        for region in (None, "'it IT'", '"it\'s"'):
            with self.subTest(region=region):
                env = self.session_environment(
                    "as 0", {"org.gnome.system.locale region": region}
                )
                self.assertFalse(set(FORMATS) & env.keys())
                self.assertNotEqual(self.journal, "")

    def test_the_cursor_comes_from_the_interface_keys(self):
        env = self.session_environment(
            settings={
                "org.gnome.desktop.interface cursor-size": "48",
                "org.gnome.desktop.interface cursor-theme": "'Bibata-Modern'",
            }
        )
        self.assertEqual(env["XCURSOR_SIZE"], "48")
        self.assertEqual(env["XCURSOR_THEME"], "Bibata-Modern")

    def test_an_unreadable_cursor_is_logged_and_left_unset(self):
        env = self.session_environment(
            settings={"org.gnome.desktop.interface cursor-size": None}
        )
        self.assertNotIn("XCURSOR_SIZE", env)
        self.assertIn("cursor", self.journal)

    def test_a_cursor_size_that_is_not_a_number_is_ignored(self):
        for size in ("-1", "24.5", "uint32 24"):
            with self.subTest(size=size):
                env = self.session_environment(
                    settings={"org.gnome.desktop.interface cursor-size": size}
                )
                self.assertNotIn("XCURSOR_SIZE", env)
                self.assertIn("ignoring the cursor size", self.journal)

    def test_the_import_list_holds_every_variable_the_session_exports(self):
        names = {
            line
            for line in IMPORT_LIST.read_text().splitlines()
            if line and not line.startswith("#")
        }
        self.assertLessEqual(
            {"LANG", "LANGUAGE", "XCURSOR_SIZE", "XCURSOR_THEME", *FORMATS}, names
        )


if __name__ == "__main__":
    unittest.main()
