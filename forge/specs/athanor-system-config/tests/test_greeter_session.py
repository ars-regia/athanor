"""The locale handling of athanor-greeter-session
(python3 -B -m unittest discover -s forge/specs/athanor-system-config/tests -v).

The script runs unchanged but for the path of locale.conf; systemd-cat is a stand-in
that records the environment the compositor would have been given.
"""

import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "SOURCES/usr/bin/athanor-greeter-session"


class GreeterSession(unittest.TestCase):
    def session_environment(self, locale_conf, vconsole_conf=None):
        work = Path(tempfile.mkdtemp())
        conf = work / "locale.conf"
        conf.write_text(locale_conf)
        vconsole = work / "vconsole.conf"
        if vconsole_conf is not None:
            vconsole.write_text(vconsole_conf)
        script = work / "athanor-greeter-session"
        script.write_text(
            SCRIPT.read_text()
            .replace("/etc/locale.conf", str(conf))
            .replace("/etc/vconsole.conf", str(vconsole))
        )
        stubs = work / "bin"
        stubs.mkdir()
        (stubs / "systemd-cat").write_text(f'#!/bin/sh\nenv > "{work}/env"\n')
        (stubs / "systemd-cat").chmod(0o755)
        subprocess.run(
            ["/bin/sh", str(script)],
            env={"PATH": f"{stubs}:/usr/bin:/bin", "XDG_RUNTIME_DIR": str(work)},
            check=True,
        )
        return dict(
            line.split("=", 1)
            for line in (work / "env").read_text().splitlines()
            if "=" in line
        )

    def test_locale_variables_are_exported_quoted_or_not(self):
        env = self.session_environment(
            'LANG="it_IT.UTF-8"\nLC_TIME=de_DE.UTF-8\nLANGUAGE=it:en'
        )
        self.assertEqual(env["LANG"], "it_IT.UTF-8")
        self.assertEqual(env["LC_TIME"], "de_DE.UTF-8")
        self.assertEqual(env["LANGUAGE"], "it:en")

    def test_the_file_is_never_run(self):
        marker = Path(tempfile.mkdtemp()) / "ran"
        env = self.session_environment(
            f"LANG=it_IT.UTF-8; touch {marker}\nLC_MESSAGES=$(touch {marker})\ntouch {marker}\n"
        )
        self.assertFalse(marker.exists())
        self.assertNotIn("LANG", env)
        self.assertNotIn("LC_MESSAGES", env)

    def test_variables_other_than_the_locale_ones_are_ignored(self):
        env = self.session_environment(
            "LANG=en_US.UTF-8\nLD_PRELOAD=/tmp/x.so\nPATH=/tmp\n"
        )
        self.assertEqual(env["LANG"], "en_US.UTF-8")
        self.assertNotIn("LD_PRELOAD", env)
        self.assertNotEqual(env["PATH"], "/tmp")

    def test_the_system_layout_reaches_libxkbcommon(self):
        env = self.session_environment(
            "LANG=it_IT.UTF-8",
            'KEYMAP=us-intl\nXKBLAYOUT="us,it"\nXKBVARIANT=intl,\n'
            "XKBMODEL=pc105\nXKBOPTIONS=grp:alt_shift_toggle,compose:ralt",
        )
        self.assertEqual(env["XKB_DEFAULT_LAYOUT"], "us,it")
        self.assertEqual(env["XKB_DEFAULT_VARIANT"], "intl,")
        self.assertEqual(env["XKB_DEFAULT_MODEL"], "pc105")
        self.assertEqual(
            env["XKB_DEFAULT_OPTIONS"], "grp:alt_shift_toggle,compose:ralt"
        )
        self.assertNotIn("KEYMAP", env)
        self.assertNotIn("XKBLAYOUT", env)

    def test_a_layout_is_never_run_nor_taken_with_other_characters(self):
        marker = Path(tempfile.mkdtemp()) / "ran"
        env = self.session_environment(
            "LANG=it_IT.UTF-8",
            f"XKBLAYOUT=us;touch {marker}\nXKBVARIANT=$(touch {marker})\n"
            "XKBOPTIONS=a b\n",
        )
        self.assertFalse(marker.exists())
        self.assertNotIn("XKB_DEFAULT_LAYOUT", env)
        self.assertNotIn("XKB_DEFAULT_VARIANT", env)
        self.assertNotIn("XKB_DEFAULT_OPTIONS", env)

    def test_no_vconsole_conf_leaves_the_layout_to_libxkbcommon(self):
        env = self.session_environment("LANG=it_IT.UTF-8")
        self.assertEqual(env["LANG"], "it_IT.UTF-8")
        self.assertFalse(any(name.startswith("XKB_DEFAULT_") for name in env))


if __name__ == "__main__":
    unittest.main()
