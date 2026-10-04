"""Unit tests of scripts/shell-bench/analysis.py (python3 -B -m unittest discover -s scripts/tests -v)."""

import json
import math
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "shell-bench"))
import analysis  # noqa: E402

MS = 1_000_000


def entry(unit, record, pid=7):
    return {
        "_SYSTEMD_USER_UNIT": f"{unit}.service",
        "_PID": str(pid),
        "MESSAGE": "INFO athanor_apps::timing: " + json.dumps(record),
    }


def frame(unit, surface, presented_ms, refresh_ms=16.666):
    return analysis.Frame(
        unit, surface, 7, 0, int(presented_ms * MS), int(refresh_ms * MS)
    )


class Parse(unittest.TestCase):
    def test_frames_and_placements_come_out_of_the_journal(self):
        frames, placements = analysis.parse_journal(
            [
                entry(
                    "athanor-dock",
                    {
                        "bench": "frame",
                        "surface": "dock",
                        "frame": 3,
                        "presented_us": 2000,
                        "predicted_us": 1990,
                        "refresh_us": 16666,
                    },
                ),
                entry(
                    "athanor-dock",
                    {
                        "bench": "surface",
                        "surface": "dock",
                        "x": 660,
                        "y": 1016,
                        "w": 600,
                        "h": 64,
                        "output_w": 1920,
                        "output_h": 1080,
                    },
                ),
                {
                    "_SYSTEMD_USER_UNIT": "athanor-bar.service",
                    "MESSAGE": "something else",
                },
                {"_SYSTEMD_USER_UNIT": "athanor-bar.service", "MESSAGE": [255, 0]},
            ]
        )
        self.assertEqual(
            frames,
            [analysis.Frame("athanor-dock", "dock", 7, 3, 2_000_000, 16_666_000)],
        )
        self.assertEqual(
            placements,
            [
                analysis.Placement(
                    "athanor-dock", "dock", 660, 1016, 600, 64, 1920, 1080
                )
            ],
        )

    def test_a_frame_without_presentation_time_is_left_out(self):
        frames, _ = analysis.parse_journal(
            [
                entry(
                    "athanor-bar",
                    {
                        "bench": "frame",
                        "surface": "bar",
                        "frame": 1,
                        "presented_us": 0,
                        "predicted_us": 5,
                        "refresh_us": 16666,
                    },
                )
            ]
        )
        self.assertEqual(frames, [])


class Targets(unittest.TestCase):
    PLACE = analysis.Placement("athanor-dock", "dock", 660, 1016, 600, 64, 1920, 1080)

    def test_buttons_of_the_matching_window_are_aimed_at_their_centre(self):
        nodes = [
            {"toplevel": 0, "name": "", "w": 600, "h": 64},
            {
                "window": 0,
                "name": "Files",
                "role": "push button",
                "popup": True,
                "x": 10,
                "y": 8,
                "w": 48,
                "h": 48,
            },
            {
                "window": 0,
                "name": "",
                "role": "filler",
                "popup": False,
                "x": 0,
                "y": 0,
                "w": 600,
                "h": 64,
            },
        ]
        self.assertEqual(
            analysis.targets(self.PLACE, nodes),
            [
                analysis.Target(
                    "athanor-dock",
                    "dock",
                    "Files",
                    True,
                    660 + 34,
                    1016 + 32,
                    1920,
                    1080,
                )
            ],
        )

    def test_buttons_take_the_role_name_at_spi_gives_them(self):
        # The reference machine's AT-SPI names GTK 4's buttons "button" (spike Q2).
        nodes = [
            {"toplevel": 0, "name": "", "w": 600, "h": 64},
            {
                "window": 0,
                "name": "Files",
                "role": "button",
                "popup": True,
                "x": 10,
                "y": 8,
                "w": 48,
                "h": 48,
            },
        ]
        self.assertEqual(
            [t.name for t in analysis.targets(self.PLACE, nodes)], ["Files"]
        )

    def test_two_windows_of_the_same_size_are_refused(self):
        nodes = [
            {"toplevel": 0, "name": "", "w": 600, "h": 64},
            {"toplevel": 1, "name": "", "w": 600, "h": 64},
        ]
        with self.assertRaisesRegex(RuntimeError, "2 windows"):
            analysis.targets(self.PLACE, nodes)


class Response(unittest.TestCase):
    def test_the_first_frame_of_the_surface_after_the_input_counts(self):
        frames = [
            frame("athanor-bar", "popover", 90),
            frame("athanor-bar", "bar", 105),
            frame("athanor-bar", "popover", 130),
            frame("athanor-bar", "popover", 147),
        ]
        self.assertEqual(
            analysis.responses_ms([100 * MS], frames, "athanor-bar", "popover"), [30.0]
        )

    def test_no_frame_within_two_seconds_is_a_miss(self):
        frames = [frame("athanor-bar", "popover", 2200)]
        self.assertEqual(
            analysis.responses_ms([100 * MS], frames, "athanor-bar", "popover"), [None]
        )

    def test_a_miss_is_infinitely_slow_in_the_percentile(self):
        self.assertEqual(analysis.percentile([10.0] * 49 + [None], 95), 10.0)
        self.assertEqual(analysis.percentile([10.0] * 45 + [None] * 5, 95), math.inf)

    def test_percentile_is_nearest_rank(self):
        self.assertEqual(analysis.percentile(list(range(1, 101)), 95), 95)


class Smoothness(unittest.TestCase):
    def test_late_frames_inside_an_animation_are_counted(self):
        times = [0, 16.7, 33.3, 50, 100, 116.7]  # one gap of three refresh intervals
        frames = [frame("athanor-dock", "dock", t) for t in times]
        on_time, worst, measured = analysis.smoothness(
            frames, [(0, 200 * MS)], "athanor-dock", "dock"
        )
        self.assertEqual(measured, 1)
        self.assertAlmostEqual(on_time, 4 / 5)
        self.assertAlmostEqual(worst, 3.0, places=2)

    def test_a_window_with_few_frames_is_no_animation(self):
        frames = [frame("athanor-dock", "dock", 10), frame("athanor-dock", "dock", 400)]
        self.assertEqual(
            analysis.smoothness(frames, [(0, 1000 * MS)], "athanor-dock", "dock"),
            (None, 0.0, 0),
        )


class Settled(unittest.TestCase):
    def test_the_last_frame_before_the_horizon_must_come_within_the_limit(self):
        frames = [frame("athanor-dock", "dock", t) for t in (1010, 2005, 4300)]
        ok = analysis.settled(
            [0, 2000 * MS, 3000 * MS],
            frames,
            "athanor-dock",
            "dock",
            limit_ns=1200 * MS,
            horizon_ns=1700 * MS,
        )
        # 0: last frame 1010 within 1200. 2000: last 2005, fine. 3000: last 4300 is past 4200.
        self.assertEqual(ok, [True, True, False])


class Kernel(unittest.TestCase):
    def test_cpu_ticks_survive_a_command_with_spaces(self):
        stat = "4242 (athanor bar) S 1 2 3 4 5 6 7 8 9 10 120 30 0 0 20 0 1 0 100"
        self.assertEqual(analysis.cpu_ticks(stat), 150)

    def test_pss(self):
        self.assertEqual(analysis.pss_kb("Rss:  9000 kB\nPss:  4096 kB\n"), 4096)


class Pixels(unittest.TestCase):
    @staticmethod
    def ppm(w, h, pixels):
        return f"P6\n{w} {h}\n255\n".encode() + bytes(pixels)

    def test_a_leading_whitespace_byte_is_pixel_data(self):
        w, h, data = analysis.read_ppm(self.ppm(1, 1, [32, 10, 9]))
        self.assertEqual((w, h, data), (1, 1, bytes([32, 10, 9])))

    def test_changed_pixels_and_their_box(self):
        a = self.ppm(4, 2, [0] * 24)
        b = self.ppm(4, 2, [0] * 3 + [200, 0, 0] + [0] * 12 + [0, 0, 100] + [0] * 3)
        self.assertEqual(analysis.changed(a, b, (0, 2)), (2, (1, 0, 2, 1)))
        self.assertEqual(analysis.changed(a, b, (0, 1)), (1, (1, 0, 1, 0)))

    def test_names_ignore_digits(self):
        self.assertEqual(
            analysis.normalise([("label", "10:41"), ("push button", "Files")]),
            [("label", "#:#"), ("push button", "Files")],
        )


class Verdict(unittest.TestCase):
    def test_not_measured_fails(self):
        self.assertEqual(
            analysis.verdict(None, 50), {"value": None, "limit": 50, "pass": False}
        )

    def test_limits(self):
        self.assertTrue(analysis.verdict(49.9, 50)["pass"])
        self.assertFalse(analysis.verdict(0.98, 0.99, at_least=True)["pass"])


if __name__ == "__main__":
    unittest.main()
