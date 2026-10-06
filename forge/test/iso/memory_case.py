"""The whole-session memory measurement of the ISO acceptance (doc_shell_standard.md, ST5).

console.py sends MEMORY_PROBE once the desktop session is up. The guest has no checkout, so
the command carries scripts/session-memory/pss.py itself, compressed and encoded, and runs it
with the guest's own python3. The framed block it prints (twice, with a byte count and a
checksum, see pss.py) lands in serial.log, which scripts/session-memory/report.py verifies
and turns into memory.json after the run.
"""

import base64
import pathlib
import zlib

PSS = (
    pathlib.Path(__file__).resolve().parents[3]
    / "scripts"
    / "session-memory"
    / "pss.py"
)

# A tty line holds 4095 bytes; a longer command is truncated by the line discipline.
MAX_COMMAND = 4000


def command(source=None):
    source = PSS.read_bytes() if source is None else source
    packed = base64.b64encode(zlib.compress(source, 9))
    text = (
        b"python3 -c \"import base64,zlib;exec(zlib.decompress(base64.b64decode('"
        + packed
        + b"')))\""
    )
    if len(text) > MAX_COMMAND:
        raise ValueError(
            f"pss.py is too large to send over the console ({len(text)} bytes)"
        )
    return text


# The session settled fifteen seconds before the session probe answered; give it twenty more.
MEMORY_PROBE = b"sleep 20; " + command()
MEMORY_PROBE_WAIT = 30.0
