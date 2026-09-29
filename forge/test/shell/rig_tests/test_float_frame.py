import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import float_frame

BACKGROUND = "srgb(39,41,42)"


def scene(directory, name, *draws):
    """A 40x30 frame of the background with a float panel pill at 4 px from the edges."""
    path = Path(directory) / f"{name}.png"
    args = [
        "magick",
        "-size",
        "40x30",
        f"xc:{BACKGROUND}",
        "-fill",
        "srgb(237,239,248)",
        "-draw",
        "rectangle 4,4 35,10",
    ]
    for colour, point in draws:
        args += ["-fill", colour, "-draw", f"point {point}"]
    subprocess.run(args + [str(path)], check=True)
    return path


class FloatFrameTest(unittest.TestCase):
    def test_a_clean_float_capture_has_nothing_in_its_frame(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(float_frame.stale_pixels(scene(directory, "clean")), 0)

    def test_content_inside_the_frame_is_not_stale(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(
                float_frame.stale_pixels(
                    scene(directory, "inner", ("srgb(252,210,74)", "20,20"))
                ),
                0,
            )

    def test_stale_pixels_in_the_gap_are_counted_on_every_edge(self):
        with tempfile.TemporaryDirectory() as directory:
            path = scene(
                directory,
                "stale",
                ("srgb(252,210,74)", "6,0"),
                ("srgb(39,41,43)", "0,15"),
                ("srgb(0,0,0)", "39,29"),
                ("srgb(255,255,255)", "20,27"),
            )
            self.assertEqual(float_frame.stale_pixels(path), 4)

    def test_the_command_fails_on_a_stale_capture_and_names_it(self):
        with tempfile.TemporaryDirectory() as directory:
            clean, stale = (
                scene(directory, "clean"),
                scene(directory, "stale", ("srgb(252,210,74)", "6,0")),
            )
            self.assertEqual(float_frame.main([str(clean)]), 0)
            self.assertEqual(float_frame.main([str(clean), str(stale)]), 1)


if __name__ == "__main__":
    unittest.main()
