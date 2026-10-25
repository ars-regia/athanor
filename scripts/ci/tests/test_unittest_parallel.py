"""scripts/ci/unittest_parallel.py: the parallel runner of a test directory (python3 -B -m unittest discover -s scripts/ci/tests)."""

import os
import pathlib
import subprocess
import sys
import tempfile
import textwrap
import time
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

    def run_dir(self, jobs="2", *rest):
        return subprocess.run([sys.executable, "-B", str(SCRIPT), str(self.dir), jobs, *rest], capture_output=True, text=True)

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

    def test_a_module_that_skips_itself_at_import_is_a_skip_with_its_reason(self):
        self.suite(
            a="import unittest\nclass A(unittest.TestCase):\n    def test_x(self): pass\n",
            b="import unittest\nraise unittest.SkipTest('no unix sockets here')\n",
        )
        r = self.run_dir()
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("skipped module test_b: no unix sockets here", r.stdout)

    def test_a_hung_class_is_killed_and_fails_by_name_while_the_others_finish(self):
        self.suite(
            a="import unittest\nclass Fine(unittest.TestCase):\n    def test_x(self): pass\n",
            b="import subprocess, sys, time, unittest\nclass Hung(unittest.TestCase):\n    def test_y(self):\n"
              "        helper = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])\n"
              f"        open({str(self.dir / 'helper.pid')!r}, 'w').write(str(helper.pid))\n"
              "        time.sleep(60)\n",
        )
        r = self.run_dir("2", "3")
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("TIMEOUT: test_b.Hung was killed after 3s", r.stdout)
        self.assertIn("failed: test_b.Hung", r.stdout)
        pid = int((self.dir / "helper.pid").read_text())
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                break
            time.sleep(0.05)
        else:
            os.kill(pid, 9)
            self.fail("the helper a hung class spawned outlived the runner")


if __name__ == "__main__":
    unittest.main()
