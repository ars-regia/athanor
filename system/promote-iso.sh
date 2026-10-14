#!/usr/bin/env bash
# The iso job of promote-stable.yml (ADR-0114, doc_pipeline.md PL35): moves athanor-iso:stable
# to the ISO the promotion record names, after promote.sh moved the system images. It refuses
# unless athanor-system:stable still names the run's athanor-system digest, so a late re-run
# of an older promotion's iso job cannot put the ISO behind the system images it installs.
# Usage: promote-iso.sh RUN_ID PLAN
#        (PLAN is the promotion.json system/evidence-bundle.sh verify checked against the copy
#        off GitHub; promotion-record.sh wrote the athanor-system digest the ISO was built from
#        into it as iso_system, also when that image was skipped as already stable)
# Environment: REGISTRY (REGISTRY_HOST/owner), GITHUB_REF (tag-images.sh); skopeo logged in.
set -euo pipefail
shopt -s inherit_errexit

die() {
    echo "${0##*/}: $*" >&2
    exit 1
}
[[ $# -eq 2 && $1 =~ ^[0-9]+$ && -n ${REGISTRY:-} ]] || {
    echo "usage: REGISTRY=REGISTRY/OWNER ${0##*/} RUN_ID PLAN" >&2
    exit 2
}
run=$1 plan=$2
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

planned=$(jq -er '.run_id' "$plan")
[[ $planned == "$run" ]] || die "$plan is the plan of run $planned, not $run"
iso=$(jq -er '.iso' "$plan")
system=$(jq -er '.iso_system' "$plan")
[[ $system =~ ^sha256:[0-9a-f]{64}$ ]] || die "$plan names no athanor-system digest for its ISO (iso_system)"
stable=$(bash "$root/forge/scripts/retry.sh" skopeo inspect --format '{{.Digest}}' "docker://$REGISTRY/athanor-system:stable")
[[ $stable == "$system" ]] ||
    die "$REGISTRY/athanor-system:stable names $stable, not run $run's $system: the system images moved on, and the ISO's stable is not moved behind them"
bash "$root/system/tag-images.sh" --registry "$REGISTRY" --tag stable --iso "$run" "$iso"
