#!/usr/bin/env bash
# Chooses the run promote-stable.yml promotes and hands it to system/promote.sh
# (docs/architecture/doc_update_trust.md, D1). Three modes:
#   scheduled  no RUN_ID: the newest Orchestrator run on a trusted branch that promote.sh
#              accepts with the dwell time (PROMOTE_DWELL_HOURS, default 24); a run that is
#              not eligible (4) or not yet (5, inside the dwell time) gives way to the next
#              older one, and a run not newer than :stable ends the search. Nothing to promote
#              is a success only when that is the reason: :stable already holds the newest
#              eligible run, or one is waiting out the dwell time. When the run list cannot
#              be read, or none of the runs looked at is eligible or waiting, the chain is
#              broken (no acceptance attestation, no build signature) and this fails;
#   manual     RUN_ID set: that run, with no dwell time; the maintainer's override;
#   security   SECURITY_REASON set (with or without RUN_ID): no dwell time, and the reason is
#              recorded. The out-of-band path for a critical fix (48 h freshness target).
# This script only picks a candidate, from the GitHub run list. It trusts nothing it reads
# there: promote.sh decides, from the acceptance attestation on each image digest, signed by
# iso-acceptance.yml on a trusted branch, and from the signature machines verify.
#
# Usage: promote-auto.sh RECORD
#   RECORD  the promotion record written when a run is promoted (JSON): the mode, the reason,
#           the dwell time and the acceptance evidence promote.sh verified
# Environment: RUN_ID, SECURITY_REASON, PROMOTE_DWELL_HOURS, PROMOTE_TRUSTED_REFS (default
#              iso-v0), PROMOTE_CANDIDATES (runs looked at, default 20), GITHUB_REPOSITORY;
#              REGISTRY as promote.sh; gh authenticated.
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 1 ]] || {
    echo "usage: ${0##*/} RECORD" >&2
    exit 2
}
record=$1
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
run=${RUN_ID:-} reason=${SECURITY_REASON:-} limit=${PROMOTE_CANDIDATES:-20}
[[ -z $run || $run =~ ^[0-9]+$ ]] || {
    echo "${0##*/}: RUN_ID must be a run id" >&2
    exit 2
}
[[ $limit =~ ^[0-9]+$ ]] || {
    echo "${0##*/}: PROMOTE_CANDIDATES must be a number" >&2
    exit 2
}
if [[ -n $reason ]]; then
    mode=security dwell=0
elif [[ -n $run ]]; then
    mode=manual dwell=0
else
    mode=scheduled dwell=${PROMOTE_DWELL_HOURS:-24}
fi
[[ $dwell =~ ^[0-9]+$ ]] || {
    echo "${0##*/}: PROMOTE_DWELL_HOURS must be a whole number of hours" >&2
    exit 2
}
work=$(mktemp -d)
trap 'rm -r "$work"' EXIT

candidates() { # the Orchestrator runs of the trusted branches; fails when a listing does
    local branch ids
    read -ra branches <<< "${PROMOTE_TRUSTED_REFS:-iso-v0}"
    for branch in "${branches[@]}"; do
        ids=$(gh run list --repo "${GITHUB_REPOSITORY:?}" --workflow athanor-forge-orchestrator.yml \
            --branch "$branch" --limit "$limit" --json databaseId | jq -r '.[].databaseId') || return
        printf '%s\n' "$ids"
    done
}

promote() { # promote RUN -> promote.sh's exit status
    PROMOTE_DWELL_HOURS=$dwell PROMOTE_EVIDENCE_OUT=$work/evidence.json bash "$root/system/promote.sh" "$1"
}

status=0
if [[ -n $run ]]; then
    promote "$run" || exit
else
    listing=$(candidates) || {
        echo "${0##*/}: cannot list the Orchestrator runs: no promotion decision is possible" >&2
        exit 1
    }
    runs=() waiting=''
    [[ -z ${listing//[[:space:]]/} ]] || mapfile -t runs < <(grep . <<< "$listing" | sort -rnu)
    for run in "${runs[@]}"; do
        status=0
        promote "$run" || status=$?
        case $status in
        0) break ;;
        3)
            echo "nothing to promote: :stable already holds run $run or a newer one"
            exit 0
            ;;
        4) echo "run $run is not eligible, trying the one before" ;;
        5)
            waiting=${waiting:-$run}
            echo "run $run is waiting out the dwell time, trying the one before"
            ;;
        *) exit "$status" ;;
        esac
    done
    if [[ ${#runs[@]} -eq 0 ]]; then
        echo "nothing to promote: no Orchestrator run on ${PROMOTE_TRUSTED_REFS:-iso-v0}"
        exit 0
    elif [[ $status -ne 0 && -n $waiting ]]; then
        echo "nothing to promote yet: run $waiting passed acceptance less than ${dwell} h ago"
        exit 0
    elif [[ $status -ne 0 ]]; then
        echo "${0##*/}: none of the last ${#runs[@]} Orchestrator runs is eligible and :stable holds none of them:" \
            "the acceptance attestations or the build signatures are missing (see the reasons above)" >&2
        exit 1
    fi
fi

mkdir -p "$(dirname "$record")"
jq -n --arg run "$run" --arg mode "$mode" --arg reason "$reason" --argjson dwell "$dwell" \
    --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --slurpfile evidence "$work/evidence.json" \
    '{schema: 1, run_id: $run, mode: $mode, reason: (if $reason == "" then null else $reason end),
      dwell_hours: $dwell, promoted_at: $at, evidence: $evidence[0]}' > "$record"
echo "promoted run $run ($mode, dwell ${dwell} h)"
