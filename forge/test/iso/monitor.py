#!/usr/bin/env python3
"""Sends commands to a running QEMU monitor on a unix socket.

Usage: monitor.py SOCKET COMMAND [COMMAND...]

One command per argument, in the monitor's own language, e.g. "screendump /tmp/s.ppm"
or "sendkey ret". The reply is printed, which matters for screendump: the monitor
answers with an error rather than a failure status when it cannot write the file.
"""

import os
import socket
import sys
import time

# The pauses wait on a real QEMU and guest. The tests' fake monitor answers at once and
# scales them down with the same variable as console.py; a run leaves it at one.
PACE = float(os.environ.get("ATHANOR_CONSOLE_PACE", "1"))


def main() -> int:
    sock_path, commands = sys.argv[1], sys.argv[2:]
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(30)
    s.connect(sock_path)
    time.sleep(0.5 * PACE)
    try:
        s.recv(65536)  # the banner
    except socket.timeout:
        pass
    for cmd in commands:
        s.sendall((cmd + "\n").encode())
        time.sleep(1.5 * PACE)
        try:
            reply = s.recv(65536).decode(errors="replace")
        except socket.timeout:
            reply = "(no reply)"
        print(f"> {cmd}\n{reply.strip()[:400]}")
    s.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
