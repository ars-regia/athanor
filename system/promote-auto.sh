#!/usr/bin/env bash
# Chooses the run promote-stable.yml promotes and hands it to system/promote.sh with its
# acceptance evidence (docs/architecture/doc_update_trust.md, D1). Three modes:
#   scheduled  no RUN_ID: the newest run whose evidence says "pass" and is older than the
#              dwell time (PROMOTE_DWELL_HOURS, default 24), if it is newer than :stable;
#              nothing to promote is a success;
#   manual     RUN_ID set: that run, with no dwell time; the maintainer's override;
#   security   SECURITY_REASON set (with or without RUN_ID): no dwell time, and the reason is
#              recorded. The out-of-band path for a critical fix (48 h freshness target).
# Every mode still goes through promote.sh, so no run reaches :stable without passing
# evidence for its exact digests and a signature machines verify.
#
# Usage: promote-auto.sh EVIDENCE_DIR RECORD
#   EVIDENCE_DIR  acceptance-<run>.json files, at any depth (one per acceptance artifact)
#   RECORD        the promotion record written when a run is promoted (JSON)
# Environment: RUN_ID, SECURITY_REASON, PROMOTE_DWELL_HOURS; REGISTRY as promote.sh.
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 2 && -d $1 ]] || {
    echo "usage: ${0##*/} EVIDENCE_DIR RECORD" >&2
    exit 2
}
dir=$1 record=$2
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
run=${RUN_ID:-} reason=${SECURITY_REASON:-}
owner=${GITHUB_REPOSITORY_OWNER:-}
REGISTRY=${REGISTRY:-${owner:+ghcr.io/${owner,,}}}
[[ -n $REGISTRY ]] || {
    echo "${0##*/}: set REGISTRY or GITHUB_REPOSITORY_OWNER" >&2
    exit 2
}
export REGISTRY
[[ -z $run || $run =~ ^[0-9]+$ ]] || {
    echo "${0##*/}: RUN_ID must be a run id" >&2
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

# One line per passing evidence file: run, finished_at in seconds, path; newest run first, and
# for a run tested more than once its latest pass first.
passing() {
    find "$dir" -type f -name 'acceptance-*.json' -print0 |
        xargs -0 -r jq -r 'select(.schema == 1 and .result == "pass" and (.run_id | test("^[0-9]+$")))
          | "\(.run_id) \(.finished_at | fromdateiso8601) \(input_filename)"' |
        sort -k1,1nr -k2,2nr
}

created() { # created REF -> seconds since the epoch, from org.opencontainers.image.created
    date -u -d "$(skopeo inspect --config "docker://$1" | jq -r '.config.Labels["org.opencontainers.image.created"]')" +%s
}

evidence=
now=$(date -u +%s)
while read -r candidate finished path; do
    if [[ -n $run ]]; then
        [[ $candidate == "$run" ]] || continue
    elif ((now - finished < dwell * 3600)); then
        echo "run $candidate passed acceptance less than ${dwell} h ago: waiting"
        continue
    fi
    evidence=$path
    run=$candidate
    break
done < <(passing)

if [[ -z $evidence ]]; then
    if [[ $mode == scheduled ]]; then
        echo "nothing to promote: no accepted run is past the ${dwell} h dwell time"
        exit 0
    fi
    echo "${0##*/}: no passing acceptance evidence for run ${run} under $dir" >&2
    exit 1
fi

if [[ $mode == scheduled ]]; then
    # The scheduled run is quiet when :stable is already this run or newer; promote.sh would
    # refuse the second case, which is an error only when someone asked for it.
    system=$REGISTRY/athanor-system
    candidate_digest=$(jq -r '.images["athanor-system"]' "$evidence")
    err=$(mktemp)
    trap 'rm -f "$err"' EXIT
    if stable=$(skopeo inspect --format '{{.Digest}}' "docker://$system:stable" 2> "$err"); then
        if [[ $stable == "$candidate_digest" ]] ||
            (($(created "$system@$candidate_digest") <= $(created "$system@$stable"))); then
            echo "nothing to promote: :stable already holds run $run or a newer one"
            exit 0
        fi
    elif ! grep -q 'manifest unknown' "$err"; then
        cat "$err" >&2
        exit 1
    fi
fi

echo "promoting run $run ($mode, dwell ${dwell} h) with $evidence"
PROMOTE_DWELL_HOURS=$dwell bash "$root/system/promote.sh" "$run" "$evidence"
mkdir -p "$(dirname "$record")"
jq -n --arg run "$run" --arg mode "$mode" --arg reason "$reason" --argjson dwell "$dwell" \
    --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --slurpfile evidence "$evidence" \
    '{schema: 1, run_id: $run, mode: $mode, reason: (if $reason == "" then null else $reason end),
      dwell_hours: $dwell, promoted_at: $at, evidence: $evidence[0]}' > "$record"
