"""scripts/ci/unittest_parallel.py: the parallel runner of a test directory (python3 -B -m unittest discover -s scripts/ci/tests)."""

import pathlib
import subprocess
import sys
import tempfile
import textwrap
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "unittest_parallel.py"


class Parallel(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = pathlib.Path(self.tmp.name)

    def suite(self, **modules):
        for name, body in modules.items():
            (self.dir / f"test_{name}.py").write_text(textwrap.dedent(body))

    def run_dir(self, jobs="2"):
        return subprocess.run([sys.executable, "-B", str(SCRIPT), str(self.dir), jobs], capture_output=True, text=True)

    def test_every_class_of_every_module_runs_and_a_green_suite_passes(self):
        self.suite(
            a="import unittest\nclass A(unittest.TestCase):\n    def test_x(self): pass\nclass B(unittest.TestCase):\n    def test_y(self): pass\n",
            b="import unittest\nclass C(unittest.TestCase):\n    def test_z(self): pass\n",
        )
        r = self.run_dir()
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        for name in ("test_a.A", "test_a.B", "test_b.C"):
            self.assertIn(f"=== {name}", r.stdout)
        self.assertIn("3 classes", r.stdout)

    def test_a_failing_test_fails_the_run_and_names_its_class(self):
        self.suite(
            a="import unittest\nclass Good(unittest.TestCase):\n    def test_x(self): pass\n",
            b="import unittest\nclass Bad(unittest.TestCase):\n    def test_y(self): self.fail('no')\n",
        )
        r = self.run_dir()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("failed: test_b.Bad", r.stdout)

    def test_a_module_that_cannot_be_imported_fails_the_run(self):
        self.suite(a="import unittest\nclass A(unittest.TestCase):\n    def test_x(self): pass\n", b="import not_a_module_anywhere\n")
        r = self.run_dir()
        self.assertEqual(r.returncode, 1)
        self.assertIn("cannot load", r.stderr)

    def test_a_directory_with_no_test_fails_rather_than_passing_empty(self):
        self.assertEqual(self.run_dir().returncode, 1)

    def test_a_test_may_import_a_helper_beside_it(self):
        (self.dir / "helper.py").write_text("VALUE = 3\n")
        self.suite(a="import unittest\nimport helper\nclass A(unittest.TestCase):\n    def test_x(self): self.assertEqual(helper.VALUE, 3)\n")
        r = self.run_dir()
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)


if __name__ == "__main__":
    unittest.main()
