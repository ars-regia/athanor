"""Unit tests of the hardware matrix (python3 -B -m unittest discover -s scripts/tests -v)."""

import contextlib
import importlib.util
import os
import pathlib
import socket
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "matrix", ROOT / "scripts" / "hardware-matrix" / "matrix.py"
)
matrix = importlib.util.module_from_spec(spec)
spec.loader.exec_module(matrix)

SMALL = {
    "machines": ["desktop", "laptop"],
    "row": [
        {"id": "a", "title": "A", "check": "kernel", "machines": ["desktop", "laptop"]},
        {"id": "b", "title": "B", "steps": "do it", "machines": ["laptop"]},
    ],
    "untested": [{"id": "u", "title": "U", "reason": "none here"}],
}


def records(**rows):
    """{machine: record} from machine=[(row, result, issue)], each with evidence."""
    return {
        m: {
            "rows": {
                r: {"result": res, "evidence": "seen", **({"issue": i} if i else {})}
                for r, res, i in entries
            }
        }
        for m, entries in rows.items()
    }


class Rows(unittest.TestCase):
    def test_committed_rows_are_valid(self):
        self.assertEqual(matrix.validate(matrix.load()), [])

    def test_a_row_needs_exactly_one_of_check_and_steps(self):
        both = {**SMALL, "row": [{**SMALL["row"][0], "steps": "x"}]}
        neither = {**SMALL, "row": [{"id": "a", "title": "A", "machines": ["desktop"]}]}
        self.assertTrue(matrix.validate(both))
        self.assertTrue(matrix.validate(neither))

    def test_unknown_check_machine_duplicate_id_and_bare_untested_fail(self):
        bad = {
            **SMALL,
            "row": [
                {"id": "a", "title": "A", "check": "nope", "machines": ["desktop"]},
                {"id": "a", "title": "A", "steps": "x", "machines": ["server"]},
            ],
            "untested": [{"id": "u", "title": "U", "reason": " "}],
        }
        problems = " ".join(matrix.validate(bad))
        self.assertIn("unknown check", problems)
        self.assertIn("machines", problems)
        self.assertIn("duplicate", problems)
        self.assertIn("needs a reason", problems)


    def test_since_must_be_a_release(self):
        bad = {**SMALL, "untested": [{**SMALL["untested"][0], "since": "next"}]}
        self.assertIn("since", " ".join(matrix.validate(bad)))

    def test_an_entry_counts_from_its_since(self):
        later = {
            **SMALL,
            "row": SMALL["row"] + [{**SMALL["row"][1], "id": "c", "since": "0.10"}],
            "untested": [{**SMALL["untested"][0], "since": "0.10"}],
        }
        for release, ids in (("0.9", ["a", "b"]), ("0.10", ["a", "b", "c"]), ("1.0", ["a", "b", "c"])):
            with self.subTest(release=release):
                scoped = matrix.scoped(later, release)
                self.assertEqual([r["id"] for r in scoped["row"]], ids)
                self.assertEqual(len(scoped["untested"]), 0 if release == "0.9" else 1)

    def test_the_committed_0_9_matrix_stays_complete(self):
        # Rows added after a release must not reopen the matrix recorded for it.
        with open(os.devnull, "w") as null, contextlib.redirect_stdout(null):
            self.assertEqual(matrix.main(["summary", "0.9"]), 0)


class Summary(unittest.TestCase):
    def test_complete_when_every_row_has_a_result(self):
        text, complete = matrix.summary(
            SMALL,
            records(
                desktop=[("a", "pass", None)],
                laptop=[("a", "untested", None), ("b", "pass", None)],
            ),
        )
        self.assertTrue(complete)
        self.assertIn("| A | pass | untested |", text)
        self.assertIn("| B | n/a | pass |", text)
        self.assertIn("Untested: U: none here.", text)

    def test_a_missing_row_is_incomplete(self):
        text, complete = matrix.summary(SMALL, records(desktop=[("a", "pass", None)]))
        self.assertFalse(complete)
        self.assertIn("missing", text)

    def test_a_failure_needs_an_issue(self):
        without = records(
            desktop=[("a", "fail", None)],
            laptop=[("a", "pass", None), ("b", "pass", None)],
        )
        with_issue = records(
            desktop=[("a", "fail", 535)],
            laptop=[("a", "pass", None), ("b", "pass", None)],
        )
        self.assertFalse(matrix.summary(SMALL, without)[1])
        text, complete = matrix.summary(SMALL, with_issue)
        self.assertTrue(complete)
        self.assertIn("fail (#535)", text)

    def test_a_result_without_evidence_or_unknown_is_incomplete(self):
        self.assertEqual(
            matrix.cell({"result": "untested", "evidence": ""}),
            ("invalid (untested)", False),
        )
        self.assertEqual(
            matrix.cell({"result": "maybe", "evidence": "x"}),
            ("invalid (maybe)", False),
        )


class Checks(unittest.TestCase):
    def test_a_command_that_cannot_run_is_a_failure_not_a_pass(self):
        bus = (1, "", "Failed to connect to bus")
        with mock.patch.object(matrix, "run", return_value=bus):
            result, evidence = matrix.attempt("failed_units")
        self.assertEqual(result, "fail")
        self.assertIn("Failed to connect to bus", evidence)

    def test_a_missing_program_is_a_failure(self):
        missing = (127, "", "mokutil: not found")
        with mock.patch.object(matrix, "run", return_value=missing):
            self.assertEqual(matrix.attempt("secure_boot")[0], "fail")

    def test_secure_boot_passes_only_on_the_bare_line(self):
        for text, result in (
            ("SecureBoot enabled\n", "pass"),
            ("SecureBoot enabled\nSecureBoot validation is disabled in shim\n", "fail"),
            ("SecureBoot enabled\nPlatform is in Setup Mode\n", "fail"),
            ("SecureBoot disabled\n", "fail"),
        ):
            with mock.patch.object(matrix, "run", return_value=(0, text, "")):
                self.assertEqual(matrix.secure_boot()[0], result, text)

    def test_no_usb_device_is_not_a_pass(self):
        with (
            mock.patch.object(matrix, "run", return_value=(0, "success\n", "")),
            mock.patch.object(matrix.glob, "glob", return_value=[]),
            mock.patch.object(matrix.pathlib.Path, "exists", return_value=True),
        ):
            result, evidence = matrix.usb_internal()
        self.assertEqual(result, "fail")
        self.assertIn("0 devices", evidence)

    def test_run_drops_the_automatic_rows_of_an_earlier_run(self):
        old = {
            "rows": {
                "stale": {"result": "pass", "evidence": "old", "how": "automatic"},
                "camera": {"result": "pass", "evidence": "seen", "how": "maintainer"},
            }
        }
        written = []
        with (
            mock.patch.object(matrix, "read_record", return_value=old),
            mock.patch.object(matrix, "write_record", side_effect=lambda r, m, rec: written.append(dict(rec["rows"]))),
            mock.patch.object(matrix, "host", return_value={}),
            mock.patch.object(matrix, "attempt", return_value=("pass", "seen")),
        ):
            matrix.cmd_run(SMALL, "0.9", "desktop")
        self.assertEqual(set(written[-1]), {"a", "camera"})

    def test_an_unreadable_journal_is_not_read_as_no_denial(self):
        with mock.patch.object(matrix, "run", return_value=(0, "", "")):
            result, evidence = matrix.attempt("avc")
        self.assertEqual(result, "fail")
        self.assertIn("not readable", evidence)

    def test_grep_exit_1_with_a_message_is_an_error(self):
        answers = iter(
            [(0, "systemd started\n", ""), (1, "", "Compiled without pattern matching")]
        )
        with mock.patch.object(matrix, "run", side_effect=lambda argv: next(answers)):
            self.assertEqual(matrix.attempt("avc")[0], "fail")

    def test_no_denial_passes(self):
        answers = iter([(0, "systemd started\n", ""), (1, "", "")])
        with mock.patch.object(matrix, "run", side_effect=lambda argv: next(answers)):
            self.assertEqual(matrix.attempt("avc"), ("pass", "no denial"))

    def test_a_denial_that_does_not_parse_is_kept(self):
        line = "avc:  denied { read } garbled"
        self.assertEqual(matrix.avc_keys(line), [line])

    def test_a_connection_state_with_a_qualifier_is_up(self):
        out = "ethernet:connected (externally)\nwifi:disconnected\n"
        with mock.patch.object(matrix, "run", return_value=(0, out, "")):
            self.assertEqual(matrix.network()[0], "pass")

    SESSION = {"XDG_RUNTIME_DIR": "/run/user/1000", "WAYLAND_DISPLAY": "wayland-0"}

    def compositor(self, session, peer=None, exe=None):
        patches = [mock.patch.dict(matrix.os.environ, session, clear=True)]
        if peer is not None:
            patches.append(mock.patch.object(matrix, "peer_pid", side_effect=peer))
        if exe is not None:
            patches.append(mock.patch.object(matrix.os, "readlink", side_effect=exe))
        with contextlib.ExitStack() as stack:
            for p in patches:
                stack.enter_context(p)
            return matrix.attempt("compositor")

    def test_compositor_passes_only_for_the_own_compositor(self):
        for path, result in (
            ("/usr/bin/athanor-compositor", "pass"),
            ("/usr/bin/cosmic-comp", "fail"),
        ):
            got = self.compositor(self.SESSION, lambda s: 42, lambda p: path)
            self.assertEqual(got[0], result)
            self.assertIn(path, got[1])

    def test_compositor_without_a_wayland_socket_is_a_failure(self):
        got = self.compositor({})
        self.assertEqual(got[0], "fail")
        self.assertIn("XDG_RUNTIME_DIR and WAYLAND_DISPLAY are not set", got[1])

    def test_compositor_with_a_dead_peer_is_a_failure(self):
        gone = mock.Mock(side_effect=FileNotFoundError("/proc/42/exe"))
        got = self.compositor(self.SESSION, lambda s: 42, gone)
        self.assertEqual(got[0], "fail")
        self.assertIn("/proc/42/exe", got[1])

    def test_compositor_without_a_listener_is_a_failure(self):
        with tempfile.TemporaryDirectory() as run:
            got = self.compositor({**self.SESSION, "XDG_RUNTIME_DIR": run})
        self.assertEqual(got[0], "fail")
        self.assertIn("could not check", got[1])

    def test_peer_pid_reads_the_listener_on_a_real_socket(self):
        with tempfile.TemporaryDirectory() as run:
            path = os.path.join(run, "wayland-0")
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
                server.bind(path)
                server.listen(1)
                self.assertEqual(matrix.peer_pid(path), os.getpid())

    def test_the_bluetooth_evidence_masks_the_controller_address(self):
        out = "Controller 5C:80:B6:BA:69:DA (public)\n\tPowered: yes\n"
        with mock.patch.object(matrix, "run", return_value=(0, out, "")):
            self.assertEqual(
                matrix.bluetooth(),
                ("pass", "Controller XX:XX:XX:XX:XX:XX (public), powered yes"),
            )

    def test_an_exception_is_recorded_as_a_failure(self):
        gone = mock.Mock(side_effect=FileNotFoundError("gone"))
        with mock.patch.dict(matrix.CHECKS, {"kernel": gone}):
            self.assertEqual(
                matrix.attempt("kernel"),
                ("fail", "could not check: FileNotFoundError: gone"),
            )


class Arguments(unittest.TestCase):
    def refused(self, argv):
        with (
            self.assertRaises(SystemExit) as stop,
            mock.patch.object(matrix, "write_record") as write,
        ):
            matrix.main(argv)
        self.assertNotEqual(stop.exception.code, 0)
        write.assert_not_called()

    def test_release_and_machine_must_not_leave_the_tree(self):
        self.refused(["set", "../../../../tmp/x", "desktop", "kernel", "pass", "n"])
        self.refused(["run", "/tmp", "desktop"])
        self.refused(["run", "0.9", "../x"])
        self.refused(["run", "0.9", "desktp"])

    def test_an_issue_is_a_positive_ascii_number(self):
        for issue in ("0", "²", "12a"):
            self.refused(
                ["set", "0.9", "laptop", "camera", "fail", "black", "--issue", issue]
            )

    def test_a_note_is_required(self):
        self.refused(["set", "0.9", "laptop", "camera", "untested", " "])


if __name__ == "__main__":
    unittest.main()
