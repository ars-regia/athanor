"""check_rpm_sonames.sh fails a built RPM whose versioned soname nothing provides, and
run_spec_build.sh runs it after every spec build in the pinned Fedora image (issue 460;
python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import os
import pathlib
import re
import subprocess
import tempfile
import unittest

SCRIPTS = pathlib.Path(__file__).resolve().parents[1]
CHECK = SCRIPTS / "check_rpm_sonames.sh"
ROOT = SCRIPTS.parents[1]

# rpm and dnf5 as the Fedora container answers them: an RPM's provides and requires come
# from FILE.provides and FILE.requires, the container's packages from $STUB/installed, the
# repositories from $STUB/repo.
RPM = """#!/usr/bin/env bash
case "$1 $2" in
"-qp --provides") shift 2; for f; do cat "$f.provides"; done ;;
"-qp --requires") cat "$3.requires" ;;
"-q --quiet") grep -qxF -- "$4" "$STUB/installed" ;;
*) echo "rpm stub: unexpected $*" >&2; exit 2 ;;
esac
"""
DNF5 = """#!/usr/bin/env bash
case "$2" in
makecache) exit "${STUB_MAKECACHE:-0}" ;;
repoquery) [[ ${STUB_QUERY:-0} -eq 0 ]] || exit "$STUB_QUERY"; grep -xF -- "$4" "$STUB/repo" | sed 's/^/provider-of-/' ;;
*) echo "dnf5 stub: unexpected $*" >&2; exit 2 ;;
esac
"""


class CheckRpmSonamesTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = pathlib.Path(tmp.name)
        for name, body in (("rpm", RPM), ("dnf5", DNF5)):
            (self.dir / name).write_text(body)
            (self.dir / name).chmod(0o755)
        (self.dir / "installed").write_text(
            "libc.so.6()(64bit)\nlibc.so.6(GLIBC_2.34)(64bit)\n"
        )
        (self.dir / "repo").write_text(
            "libsqlite3.so.0()(64bit)\nlibdisplay-info.so.2()(64bit)\n"
        )

    def rpm(self, name, requires, provides=()):
        path = self.dir / f"{name}.rpm"
        path.write_text("")
        pathlib.Path(f"{path}.requires").write_text("".join(f"{r}\n" for r in requires))
        pathlib.Path(f"{path}.provides").write_text("".join(f"{p}\n" for p in provides))
        return str(path)

    def check(self, *rpms, **env):
        return subprocess.run(
            ["bash", CHECK, *rpms],
            capture_output=True,
            text=True,
            env={
                **os.environ,
                "PATH": f"{self.dir}:{os.environ['PATH']}",
                "STUB": str(self.dir),
                "RETRY_ATTEMPTS": "1",
                **env,
            },
        )

    def test_provided_by_the_container_the_repositories_or_the_build_passes(self):
        lib = self.rpm(
            "athanor-lib", ["libc.so.6()(64bit)"], ["libathanor.so.1()(64bit)"]
        )
        app = self.rpm(
            "athanor-app",
            [
                "libc.so.6(GLIBC_2.34)(64bit)",
                "libsqlite3.so.0()(64bit)",
                "libathanor.so.1()(64bit)",
                "athanor-lib >= 1.0",
                "/bin/sh",
            ],
        )
        result = self.check(lib, app)
        self.assertEqual(0, result.returncode, result.stderr)

    def test_a_versioned_soname_nothing_provides_fails_naming_package_and_requirement(
        self,
    ):
        app = self.rpm(
            "athanor-app", ["libc.so.6()(64bit)", "libdisplay-info.so.3()(64bit)"]
        )
        result = self.check(app)
        self.assertEqual(1, result.returncode)
        self.assertIn(
            "athanor-app.rpm requires libdisplay-info.so.3()(64bit)", result.stderr
        )

    def test_every_package_is_checked(self):
        good = self.rpm("athanor-good", ["libsqlite3.so.0()(64bit)"])
        bad = self.rpm("athanor-bad", ["libfoo.so.7()(64bit)"])
        result = self.check(bad, good)
        self.assertEqual(1, result.returncode)
        self.assertIn("athanor-bad.rpm requires libfoo.so.7()(64bit)", result.stderr)

    def test_a_failing_query_fails_rather_than_passes(self):
        app = self.rpm("athanor-app", ["libsqlite3.so.0()(64bit)"])
        self.assertNotEqual(0, self.check(app, STUB_QUERY="1").returncode)
        self.assertNotEqual(0, self.check(app, STUB_MAKECACHE="1").returncode)

    def test_unversioned_sonames_are_left_to_check_sonames(self):
        app = self.rpm("athanor-app", ["libsqlite3.so()(64bit)"])
        self.assertEqual(0, self.check(app).returncode)


class RunSpecBuildTest(unittest.TestCase):
    def test_checks_the_build_in_the_fedora_image_system_containerfile_pins(self):
        with tempfile.TemporaryDirectory() as tmp:
            stub = pathlib.Path(tmp) / "podman"
            stub.write_text(
                '#!/usr/bin/env bash\nprintf "%s\\n" "$*" >> "$STUB_LOG"\n'
                "[[ $1 == volume && $2 == create ]] && echo vol\nexit 0\n"
            )
            stub.chmod(0o755)
            log = pathlib.Path(tmp) / "calls"
            result = subprocess.run(
                [
                    "bash",
                    SCRIPTS / "run_spec_build.sh",
                    "builder:x",
                    "specs/athanor-bar",
                ],
                capture_output=True,
                text=True,
                env={
                    **os.environ,
                    "PATH": f"{tmp}:{os.environ['PATH']}",
                    "STUB_LOG": str(log),
                },
            )
            self.assertEqual(0, result.returncode, result.stderr)
            pinned = re.search(
                r"^FROM (\S*/fedora:\d+@sha256:[0-9a-f]{64})",
                (ROOT / "system" / "Containerfile").read_text(),
                re.M,
            ).group(1)
            runs = [
                line for line in log.read_text().splitlines() if line.startswith("run ")
            ]
            self.assertEqual(3, len(runs), runs)
            self.assertIn("build_spec.sh build", runs[1])
            self.assertIn(pinned, runs[2])
            self.assertIn("check_rpm_sonames.sh /root/rpmbuild/RPMS/*/*.rpm", runs[2])


if __name__ == "__main__":
    unittest.main()
