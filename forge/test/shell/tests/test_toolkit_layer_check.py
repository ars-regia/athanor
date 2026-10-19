import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from toolkit_layer_check import drawn_at, live_surfaces, physical

TRACE = """\
[1.0]  -> zwlr_layer_shell_v1#10.get_layer_surface(new id zwlr_layer_surface_v1#30, wl_surface#29, wl_output#12, 2, "x")
[1.1]  -> zwlr_layer_shell_v1#10.get_layer_surface(new id zwlr_layer_surface_v1#31, wl_surface#28, wl_output#21, 2, "x")
[1.2]  -> wl_shm_pool#4.create_buffer(new id wl_buffer#32, 0, 127, 50, 508, 0)
[1.3]  -> wp_viewport#19.set_destination(127, 50)
[1.4]  -> zwlr_layer_surface_v1#31.destroy()
[1.5]  -> zwlr_layer_shell_v1#10.get_layer_surface(new id zwlr_layer_surface_v1#31, wl_surface#28, wl_output#23, 2, "x")
[1.6]  -> wl_shm_pool#4.create_buffer(new id wl_buffer#24, 0, 191, 75, 764, 0)
[1.7]  -> wp_viewport#19.set_destination(127, 50)
""".splitlines()


class TestToolkitLayerCheck(unittest.TestCase):
    def test_a_destroyed_surface_is_not_live_and_its_id_may_return(self):
        self.assertEqual(live_surfaces(TRACE), {"30": "12", "31": "23"})

    def test_a_buffer_counts_at_the_scale_its_viewport_maps_it_from(self):
        self.assertTrue(drawn_at(TRACE, 1.0))
        self.assertTrue(drawn_at(TRACE, 1.5))
        self.assertFalse(drawn_at(TRACE, 2.0))
        self.assertFalse(drawn_at(TRACE[:4], 1.5))

    def test_the_physical_size_rounds_as_the_fractional_scale_protocol_does(self):
        self.assertEqual(physical(101, 150 / 120), 126)
        self.assertEqual(physical(100, 144 / 120), 120)
        self.assertEqual(physical(101, 1.5), 152)


if __name__ == "__main__":
    unittest.main()
