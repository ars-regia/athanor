"""Unit tests of forge/scripts/check_shim_link_order.py
(python3 -B -m unittest discover -s forge/scripts/tests -v)."""

import importlib.util
import os
import pathlib
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "check_shim_link_order.py"
spec = importlib.util.spec_from_file_location("check_shim_link_order", SCRIPT)
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)

GOOD = ["libgtk4-layer-shell.so.0", "libpango-1.0.so.0", "libgtk-4.so.1", "libc.so.6"]


class LinkOrderTest(unittest.TestCase):
    def test_the_measured_order_passes(self):
        self.assertEqual(check.problems(GOOD), [])

    def test_the_shim_after_gtk_is_a_problem(self):
        self.assertEqual(
            len(check.problems(["libgtk-4.so.1", "libgtk4-layer-shell.so.0"])), 1
        )

    def test_the_shim_after_libwayland_is_a_problem(self):
        needed = ["libwayland-client.so.0", "libgtk4-layer-shell.so.0", "libgtk-4.so.1"]
        self.assertEqual(len(check.problems(needed)), 1)

    def test_a_binary_without_the_shim_is_a_problem(self):
        self.assertEqual(len(check.problems(["libgtk-4.so.1"])), 1)

    def test_readelf_output_is_parsed(self):
        text = (
            " 0x0000000000000001 (NEEDED)             Shared library: [libgtk4-layer-shell.so.0]\n"
            " 0x0000000000000001 (NEEDED)             Shared library: [libgtk-4.so.1]\n"
            " 0x000000000000000e (SONAME)             Library soname: [x]\n"
        )
        self.assertEqual(
            check.needed(text), ["libgtk4-layer-shell.so.0", "libgtk-4.so.1"]
        )

    def test_main_forces_the_c_locale_for_readelf(self):
        with mock.patch.dict(os.environ, {"LANG": "it_IT.UTF-8", "LC_ALL": "it_IT.UTF-8"}):
            with mock.patch.object(check.subprocess, "run") as run:
                run.return_value.stdout = ""
                check.main(["bin"])
        self.assertEqual(run.call_args.kwargs["env"]["LC_ALL"], "C")


if __name__ == "__main__":
    unittest.main()
