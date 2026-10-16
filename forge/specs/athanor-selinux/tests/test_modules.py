"""Checks of the athanor-selinux sources
(python3 -B -m unittest discover -s forge/specs/athanor-selinux/tests -v)."""

import pathlib
import re
import shutil
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parents[1]
SOURCES = HERE / "SOURCES"
SPEC = (HERE / "athanor-selinux.spec").read_text()
PSTORE = SOURCES / "athanor_systemd_pstore.te"
PCSCD = SOURCES / "athanor_pcscd_cgroup.te"


def rules(text):
    """The statements of a .te file, comments and the require block removed. The block ends
    at the first line that starts with a closing brace: a class inside it has braces too."""
    code = re.sub(r"#[^\n]*", "", text)
    return [s.strip() for s in re.split(r"[;]", re.sub(r"require\s*\{.*?^\}", "", code, flags=re.S | re.M)) if s.strip()]


class Modules(unittest.TestCase):
    def test_spec_builds_and_ships_every_te(self):
        loop = re.search(r"^for module in ([^;]+); do$", SPEC, re.M)
        self.assertIsNotNone(loop, "the spec has no checkmodule loop")
        for te in SOURCES.glob("*.te"):
            self.assertRegex(SPEC, rf"(?m)^Source\d+:\s+{re.escape(te.name)}$")
            self.assertIn(te.stem, loop.group(1).split(), te.name)
            self.assertRegex(SPEC, rf"(?m)^install .* {re.escape(te.stem)}\.cil %{{buildroot}}\S*/packages/{re.escape(te.stem)}\.cil$")
            self.assertRegex(SPEC, rf"(?m)^%{{_datadir}}/selinux/packages/{re.escape(te.stem)}\.cil$")

    def test_pstore_grants_dir_create_and_nothing_else(self):
        got = [" ".join(r.split()) for r in rules(PSTORE.read_text()) if not r.startswith("module ")]
        self.assertEqual(got, ["allow systemd_pstore_t systemd_pstore_var_lib_t:dir create"])

    def test_pcscd_grants_cgroup_dir_search_and_nothing_else(self):
        got = [" ".join(r.split()) for r in rules(PCSCD.read_text()) if not r.startswith("module ")]
        self.assertEqual(got, ["allow pcscd_t cgroup_t:dir { getattr search open }"])

    # A local aid: in CI the rpmbuild of spec-build-check.yml compiles every module.
    @unittest.skipUnless(shutil.which("checkmodule"), "checkmodule is not installed")
    def test_every_te_compiles(self):
        with tempfile.TemporaryDirectory() as tmp:
            for te in SOURCES.glob("*.te"):
                out = pathlib.Path(tmp) / (te.stem + ".cil")
                subprocess.run(["checkmodule", "-M", "-m", "-C", "-o", str(out), str(te)], check=True)
                if te == PSTORE:
                    cil = " ".join(out.read_text().split())
                    self.assertIn("(allow systemd_pstore_t systemd_pstore_var_lib_t (dir (create)))", cil)
                    self.assertEqual(cil.count("(allow "), 1)


if __name__ == "__main__":
    unittest.main()
