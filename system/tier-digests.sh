#!/usr/bin/env bash
# The forge tier repository images a system image build installs, by digest
# (docs/architecture/doc_update_delivery.md, UD28), and their signatures (UD44).
#
#   resolve     write tier-digests.json for a build outside the Orchestrator run that
#               published the tiers (System Image Check and local builds): the registry, the
#               digest that each athanor-forge-tier<N>-repo:latest names now, read once, so
#               every image the job builds installs the same tiers whatever is published
#               meanwhile, and when it was resolved (UTC), which build-image.sh prints. The
#               file is written only when every digest verifies, as verify does.
#   verify      check that each tier digest of tier-digests.json is signed by a trusted run;
#               the Orchestrator runs it before the system image build
#   signed REF  print signed or unsigned for one image reference, retrying a failure of
#               cosign itself; forge/scripts/publish_tiers.sh asks it before it reuses a tier
#   probe REF   one attempt of signed
#
# The Orchestrator does not resolve: forge/scripts/publish_tiers.sh writes the same file with
# the digests it published in the same run, and signs keyless each image it pushes. The file is
# $TIER_DIGESTS_DIR/tier-digests.json (default: tier-digests/ at the repository root);
# system/build-image.sh reads it. The registry is $REGISTRY_HOST/$GITHUB_REPOSITORY_OWNER
# (default ghcr.io and ars-regia), in lower case.
#
# A trusted signature is one by .github/workflows/call-system-image.yml of GITHUB_SERVER_URL and
# GITHUB_REPOSITORY (default https://github.com and ars-regia/athanor) run on a branch of
# TIER_TRUSTED_REFS (space separated, default "iso-v0 main"), or on the branch of this run when
# GITHUB_REF is refs/heads/<name>: a branch run builds from the tiers it published itself, and no
# trusted run accepts them. No other branch is trusted, rebase/* included: creating a branch is
# not protected, and a tier signed there could be adopted by the next iso-v0 run. A
# missing or foreign signature fails the check; any other failure of cosign (registry, Rekor,
# TUF, network) is an error, never a missing signature. Needs skopeo, cosign and jq, and a
# registry session when the tier images are private.
set -euo pipefail
shopt -s inherit_errexit

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SELF=$HERE/$(basename "${BASH_SOURCE[0]}")
ROOT=$(dirname "$HERE")
FILE=${TIER_DIGESTS_DIR:-$ROOT/tier-digests}/tier-digests.json
usage() {
    sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "$SELF" >&2
    exit 2
}
die() {
    echo "tier-digests: $*" >&2
    exit 1
}
case ${1:-}:$# in
resolve:1 | verify:1 | signed:2 | probe:2) ;;
*) usage ;;
esac

ISSUER=https://token.actions.githubusercontent.com
workflow="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-ars-regia/athanor}/.github/workflows/call-system-image.yml"
# Owner, repository, host and branch names hold no regex metacharacter other than the dot.
workflow=${workflow//./\\.}
command -v cosign > /dev/null || die "cosign is not on PATH"
read -ra trusted <<< "${TIER_TRUSTED_REFS:-iso-v0 main}"
refs=''
for ref in "${trusted[@]}"; do
    [[ $ref =~ ^[A-Za-z0-9._/-]+$ ]] || die "'$ref' of TIER_TRUSTED_REFS is not a branch name"
    refs+="${refs:+|}${ref//./\\.}"
done
[[ -n $refs ]] || die "TIER_TRUSTED_REFS names no branch"
# The run's own branch, when its name needs no escaping beyond the dot; any other name is left
# out, which only makes the check stricter.
own=${GITHUB_REF:-}
own=${own#refs/heads/}
if [[ ${GITHUB_REF:-} == refs/heads/* && $own =~ ^[A-Za-z0-9._/-]+$ ]]; then
    refs+="|${own//./\\.}"
elif [[ ${GITHUB_REF:-} == refs/heads/* ]]; then
    echo "tier-digests: the branch '$own' is not trusted for its own tiers: its name needs escaping" >&2
fi
IDENTITY="^${workflow}@refs/heads/(${refs})\$"
# cosign v3 reports a missing or foreign signature with these messages. "no matching
# signatures" is anchored to the end of the line: cosign appends a transient cause (a Rekor
# lookup that failed) to the same prefix, and that is an error, not a missing signature.
UNVERIFIED='no signatures found|no matching signatures: *$|no matching CertificateIdentity'

probe() {
    local err status=0 rc=0
    err=$(mktemp)
    cosign verify --certificate-identity-regexp "$IDENTITY" --certificate-oidc-issuer "$ISSUER" "$1" > /dev/null 2> "$err" || status=$?
    if [[ $status -eq 0 ]]; then
        echo signed
    elif grep -qE "$UNVERIFIED" "$err"; then
        # stderr: stdout is the verdict. The identity it did have is what a maintainer needs.
        if grep -q 'got "' "$err"; then
            sed -n "s|.*got \"\(.*\)\".*|tier-digests: $1 is signed as \1, not by a trusted run|p" "$err" >&2
        else
            echo "tier-digests: $1 has no signature" >&2
        fi
        echo unsigned
    else
        cat "$err" >&2
        rc=1
    fi
    rm -f "$err"
    return "$rc"
}

signed() { bash "$ROOT/forge/scripts/retry.sh" bash "$SELF" probe "$1"; }

# verify_tiers REGISTRY DIGEST0 DIGEST1 DIGEST2 DIGEST3: fails unless all four are signed.
verify_tiers() {
    local registry=$1 n=0 digest verdict unsigned=()
    shift
    for digest; do
        verdict=$(signed "$registry/athanor-forge-tier$n-repo@$digest")
        [[ $verdict == signed ]] || unsigned+=("tier$n")
        n=$((n + 1))
    done
    [[ ${#unsigned[@]} -eq 0 ]] || die "${unsigned[*]}: not signed by a trusted run (UD44): $IDENTITY"
    echo "tier-digests: the four tiers are signed by a trusted run"
}

case $1 in
probe) probe "$2" ;;
signed) signed "$2" ;;
verify)
    [[ -f $FILE ]] || die "$FILE is missing"
    registry=$(jq -er '.registry | strings | select(test("^[a-z0-9][a-z0-9.:-]*(/[a-z0-9._-]+)+$"))' "$FILE") ||
        die "$FILE names no valid registry"
    digests=()
    for n in 0 1 2 3; do
        digests+=("$(jq -er ".tier$n | strings | select(test(\"^sha256:[0-9a-f]{64}$\"))" "$FILE")") ||
            die "$FILE has no valid tier$n digest"
    done
    verify_tiers "$registry" "${digests[@]}"
    ;;
resolve)
    registry=${REGISTRY_HOST:-ghcr.io}/${GITHUB_REPOSITORY_OWNER:-ars-regia}
    registry=${registry,,}
    args=(--arg registry "$registry" --arg resolved "$(date -u +%Y-%m-%dT%H:%M:%SZ)")
    digests=()
    for n in 0 1 2 3; do
        ref=$registry/athanor-forge-tier$n-repo:latest
        digest=$(bash "$ROOT/forge/scripts/retry.sh" skopeo inspect --format '{{.Digest}}' "docker://$ref")
        [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || die "$ref gave no digest: '$digest'"
        args+=(--arg "tier$n" "$digest")
        digests+=("$digest")
    done
    verify_tiers "$registry" "${digests[@]}"
    mkdir -p "$(dirname "$FILE")"
    jq -n "${args[@]}" '$ARGS.named' > "$FILE"
    echo "tier-digests: $(jq -c . "$FILE")"
    ;;
esac
