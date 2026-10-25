#!/usr/bin/env python3
"""`python3 -B -m unittest discover -s DIR`, with the test classes of DIR run in parallel.

The suites of system/tests start hundreds of shell and fake-tool processes, so they are bound by
process launches, not by the CPU of one interpreter (issue #790). Every TestCase class runs in
its own `python3 -m unittest` process, at most JOBS at a time, from the directory the command
was started in; each class prints its report in one piece when it ends. The exit status is
non-zero when a class fails, when a module cannot be imported, or when nothing is found.

A class that runs past TIMEOUT seconds (default 600) is killed and fails on its own, by name.
A module that raises SkipTest when imported is reported as skipped, as `discover` does.

Usage: unittest_parallel.py DIR [JOBS [TIMEOUT]]    (JOBS defaults to the number of CPUs)
"""

import concurrent.futures
import os
import pathlib
import select
import signal
import subprocess
import sys
import threading
import time
import unittest


def classes(directory: pathlib.Path) -> tuple[list[str], list[str], list[str]]:
    """The dotted names of the TestCase classes `discover -s DIR` would run, the modules
    that failed to load, and the modules that skipped themselves, with their reasons."""
    sys.path.insert(0, str(directory))
    found: set[str] = set()
    broken: list[str] = []
    skipped: list[str] = []

    def walk(suite: unittest.TestSuite) -> None:
        for test in suite:
            if isinstance(test, unittest.TestSuite):
                walk(test)
            elif type(test).__name__ == "_FailedTest":
                broken.append(test.id())
            elif type(test).__name__ == "ModuleSkipped":
                try:
                    getattr(test, test._testMethodName)()
                except unittest.SkipTest as skip:
                    skipped.append(f"{test.id().rsplit('.', 1)[-1]}: {skip}")
            else:
                found.add(f"{type(test).__module__}.{type(test).__qualname__}")

    walk(unittest.defaultTestLoader.discover(str(directory)))
    return sorted(found), broken, skipped


LIVE: set[subprocess.Popen] = set()  # the children running now, for kill_all
STOPPING = threading.Event()  # set by an interrupt: no child may outlive it
DRAIN = 2.0  # seconds to read what is left after the child ended or was killed


def kill_group(child: subprocess.Popen) -> None:
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def kill_all() -> None:
    for child in list(LIVE):
        kill_group(child)


def read_output(child: subprocess.Popen, deadline: float) -> tuple[bytes, bool]:
    """The child's output until EOF, or until `deadline`, or until DRAIN seconds after it ended
    (a helper that left the group may hold the pipe open for ever). Returns it and whether the
    deadline was reached."""
    fd = child.stdout.fileno()
    chunks: list[bytes] = []
    ended = None
    while True:
        now = time.monotonic()
        if now >= deadline:
            return b"".join(chunks), True
        if child.poll() is not None:
            ended = ended or now
            if now - ended >= DRAIN:
                return b"".join(chunks), False
        if select.select([fd], [], [], 0.1)[0]:
            data = os.read(fd, 65536)
            if not data:
                return b"".join(chunks), False
            chunks.append(data)


def run_class(name: str, directory: pathlib.Path, timeout: float) -> tuple[str, int, str, float]:
    env = {**os.environ, "PYTHONPATH": os.pathsep.join([str(directory), os.environ.get("PYTHONPATH", "")]).rstrip(os.pathsep)}
    started = time.monotonic()
    # Its own session, so that a timeout or an interrupt kills the helpers the tests spawned
    # as well as the child.
    child = subprocess.Popen(
        [sys.executable, "-B", "-m", "unittest", "-v", name],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env, start_new_session=True,
    )
    LIVE.add(child)
    try:
        if STOPPING.is_set():
            kill_group(child)
        data, timed_out = read_output(child, started + timeout)
        if timed_out:
            kill_group(child)
            child.wait()
            tail, _ = read_output(child, time.monotonic() + DRAIN)
            text = (data + tail).decode(errors="replace")
            return name, 124, f"{text}\nTIMEOUT: {name} was killed after {timeout:g}s\n", time.monotonic() - started
        code = child.wait()
        return name, code, data.decode(errors="replace"), time.monotonic() - started
    finally:
        if child.poll() is None:
            kill_group(child)
            child.wait()
        LIVE.discard(child)
        child.stdout.close()


def main(argv: list[str]) -> int:
    if len(argv) not in (1, 2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    directory = pathlib.Path(argv[0]).resolve()
    jobs = int(argv[1]) if len(argv) >= 2 else os.cpu_count() or 1
    timeout = float(argv[2]) if len(argv) == 3 else 600.0
    names, broken, skipped = classes(directory)
    for line in skipped:
        print(f"skipped module {line}")
    for name in broken:
        print(f"cannot load {name}", file=sys.stderr)
    if not names or broken:
        print(f"{directory}: {'no test class found' if not names else 'a module failed to load'}", file=sys.stderr)
        return 1
    failed = []
    started = time.monotonic()
    pool = concurrent.futures.ThreadPoolExecutor(max_workers=jobs)
    try:
        # Longest first is unknown here; the pool keeps `jobs` classes running as they end.
        for name, code, output, seconds in pool.map(lambda n: run_class(n, directory, timeout), names):
            print(f"=== {name} ({seconds:.1f}s, exit {code})\n{output}", flush=True)
            if code != 0:
                failed.append(name)
    except BaseException:
        # An interrupt must not wait for the other workers: kill the live groups, drop the queue.
        STOPPING.set()
        pool.shutdown(wait=False, cancel_futures=True)
        kill_all()
        raise
    finally:
        pool.shutdown(wait=True)
    print(f"{len(names)} classes, {jobs} at a time, {time.monotonic() - started:.1f}s; failed: {', '.join(failed) or 'none'}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
