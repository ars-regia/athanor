"""keyboard_type.py: the named keys, the media keys and the letters of a chord."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "devvm"))
import keyboard_type as kt  # noqa: E402


class StrokeTests(unittest.TestCase):
    def codes(self, *tokens):
        return [codes for codes, *_ in kt.strokes(tokens)]

    def test_media_keys_are_linux_codes(self):
        self.assertEqual(
            self.codes("@volup", "@voldown", "@mute"), [[115], [114], [113]]
        )

    def test_media_keys_tap_with_a_count(self):
        self.assertEqual(self.codes("@volup*3:0.1"), [[115]] * 3)

    def test_chord_takes_letters(self):
        self.assertEqual(self.codes("@ctrl+o"), [[kt.NAMED["ctrl"], kt.PLAIN["o"]]])

    def test_unknown_key_is_refused(self):
        with self.assertRaises(SystemExit):
            self.codes("@ctrl+nokey")


if __name__ == "__main__":
    unittest.main()
