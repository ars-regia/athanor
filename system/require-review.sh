#!/usr/bin/env bash
# Refuses to go on unless the GitHub environment this job runs in requires a reviewer, and an
# independent person with write access approved this very run in it.
# promote-stable.yml runs the dwell-skipping paths (a run id, a security reason) in the
# stable-override environment, so someone other than the dispatcher approves them. GitHub's own
# gate is not enough on its own terms: it creates an environment named by a job on first use
# with no protection, lets administrators bypass the rules by default, and lets a reviewer
# approve a run they started. So, failing closed on any API error:
#   - the environment must have a required-reviewers rule with at least one reviewer;
#   - this run (GITHUB_RUN_ID; approvals are per run, so none is carried over from another)
#     must carry an "approved" review for that environment from a user account (not a bot or
#     an app) that is neither the actor who dispatched the run nor the one who re-ran it, and
#     who has write or admin permission on the repository now. A rejection, a bypass that left
#     no review, or a review by anyone else counts for nothing.
# Usage: require-review.sh ENVIRONMENT
# Environment: GITHUB_REPOSITORY, GITHUB_RUN_ID, GITHUB_ACTOR, GITHUB_TRIGGERING_ACTOR (set by
#              GitHub Actions); GH_TOKEN able to read the environment and the run (actions: read).
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 1 && $1 =~ ^[A-Za-z0-9._-]+$ ]] || {
    echo "usage: ${0##*/} ENVIRONMENT" >&2
    exit 2
}
environment=$1
repo=${GITHUB_REPOSITORY:?} run=${GITHUB_RUN_ID:?} actor=${GITHUB_ACTOR:?}
triggering=${GITHUB_TRIGGERING_ACTOR:-$actor}
[[ $run =~ ^[0-9]+$ ]] || {
    echo "${0##*/}: GITHUB_RUN_ID is not a run id: '$run'" >&2
    exit 2
}

config=$(gh api "/repos/$repo/environments/$environment")
reviewers=$(jq '[.protection_rules[]? | select(.type == "required_reviewers") | .reviewers[]?] | length' <<< "$config")
if [[ $reviewers -eq 0 ]]; then
    echo "${0##*/}: environment $environment has no required reviewers: an override would run unreviewed." \
        "Set Settings > Environments > $environment > Required reviewers (docs/architecture/doc_update_trust.md, D1)." >&2
    exit 1
fi

approvals=$(gh api --paginate "/repos/$repo/actions/runs/$run/approvals" | jq -s 'add // []')
approvers=$(jq -r --arg env "$environment" '
  .[] | select(.state == "approved" and any(.environments[]?; .name == $env) and .user.type == "User")
  | .user.login' <<< "$approvals" | LC_ALL=C sort -u)
for login in $approvers; do
    [[ $login =~ ^[A-Za-z0-9-]+$ ]] || continue
    if [[ ${login,,} == "${actor,,}" || ${login,,} == "${triggering,,}" ]]; then
        echo "not counted: $login approved a run they started"
        continue
    fi
    permission=$(gh api "/repos/$repo/collaborators/$login/permission" --jq .permission)
    if [[ $permission == admin || $permission == write ]]; then
        echo "environment $environment: run $run approved by $login ($permission)"
        exit 0
    fi
    echo "not counted: $login has $permission permission, not write"
done
echo "${0##*/}: run $run has no approval for $environment from a person with write access other than $actor:" \
    "the override does not run" >&2
exit 1
