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
SPEC = "athanor-lock/athanor-lock.spec"
DATA = "athanor-lock/athanor-lock-1.0.0/data"
REFUSALS = next(
    line + "\n"
    for line in SHIPPED.read_text().splitlines()
    if line.startswith("SystemCallFilter=~")
)
NBSP = "\N{NO-BREAK SPACE}"
KEPT = (
    "NoNewPrivileges=yes\nSystemCallArchitectures=native\nSystemCallFilter=@system-service\n"
    + REFUSALS
    + "RestrictRealtime=yes\nRestrictSUIDSGID=yes\nLockPersonality=yes\nRestrictAddressFamilies=AF_UNIX\n"
)


def unit(lines):
    return (
        f"[Unit]\nDescription=x\n\n[Service]\nExecStart=/usr/bin/athanor-lock\n{lines}"
    )


def problems(units, dropins=None):
    """`dropins` maps `<spec>/<directory>/<file>` to the text of a user drop-in installed
    under usr/lib/systemd/user/, or a longer path under forge/specs to a file put there,
    such as a spec."""
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
        found, dropin_problems = verify.separated_unit_dropins(root, units)
        return dropin_problems + verify.separated_unit_problems(units, found)


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
        ):
            with self.subTest(lines=lines):
                found = problems({LOCK: unit(lines)})
                self.assertEqual(len(found), 1, found)
                self.assertIn("without SystemCallArchitectures=native", found[0])

    def test_a_unit_without_the_filter_fails(self):
        # Each of the three lists is required; a reset in the unit itself empties it.
        for lines in (
            "NoNewPrivileges=yes\n",
            "SystemCallArchitectures=native\nSystemCallArchitectures=\n"
            "SystemCallFilter=@system-service\nSystemCallFilter=\n"
            "RestrictAddressFamilies=AF_UNIX\nRestrictAddressFamilies=\n",
        ):
            with self.subTest(lines=lines):
                found = problems({LOCK: unit(lines)})
                self.assertEqual(len(found), 3, found)

    def test_the_shipped_filter_widened_or_cut_fails(self):
        # The negative control on the real unit: each change keeps every key LP15 names
        # and breaks one of its values.
        text = SHIPPED.read_text()
        for old, new in (
            (
                "RestrictAddressFamilies=AF_UNIX\n",
                "RestrictAddressFamilies=AF_UNIX AF_INET\n",
            ),
            (
                "RestrictAddressFamilies=AF_UNIX\n",
                "RestrictAddressFamilies=AF_UNIX\nRestrictAddressFamilies=AF_NETLINK\n",
            ),
            ("RestrictAddressFamilies=AF_UNIX\n", "RestrictAddressFamilies=~AF_INET\n"),
            (REFUSALS, ""),
            (" chmod:EPERM", ""),
            (" removexattrat:EPERM", ""),
            (
                "SystemCallFilter=@system-service\n",
                "SystemCallFilter=@system-service @mount\n",
            ),
            (REFUSALS, REFUSALS + "SystemCallFilter=chmod\n"),
            ("SystemCallFilter=@system-service\n", "SystemCallFilter=~@mount\n"),
        ):
            with self.subTest(new=new):
                self.assertEqual(text.count(old), 1)
                found = problems({LOCK: text.replace(old, new)})
                self.assertEqual(len(found), 1, found)

    def test_a_drop_in_that_changes_the_filter_fails(self):
        for dropin in (
            "[Service]\nRestrictAddressFamilies=AF_INET\n",
            "[Service]\nSystemCallFilter=@mount\n",
            "[Service]\nSystemCallArchitectures=x86\n",
        ):
            with self.subTest(dropin=dropin):
                found = problems(
                    {LOCK: unit(KEPT)}, {f"athanor-lock/{LOCK}.d/50-x.conf": dropin}
                )
                self.assertEqual(len(found), 1, found)
        # An added refusal narrows the filter, and passes.
        self.assertEqual(
            problems(
                {LOCK: unit(KEPT)},
                {
                    f"athanor-lock/{LOCK}.d/50-x.conf": "[Service]\nSystemCallFilter=~@mount\n"
                },
            ),
            [],
        )

    def test_a_drop_in_that_resets_a_filter_fails(self):
        # Even when it sets the same values again: the reset is what the unit cannot see.
        for key, again in (
            ("SystemCallArchitectures", "SystemCallArchitectures=native\n"),
            ("RestrictAddressFamilies", "RestrictAddressFamilies=AF_UNIX\n"),
            ("SystemCallFilter", "SystemCallFilter=@system-service\n" + REFUSALS),
        ):
            with self.subTest(key=key):
                found = problems(
                    {LOCK: unit(KEPT)},
                    {f"athanor-lock/{LOCK}.d/50-x.conf": f"[Service]\n{key}=\n{again}"},
                )
                self.assertEqual(len(found), 1, found)
                self.assertIn(f"resets {key}=", found[0])

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
        # An allowed call after a refusal is allowed again.
        without = unit(KEPT.replace(REFUSALS, ""))
        allow = "[Service]\nSystemCallFilter=@system-service\n"
        refuse = "[Service]\n" + REFUSALS
        self.assertEqual(
            problems(
                {LOCK: without},
                {
                    "zz/service.d/10-allow.conf": allow,
                    f"aa/{LOCK}.d/90-refuse.conf": refuse,
                },
            ),
            [],
        )
        self.assertEqual(
            len(
                problems(
                    {LOCK: without},
                    {
                        f"aa/{LOCK}.d/10-refuse.conf": refuse,
                        "zz/service.d/90-allow.conf": allow,
                    },
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

    def test_a_drop_in_a_spec_installs_is_read_from_its_source(self):
        # The lock's spec installs its unit with install -D from data/; a drop-in installed
        # the same way sits in no `<unit>.d` directory of the tree.
        for directory in (f"{LOCK}.d", "athanor-.service.d", "service.d"):
            for install in (
                f"install -D -m 0644 data/20-extra.conf %{{buildroot}}/usr/lib/systemd/user/{directory}/20-extra.conf",
                f"install -m 0644 -t %{{buildroot}}%{{_userunitdir}}/{directory} data/20-extra.conf",
                f"cp data/20-extra.conf $RPM_BUILD_ROOT/usr/lib/systemd/user/{directory}/",
            ):
                with self.subTest(directory=directory, install=install):
                    found = problems(
                        {LOCK: unit(KEPT)},
                        {
                            SPEC: f"%install\n{install}\n\n%files\n%dir /usr/lib/systemd/user/{directory}\n/usr/lib/systemd/user/{directory}/20-extra.conf\n",
                            f"{DATA}/20-extra.conf": "[Service]\nPrivateTmp=yes\n",
                        },
                    )
                    self.assertEqual(len(found), 1, found)
                    self.assertIn("PrivateTmp=yes", found[0])
        # The negative control: the same spec with a drop-in of measured lines passes.
        self.assertEqual(
            problems(
                {LOCK: unit(KEPT)},
                {
                    SPEC: f"%install\ninstall -D data/20-extra.conf %{{buildroot}}/usr/lib/systemd/user/{LOCK}.d/20-extra.conf\n\n%files\n/usr/lib/systemd/user/{LOCK}.d/20-extra.conf\n",
                    f"{DATA}/20-extra.conf": "[Service]\nRestrictRealtime=yes\n",
                },
            ),
            [],
        )

    def test_a_drop_in_the_check_cannot_read_fails(self):
        for text in (
            # Its source holds a variable, or names no file of the repository.
            f"%install\ninstall -D $SRC %{{buildroot}}/usr/lib/systemd/user/{LOCK}.d/20-x.conf\n%files\n/usr/lib/systemd/user/{LOCK}.d/20-x.conf\n",
            f"%install\ninstall -D data/missing.conf %{{buildroot}}/usr/lib/systemd/user/{LOCK}.d/20-x.conf\n%files\n/usr/lib/systemd/user/{LOCK}.d/20-x.conf\n",
            # Listed in %files, and neither installed nor in the tree.
            "%files\n/usr/lib/systemd/user/service.d/20-x.conf\n",
            # Its destination holds a macro the check does not know.
            "%install\ninstall -D data/a.conf %{buildroot}%{_mydir}/athanor-.service.d/a.conf\n",
            # The whole directory listed, which ships whatever lands in it.
            f"%files\n/usr/lib/systemd/user/{LOCK}.d\n",
        ):
            with self.subTest(spec=text):
                found = problems(
                    {LOCK: unit(KEPT)},
                    {SPEC: text, f"{DATA}/a.conf": "[Service]\n"},
                )
                self.assertEqual(len(found), 1, found)
                self.assertIn("separated", found[0])

    def test_a_drop_in_outside_systemd_user_that_no_spec_installs_fails(self):
        for directory in (f"{LOCK}.d", "athanor-.service.d", "service.d"):
            with self.subTest(directory=directory):
                found = problems(
                    {LOCK: unit(KEPT)},
                    {
                        f"{DATA}/{directory}/50-x.conf": "[Service]\nRestrictRealtime=yes\n"
                    },
                )
                self.assertEqual(len(found), 1, found)
                self.assertIn("cannot merge it", found[0])

    def test_words_split_where_systemd_splits_them(self):
        # A no-break space is not systemd's whitespace: `@chown:EPERM<NBSP>chmod:EPERM` is one
        # word, which it drops, so neither call is refused.
        text = SHIPPED.read_text()
        self.assertEqual(text.count("~@chown:EPERM chmod:EPERM"), 1)
        found = problems(
            {
                LOCK: text.replace(
                    "~@chown:EPERM chmod:EPERM", f"~@chown:EPERM{NBSP}chmod:EPERM"
                )
            }
        )
        self.assertEqual(len(found), 2, found)
        self.assertIn(f"refuses @chown:EPERM{NBSP}chmod:EPERM,", found[0])
        self.assertIn("refusals missing: @chown, chmod", found[1])
        # Nor around a key: systemd reads `<NBSP>PrivateTmp` as a key of its own.
        found = problems({LOCK: unit(KEPT + f"{NBSP}PrivateTmp=yes\n")})
        self.assertEqual(len(found), 1, found)
        self.assertIn(f"{NBSP}PrivateTmp=yes", found[0])

    def test_a_refusal_with_an_error_number_other_than_eperm_fails(self):
        text = SHIPPED.read_text()
        for word in ("chmod:NOTANERRNO", "chmod:EACCES", "chmod:1", "chmod:"):
            with self.subTest(word=word):
                found = problems({LOCK: text.replace(" chmod:EPERM", f" {word}")})
                self.assertTrue(found, found)
                self.assertTrue(any(f"refuses {word}," in p for p in found), found)
        # The negative control: a call alone, or with :EPERM, counts.
        self.assertEqual(problems({LOCK: text.replace(" chmod:EPERM", " chmod")}), [])

    def test_exec_start_runs_one_plain_command(self):
        for prefix in ("+", "!", "!!", ":", "-", "@", "|"):
            with self.subTest(prefix=prefix):
                found = problems(
                    {LOCK: unit(KEPT).replace("ExecStart=/", f"ExecStart={prefix}/")}
                )
                self.assertEqual(len(found), 1, found)
                self.assertIn("ExecStart=", found[0])
        for dropin in (
            "[Service]\nExecStart=\nExecStart=+/usr/bin/athanor-lock\n",
            "[Service]\nExecStart=/usr/bin/other\n",
            "[Service]\nExecStart=\n",
        ):
            with self.subTest(dropin=dropin):
                found = problems(
                    {LOCK: unit(KEPT)}, {f"athanor-lock/{LOCK}.d/50-x.conf": dropin}
                )
                self.assertEqual(len(found), 1, found)
                self.assertIn("ExecStart=", found[0])
        # The negative control: a drop-in that resets the command to another plain one.
        self.assertEqual(
            problems(
                {LOCK: unit(KEPT)},
                {
                    f"athanor-lock/{LOCK}.d/50-x.conf": "[Service]\nExecStart=\nExecStart=/usr/libexec/athanor-lock\n"
                },
            ),
            [],
        )

    def test_files_that_may_ship_an_unnamed_drop_in_fail(self):
        # The spec writes its drop-in in %install, which no copy shows, and %files ships it
        # without naming it.
        install = f"%install\necho '[Service]' > %{{buildroot}}%{{_userunitdir}}/{LOCK}.d/50-x.conf\n"
        for files in (
            "%files -f list\n",
            "%files -n other -f %{name}.files\n",
            "%files\n%{_prefix}/lib/systemd/user\n",
            "%files\n%{_userunitdir}/\n",
            "%files\n/usr/lib/systemd/user/*\n",
            "%files\n%attr(0644,root,root) /usr/lib/systemd\n",
            f"%files\n/usr/lib/systemd/user/{LOCK}.d\n",
            "%files\n/usr/lib/systemd/user/service.d\n",
            "%files\n%{_libdir}/../lib/systemd/user\n",
            "%files\n%{_mydir}\n",
            "%files\n/usr\n",
        ):
            with self.subTest(files=files):
                found = problems({LOCK: unit(KEPT)}, {SPEC: install + files})
                self.assertEqual(len(found), 1, found)
                self.assertIn(SPEC, found[0])
                self.assertIn("cannot name or read", found[0])
        # The negative controls: a list %find_lang writes, a directory listed with %dir, the
        # unit itself, and paths that lead elsewhere.
        for files in (
            "%files -f polkit-1.lang\n",
            f"%files\n%dir /usr/lib/systemd/user/{LOCK}.d\n%attr(0755,root,root) %dir %{{_userunitdir}}/service.d\n",
            f"%files\n%{{_userunitdir}}/{LOCK}\n%{{_unitdir}}/x.service\n%{{_datadir}}/x/*\n%license COPYING\n",
            "%files\n%exclude /usr/lib/systemd/user\n%ghost /usr/lib/systemd/user\n%{_libexecdir}/%{name}\n",
        ):
            with self.subTest(files=files):
                self.assertEqual(
                    problems(
                        {LOCK: unit(KEPT)},
                        {SPEC: f"%install\n%find_lang polkit-1\n{files}"},
                    ),
                    [],
                )

    def test_a_drop_in_that_links_out_of_the_repository_fails(self):
        with (
            tempfile.TemporaryDirectory() as tmp,
            tempfile.TemporaryDirectory() as away,
        ):
            root, outside = pathlib.Path(tmp), pathlib.Path(away) / "50-x.conf"
            outside.write_text("[Service]\nPrivateTmp=yes\n")
            data = root / "forge/specs" / DATA
            tree = data / f"usr/lib/systemd/user/{LOCK}.d"
            tree.mkdir(parents=True)
            (tree / "50-x.conf").symlink_to(outside)
            (data / "20-x.conf").symlink_to(outside)
            (root / "forge/specs" / SPEC).write_text(
                f"%install\ninstall -D data/20-x.conf %{{buildroot}}/usr/lib/systemd/user/{LOCK}.d/20-x.conf\n"
            )
            found, dropin_problems = verify.separated_unit_dropins(
                root, {LOCK: unit(KEPT)}
            )
        self.assertEqual(found, {LOCK: []})
        self.assertEqual(len(dropin_problems), 2, dropin_problems)
        for name in ("20-x.conf", "50-x.conf"):
            self.assertTrue(
                any(
                    f"{LOCK}.d/{name} from forge/specs/{DATA}/" in p
                    and "a link that leads out of the repository" in p
                    for p in dropin_problems
                ),
                dropin_problems,
            )

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
