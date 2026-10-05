"""Unit test of the session memory measurement (python3 -B -m unittest discover -s scripts/session-memory/tests -v)."""

import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[1] / "forge" / "test" / "iso"))
import memory_case  # noqa: E402
import pss  # noqa: E402
import report  # noqa: E402


def fake_system(base, procs):
    """procs: pid -> (cgroup path under the root, comm, pss kB or None for 'ended')."""
    root, proc = base / "cgroup", base / "proc"
    (proc / "self").mkdir(parents=True)
    (proc / "self" / "cgroup").write_text(
        "0::/user.slice/user-1000.slice/session-c1.scope\n"
    )
    for pid, (cg, comm, kb) in procs.items():
        group = root / cg
        group.mkdir(parents=True, exist_ok=True)
        with open(group / "cgroup.procs", "a") as f:
            f.write(f"{pid}\n")
        if kb is None:
            continue
        (proc / str(pid)).mkdir(parents=True)
        (proc / str(pid) / "comm").write_text(comm + "\n")
        (proc / str(pid) / "smaps_rollup").write_text(
            f"Rss: 9 kB\nPss:   {kb} kB\nPss_Anon: 1 kB\n"
        )
    return root, proc


class Collect(unittest.TestCase):
    def test_sums_the_users_slice_and_skips_the_asking_scope_and_ended_processes(self):
        with tempfile.TemporaryDirectory() as tmp:
            u = "user.slice/user-1000.slice"
            root, proc = fake_system(
                pathlib.Path(tmp),
                {
                    10: (
                        f"{u}/user@1000.service/app.slice/bar.service",
                        "athanor-bar",
                        100,
                    ),
                    11: (
                        f"{u}/user@1000.service/session.slice/comp.service",
                        "cosmic-comp",
                        250,
                    ),
                    12: (
                        f"{u}/session-c1.scope",
                        "bash",
                        9999,
                    ),  # the serial login that asked
                    13: (f"{u}/user@1000.service/app.slice/gone.service", "gone", None),
                    14: (
                        "user.slice/user-1001.slice/user@1001.service",
                        "other-user",
                        777,
                    ),
                },
            )
            data = pss.collect(uid=1000, root=root, proc=proc)
        self.assertEqual(data["pss_kb"], 350)
        self.assertEqual(
            {p["comm"] for p in data["processes"]}, {"athanor-bar", "cosmic-comp"}
        )
        self.assertEqual(data["unreadable"], 0)


class Report(unittest.TestCase):
    LOG = (
        "boot noise\r\nPSS_REPORT "
        + json.dumps(
            {
                "uid": 1000,
                "pss_kb": 512 * 1024,
                "unreadable": 1,
                "processes": [
                    {"pid": 1, "comm": "a", "unit": "u", "pss_kb": 512 * 1024}
                ],
            }
        )
        + "\r\n"
    )

    def test_without_a_budget_it_reports_and_says_so(self):
        r = report.build(self.LOG, None)
        self.assertEqual((r["pss_mb"], r["verdict"]), (512.0, "no-budget"))
        self.assertIn("does not fail", report.summary(r))

    def test_a_budget_is_compared(self):
        self.assertEqual(report.build(self.LOG, 600)["verdict"], "within")
        self.assertEqual(report.build(self.LOG, 500)["verdict"], "over")

    def test_no_report_line_is_no_measurement(self):
        r = report.build("nothing here\n", 500)
        self.assertEqual(r["verdict"], "no-measurement")

    def test_the_budget_file_without_the_figure_is_no_budget(self):
        with tempfile.TemporaryDirectory() as tmp:
            f = pathlib.Path(tmp, "b.json")
            self.assertIsNone(report.budget_mb(f))  # no file
            f.write_text("{}")
            self.assertIsNone(report.budget_mb(f))
            f.write_text('{"max_pss_mb": 900}')
            self.assertEqual(report.budget_mb(f), 900.0)

    def test_exit_status_fails_only_when_over_budget(self):
        with tempfile.TemporaryDirectory() as tmp:
            log, out, budget = (
                pathlib.Path(tmp, n)
                for n in ("serial.log", "memory.json", "budget.json")
            )
            log.write_text(self.LOG)
            budget.write_text("{}")
            run = lambda: (
                subprocess.run(
                    [
                        sys.executable,
                        str(HERE / "report.py"),
                        str(log),
                        str(out),
                        str(budget),
                    ],
                    capture_output=True,
                ).returncode
            )
            self.assertEqual(run(), 0)
            budget.write_text('{"max_pss_mb": 100}')
            self.assertEqual(run(), 1)
            self.assertEqual(json.loads(out.read_text())["verdict"], "over")


class Probe(unittest.TestCase):
    def test_the_command_fits_a_tty_line_and_runs_the_script(self):
        text = memory_case.command()
        self.assertLessEqual(len(text), memory_case.MAX_COMMAND)
        # The command must not contain the marker, or the echo of it would read as a report.
        self.assertNotIn(b"PSS_REPORT", text)


if __name__ == "__main__":
    unittest.main()
