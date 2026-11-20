import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))

import deploy


class PlanTest(unittest.TestCase):
    def test_bar_installs_binary_and_unit(self):
        pairs = deploy.plan("athanor-bar", pathlib.Path("/built"))
        destinations = {str(dst): str(src) for src, dst in pairs}
        self.assertEqual(destinations["/usr/bin/athanor-bar"], "/built/athanor-bar")
        unit = pathlib.Path(destinations["/usr/lib/systemd/user/athanor-bar.service"])
        self.assertEqual(unit.name, "athanor-bar.service")
        self.assertTrue(unit.is_file(), unit)

    def test_shelld_installs_its_dbus_activation_files(self):
        destinations = {str(dst) for _, dst in deploy.plan("athanor-shelld", pathlib.Path("/b"))}
        self.assertIn("/usr/share/dbus-1/services/org.freedesktop.Notifications.service", destinations)
        self.assertIn("/usr/share/dbus-1/services/org.kde.StatusNotifierWatcher.service", destinations)

    def test_every_data_source_exists(self):
        for crate in deploy.CRATES:
            for src, dst in deploy.plan(crate, pathlib.Path("/b")):
                if not str(src).startswith("/b/"):
                    self.assertTrue(src.is_file(), src)

    def test_unknown_crate_is_refused(self):
        with self.assertRaises(ValueError):
            deploy.plan("athanor-nothing", pathlib.Path("/built"))


class MeasurementGuardTest(unittest.TestCase):
    def test_refuses_while_a_measurement_runs(self):
        with self.assertRaises(SystemExit):
            deploy.refuse_while_measuring(lambda: ["1234 python3 scripts/shell-bench/soak.py"])

    def test_passes_when_idle(self):
        deploy.refuse_while_measuring(lambda: [])


if __name__ == "__main__":
    unittest.main()
