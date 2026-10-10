# shellcheck shell=bash
# Sourced, not run: the checks of one promotion and its record (doc_update_delivery.md UD3,
# UD4, UD5; ADR-0103 D16, D24; ADR-0106). scripts/ci/promotion-plan.sh (the plan job, no
# environment) and system/promote.sh (the promote job, environment release) both source it,
# so the record the bundle signs and the release approval covers is computed by the same code
# that checks it again after the approval (doc_pipeline.md PL60). Nothing here writes to a
# registry: the only copy is the policy pull into a local directory.
# The checks, for each image promoted:
#   - image-digests.txt is this run's and passes image-digests.sh --check, and :RUN_ID still
#     names the recorded digest;
#   - the evidence of UD4 for that digest passes evidence.py check: iso-acceptance and
#     signature for athanor-system; signature for athanor-system-nvidia, which promotes only
#     with --hardware-override (ADR-0106 item 5); athanor-system-nvidia-legacy never before
#     its hardware evidence in 0.9 (release-1.0.md 4.2);
#   - the newest evidence is older than the dwell (UD5), and the package set is there (D16);
#   - the image carries the classic cosign attachment machines verify, and it pulls through
#     the policy rendered from the public keys under system/keys, as a machine would;
#   - its build time is newer than the current stable's: a machine never follows a tag
#     backwards (UT5).
# Defines promotion_args "$@" (sets run and override) and promotion_record DAY OUT (runs
# every check, writes the record to OUT, sets promote, recorded and previous).
# Environment: REGISTRY (default ghcr.io/<GITHUB_REPOSITORY_OWNER>); PROMOTE_ARTIFACTS
#              (default artifacts); PROMOTE_DWELL_HOURS (default 24); PROMOTE_KEYS_DIR
#              (default system/keys); skopeo logged in.

usage() {
    echo "usage: ${0##*/} [--hardware-override athanor-system-nvidia] RUN_ID" >&2
    exit 2
}
die() {
    echo "${0##*/}: $*" >&2
    exit 1
}

promotion_args() {
    override=''
    while [[ $# -gt 1 ]]; do
        case $1 in
        --hardware-override)
            [[ ${2:-} == athanor-system-nvidia ]] || usage
            override=$2
            shift 2
            ;;
        *) usage ;;
        esac
    done
    [[ $# -eq 1 && $1 =~ ^[0-9]+$ ]] || usage
    run=$1
    local owner=${GITHUB_REPOSITORY_OWNER:-}
    REGISTRY=${REGISTRY:-${owner:+ghcr.io/${owner,,}}}
    [[ -n $REGISTRY ]] || {
        echo "${0##*/}: set REGISTRY or GITHUB_REPOSITORY_OWNER" >&2
        exit 2
    }
}

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
retry="$root/forge/scripts/retry.sh"
keys_dir=${PROMOTE_KEYS_DIR:-$root/system/keys}
artifacts=${PROMOTE_ARTIFACTS:-artifacts}
dwell=$((${PROMOTE_DWELL_HOURS:-24} * 3600))
SIMPLE_SIGNING=application/vnd.dev.cosign.simplesigning.v1+json
work=$(mktemp -d)
trap 'rm -r "$work"' EXIT
err=$work/err

created() { # created REPOSITORY DIGEST -> seconds since the epoch
    local label
    label=$(skopeo inspect --config "docker://$1@$2" | jq -r '.config.Labels["org.opencontainers.image.created"] // empty')
    [[ -n $label ]] || {
        echo "${0##*/}: $1@$2 has no org.opencontainers.image.created label" >&2
        return 1
    }
    date -u -d "$label" +%s
}

evidence() { # evidence GATE IMAGE DIGEST -> the evidence's finished_at, seconds since the epoch
    python3 -B "$root/scripts/ci/evidence.py" check --dir "$artifacts/evidence" --gate "$1" --image "$2" --digest "$3" --run-id "$run"
}

sums() { # sums PATH... (relative to $artifacts) -> {"PATH": "<sha256>", ...}
    (cd "$artifacts" && sha256sum -- "$@") |
        jq -R -s '[splits("\n") | select(length > 0) | capture("^(?<sha>[0-9a-f]{64})  (?<path>.+)$") | {(.path): .sha}] | add'
}

promotion_record() { # promotion_record DAY OUT
    local day=$1 out=$2 repository tag digest name gate finished current stable age images skips trigger evidence_sums package_sums
    local -a gates package_files evidence_files
    bash "$root/forge/specs/athanor-update/SOURCES/usr/libexec/athanor-update/render-policy" \
        --registry "$REGISTRY" --keys-dir "$keys_dir" --out "$work/policy"
    bash "$root/system/image-digests.sh" --registry "$REGISTRY" --check "$artifacts/image-digests.txt"
    declare -gA recorded=() previous=()
    while read -r repository tag digest; do
        [[ $tag == "$run" ]] || die "image-digests.txt is for run $tag, not $run"
        recorded[${repository##*/}]=$digest
    done < "$artifacts/image-digests.txt"

    local -A skipped=()
    promote=()
    local newest=0
    for name in athanor-system athanor-system-nvidia athanor-system-nvidia-legacy; do
        digest=${recorded[$name]:-}
        if [[ -z $digest ]]; then
            skipped[$name]="not built by run $run"
            continue
        fi
        case $name in
        athanor-system) gates=(iso-acceptance signature) ;;
        athanor-system-nvidia)
            if [[ $override != "$name" ]]; then
                skipped[$name]="no hardware evidence and no hardware override (UD4, ADR-0106)"
                continue
            fi
            gates=(signature)
            ;;
        athanor-system-nvidia-legacy)
            skipped[$name]="not promoted before its hardware evidence in 0.9 (release-1.0.md 4.2)"
            continue
            ;;
        esac
        for gate in "${gates[@]}"; do
            finished=$(evidence "$gate" "$name" "$digest")
            if ((finished > newest)); then newest=$finished; fi
        done
        [[ -s $artifacts/packages/$name.txt ]] || die "no package set for $name ($artifacts/packages/$name.txt, D16)"

        repository=$REGISTRY/$name
        current=$(skopeo inspect --format '{{.Digest}}' "docker://$repository:$run")
        [[ $current == "$digest" ]] || die "$repository:$run moved to $current since run $run recorded $digest"
        skopeo inspect --raw "docker://$repository:sha256-${digest#sha256:}.sig" |
            jq -e --arg type "$SIMPLE_SIGNING" '.layers | any(.mediaType == $type)' > /dev/null ||
            die "$repository@$digest has no signature a machine can verify (sha256-<hex>.sig)"
        bash "$retry" skopeo --registries.d "$work/policy/registries.d" copy --policy "$work/policy/policy.json" \
            "docker://$repository@$digest" "dir:$work/pull" ||
            die "$repository@$digest does not verify with the keys under system/keys: machines would refuse it"
        rm -r "$work/pull"
        if stable=$(skopeo inspect --format '{{.Digest}}' "docker://$repository:stable" 2> "$err"); then
            previous[$name]=$stable
            if [[ $stable != "$digest" && $(created "$repository" "$digest") -le $(created "$repository" "$stable") ]]; then
                die "$repository:$run is not newer than the current stable: machines would not follow it"
            fi
        elif ! grep -q 'manifest unknown' "$err"; then
            # Anything but "there is no stable tag yet" is a real failure.
            cat "$err" >&2
            exit 1
        fi
        promote+=("$name")
    done

    age=$(($(date -u +%s) - newest))
    ((age >= dwell)) ||
        die "run $run is inside its dwell: its newest evidence is from $(date -u -d "@$newest" +%FT%TZ), $(((dwell - age + 3599) / 3600)) h to go (UD5)"

    # The record. Every evidence file fetched is hashed, not only the ones checked above: a file
    # replaced between the plan and the apply changes the record and stops the apply.
    package_files=()
    for name in "${promote[@]}"; do package_files+=("packages/$name.txt"); done
    evidence_files=("$artifacts"/evidence/*.json)
    # Assigned, not passed inline, so a failed hash stops the record instead of writing null.
    evidence_sums=$(sums "${evidence_files[@]#"$artifacts/"}")
    package_sums=$(sums "${package_files[@]}")
    images=$(for name in "${promote[@]}"; do
        jq -n --arg name "$name" --arg digest "${recorded[$name]}" --arg previous "${previous[$name]:-}" \
            '{name: $name, digest: $digest, previous_stable: (if $previous == "" then null else $previous end)}'
    done | jq -s .)
    skips=$(for name in "${!skipped[@]}"; do
        jq -n --arg name "$name" --arg reason "${skipped[$name]}" '{name: $name, reason: $reason}'
    done | jq -s 'sort_by(.name)')
    trigger=manual
    if [[ -n ${GITHUB_RUN_ID:-} ]]; then trigger=$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID; fi
    jq -n -S --indent 2 --argjson run "$run" --arg day "$day" --arg trigger "$trigger" --arg override "$override" \
        --argjson images "$images" --argjson skipped "$skips" \
        --argjson evidence "$evidence_sums" --argjson packages "$package_sums" \
        '{run_id: $run, day: $day, trigger: $trigger, images: $images, skipped: $skipped,
      overrides: (if $override == "" then [] else [$override] end), evidence: $evidence, packages: $packages}' \
        > "$out"
}
