"""Unit tests of the panics check of `verify.py`: a module declared `#[cfg(test)] mod name;`
is test code, as a `#[cfg(test)]` block is (python3 -B -m unittest discover -s scripts/tests -v)."""

import importlib.util
import pathlib
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "verify.py"
spec = importlib.util.spec_from_file_location("verify", SCRIPT)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class TestModuleFilesTest(unittest.TestCase):
    def test_only_modules_declared_under_cfg_test_are_test_code(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = pathlib.Path(tmp)
            (src / "lib.rs").write_text("pub mod model;\n#[cfg(test)]\nmod fixture;\n")
            (src / "model.rs").write_text("#[cfg(test)] pub(crate) mod helper;\n")
            found = verify.test_module_files([src / "lib.rs", src / "model.rs"])
        self.assertIn(src / "fixture.rs", found)
        self.assertIn(src / "model" / "helper.rs", found)
        self.assertNotIn(src / "model.rs", found)


if __name__ == "__main__":
    unittest.main()
