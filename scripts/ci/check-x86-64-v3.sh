#!/usr/bin/env bash
# Fails with a named cause when this host is below x86-64-v3, the level `.cargo/config.toml`
# builds the workspace for (ADR-0143 decision 2, #1508): a test binary run on a lesser CPU dies
# with SIGILL, intermittently, on whichever runner the job lands on. The answer is the dynamic
# loader's own: it prints "x86-64-v3 (supported, searched)" when the CPU has the level, and
# omits "(supported" when it has not. A loader that prints no x86-64-v3 line at all (glibc
# older than 2.33) fails too: the level is unknown, not met.
#
# Callers: forge/test/shell/rig.sh for the subcommands that build or run workspace code (the guard
# is one line after the cargo cache setup, so that the line citations of the specifications below
# it stay put; forge/test/shell/tests/test_rig_v3_guard.py classifies every subcommand),
# forge/scripts/run_spec_build.sh, scripts/ci/security-audit.sh, fuzz.sh and mutants.sh.
#
# usage: scripts/ci/check-x86-64-v3.sh
# environment:
#   ATHANOR_LD_SO     the loader to ask (default /lib64/ld-linux-x86-64.so.2)
#   ATHANOR_CPUINFO   the file the CPU model is read from (default /proc/cpuinfo)
set -euo pipefail

loader=${ATHANOR_LD_SO:-/lib64/ld-linux-x86-64.so.2}
cpuinfo=${ATHANOR_CPUINFO:-/proc/cpuinfo}

report=$("$loader" --help) || {
    echo "${0##*/}: $loader --help failed, so the host's x86-64 level is unknown" >&2
    exit 1
}
if grep -q 'x86-64-v3 (supported' <<< "$report"; then
    exit 0
fi

# One sed that quits at the first model line: no pipe to `head`, which would end it with SIGPIPE
# (status 141 under pipefail) on a long file. An unreadable file keeps the named message.
if [ -r "$cpuinfo" ]; then
    model=$(sed -n '/^model name/{s/^[^:]*: *//p;q}' "$cpuinfo")
else
    model="unknown ($cpuinfo unreadable)"
fi
if grep -q 'x86-64-v3' <<< "$report"; then
    cause="the loader does not list x86-64-v3 as supported"
else
    cause="the loader prints no x86-64-v3 line (glibc older than 2.33?)"
fi
echo "${0##*/}: this host is below x86-64-v3 (${cause}); CPU model: ${model:-unknown}." >&2
echo "${0##*/}: the workspace is built with target-cpu=x86-64-v3 (ADR-0143) and its binaries would die with SIGILL here." >&2
exit 1
