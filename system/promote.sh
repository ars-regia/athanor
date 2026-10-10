#!/usr/bin/env bash
# Points the `stable` tag of the system images at the digests of one build run, on the
# evidence of that run (doc_update_delivery.md UD3, UD4, UD5; ADR-0106). Users follow
# :stable; :latest stays for testing. Nothing is signed here and no private key is needed.
# It runs only in the promote job of promote-stable.yml, in the environment release, after
# the approval (doc_pipeline.md PL60): on the evidence fetched again, it recomputes the
# record with scripts/ci/promotion-record.sh, the code that wrote the plan, refuses unless it
# equals the plan the bundle signed and the approval covered, moves the tags by digest and
# reads :stable back. The plan is scripts/ci/promotion-plan.sh's; this script has no plan mode.
# Besides `stable`, each promoted image gets `stable-previous` (the digest stable pointed at)
# and `stable-<YYYYMMDD>` of the plan's day; forge/scripts/clean_ghcr.sh keeps all three (UT10).
# Usage: promote.sh [--hardware-override athanor-system-nvidia] RUN_ID
# Environment: as scripts/ci/promotion-record.sh; $PROMOTE_ARTIFACTS/promotion.json is the
#              plan, verified against its copy off GitHub before this runs.
set -euo pipefail
shopt -s inherit_errexit

# shellcheck source-path=SCRIPTDIR
source "$(dirname "${BASH_SOURCE[0]}")/../scripts/ci/promotion-record.sh"
promotion_args "$@"
plan=$artifacts/promotion.json
[[ -f $plan ]] || die "no $plan: run scripts/ci/promotion-plan.sh first, then publish and verify the bundle"
day=$(jq -r '.day' "$plan")
[[ $day =~ ^[0-9]{8}$ ]] || die "$plan names no day"

promotion_record "$day" "$work/promotion.json"
if ! cmp -s "$work/promotion.json" "$plan"; then
    echo "${0##*/}: the record computed now:" >&2
    cat "$work/promotion.json" >&2
    die "the promotion differs from the plan the bundle signed and the release approval covered (evidence, :stable or the override changed since): plan again"
fi

for name in "${promote[@]}"; do
    repository=$REGISTRY/$name
    digest=${recorded[$name]}
    # Read again just before it moves: the copies of the images before this one take minutes
    # with their retries, and a :stable moved from outside meanwhile is not overwritten.
    stable=$(stable_of "$repository")
    [[ $stable == "${previous[$name]:-}" ]] ||
        die "$repository:stable moved to ${stable:-nothing} since the record named ${previous[$name]:-no stable}: plan again"
    if [[ -n ${previous[$name]:-} ]]; then
        bash "$retry" skopeo copy --preserve-digests "docker://$repository@${previous[$name]}" "docker://$repository:stable-previous"
    fi
    bash "$retry" skopeo copy --preserve-digests "docker://$repository@$digest" "docker://$repository:stable-$day"
    bash "$retry" skopeo copy --preserve-digests "docker://$repository@$digest" "docker://$repository:stable"
    now=$(skopeo inspect --format '{{.Digest}}' "docker://$repository:stable")
    [[ $now == "$digest" ]] || die "$repository:stable points at $now, not the evidence digest $digest"
    echo "stable -> $repository@$digest"
done
