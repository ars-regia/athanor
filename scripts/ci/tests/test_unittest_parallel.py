"""scripts/ci/unittest_parallel.py: the parallel runner of a test directory (python3 -B -m unittest discover -s scripts/ci/tests)."""

import os
import pathlib
import signal
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

    def hung_suite(self, escape=False):
        """A suite with a class that hangs after starting a helper. The helper's pid and the
        test process's are written atomically to files beside the suite."""
        self.suite(
            a="import unittest\nclass Fine(unittest.TestCase):\n    def test_x(self): pass\n",
            b=textwrap.dedent(f"""
                import os, subprocess, sys, time, unittest
                def put(name, pid):
                    tmp = {str(self.dir)!r} + "/" + name + ".tmp"
                    open(tmp, "w").write(str(pid))
                    os.replace(tmp, {str(self.dir)!r} + "/" + name)
                class Hung(unittest.TestCase):
                    def test_y(self):
                        helper = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(61.5)  # lint-speed-helper"],
                                                  start_new_session={escape!r})
                        put("helper.pid", helper.pid)
                        put("test.pid", os.getpid())
                        time.sleep(60)
            """),
        )

    def pid(self, name, wait=10):
        path = self.dir / name
        deadline = time.monotonic() + wait
        while not path.exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertTrue(path.exists(), f"{name} never appeared")
        return int(path.read_text())

    def kill_helper(self):
        """Cleanup that tolerates a helper.pid that was never written."""
        try:
            pid = int((self.dir / "helper.pid").read_text())
        except (FileNotFoundError, ValueError):
            return
        if not self.is_dead(pid):
            os.kill(pid, 9)

    @staticmethod
    def is_dead(pid):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return True
        try:
            return pathlib.Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0] == "Z"
        except FileNotFoundError:
            return True

    def assert_gone(self, pid, what):
        deadline = time.monotonic() + 5
        while not self.is_dead(pid) and time.monotonic() < deadline:
            time.sleep(0.05)
        if not self.is_dead(pid):
            os.kill(pid, 9)
            self.fail(f"{what} outlived the runner")

    def test_a_hung_class_is_killed_and_fails_by_name_while_the_others_finish(self):
        self.hung_suite()
        r = self.run_dir("2", "6")
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("TIMEOUT: test_b.Hung was killed after 6s", r.stdout)
        self.assertIn("failed: test_b.Hung", r.stdout)
        self.assert_gone(self.pid("helper.pid"), "the helper a hung class spawned")

    def test_a_helper_that_left_the_group_and_holds_the_pipe_does_not_stall_the_report(self):
        self.hung_suite(escape=True)
        self.addCleanup(self.kill_helper)
        started = time.monotonic()
        r = self.run_dir("2", "6")
        took = time.monotonic() - started
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("TIMEOUT: test_b.Hung was killed after 6s", r.stdout)
        self.assertLess(took, 6 + 5, f"the runner stalled for {took:.1f}s with a helper holding its output")

    def interrupted(self, sig, burst=1):
        self.hung_suite()
        runner = subprocess.Popen([sys.executable, "-B", str(SCRIPT), str(self.dir), "2", "300"],
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        # Cleanups run last in, first out: these run before the runner is killed, so a failure
        # leaves nothing alive.
        self.addCleanup(runner.kill)
        self.addCleanup(self.kill_helper)
        self.addCleanup(self.kill_test)
        helper, test = self.pid("helper.pid"), self.pid("test.pid")
        for _ in range(burst):
            runner.send_signal(sig)
        try:
            runner.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            self.fail(f"the runner did not end on {sig.name}")
        self.assertNotEqual(runner.returncode, 0)
        self.assert_gone(test, "the hung test process")
        self.assert_gone(helper, "the helper it spawned")

    def kill_test(self):
        try:
            pid = int((self.dir / "test.pid").read_text())
        except (FileNotFoundError, ValueError):
            return
        if not self.is_dead(pid):
            os.kill(pid, 9)

    def test_an_interrupt_kills_the_running_classes_and_ends_the_runner_promptly(self):
        self.interrupted(signal.SIGINT)

    def test_sigterm_of_a_cancelled_job_cleans_up_too(self):
        self.interrupted(signal.SIGTERM)

    def test_sighup_cleans_up_too(self):
        self.interrupted(signal.SIGHUP)

    def test_a_burst_of_signals_cannot_cut_the_cleanup_short(self):
        self.interrupted(signal.SIGHUP, burst=50)

    def test_a_class_that_exits_by_itself_leaves_no_descendant_behind(self):
        self.suite(
            a=textwrap.dedent(f"""
                import os, subprocess, sys, unittest
                class Leaves(unittest.TestCase):
                    def test_x(self):
                        helper = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(37.25)"])
                        tmp = {str(self.dir)!r} + "/helper.tmp"
                        open(tmp, "w").write(str(helper.pid))
                        os.replace(tmp, {str(self.dir)!r} + "/helper.pid")
            """),
        )
        self.addCleanup(self.kill_helper)
        r = self.run_dir()
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assert_gone(self.pid("helper.pid"), "the descendant of a class that exited 0")

if __name__ == "__main__":
    unittest.main()
