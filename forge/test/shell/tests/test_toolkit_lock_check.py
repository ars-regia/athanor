"""Unit tests of forge/test/shell/toolkit_lock_check.py
(python3 -B -m unittest discover -s forge/test/shell/tests -v)."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import toolkit_lock_check as check


def surface(wl, viewport, lock, width, height, destination):
    """The requests and events of one lock surface, in the order Sal sends them."""
    return [
        f" -> wl_compositor#5.create_surface(new id wl_surface#{wl})",
        f" -> wp_viewporter#10.get_viewport(new id wp_viewport#{viewport}, wl_surface#{wl})",
        f" -> ext_session_lock_v1#21.get_lock_surface(new id ext_session_lock_surface_v1#{lock}, "
        f"wl_surface#{wl}, wl_output#14)",
        f" ext_session_lock_surface_v1#{lock}.configure(2, {width}, {height})",
        f" -> ext_session_lock_surface_v1#{lock}.ack_configure(2)",
        f" -> wp_viewport#{viewport}.set_destination({destination[0]}, {destination[1]})",
        f" -> wl_surface#{wl}.attach(wl_buffer#40, 0, 0)",
        f" -> wl_surface#{wl}.commit()",
    ]


class CommittedTest(unittest.TestCase):
    def test_a_commit_at_the_configured_size_holds(self):
        lines = surface(22, 24, 25, 1280, 800, (1280, 800))
        self.assertIsNone(check.committed_as_the_protocol_asks(lines, "25"))

    def test_a_commit_at_another_size_is_named(self):
        # The viewport is created before the lock surface, so it must still be read.
        lines = surface(22, 24, 25, 1280, 800, (640, 400))
        self.assertEqual(
            check.committed_as_the_protocol_asks(lines, "25"),
            "a commit at (640, 400), configured (1280, 800)",
        )

    def test_a_reused_id_reads_only_its_last_surface(self):
        old = surface(22, 24, 25, 1280, 800, (640, 400))
        old.append(" -> ext_session_lock_surface_v1#25.destroy()")
        lines = old + surface(22, 24, 25, 1920, 1080, (1920, 1080))
        self.assertIsNone(check.committed_as_the_protocol_asks(lines, "25"))



UNLOCK = " -> ext_session_lock_v1#21.unlock_and_destroy()"


def destroy(lock):
    return f" -> ext_session_lock_surface_v1#{lock}.destroy()"


class UnlockedFirstTest(unittest.TestCase):
    def two(self):
        return surface(3, 4, 30, 1280, 800, (1280, 800)) + surface(
            5, 6, 31, 1280, 800, (1280, 800)
        )

    def test_an_unlock_before_every_surface_goes_holds(self):
        lines = self.two() + [UNLOCK, destroy(30), destroy(31)]
        self.assertTrue(check.unlocked_first(lines, 2))

    def test_a_surface_destroyed_before_the_unlock_is_named(self):
        lines = self.two() + [destroy(30), UNLOCK, destroy(31)]
        self.assertFalse(check.unlocked_first(lines, 2))

    def test_a_surface_left_after_the_unlock_is_named(self):
        lines = self.two() + [UNLOCK, destroy(30)]
        self.assertFalse(check.unlocked_first(lines, 2))


if __name__ == "__main__":
    unittest.main()
