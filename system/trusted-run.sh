#!/usr/bin/env bash
# Exits 0 when a workflow run may be trusted as a source of release inputs: a completed,
# successful run of one of the given workflow files, on RELEASE_BRANCH (and built from a commit
# contained in it), in this repository, started by push, schedule or workflow_dispatch. With --completed a failed run is trusted too: an acceptance
# that failed still carries the evidence of its fail verdict. An artifact is only as
# trustworthy as the run that uploaded it, and artifact names are free for any workflow to
# reuse (doc_update_delivery.md, UD4).
# Exit status: 0 trusted; 10 not trusted (every differing field named), a status neither jq
# (2 to 5) nor gh uses; anything else is a failure (2 is also this script's usage error).
# Usage: trusted-run.sh [--completed] RUN_ID WORKFLOW_FILE...
# Environment: GITHUB_REPOSITORY; RELEASE_BRANCH (default iso-v0); gh authenticated.
set -euo pipefail
shopt -s inherit_errexit

conclusions='["success"]'
if [[ ${1:-} == --completed ]]; then
    conclusions='["success", "failure"]'
    shift
fi
[[ $# -ge 2 && $1 =~ ^[0-9]+$ ]] || {
    echo "usage: ${0##*/} [--completed] RUN_ID WORKFLOW_FILE..." >&2
    exit 2
}
run=$(gh api "/repos/$GITHUB_REPOSITORY/actions/runs/$1")
why=$(jq -r --arg repo "$GITHUB_REPOSITORY" --arg branch "${RELEASE_BRANCH:-iso-v0}" \
    --argjson conclusions "$conclusions" --args '
  [ (if (.path | IN($ARGS.positional[])) then empty else "path \(.path)" end),
    (if .head_branch == $branch then empty else "branch \(.head_branch)" end),
    (if .head_repository.full_name == $repo then empty else "repository \(.head_repository.full_name)" end),
    (if (.event | IN("push", "schedule", "workflow_dispatch")) then empty else "event \(.event)" end),
    (if .status == "completed" then empty else "status \(.status)" end),
    (if (.conclusion | IN($conclusions[])) then empty else "conclusion \(.conclusion)" end)
  ] | join(", ")' "${@:2}" <<< "$run")
# A tag can be named like the release branch, so head_branch alone proves nothing: the commit
# the run built must be on the branch (the branch head is identical to it or ahead of it).
if [[ -z $why ]]; then
    branch=${RELEASE_BRANCH:-iso-v0}
    sha=$(jq -r '.head_sha' <<< "$run")
    [[ $sha =~ ^[0-9a-f]{40}$ ]] || why="sha '$sha' is not a commit"
fi
if [[ -z $why ]]; then
    # The branches endpoint knows branches only, and the compare below takes two commit ids:
    # a tag named like the branch can steer neither.
    branch_sha=$(gh api "/repos/$GITHUB_REPOSITORY/branches/$branch" --jq .commit.sha)
    [[ $branch_sha =~ ^[0-9a-f]{40}$ ]] || why="branch $branch has no commit id ('$branch_sha')"
fi
if [[ -z $why ]]; then
    status=$(gh api "/repos/$GITHUB_REPOSITORY/compare/$sha...$branch_sha" --jq .status)
    [[ $status == identical || $status == ahead ]] || why="sha $sha is not on $branch ($status)"
fi
[[ -z $why ]] || {
    echo "${0##*/}: run $1 is not a trusted release run: $why" >&2
    exit 10
}
