import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from toolkit_popup_check import (
    BOTTOM,
    FLIP_Y,
    SLIDE_X,
    popups,
    popups_before_layer,
    rects,
    whole,
)

TRACE = """\
[1.0]  -> xdg_wm_base#9.create_positioner(new id xdg_positioner#30)
[1.1]  -> xdg_positioner#30.set_size(120, 40)
[1.2]  -> xdg_positioner#30.set_anchor_rect(8, 4, 72, 44)
[1.3]  -> xdg_positioner#30.set_anchor(2)
[1.4]  -> xdg_positioner#30.set_gravity(2)
[1.5]  -> xdg_positioner#30.set_constraint_adjustment(9)
[1.6]  -> xdg_surface#32.get_popup(new id xdg_popup#33, nil, xdg_positioner#30)
[1.7]  -> zwlr_layer_surface_v1#20.get_popup(xdg_popup#33)
[1.8] xdg_popup#33.configure(-24, 52, 120, 40)
[1.9]  -> xdg_popup#33.destroy()
[2.0]  -> xdg_wm_base#9.create_positioner(new id xdg_positioner#30)
[2.1]  -> xdg_positioner#30.set_anchor(1)
[2.2]  -> xdg_surface#32.get_popup(new id xdg_popup#33, nil, xdg_positioner#30)
[2.3]  -> zwlr_layer_surface_v1#20.get_popup(xdg_popup#33)
[2.4]  -> xdg_popup#33.grab(wl_seat#5, 17)
[2.5] xdg_popup#33.configure(0, -120, 160, 120)
[2.6] xdg_popup#33.popup_done()
[2.7]  -> xdg_popup#33.destroy()
""".splitlines()


class TestToolkitPopupCheck(unittest.TestCase):
    def test_each_popup_keeps_its_own_positioner_grab_and_end_when_ids_return(self):
        tip, menu = popups(TRACE)
        self.assertEqual(tip["positioner"]["set_anchor_rect"], (8, 4, 72, 44))
        self.assertEqual(tip["positioner"]["set_anchor"], (BOTTOM,))
        self.assertEqual(
            tip["positioner"]["set_constraint_adjustment"], (FLIP_Y | SLIDE_X,)
        )
        self.assertEqual(
            (tip["layer"], tip["grab"], tip["done"], tip["destroyed"]),
            (True, False, False, True),
        )
        self.assertEqual(tip["position"], (-24, 52, 120, 40))
        self.assertEqual(menu["positioner"], {"set_anchor": (1,)})
        self.assertEqual(
            (menu["grab"], menu["done"], menu["destroyed"]), (True, True, True)
        )

    def test_an_unfinished_trace_leaves_the_popup_live(self):
        (menu,) = popups(TRACE[10:16])
        self.assertEqual((menu["grab"], menu["destroyed"]), (True, False))

    def test_the_rectangles_are_read_by_name(self):
        text = "rect surface 0 0 96 52\nrect button 12 6.5 72 40\nactivated Quit\n"
        self.assertEqual(
            rects(text), {"surface": (0, 0, 96, 52), "button": (12, 6.5, 72, 40)}
        )

    def test_an_anchor_covers_the_widget_in_whole_pixels(self):
        self.assertEqual(whole(10, 10, 51.17, 30, 4), (6, 6, 60, 38))
        self.assertEqual(whole(10, 10, 51.17, 30), (10, 10, 52, 30))

    def test_every_popup_goes_before_its_layer_surface(self):
        layer = "[3.0]  -> zwlr_layer_surface_v1#20.destroy()"
        self.assertTrue(popups_before_layer([*TRACE, layer]))
        self.assertFalse(popups_before_layer(TRACE), "the layer surface lives")
        self.assertFalse(
            popups_before_layer([*TRACE[:-1], layer, TRACE[-1]]),
            "the menu outlived its parent",
        )


if __name__ == "__main__":
    unittest.main()
