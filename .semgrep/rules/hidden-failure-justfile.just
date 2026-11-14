probe:
    #!/usr/bin/env bash
    set -euo pipefail
    # ruleid: hidden-failure-justfile
    rm -f "$f" || true
    # ruleid: hidden-failure-justfile
    probe 2> /dev/null
    # ok: hidden-failure-justfile
    command -v probe > /dev/null || { echo "probe is not on PATH" >&2; exit 1; }
    # ok: hidden-failure-justfile
    while kill -0 "$pid" 2> /dev/null; do sleep 1; done
