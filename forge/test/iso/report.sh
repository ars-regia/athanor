#!/usr/bin/env bash
# report.sh DIR...: converts the screenshots of each acceptance output directory and appends
# its verdict to the step summary. A directory with nothing in it (the case never started)
# is skipped, and a failure in one directory does not skip the next; the exit status is
# non-zero if any directory failed.
set -uo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
summary=${GITHUB_STEP_SUMMARY:-/dev/stdout}
status=0
for dir in "$@"; do
    if compgen -G "${dir}/*.ppm" > /dev/null; then
        python3 "${here}/screenshots.py" "./${dir}" || status=$?
    fi
    if [[ -f ${dir}/verdict.md ]]; then
        { echo "### ${dir}"; cat "${dir}/verdict.md"; } >> "$summary" || status=$?
    fi
done
exit "$status"
