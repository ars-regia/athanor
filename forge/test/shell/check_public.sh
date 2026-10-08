#!/usr/bin/env bash
# check_public.sh REFFILE - the reference in REFFILE can be pulled without credentials, as the
# hosted gate does. Fails with what the maintainer has to do when it cannot.
set -euo pipefail
(($# == 1)) || { sed -n '2,3p' "$0" >&2; exit 2; }
ref=$(< "$1")
skopeo inspect --no-creds --raw "docker://${ref}" > /dev/null || {
    echo "check_public.sh: ${ref} cannot be pulled anonymously" >&2
    echo "make the package athanor-shell-rig-build public, then re-run the failed jobs" >&2
    exit 1
}
