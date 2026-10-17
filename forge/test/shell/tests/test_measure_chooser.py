import sys
import tempfile
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
