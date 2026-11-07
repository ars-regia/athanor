import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from toolkit_motion_check import frames, problems, settled

TRACE = """\
[1.0]  -> zwlr_layer_shell_v1#10.get_layer_surface(new id zwlr_layer_surface_v1#30, wl_surface#29, nil, 2, "athanor-sal-motion")
[1.1]  -> wl_surface#29.frame(new id wl_callback#31)
[1.2]  -> wl_surface#28.frame(new id wl_callback#32)
[1.3]  -> wl_surface#29.frame(new id wl_callback#33)
""".splitlines()


class TestToolkitMotionCheck(unittest.TestCase):
    def test_frames_count_on_the_surface_of_the_namespace_only(self):
        self.assertEqual(frames(TRACE, "athanor-sal-motion"), 2)
        self.assertEqual(frames(TRACE, "other"), 0)

    def test_settled_reads_the_lines_of_the_client(self):
        lines = ["entered in 216", "left in 150", "entered reduced in 117", "noise"]
        self.assertEqual(
            settled(lines), {"entered": 216, "left": 150, "entered reduced": 117}
        )

    def test_a_motion_too_early_too_late_or_missing_is_a_problem(self):
        good = {"entered": 216, "left": 150, "entered reduced": 117}
        self.assertEqual(problems(good, 30), [])
        self.assertEqual(len(problems(dict(good, entered=150), 30)), 1, "sooner than its time")
        self.assertEqual(len(problems(dict(good, left=900), 30)), 1, "too late")
        self.assertEqual(len(problems({"entered": 216}, 30)), 2)
        self.assertEqual(len(problems(good, 3)), 1, "drawn without its frames")


if __name__ == "__main__":
    unittest.main()
