#!/usr/bin/env bash
# The evidence bundle of a promotion (ADR-0103 D23, doc_pipeline.md PL42): the plan
# promotion-plan.sh wrote, the run's digests, the evidence and the package sets, with a
# bundle.sha256 signed keylessly by promote-stable.yml. It is copied to a registry off
# GitHub, pulled back and verified before :stable moves, so a promotion whose record did
# not reach that copy intact does not happen. No key: the signature is the workflow's
# OIDC identity (Sigstore).
# Usage: evidence-bundle.sh create ARTIFACTS_DIR BUNDLE_DIR
#        evidence-bundle.sh publish BUNDLE_DIR REF
#        evidence-bundle.sh verify REF ARTIFACTS_DIR
#        evidence-bundle.sh verify-dir COPY_DIR ARTIFACTS_DIR
#        evidence-bundle.sh summary ARTIFACTS_DIR
set -euo pipefail
shopt -s inherit_errexit

usage() {
    sed -n 's/^# Usage: //p; s/^#        //p' "$0" >&2
    exit 2
}
identity="${GITHUB_SERVER_URL:?}/${GITHUB_REPOSITORY:?}/.github/workflows/promote-stable.yml@refs/heads/${RELEASE_BRANCH:-iso-v0}"
issuer=https://token.actions.githubusercontent.com

create() {
    [[ -f $1/promotion.json ]] || {
        echo "${0##*/}: $1/promotion.json is missing: run scripts/ci/promotion-plan.sh first" >&2
        exit 1
    }
    mkdir -p "$2"
    cp "$1/promotion.json" "$1/image-digests.txt" "$2/"
    cp -r "$1/evidence" "$1/packages" "$2/"
    # verify-dir compares names line by line: refuse what it could not, before anything is signed.
    local nl=$'\n' bad
    bad=$(cd "$2" && find . ! -type d \( ! -type f -o -path '*\\*' -o -path "*${nl}*" \) -printf '%P\n')
    [[ -z $bad ]] || {
        echo "${0##*/}: not a regular file, or a name with a backslash or newline: $bad" >&2
        exit 1
    }
    (cd "$2" && find . -type f ! -path ./bundle.sha256 ! -path ./bundle.sha256.sigstore.json -printf '%P\0' |
        LC_ALL=C sort -z | xargs -0 sha256sum) > "$2/bundle.sha256"
    cosign sign-blob --yes --bundle "$2/bundle.sha256.sigstore.json" "$2/bundle.sha256"
}

verify_dir() {
    cosign verify-blob --bundle "$1/bundle.sha256.sigstore.json" --certificate-identity "$identity" \
        --certificate-oidc-issuer "$issuer" "$1/bundle.sha256"
    (cd "$1" && sha256sum --strict -c bundle.sha256)
    # The sums bind only the files they list: a copy holding anything else is not the
    # signed bundle, whatever a later reader would make of the extra file.
    local listed present
    listed=$(sed 's/^[0-9a-f]\{64\}  //' "$1/bundle.sha256" | LC_ALL=C sort)
    present=$(cd "$1" && find . ! -type d ! -path ./bundle.sha256 ! -path ./bundle.sha256.sigstore.json -printf '%P\n' | LC_ALL=C sort)
    [[ $listed == "$present" ]] || {
        echo "${0##*/}: the copy holds files the signed sums do not list" >&2
        exit 1
    }
    cmp -s "$1/promotion.json" "$2/promotion.json" ||
        {
            echo "${0##*/}: the copy's promotion.json is not the plan of this promotion" >&2
            exit 1
        }
}

summary() { # summary ARTIFACTS_DIR: Markdown for the job summary the release approval covers (PL60)
    jq -r '"### Promotion plan of build run \(.run_id)", "",
    "| Image | Digest | Previous stable |", "| --- | --- | --- |",
    (.images[] | "| \(.name) | `\(.digest)` | `\(.previous_stable // "none")` |"), "",
    "Skipped: " + ([.skipped[] | "\(.name) (\(.reason))"] | if length == 0 then "none" else join("; ") end), "",
    "Hardware override: " + (.overrides | if length == 0 then "none" else join(", ") end), ""' "$1/promotion.json"
    jq -r -s '"| Evidence | Image | Verdict | Finished |", "| --- | --- | --- | --- |",
    (.[] | "| \(.gate) | \(.image) | \(.verdict) | \(.finished_at) |")' "$1"/evidence/*.json
}

case ${1:-} in
create)
    [[ $# -eq 3 ]] || usage
    create "$2" "$3"
    ;;
publish)
    [[ $# -eq 3 ]] || usage
    printf 'FROM scratch\nCOPY . /\n' | podman build --file - --tag "$3" "$2"
    bash "$(dirname "$0")/../forge/scripts/retry.sh" podman push "$3"
    ;;
verify)
    [[ $# -eq 3 ]] || usage
    work=$(mktemp -d) ctr=''
    trap 'rm -rf "$work"; [[ -z $ctr ]] || podman rm -f "$ctr" > /dev/null || echo "${0##*/}: could not remove container $ctr" >&2' EXIT
    bash "$(dirname "$0")/../forge/scripts/retry.sh" podman pull "$2"
    ctr=$(podman create "$2" /bin/true)
    podman cp "$ctr:/." "$work/"
    verify_dir "$work" "$3"
    ;;
verify-dir)
    [[ $# -eq 3 ]] || usage
    verify_dir "$2" "$3"
    ;;
summary)
    [[ $# -eq 2 ]] || usage
    summary "$2"
    ;;
*) usage ;;
esac
