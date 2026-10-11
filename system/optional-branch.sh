#!/usr/bin/env bash
# Runs the build of one NVIDIA module branch into OUT_DIR. When the branch is optional (the
# legacy branch in the kmod cycle, ADR-0103 D24) and its build fails, OUT_DIR is removed
# whatever it holds, so nothing half-built can be uploaded, the marker OUT_DIR.dropped is
# written for the upload step's condition, the failure is named in an annotation and in the
# job summary, and the step succeeds, so the run goes on with the other branch. A required
# branch, or a source that does not match its pin (exit 4 of nvidia.sh), ends the step with
# the command's status: an integrity failure is never a build failure the run may leave.
# Usage: optional-branch.sh BRANCH true|false OUT_DIR -- COMMAND...
set -euo pipefail
shopt -s inherit_errexit

[[ $# -ge 5 && $2 =~ ^(true|false)$ && -n $3 && $4 == -- ]] \
  || { echo "usage: ${0##*/} BRANCH true|false OUT_DIR -- COMMAND..." >&2; exit 2; }
branch=$1 optional=$2 out=$3
shift 4
rm -f -- "$out.dropped"
status=0
"$@" || status=$?
if (( status != 0 && status != 4 )) && [[ $optional == true ]]; then
  if [[ -e $out ]]; then rm -r -- "$out"; fi
  echo "the $branch NVIDIA modules failed to build (exit $status)" > "$out.dropped"
  echo "::error title=Branch dropped::the $branch NVIDIA modules failed to build (exit $status); this run continues without them and builds no image that needs them (ADR-0103 D24)"
  if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then
    echo "- :warning: the **$branch** NVIDIA modules failed to build; the run continues without them (ADR-0103 D24)" >> "$GITHUB_STEP_SUMMARY"
  fi
  exit 0
fi
exit "$status"
