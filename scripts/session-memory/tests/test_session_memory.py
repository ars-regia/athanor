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

U = "user.slice/user-1000.slice"


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
            root, proc = fake_system(
                pathlib.Path(tmp),
                {
                    10: (
                        f"{U}/user@1000.service/app.slice/bar.service",
                        "athanor-bar",
                        100,
                    ),
                    11: (
                        f"{U}/user@1000.service/session.slice/comp.service",
                        "cosmic-comp",
                        250,
                    ),
                    12: (
                        f"{U}/session-c1.scope",
                        "bash",
                        9999,
                    ),  # the serial login that asked
                    13: (f"{U}/user@1000.service/app.slice/gone.service", "gone", None),
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

    def test_a_cgroup_that_vanishes_after_the_walk_is_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, proc = fake_system(
                pathlib.Path(tmp), {10: (f"{U}/user@1000.service/a.service", "a", 40)}
            )
            gone = root / U / "user@1000.service" / "transient.scope"
            gone.mkdir()
            (gone / "cgroup.procs").symlink_to(
                root / "nowhere"
            )  # listed by the walk, unreadable
            data = pss.collect(uid=1000, root=root, proc=proc)
        self.assertEqual(data["pss_kb"], 40)


def block(data):
    return pss.frame(data) + "\n"


DATA = {
    "uid": 1000,
    "pss_kb": 512 * 1024,
    "unreadable": 1,
    "processes": [{"pid": 1, "comm": "a", "unit": "u", "pss_kb": 512 * 1024}],
}


class Report(unittest.TestCase):
    LOG = "boot noise\r\n" + block(DATA).replace("\n", "\r\n")

    def test_without_a_budget_it_reports_and_says_so(self):
        r = report.build(self.LOG, None)
        self.assertEqual((r["pss_mb"], r["verdict"]), (512.0, "no-budget"))
        self.assertIn("does not fail", report.summary(r))

    def test_a_budget_is_compared(self):
        self.assertEqual(report.build(self.LOG, 600)["verdict"], "within")
        self.assertEqual(report.build(self.LOG, 500)["verdict"], "over")

    def test_a_damaged_copy_is_skipped_for_an_intact_one(self):
        damaged = block(DATA).replace("PSS_D 0 ", "PSS_D 0 kernel: noise ", 1)
        self.assertEqual(report.build(damaged + block(DATA), None)["pss_mb"], 512.0)

    def test_a_chunk_cut_by_console_noise_is_corrupt_not_a_crash(self):
        lines = block(DATA).splitlines()
        cut = "\n".join(lines[:1] + [lines[1][:-9] + "[ 12.3] usb"] + lines[2:])
        r = report.build(cut, None)
        self.assertEqual((r["measured"], r["verdict"]), (False, "corrupt"))

    def test_a_block_with_a_wrong_checksum_is_corrupt(self):
        head, rest = block(DATA).split("\n", 1)
        wrong = head.rsplit(" ", 1)[0] + " 00000000\n" + rest
        self.assertEqual(report.build(wrong, None)["verdict"], "corrupt")
        garbled = block(DATA).replace(
            "PSS_BEGIN", "PSS_BEGIN 1 ", 1
        )  # the start line itself is damaged
        self.assertEqual(report.build(garbled, None)["verdict"], "corrupt")

    def test_no_block_is_no_measurement(self):
        self.assertEqual(
            report.build("nothing here\n", 500)["verdict"], "no-measurement"
        )

    def test_the_budget_file_without_the_figure_is_no_budget(self):
        with tempfile.TemporaryDirectory() as tmp:
            f = pathlib.Path(tmp, "b.json")
            self.assertIsNone(report.budget_mb(f))  # no file
            f.write_text("{}")
            self.assertIsNone(report.budget_mb(f))
            f.write_text('{"max_pss_mb": 900}')
            self.assertEqual(report.budget_mb(f), 900.0)

    def status(self, log_text, budget_text):
        with tempfile.TemporaryDirectory() as tmp:
            log, out, budget = (
                pathlib.Path(tmp, n)
                for n in ("serial.log", "memory.json", "budget.json")
            )
            log.write_text(log_text)
            budget.write_text(budget_text)
            cmd = [
                sys.executable,
                str(HERE / "report.py"),
                str(log),
                str(out),
                str(budget),
            ]
            code = subprocess.run(cmd, capture_output=True).returncode
            return code, json.loads(out.read_text())["verdict"]

    def test_exit_status_without_a_budget_is_always_zero(self):
        for text in (self.LOG, "nothing here\n", block(DATA)[:-20]):
            self.assertEqual(self.status(text, "{}")[0], 0)

    def test_exit_status_with_a_budget_fails_on_over_and_on_a_lost_report(self):
        self.assertEqual(self.status(self.LOG, '{"max_pss_mb": 900}'), (0, "within"))
        self.assertEqual(self.status(self.LOG, '{"max_pss_mb": 100}'), (1, "over"))
        self.assertEqual(
            self.status("nothing here\n", '{"max_pss_mb": 900}'), (1, "no-measurement")
        )
        self.assertEqual(
            self.status(block(DATA)[:-20], '{"max_pss_mb": 900}'), (1, "corrupt")
        )


class Probe(unittest.TestCase):
    def test_the_command_fits_a_tty_line_and_runs_the_script(self):
        text = memory_case.command()
        self.assertLessEqual(len(text), memory_case.MAX_COMMAND)
        # The command must not carry the markers, or the console's echo of it would read as a report.
        self.assertNotIn(b"PSS_BEGIN", text)


if __name__ == "__main__":
    unittest.main()
