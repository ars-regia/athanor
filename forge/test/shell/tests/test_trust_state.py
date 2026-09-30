"""trust_state.py writes what athanor-trust-state parses: checked by the schema's shape."""

import json
import os
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import trust_state  # noqa: E402

KEYS = {
    "schema",
    "booted",
    "downloaded",
    "previous",
    "verified",
    "update",
    "policy",
    "secure_boot",
    "newest_booted_build_time",
    "last_successful_check",
    "last_error",
    "last_error_host",
}


class TrustState(unittest.TestCase):
    def test_every_state_has_the_schema_1_keys(self):
        for name in trust_state.NAMES:
            if name == "missing":
                continue
            with self.subTest(name=name):
                self.assertEqual(set(trust_state.state(name, trust_state.FROZEN)), KEYS)

    def test_the_states_differ_where_the_shield_looks(self):
        s = {
            name: trust_state.state(name, trust_state.FROZEN)
            for name in trust_state.NAMES
            if name != "missing"
        }
        self.assertEqual(s["downloaded"]["update"], "downloaded")
        self.assertIsNotNone(s["downloaded"]["downloaded"])
        self.assertFalse(s["attention"]["verified"]["value"])
        self.assertIsNone(s["attention"]["last_successful_check"])
        self.assertEqual(s["refused"]["update"], "refused")
        self.assertIn("\u202e", s["hostile"]["booted"]["version"])
        self.assertGreater(len(s["hostile"]["booted"]["version"]), 128)
        self.assertLess(
            trust_state.FROZEN - s["verified"]["last_successful_check"], 14 * 86400
        )

    def test_write_is_0644_and_missing_removes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp, "run/athanor-update/state.json")
            trust_state.write("verified", trust_state.FROZEN, path)
            self.assertEqual(os.stat(path).st_mode & 0o777, 0o644)
            self.assertEqual(json.loads(path.read_text())["schema"], 1)
            trust_state.write("missing", trust_state.FROZEN, path)
            self.assertFalse(path.exists())
