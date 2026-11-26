"""Unit tests of the separated-units check in scripts/verify.py
(python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

LOCK = "athanor-lock.service"
SHIPPED = SCRIPT.parents[1] / "forge/specs/athanor-lock/athanor-lock-1.0.0/data" / LOCK
KEPT = "NoNewPrivileges=yes\nSystemCallArchitectures=native\nSystemCallFilter=@system-service\nRestrictRealtime=yes\nRestrictSUIDSGID=yes\nLockPersonality=yes\nRestrictAddressFamilies=AF_UNIX\n"


def unit(lines):
    return (
        f"[Unit]\nDescription=x\n\n[Service]\nExecStart=/usr/bin/athanor-lock\n{lines}"
    )


def problems(units, dropins=None):
    """`dropins` maps `<spec>/<directory>/<file>` to the text of a user drop-in installed
    under usr/lib/systemd/user/, or a longer path under forge/specs to a file put there."""
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        for name, text in (dropins or {}).items():
            if name.count("/") == 2:
                spec, directory, file = name.split("/")
                name = (
                    f"{spec}/{spec}-1.0.0/data/usr/lib/systemd/user/{directory}/{file}"
                )
            path = root / "forge/specs" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        texts, found = verify.separated_unit_texts(root, units)
        return found + verify.separated_unit_problems({**units, **texts})


class SeparatedUnitsTest(unittest.TestCase):
    def test_the_lines_kept_pass(self):
        # The negative control: the unit D43 leaves, and the shipped unit itself.
        self.assertEqual(problems({LOCK: unit(KEPT)}), [])
        self.assertEqual(problems({LOCK: SHIPPED.read_text()}), [])

    def test_each_directive_measured_to_create_a_user_namespace_fails(self):
        for line in (
            "BindLogSockets=yes",
            "UserNamespacePath=/proc/1/ns/user",
            "JoinsNamespaceOf=other.service",
            "ProtectSystem=strict",
            "ProtectHome=read-only",
            "PrivateNetwork=yes",
            "PrivateTmp=yes",
            "PrivateUsers=self",
            "ProtectKernelTunables=yes",
            "ProtectControlGroups=yes",
            "ProtectHostname=private",
            "ReadWritePaths=/run/user/1000",
            "BindReadOnlyPaths=/etc",
            "DelegateNamespaces=mnt",
            "LogNamespace=lock",
            "NetworkNamespacePath=/run/netns/x",
            "IPCNamespacePath=/proc/1/ns/ipc",
        ):
            with self.subTest(line=line):
                found = problems({LOCK: unit(KEPT + line + "\n")})
                self.assertEqual(len(found), 1, found)
                self.assertIn(line, found[0])
                self.assertIn("not measured for separated units", found[0])

    def test_a_key_not_measured_fails_whatever_its_value(self):
        # The allow-list holds keys, not values: an off value is not measured either.
        for line in (
            "Frobnicate=yes",
            "ProtectSystem=no",
            "PrivateNetwork=false",
            "ProtectProc=default",
            "ReadWritePaths=",
        ):
            with self.subTest(line=line):
                found = problems({LOCK: unit(KEPT + line + "\n")})
                self.assertEqual(len(found), 1, found)
                self.assertIn("not measured for separated units", found[0])

    def test_every_assignment_is_read(self):
        self.assertEqual(
            len(problems({LOCK: unit(KEPT + "PrivateTmp=yes\nPrivateTmp=no\n")})), 2
        )

    def test_a_filter_without_the_native_architecture_fails(self):
        without = KEPT.replace("SystemCallArchitectures=native\n", "")
        for lines in (
            without,
            without + "SystemCallArchitectures=native x86\n",
            KEPT + "SystemCallArchitectures=\n",
            "RestrictAddressFamilies=AF_UNIX\n",
        ):
            with self.subTest(lines=lines):
                found = problems({LOCK: unit(lines)})
                self.assertEqual(len(found), 1, found)
                self.assertIn("without SystemCallArchitectures=native", found[0])
        # No filter, no requirement; and a filter reset by an empty assignment needs none.
        self.assertEqual(problems({LOCK: unit("NoNewPrivileges=yes\n")}), [])
        self.assertEqual(
            problems(
                {LOCK: unit("SystemCallFilter=@system-service\nSystemCallFilter=\n")}
            ),
            [],
        )

    def test_the_shipped_unit_without_its_native_line_fails(self):
        # The negative control on the real unit.
        text = SHIPPED.read_text()
        self.assertIn("SystemCallArchitectures=native\n", text)
        found = problems({LOCK: text.replace("SystemCallArchitectures=native\n", "")})
        self.assertEqual(len(found), 1, found)

    def test_a_drop_in_that_adds_a_directive_fails(self):
        found = problems(
            {LOCK: unit(KEPT)},
            {f"athanor-lock/{LOCK}.d/50-tmp.conf": "[Service]\nPrivateTmp=yes\n"},
        )
        self.assertEqual(len(found), 1, found)
        self.assertIn("PrivateTmp=yes", found[0])

    def test_drop_ins_apply_in_file_name_order_across_directories(self):
        # systemd sorts every drop-in of a unit by file name, whatever its directory or
        # spec; a walk in directory order would read these two the other way round.
        on = "[Service]\nSystemCallArchitectures=native\n"
        off = "[Service]\nSystemCallArchitectures=\n"
        self.assertEqual(
            problems(
                {LOCK: unit(KEPT)},
                {"zz/service.d/10-off.conf": off, f"aa/{LOCK}.d/90-on.conf": on},
            ),
            [],
        )
        self.assertEqual(
            len(
                problems(
                    {LOCK: unit(KEPT)},
                    {f"aa/{LOCK}.d/10-on.conf": on, "zz/service.d/90-off.conf": off},
                )
            ),
            1,
        )

    def test_prefix_and_type_wide_drop_ins_apply(self):
        for directory in ("athanor-.service.d", "service.d"):
            with self.subTest(directory=directory):
                found = problems(
                    {LOCK: unit(KEPT)},
                    {f"other/{directory}/50-tmp.conf": "[Service]\nPrivateTmp=yes\n"},
                )
                self.assertEqual(len(found), 1, found)
        # The prefixes are those of the name's own dashes (systemd.unit(5)):
        # athanor-lock-.service.d holds drop-ins of athanor-lock-*.service, not of this one.
        for directory in ("athanor-dock.service.d", "athanor-lock-.service.d"):
            with self.subTest(directory=directory):
                self.assertEqual(
                    problems(
                        {LOCK: unit(KEPT)},
                        {
                            f"other/{directory}/50-tmp.conf": "[Service]\nPrivateTmp=yes\n"
                        },
                    ),
                    [],
                )

    def test_a_drop_in_of_the_unit_replaces_a_wider_one_of_the_same_name(self):
        self.assertEqual(
            problems(
                {LOCK: unit(KEPT)},
                {
                    "aa/service.d/50-tmp.conf": "[Service]\nPrivateTmp=yes\n",
                    f"zz/{LOCK}.d/50-tmp.conf": "[Service]\nRestrictRealtime=yes\n",
                },
            ),
            [],
        )

    def test_every_section_is_read(self):
        found = problems({LOCK: "[Unit]\nProtectSystem=strict\n" + unit(KEPT)})
        self.assertEqual(len(found), 1, found)
        self.assertIn("[Unit] ProtectSystem=strict", found[0])
        self.assertEqual(
            problems({LOCK: unit(KEPT) + "[Install]\nWantedBy=x.target\n"}), []
        )

    def test_a_drop_in_the_check_cannot_merge_fails(self):
        for path in (
            f"athanor-lock/athanor-lock-1.0.0/data/{LOCK}.d/50-x.conf",
            f"athanor-lock/athanor-lock-1.0.0/data/etc/skel/.config/systemd/user/{LOCK}.d/50-x.conf",
        ):
            with self.subTest(path=path):
                found = problems(
                    {LOCK: unit(KEPT)}, {path: "[Service]\nRestrictRealtime=yes\n"}
                )
                self.assertEqual(len(found), 1, found)
                self.assertIn("cannot merge it", found[0])

    def test_a_continued_line_is_read_whole(self):
        found = problems({LOCK: unit(KEPT + "ReadOnlyPaths=/etc \\\n  /usr\n")})
        self.assertEqual(len(found), 1, found)

    def test_lines_are_read_as_systemd_reads_them(self):
        # Each hides PrivateTmp=yes from a parser that joins on any trailing backslash, while
        # systemd's conf-parser applies it.
        for lines in (
            "# a comment that ends in a backslash \\\nPrivateTmp=yes\n",
            "Environment=A=b\\\\\nPrivateTmp=yes\n",
            "Environment=A=b\\ \nPrivateTmp=yes\n",
            "Environment=A=b\rPrivateTmp=yes\n",
            "Environment=A=b\0PrivateTmp=yes\n",
        ):
            with self.subTest(lines=lines):
                found = problems({LOCK: unit(KEPT + lines)})
                self.assertEqual(len(found), 1, found)
                self.assertIn("PrivateTmp=yes", found[0])
        # A comment line inside a continued line is skipped, and the line goes on.
        self.assertEqual(
            verify.unit_lines("Environment=A=b \\\n# note\n  C=d\n"),
            ["Environment=A=b    C=d", ""],
        )

    def test_a_listed_unit_that_no_spec_ships_fails(self):
        found = problems({})
        self.assertEqual(len(found), 1, found)
        self.assertIn("shipped by no spec", found[0])

    def test_only_listed_units_are_held(self):
        self.assertEqual(
            problems({LOCK: unit(KEPT), "other.service": unit("PrivateNetwork=yes\n")}),
            [],
        )


if __name__ == "__main__":
    unittest.main()
