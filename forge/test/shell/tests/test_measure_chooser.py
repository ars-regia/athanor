import os
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import measure_chooser


def first_frame(lines):
    frame = measure_chooser.FirstFrame()
    return [n for n, line in enumerate(lines) if frame.feed(line)][:1]


class FirstFrameTest(unittest.TestCase):
    def test_libwayland_trace_window_commit_after_a_buffer(self):
        lines = [
            "[1234567.001] {Default Queue}  -> wl_compositor@4.create_surface(new id wl_surface@20)",
            "[1234567.002] {Default Queue}  -> xdg_wm_base@8.get_xdg_surface(new id xdg_surface@21, wl_surface@20)",
            "[1234567.003] {Default Queue}  -> wl_surface@20.commit()",
            "[1234567.004] {Default Queue} xdg_surface@21.configure(1)",
            "[1234567.005] {Default Queue}  -> wl_surface@20.attach(wl_buffer@35, 0, 0)",
            "[1234567.006] {Default Queue}  -> wl_surface@20.commit()",
        ]
        self.assertEqual(first_frame(lines), [5])

    def test_libwayland_1_24_trace_names_objects_with_a_hash(self):
        lines = [
            "[10:00:00.105323] {Default Queue}  -> xdg_wm_base#33.get_xdg_surface(new id xdg_surface#42, wl_surface#39)",
            "[10:00:00.105359]  -> wl_surface#39.commit()",
            "[10:00:00.123802]  -> wl_surface#39.attach(wl_buffer#46, 0, 0)",
            "[10:00:00.123827]  -> wl_surface#39.commit()",
        ]
        self.assertEqual(first_frame(lines), [3])

    def test_wayland_backend_trace(self):
        lines = [
            "[ 12.000000] -> xdg_wm_base@9.get_xdg_surface(xdg_surface@12, wl_surface@11)",
            "[ 12.100000] -> wl_surface@11.attach(wl_buffer@14, 0, 0)",
            "[ 12.200000] -> wl_surface@11.commit()",
        ]
        self.assertEqual(first_frame(lines), [2])

    def test_another_surface_or_a_null_buffer_is_not_the_first_frame(self):
        lines = [
            "[1.0]  -> xdg_wm_base@8.get_xdg_surface(new id xdg_surface@21, wl_surface@20)",
            "[1.1]  -> wl_surface@30.attach(wl_buffer@35, 0, 0)",
            "[1.2]  -> wl_surface@30.commit()",
            "[1.3]  -> wl_surface@20.attach(nil, 0, 0)",
            "[1.4]  -> wl_surface@20.commit()",
            "[1.5]  -> wl_surface@20.commit()",
        ]
        self.assertEqual(first_frame(lines), [])

    def test_events_are_not_requests(self):
        lines = [
            "[1.0]  -> xdg_wm_base@8.get_xdg_surface(new id xdg_surface@21, wl_surface@20)",
            "[1.1] wl_surface@20.attach(wl_buffer@35, 0, 0)",
            "[1.2]  -> wl_surface@20.commit()",
        ]
        self.assertEqual(first_frame(lines), [])

    def test_the_first_xdg_surface_is_the_window(self):
        lines = [
            "[1.0]  -> xdg_wm_base@8.get_xdg_surface(new id xdg_surface@21, wl_surface@20)",
            "[1.1]  -> xdg_wm_base@8.get_xdg_surface(new id xdg_surface@31, wl_surface@30)",
            "[1.2]  -> wl_surface@30.attach(wl_buffer@35, 0, 0)",
            "[1.3]  -> wl_surface@30.commit()",
            "[1.4]  -> wl_surface@20.attach(wl_buffer@36, 0, 0)",
            "[1.5]  -> wl_surface@20.commit()",
        ]
        self.assertEqual(first_frame(lines), [5])

    def test_discarded_messages_are_ignored(self):
        lines = [
            "[1.0]  -> xdg_wm_base#8.get_xdg_surface(new id xdg_surface#21, wl_surface#20)",
            "[1.1]  -> wl_surface#20.attach(wl_buffer#35, 0, 0)",
            "[1.2] {Default Queue} [discarded]  -> wl_surface#20.commit()",
            "[1.3] [discarded]  -> wl_surface#20.commit()",
        ]
        self.assertEqual(first_frame(lines), [])

    def test_the_window_buffer_size_from_shm_or_dmabuf(self):
        for create, size in [
            (
                "wl_shm_pool#12.create_buffer(new id wl_buffer#35, 0, 448, 326, 1792, 0)",
                (448, 326),
            ),
            (
                "wl_shm_pool@12.create_buffer(wl_buffer@35, 4096, 420, 293, 1680, 0)",
                (420, 293),
            ),
            (
                "zwp_linux_buffer_params_v1#40.create_immed(new id wl_buffer#35, 840, 586, 875708993, 0)",
                (840, 586),
            ),
        ]:
            frame = measure_chooser.FirstFrame()
            for line in [
                "[1.0]  -> xdg_wm_base#8.get_xdg_surface(new id xdg_surface#21, wl_surface#20)",
                f"[1.1]  -> {create}",
                "[1.2]  -> wl_surface#20.attach(wl_buffer#35, 0, 0)",
                "[1.3]  -> wl_surface#20.commit()",
            ]:
                frame.feed(line)
            self.assertEqual((frame.done, frame.size), (True, size), create)


class ProcessTest(unittest.TestCase):
    def test_a_client_that_exits_is_reported_at_once(self):
        start = time.monotonic()
        with self.assertRaisesRegex(
            RuntimeError, "exited with 3 before its first frame"
        ):
            measure_chooser.first_frame(["sh", "-c", "exit 3"], timeout=20)
        self.assertLess(time.monotonic() - start, 10)

    def test_a_client_that_ignores_terminate_is_killed(self):
        proc = measure_chooser.spawn(["sh", "-c", "trap '' TERM; sleep 60"], os.environ)
        time.sleep(0.2)  # let the shell install its trap
        start = time.monotonic()
        measure_chooser.stop(proc, grace=0.5)
        self.assertEqual(proc.returncode, -9)
        self.assertLess(time.monotonic() - start, 5)

    def test_a_client_that_exits_before_the_memory_reading_is_reported(self):
        with self.assertRaisesRegex(
            RuntimeError, "exited with 0 before the memory reading"
        ):
            measure_chooser.resident(["true"], after=10)

    def test_shared_objects_are_counted_once_per_file_and_deleted_ones_left_out(self):
        with tempfile.TemporaryDirectory() as root:
            lib = Path(root) / "libx.so.1"
            lib.write_bytes(b"x" * 10)
            (Path(root) / "libx.so").symlink_to(lib.name)
            maps = "\n".join(
                [
                    f"7f00-7f01 r--p 00000000 00:1f 42 {lib}",
                    f"7f01-7f02 r-xp 00001000 00:1f 42 {lib}",
                    f"7f02-7f03 r--p 00000000 00:1f 42 {Path(root) / 'libx.so'}",
                    f"7f03-7f04 r--p 00000000 00:1f 43 {Path(root) / 'liby.so'} (deleted)",
                    "7f04-7f05 rw-p 00000000 00:00 0 [heap]",
                ]
            )
            self.assertEqual(list(measure_chooser.shared_objects(maps).values()), [10])


class CountTest(unittest.TestCase):
    def test_code_comments_blanks_and_tests(self):
        source = (
            "//! Module doc.\n"
            "use std::fmt;\n"
            "\n"
            "/* a block\n"
            "   still the block */\n"
            "fn f() {} // trailing comment is code\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    /// doc\n"
            "    fn t() {}\n"
            "}\n"
        )
        self.assertEqual(measure_chooser.count(source), (6, 4, 1, 4))

    def test_an_unplaced_file_is_an_error(self):
        with tempfile.TemporaryDirectory() as root:
            (Path(root) / "src").mkdir()
            (Path(root) / "src" / "new.rs").write_text("fn f() {}\n", encoding="utf-8")
            with self.assertRaises(SystemExit):
                measure_chooser.lines(root)


if __name__ == "__main__":
    unittest.main()
