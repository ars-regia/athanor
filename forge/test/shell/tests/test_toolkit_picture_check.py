import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from toolkit_picture_check import full_size, pictures

TRACE = """\
[1.0] wl_output#12.name("HEADLESS-1")
[1.1]  -> wp_viewporter#9.get_viewport(new id wp_viewport#19, wl_surface#29)
[1.2]  -> zwlr_layer_shell_v1#10.get_layer_surface(new id zwlr_layer_surface_v1#30, wl_surface#29, wl_output#12, 0, "athanor-sal-picture")
[1.3]  -> zwlr_layer_surface_v1#30.set_anchor(15)
[1.4]  -> zwlr_layer_surface_v1#30.set_margin(0, 0, 0, 0)
[1.5]  -> zwlr_layer_surface_v1#30.set_exclusive_zone(-1)
[1.6]  -> zwlr_layer_surface_v1#30.set_keyboard_interactivity(0)
[1.7]  -> zwlr_layer_surface_v1#30.set_size(0, 0)
[1.8]  -> wl_compositor#3.create_region(new id wl_region#31)
[1.9]  -> wl_surface#29.set_input_region(wl_region#31)
[2.0]  -> wl_shm_pool#4.create_buffer(new id wl_buffer#32, 0, 1280, 800, 5120, 0)
[2.1]  -> wp_viewport#19.set_destination(853, 533)
[2.2]  -> wl_surface#29.attach(wl_buffer#32, 0, 0)
""".splitlines()

OUTPUT = {"name": "HEADLESS-1", "rect": {"width": 853, "height": 533}, "scale": 1.5}


class TestToolkitPictureCheck(unittest.TestCase):
    def test_a_full_size_picture_passes(self):
        seen = pictures(TRACE)
        self.assertEqual(list(seen), ["29"])
        self.assertEqual(seen["29"]["output"], "HEADLESS-1")
        self.assertIsNone(full_size(seen["29"], OUTPUT))

    def test_a_buffer_of_another_size_fails(self):
        at_one = dict(OUTPUT, scale=1.0)
        self.assertIn(
            "a buffer of (1280, 800)", full_size(pictures(TRACE)["29"], at_one)
        )

    def test_an_input_region_with_a_rectangle_fails(self):
        trace = TRACE + ["[2.3]  -> wl_region#31.add(0, 0, 1, 1)"]
        self.assertIn("input region", full_size(pictures(trace)["29"], OUTPUT))

    def test_a_role_without_the_full_size_state_fails(self):
        trace = [line for line in TRACE if "set_exclusive_zone" not in line]
        self.assertIn(
            "set_exclusive_zone(-1)", full_size(pictures(trace)["29"], OUTPUT)
        )

    def test_a_destroyed_surface_is_not_a_picture(self):
        trace = TRACE + ["[2.4]  -> zwlr_layer_surface_v1#30.destroy()"]
        self.assertEqual(pictures(trace), {})


if __name__ == "__main__":
    unittest.main()
