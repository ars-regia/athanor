#!/usr/bin/env python3
"""balloon.py QMP_SOCKET - keeps HOST_RESERVE of host memory available while a runner
guest is up, by resizing the guest's virtio balloon (scripts/runner/README.md).

A guest that builds images fills its memory with page cache, and QEMU never gives those
pages back on its own: the host then runs out of memory and its OOM killer picks QEMU,
ending the job. Free page reporting returns what the guest frees; this regulator makes it
free something when the host is short. Every BALLOON_INTERVAL seconds it reads the host's
MemAvailable and sets the guest's size to its current size plus the host's surplus over
HOST_RESERVE, within [BALLOON_FLOOR, VM_MEMORY]: under pressure the balloon inflates and
the guest drops its cache first; once the host has room again it deflates. deflate-on-oom
lets the guest take pages back rather than OOM itself.

Sizes come from the environment in QEMU's -m syntax (16G, 512M; a bare number is MiB).
It runs on a QMP socket of its own, so vm.sh's power-off path never waits on it, and it
exits 0 when QEMU goes away. A failure here never ends the job: vm.sh only logs it.
"""

import json
import os
import socket
import sys
import time

MIB = 1 << 20
UNITS = {"K": 1 << 10, "M": MIB, "G": 1 << 30, "T": 1 << 40}
# Resizes smaller than this are not worth a balloon round trip (and keep the log quiet).
STEP = 256 * MIB
CONNECT_SECONDS = 60


def size(value):
    """Bytes of a QEMU -m style size: 16G, 512M, or a bare number of MiB."""
    value = value.strip().upper()
    if value[-1:] in UNITS:
        return int(value[:-1]) * UNITS[value[-1]]
    return int(value) * MIB


def target(actual, available, reserve, floor, ceiling):
    """The guest size that leaves `reserve` of host memory available."""
    return max(floor, min(ceiling, actual + available - reserve))


def worth_resizing(actual, wanted, sent):
    """Whether to ask for `wanted`: it must differ from the guest's size by at least STEP,
    and from the last request by as much. The guest reaches a new size over seconds; the
    same request is not repeated while it gets there, nor when deflate-on-oom refused it."""
    return abs(wanted - actual) >= STEP and (sent is None or abs(wanted - sent) >= STEP)


def host_available(meminfo="/proc/meminfo"):
    with open(meminfo, encoding="ascii") as lines:
        for line in lines:
            if line.startswith("MemAvailable:"):
                return int(line.split()[1]) * 1024
    raise RuntimeError(f"{meminfo}: no MemAvailable")


class Qmp:
    def __init__(self, path):
        deadline = time.monotonic() + CONNECT_SECONDS
        while True:
            try:
                self.sock = socket.socket(socket.AF_UNIX)
                self.sock.connect(path)
                break
            except (FileNotFoundError, ConnectionRefusedError):
                self.sock.close()
                if time.monotonic() > deadline:
                    raise
                time.sleep(1)
        self.stream = self.sock.makefile("rw")
        self._read()  # the greeting
        self.call("qmp_capabilities")

    def _read(self):
        line = self.stream.readline()
        if not line:
            raise EOFError("QEMU closed the QMP socket")
        return json.loads(line)

    def call(self, command, **arguments):
        """The command's return value, or None when QEMU answers with an error."""
        message = {"execute": command}
        if arguments:
            message["arguments"] = arguments
        self.stream.write(json.dumps(message) + "\n")
        self.stream.flush()
        while True:  # asynchronous events (BALLOON_CHANGE) may arrive before the reply
            reply = self._read()
            if "return" in reply:
                return reply["return"]
            if "error" in reply:
                return None


def gib(value):
    return f"{value / (1 << 30):.1f} GiB"


def main(argv):
    ceiling = size(os.environ.get("VM_MEMORY", "16G"))
    floor = size(os.environ.get("BALLOON_FLOOR", "6G"))
    reserve = size(os.environ.get("HOST_RESERVE", "3G"))
    interval = float(os.environ.get("BALLOON_INTERVAL", "2"))
    try:
        qmp = Qmp(argv[1])
        sent = None
        while True:
            # Errors until the guest's driver has loaded: nothing to steer yet.
            balloon = qmp.call("query-balloon")
            if balloon is not None:
                available = host_available()
                wanted = target(balloon["actual"], available, reserve, floor, ceiling)
                if worth_resizing(balloon["actual"], wanted, sent):
                    print(
                        f"balloon: guest {gib(balloon['actual'])} -> {gib(wanted)}"
                        f" (host available {gib(available)}, reserve {gib(reserve)})",
                        flush=True,
                    )
                    qmp.call("balloon", value=wanted)
                    sent = wanted
            time.sleep(interval)
    except (EOFError, ConnectionResetError, BrokenPipeError):
        return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
