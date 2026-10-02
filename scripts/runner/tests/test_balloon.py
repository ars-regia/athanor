"""Unit tests of balloon.py's arithmetic; the QMP side is exercised on a live guest."""

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import balloon  # noqa: E402

G = 1 << 30
M = 1 << 20


class Size(unittest.TestCase):
    def test_qemu_syntax(self):
        self.assertEqual(balloon.size("16G"), 16 * G)
        self.assertEqual(balloon.size("512m"), 512 * M)
        self.assertEqual(balloon.size("2048"), 2048 * M)


class Target(unittest.TestCase):
    def test_short_host_shrinks_the_guest_by_the_deficit(self):
        self.assertEqual(balloon.target(16 * G, 1 * G, 3 * G, 6 * G, 16 * G), 14 * G)

    def test_never_below_the_floor(self):
        self.assertEqual(balloon.target(8 * G, 0, 3 * G, 6 * G, 16 * G), 6 * G)

    def test_room_on_the_host_grows_it_back_to_the_ceiling(self):
        self.assertEqual(balloon.target(10 * G, 12 * G, 3 * G, 6 * G, 16 * G), 16 * G)

    def test_stable_once_the_reserve_is_met(self):
        # The pages the balloon took are available to the host again: same target.
        self.assertEqual(balloon.target(14 * G, 3 * G, 3 * G, 6 * G, 16 * G), 14 * G)


class WorthResizing(unittest.TestCase):
    def test_small_changes_are_ignored(self):
        self.assertFalse(balloon.worth_resizing(16 * G, 16 * G - 100 * M, None))

    def test_a_large_change_is_sent_once(self):
        self.assertTrue(balloon.worth_resizing(16 * G, 12 * G, None))
        # The guest is still getting there: the same request is not repeated.
        self.assertFalse(balloon.worth_resizing(15 * G, 12 * G, 12 * G))

    def test_a_new_target_is_sent(self):
        self.assertTrue(balloon.worth_resizing(12 * G, 16 * G, 12 * G))


class HostAvailable(unittest.TestCase):
    def test_reads_mem_available_in_bytes(self):
        with tempfile.NamedTemporaryFile("w", suffix=".meminfo") as meminfo:
            meminfo.write("MemTotal:       32000000 kB\nMemAvailable:    2048 kB\n")
            meminfo.flush()
            self.assertEqual(balloon.host_available(meminfo.name), 2 * M)


if __name__ == "__main__":
    unittest.main()
