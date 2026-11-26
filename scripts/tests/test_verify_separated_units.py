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
KEPT = "NoNewPrivileges=yes\nSystemCallFilter=@system-service\nRestrictRealtime=yes\nRestrictSUIDSGID=yes\nLockPersonality=yes\nRestrictAddressFamilies=AF_UNIX\n"


def unit(lines):
    return (
        f"[Unit]\nDescription=x\n\n[Service]\nExecStart=/usr/bin/athanor-lock\n{lines}"
    )


def problems(units, dropins=None):
    """`dropins` maps `<spec>/<directory>/<file>` to the text of a user drop-in."""
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        for name, text in (dropins or {}).items():
            spec, directory, file = name.split("/")
            path = (
                root
                / "forge/specs"
                / spec
                / f"{spec}-1.0.0/data/usr/lib/systemd/user"
                / directory
                / file
            )
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        return verify.separated_unit_problems(
            {**units, **verify.separated_unit_texts(root, units)}
        )


class SeparatedUnitsTest(unittest.TestCase):
    def test_the_lines_kept_pass(self):
        # The negative control: the unit D43 leaves, and the shipped unit itself.
        self.assertEqual(problems({LOCK: unit(KEPT)}), [])
        self.assertEqual(problems({LOCK: SHIPPED.read_text()}), [])

    def test_each_directive_measured_to_create_a_user_namespace_fails(self):
        for line in (
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

    def test_a_property_turned_off_passes(self):
        for line in (
            "ProtectSystem=no",
            "PrivateNetwork=false",
            "ProtectProc=default",
            "ReadWritePaths=",
        ):
            with self.subTest(line=line):
                self.assertEqual(problems({LOCK: unit(KEPT + line + "\n")}), [])

    def test_the_last_assignment_decides(self):
        self.assertEqual(
            problems({LOCK: unit(KEPT + "PrivateTmp=yes\nPrivateTmp=no\n")}), []
        )
        self.assertEqual(
            len(problems({LOCK: unit(KEPT + "PrivateTmp=no\nPrivateTmp=yes\n")})), 1
        )

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
        on, off = "[Service]\nPrivateTmp=yes\n", "[Service]\nPrivateTmp=no\n"
        self.assertEqual(
            problems(
                {LOCK: unit(KEPT)},
                {"zz/service.d/10-on.conf": on, f"aa/{LOCK}.d/90-off.conf": off},
            ),
            [],
        )
        self.assertEqual(
            len(
                problems(
                    {LOCK: unit(KEPT)},
                    {f"aa/{LOCK}.d/10-off.conf": off, "zz/service.d/90-on.conf": on},
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

    def test_a_directive_outside_the_service_section_is_not_read(self):
        self.assertEqual(
            problems({LOCK: "[Unit]\nProtectSystem=strict\n" + unit(KEPT)}), []
        )

    def test_a_continued_line_is_read_whole(self):
        found = problems({LOCK: unit(KEPT + "ReadOnlyPaths=/etc \\\n  /usr\n")})
        self.assertEqual(len(found), 1, found)

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
