#!/usr/bin/env bash
# Opens the PR of a bump that bump.py has applied to the working tree.
#
#   open_bump_pr.sh LABEL BRANCH_PREFIX PATH OUT_DIR [BASE]
#
# OUT_DIR holds `title` and `body.md`, which bump.py writes only when something moved. Nothing
# there, or a PR with LABEL still open: nothing is done. Otherwise PATH is committed on a new
# branch BRANCH_PREFIX-<last word of the title, the new NVR>, pushed, and a PR with LABEL is
# opened against BASE (default: the current branch; give it on a detached HEAD). A branch of
# that name already on the remote, left by a run that failed after the push, stops the script
# naming it: remove it, or open its PR by hand. The PR is never merged here. Needs git and an authenticated gh. The
# summary goes to $GITHUB_STEP_SUMMARY when it is set, else to stdout.
set -euo pipefail

(($# >= 4 && $# <= 5)) || { sed -n '2,13p' "$0" >&2; exit 2; }
label=$1 prefix=$2 path=$3 out=$4
base=${5:-$(git rev-parse --abbrev-ref HEAD)}
[[ $base != HEAD ]] || { echo "open_bump_pr.sh: detached HEAD: give BASE" >&2; exit 2; }
summary=${GITHUB_STEP_SUMMARY:-/dev/stdout}

if [[ ! -s $out/title ]]; then
    echo "unchanged: nothing to open" >> "$summary"
    exit 0
fi
open=$(gh pr list --label "$label" --state open --json number --jq 'map("#\(.number)") | join(", ")')
if [[ -n $open ]]; then
    echo "PR ${open} with the label ${label} is open: nothing to do until it is closed" >> "$summary"
    exit 0
fi

title=$(< "$out/title")
branch="${prefix}-${title##* }"
if git ls-remote --exit-code --heads origin "$branch" > /dev/null; then
    echo "open_bump_pr.sh: the branch ${branch} is already on the remote without an open PR with the label ${label}: remove it or open its PR" >&2
    exit 1
fi
gh label create "$label" --force --description "Opened by a bump bot" > /dev/null
git add -- "$path"
git config user.name "${BUMP_GIT_NAME:-athanor-bump}"
git config user.email "${BUMP_GIT_EMAIL:-kernel@athanor.os}"
git checkout -q -b "$branch"
git commit -q -m "$title" -m "Opened by the bump bot (${label})."
git push -q -u origin "$branch"
url=$(gh pr create --base "$base" --head "$branch" --label "$label" --title "$title" --body-file "$out/body.md")
echo "PR: ${url} (not merged: the DAG build and a review decide)" >> "$summary"
