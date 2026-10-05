#!/usr/bin/env bash
# Refuses to go on unless the GitHub environment this job runs in requires a reviewer.
# promote-stable.yml runs the dwell-skipping paths (a run id, a security reason) in the
# stable-override environment, so a person other than the dispatcher's write access approves
# them. GitHub creates an environment named by a job on first use, with no protection: without
# this check a missing or unconfigured environment would turn the approval into a no-op.
# Usage: require-review.sh ENVIRONMENT
# Environment: GITHUB_REPOSITORY; GH_TOKEN able to read the environment (actions: read).
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 1 && $1 =~ ^[A-Za-z0-9._-]+$ ]] || {
    echo "usage: ${0##*/} ENVIRONMENT" >&2
    exit 2
}
environment=$1
config=$(gh api "/repos/${GITHUB_REPOSITORY:?}/environments/$environment")
reviewers=$(jq '[.protection_rules[]? | select(.type == "required_reviewers") | .reviewers[]?] | length' <<< "$config")
if [[ $reviewers -eq 0 ]]; then
    echo "${0##*/}: environment $environment has no required reviewers: an override would run unreviewed." \
        "Set Settings > Environments > $environment > Required reviewers (docs/architecture/doc_update_trust.md, D1)." >&2
    exit 1
fi
echo "environment $environment requires review ($reviewers reviewer(s)): approved"
