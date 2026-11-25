"""A loader and a cpuinfo that report x86-64-v3 as supported, for the tests that run rig.sh and the
scripts it guards with scripts/ci/check-x86-64-v3.sh (#1508): their result must not depend on the
CPU of the machine that runs them. ENV holds the two variables the check reads."""

import atexit
import shutil
import tempfile
from pathlib import Path

_dir = Path(tempfile.mkdtemp(prefix="athanor-fake-v3-"))
atexit.register(shutil.rmtree, _dir, True)
_loader = _dir / "ld.so"
_loader.write_text("#!/bin/sh\necho '  x86-64-v3 (supported, searched)'\n")
_loader.chmod(0o755)
(_dir / "cpuinfo").write_text("model name\t: Fake CPU\n")

ENV = {"ATHANOR_LD_SO": str(_loader), "ATHANOR_CPUINFO": str(_dir / "cpuinfo")}
