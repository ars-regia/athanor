#!/usr/bin/env bash
# Gathers what system/promote.sh reads for one build run (doc_update_delivery.md, UD4) into
# one directory: the run's digests and package sets from the run itself, and for each gate
# the newest evidence artifact whose run trusted-run.sh accepts. Trust is checked before the
# pick: an artifact name is free for any workflow to reuse, so an untrusted artifact is
# named and passed over, and never hides an older trusted one.
# Usage: fetch-evidence.sh RUN_ID ARTIFACTS_DIR
# Environment: GITHUB_REPOSITORY; gh authenticated; BUILD_WORKFLOWS, ACCEPT_WORKFLOWS,
#              SIGNATURE_WORKFLOWS (space-separated workflow paths), RELEASE_BRANCH.
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 2 && $1 =~ ^[0-9]+$ ]] || {
    echo "usage: ${0##*/} RUN_ID ARTIFACTS_DIR" >&2
    exit 2
}
run=$1 out=$2
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
orch=.github/workflows/athanor-forge-orchestrator.yml
declare -A gate_workflows=(
    ['iso-acceptance']=${ACCEPT_WORKFLOWS:-.github/workflows/accept.yml}
    [signature]=${SIGNATURE_WORKFLOWS:-$orch .github/workflows/release.yml}
)
# A failed acceptance still carries the evidence of its fail verdict; a signature is only
# written by a run that verified.
declare -A gate_flags=(['iso-acceptance']=--completed [signature]='')
read -ra build <<< "${BUILD_WORKFLOWS:-$orch}"

# A gate without trusted evidence downloads nothing, so a file left by another run would pass
# for this run's evidence.
[[ ! -e $out ]] || [[ -d $out && -z $(find "$out" -mindepth 1 -print -quit) ]] || {
    echo "${0##*/}: $out is not empty: evidence left there would be read as run $run's" >&2
    exit 2
}

bash "$root/system/trusted-run.sh" "$run" "${build[@]}"
mkdir -p "$out/evidence" "$out/packages"
gh run download "$run" -n image-digests -D "$out"
gh run download "$run" -n package-sets -D "$out/packages"

for gate in iso-acceptance signature; do
    name=evidence-$run-$gate
    read -ra workflows <<< "${gate_workflows[$gate]}"
    read -ra flags <<< "${gate_flags[$gate]}"
    candidates=$(gh api "/repos/$GITHUB_REPOSITORY/actions/artifacts?name=$name&per_page=100" |
        jq -r --arg name "$name" '
          if (.total_count // 0) > (.artifacts | length)
          then error("\(.total_count) artifacts named \($name), more than one page: refusing to pick from part of them")
          else [.artifacts[] | select(.expired | not)] | sort_by(.created_at) | reverse | .[].workflow_run.id end')
    picked=''
    while read -r source_run; do
        [[ -n $source_run ]] || continue
        status=0
        bash "$root/system/trusted-run.sh" "${flags[@]}" "$source_run" "${workflows[@]}" || status=$?
        case $status in
        0)
            picked=$source_run
            break
            ;;
        10) echo "::warning title=Untrusted evidence::$name of run $source_run is not from a trusted release run: ignored" ;;
        *) exit "$status" ;;
        esac
    done <<< "$candidates"
    if [[ -z $picked ]]; then
        echo "${0##*/}: no trusted $name artifact; promote.sh will refuse the run without it" >&2
        continue
    fi
    # The run is trusted for this gate only: every file it carries must be one of this gate.
    gh run download "$picked" -n "$name" -D "$out/.$gate"
    for file in "$out/.$gate"/*; do
        [[ -f $file && ${file##*/} == "$gate".*.json ]] || {
            echo "${0##*/}: $name of run $picked carries ${file##*/}, not a $gate evidence file" >&2
            exit 1
        }
    done
    mv "$out/.$gate"/* "$out/evidence/"
    rmdir "$out/.$gate"
done
