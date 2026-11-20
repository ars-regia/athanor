"""Unit tests of scripts/decode-drm-panic.py (python3 -B -m unittest discover -s scripts/tests -v).

The fixtures are built by encoding a known string the way drm_panic does, so the
test is a round trip: whatever the script decodes has to be what went in.
"""

import pathlib
import subprocess
import sys
import unittest
import zlib

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "decode-drm-panic.py"
LOG = ("[  12.345678] Kernel panic - not syncing: Attempted to kill init!\n"
       "[  12.345679] CPU: 3 PID: 1 Comm: systemd Not tainted 7.2.5-100.azoth.fc43.x86_64\n")


def encode(data):
    """The inverse of the script: pack bytes into the decimal digits of a QR numeric segment."""
    blob = zlib.compress(data)
    digits = ""
    for start in range(0, len(blob), 7):
        group = blob[start:start + 7]
        # 7 bytes are 17 digits by definition; a shorter group takes ceil(bytes * 5 / 2).
        width = 17 if len(group) == 7 else (len(group) * 5 + 1) // 2
        digits += str(int.from_bytes(group, "little")).zfill(width)
    return digits


def run(*args, stdin=None):
    return subprocess.run([sys.executable, "-B", str(SCRIPT), *args],
                          capture_output=True, text=True, input=stdin)


class Decode(unittest.TestCase):
    def test_round_trip_through_the_panic_url(self):
        url = (f"https://drm-panic.fedoraproject.org/#?a=x86_64"
               f"&v=7.2.5-100.azoth.fc43.x86_64&z={encode(LOG.encode())}")
        done = run(url)
        self.assertEqual(done.returncode, 0, done.stderr)
        header, _, body = done.stdout.partition("\n")
        self.assertIn("x86_64", header)
        self.assertIn("7.2.5-100.azoth.fc43.x86_64", header)
        self.assertEqual(body, LOG)

    def test_round_trip_from_the_bare_payload(self):
        done = run(encode(LOG.encode()))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue(done.stdout.endswith(LOG))
        self.assertIn("unknown", done.stdout.partition("\n")[0])

    def test_payload_on_stdin(self):
        done = run("-", stdin=encode(LOG.encode()) + "\n")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue(done.stdout.endswith(LOG))

    def test_every_partial_group_width_round_trips(self):
        for length in range(1, 40):
            payload = bytes(range(length))
            self.assertEqual(run(encode(payload)).returncode, 0, f"length {length}")

    def test_non_numeric_payload_fails_with_a_message(self):
        done = run("12345abcde")
        self.assertEqual(done.returncode, 1)
        self.assertNotIn("Traceback", done.stderr)
        self.assertIn("decimal digits", done.stderr)

    def test_digits_that_are_not_a_zlib_stream_fail_with_a_message(self):
        done = run("1" * 34)
        self.assertEqual(done.returncode, 1)
        self.assertNotIn("Traceback", done.stderr)
        self.assertIn("zlib", done.stderr)

    def test_url_without_a_payload_fails_with_a_message(self):
        done = run("https://drm-panic.fedoraproject.org/#?a=x86_64&v=7.2.5")
        self.assertEqual(done.returncode, 1)
        self.assertNotIn("Traceback", done.stderr)
        self.assertIn("z=", done.stderr)


if __name__ == "__main__":
    unittest.main()
