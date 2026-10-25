#!/usr/bin/env python3
"""`python3 -B -m unittest discover -s DIR`, with the test classes of DIR run in parallel.

The suites of system/tests start hundreds of shell and fake-tool processes, so they are bound by
process launches, not by the CPU of one interpreter (issue #790). Every TestCase class runs in
its own `python3 -m unittest` process, at most JOBS at a time, from the directory the command
was started in; each class prints its report in one piece when it ends. The exit status is
non-zero when a class fails, when a module cannot be imported, or when nothing is found.

Usage: unittest_parallel.py DIR [JOBS]    (JOBS defaults to the number of CPUs)
"""

import concurrent.futures
import os
import pathlib
import subprocess
import sys
import time
import unittest


def classes(directory: pathlib.Path) -> tuple[list[str], list[str]]:
    """The dotted names of the TestCase classes `discover -s DIR` would run, and the modules
    that failed to load."""
    sys.path.insert(0, str(directory))
    found: set[str] = set()
    broken: list[str] = []

    def walk(suite: unittest.TestSuite) -> None:
        for test in suite:
            if isinstance(test, unittest.TestSuite):
                walk(test)
            elif type(test).__name__ == "_FailedTest":
                broken.append(test.id())
            else:
                found.add(f"{type(test).__module__}.{type(test).__qualname__}")

    walk(unittest.defaultTestLoader.discover(str(directory)))
    return sorted(found), broken


def run_class(name: str, directory: pathlib.Path) -> tuple[str, int, str, float]:
    env = {**os.environ, "PYTHONPATH": os.pathsep.join([str(directory), os.environ.get("PYTHONPATH", "")]).rstrip(os.pathsep)}
    started = time.monotonic()
    done = subprocess.run(
        [sys.executable, "-B", "-m", "unittest", "-v", name],
        capture_output=True, text=True, env=env,
    )
    return name, done.returncode, done.stdout + done.stderr, time.monotonic() - started


def main(argv: list[str]) -> int:
    if len(argv) not in (1, 2):
        print(__doc__, file=sys.stderr)
        return 2
    directory = pathlib.Path(argv[0]).resolve()
    jobs = int(argv[1]) if len(argv) == 2 else os.cpu_count() or 1
    names, broken = classes(directory)
    for name in broken:
        print(f"cannot load {name}", file=sys.stderr)
    if not names or broken:
        print(f"{directory}: {'no test class found' if not names else 'a module failed to load'}", file=sys.stderr)
        return 1
    failed = []
    started = time.monotonic()
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        # Longest first is unknown here; the pool keeps `jobs` classes running as they end.
        for name, code, output, seconds in pool.map(lambda n: run_class(n, directory), names):
            print(f"=== {name} ({seconds:.1f}s, exit {code})\n{output}", flush=True)
            if code != 0:
                failed.append(name)
    print(f"{len(names)} classes, {jobs} at a time, {time.monotonic() - started:.1f}s; failed: {', '.join(failed) or 'none'}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
