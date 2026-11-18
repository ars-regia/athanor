#!/usr/bin/env bash
# Points the `stable` tag of the system images at the digests of one build run, on the
# evidence of that run (doc_update_delivery.md UD3, UD4, UD5; ADR-0106). Users follow
# :stable; :latest stays for testing. Nothing is signed here and no private key is needed.
# It runs only in the promote job of promote-stable.yml, in the environment release, after
# the approval (doc_pipeline.md PL60): on the evidence fetched again, it recomputes the
# record with scripts/ci/promotion-record.sh, the code that wrote the plan, refuses unless it
# equals the plan the bundle signed and the approval covered, moves the tags by digest and
# reads :stable back. The plan is scripts/ci/promotion-plan.sh's; this script has no plan mode.
# A re-run of the job after an attempt that moved every image of the plan moves nothing and
# succeeds, so the job's last step, system/promote-iso.sh, can be retried (ADR-0136).
# Besides `stable`, each promoted image gets `stable-previous` (the digest stable pointed at),
# `stable-<YYYYMMDD>` of the plan's day and `promoted-<run_id>`, written once and never moved:
# a second promotion of the day moves the day tag, this one stays. forge/scripts/clean_ghcr.sh
# keeps all four (UT10, UD8).
# Usage: promote.sh [--hardware-override athanor-system-nvidia] RUN_ID
# Environment: as scripts/ci/promotion-record.sh; $PROMOTE_ARTIFACTS/promotion.json is the
#              plan, verified against its copy off GitHub before this runs.
set -euo pipefail
shopt -s inherit_errexit

# shellcheck source-path=SCRIPTDIR
source "$(dirname "${BASH_SOURCE[0]}")/../scripts/ci/promotion-record.sh"
promotion_args "$@"
retry="$root/forge/scripts/retry.sh"
plan=$artifacts/promotion.json
[[ -f $plan ]] || die "no $plan: run scripts/ci/promotion-plan.sh first, then publish and verify the bundle"
day=$(jq -r '.day' "$plan")
[[ $day =~ ^[0-9]{8}$ ]] || die "$plan names no day"

promotion_record "$day" "$work/promotion.json"
# The plan as the record reads it once an earlier attempt of this run moved every image it
# names: each skipped as already stable at the plan's digest, with no image or package set
# left (ADR-0136). A re-run of the job then moves nothing and goes on to the ISO step; a
# partial apply differs from both and stops, as before.
jq -S --indent 2 '.skipped = ((.skipped + [.images[] | {name, reason: "already stable at \(.digest)"}]) | sort_by(.name))
    | .images = [] | .packages = {}' "$plan" > "$work/applied.json"
applied=false
if ! cmp -s "$work/promotion.json" "$plan"; then
    if ! cmp -s "$work/promotion.json" "$work/applied.json"; then
        echo "${0##*/}: the record computed now:" >&2
        cat "$work/promotion.json" >&2
        die "the promotion differs from the plan the bundle signed and the release approval covered (evidence, :stable or the override changed since): plan again"
    fi
    applied=true
fi

if $applied; then
    # :stable is the last tag an apply writes, so this run wrote the other three before it; a
    # :stable copied onto the digest by anything else (ADR-0111 item 5) lacks them and stops.
    entries=$(jq -r '.images[] | "\(.name) \(.digest) \(.previous_stable // "")"' "$plan")
    while read -r name digest prev; do
        repository=$REGISTRY/$name
        for tag in "promoted-$run" "stable-$day" stable-previous; do
            want=$digest
            if [[ $tag == stable-previous ]]; then
                [[ -n $prev ]] || continue
                want=$prev
            fi
            at=$(tag_of "$repository" "$tag")
            [[ $at == "$want" ]] ||
                die "$repository:stable names the plan's $digest, but :$tag names ${at:-nothing}, not $want: :stable was not moved by this promotion, plan again"
        done
        echo "stable -> $repository@$digest (applied by an earlier attempt of this run)"
    done <<< "$entries"
    echo "stable already at the plan: run $run is applied, nothing moved"
    exit 0
fi

for name in "${promote[@]}"; do
    repository=$REGISTRY/$name
    digest=${recorded[$name]}
    # Read again just before it moves: the copies of the images before this one take minutes
    # with their retries, and a :stable moved from outside meanwhile is not overwritten.
    stable=$(stable_of "$repository")
    [[ $stable == "${previous[$name]:-}" ]] ||
        die "$repository:stable moved to ${stable:-nothing} since the record named ${previous[$name]:-no stable}: plan again"
    # The immutable tag first: a partial apply that rerun finds it at the same digest and goes on.
    # Assigned first: a substitution inside [[ ]] would not stop the script when it fails.
    check_promoted "$repository" "$digest"
    at=$(tag_of "$repository" "promoted-$run")
    if [[ -z $at ]]; then
        bash "$retry" skopeo copy --preserve-digests "docker://$repository@$digest" "docker://$repository:promoted-$run"
        at=$(tag_of "$repository" "promoted-$run")
    fi
    [[ $at == "$digest" ]] || die "$repository:promoted-$run points at ${at:-nothing}, not the evidence digest $digest"
    if [[ -n ${previous[$name]:-} ]]; then
        bash "$retry" skopeo copy --preserve-digests "docker://$repository@${previous[$name]}" "docker://$repository:stable-previous"
    fi
    bash "$retry" skopeo copy --preserve-digests "docker://$repository@$digest" "docker://$repository:stable-$day"
    bash "$retry" skopeo copy --preserve-digests "docker://$repository@$digest" "docker://$repository:stable"
    now=$(skopeo inspect --format '{{.Digest}}' "docker://$repository:stable")
    [[ $now == "$digest" ]] || die "$repository:stable points at $now, not the evidence digest $digest"
    echo "stable -> $repository@$digest"
done
((${#promote[@]} > 0)) || echo "stable unchanged: run $run is the stable build already"
