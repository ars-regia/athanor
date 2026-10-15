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


def rules(text):
    """The statements of a .te file, comments removed."""
    code = re.sub(r"#[^\n]*", "", text)
    return [s.strip() for s in re.split(r"[;]", re.sub(r"require\s*\{.*?\}", "", code, flags=re.S)) if s.strip()]


class Modules(unittest.TestCase):
    def test_spec_lists_every_te(self):
        for te in SOURCES.glob("*.te"):
            self.assertIn(te.name, SPEC, te.name)
            self.assertIn(f"packages/{te.stem}.cil", SPEC, te.name)

    def test_pstore_grants_dir_create_and_nothing_else(self):
        got = [" ".join(r.split()) for r in rules(PSTORE.read_text()) if not r.startswith("module ")]
        self.assertEqual(got, ["allow systemd_pstore_t systemd_pstore_var_lib_t:dir create"])

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
