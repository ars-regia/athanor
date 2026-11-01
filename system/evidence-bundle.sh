#!/usr/bin/env bash
# The evidence bundle of a promotion (ADR-0103 D23, doc_pipeline.md PL42): the plan
# promotion-plan.sh wrote, the GitHub Release github-release.sh plan chose (release.json),
# the run's digests, the evidence and the package sets, with a
# bundle.sha256 signed keylessly by promote-stable.yml. It is copied to a registry off
# GitHub, pulled back and verified before :stable moves, so a promotion whose record did
# not reach that copy intact does not happen. No key: the signature is the workflow's
# OIDC identity (Sigstore).
# Usage: evidence-bundle.sh create ARTIFACTS_DIR BUNDLE_DIR
#        evidence-bundle.sh publish BUNDLE_DIR REF
#        evidence-bundle.sh verify REF ARTIFACTS_DIR
#        evidence-bundle.sh verify-dir COPY_DIR ARTIFACTS_DIR
#        evidence-bundle.sh summary ARTIFACTS_DIR
#        evidence-bundle.sh summary --promotion ARTIFACTS_DIR TAG
set -euo pipefail
shopt -s inherit_errexit

usage() {
    sed -n 's/^# Usage: //p; s/^#        //p' "$0" >&2
    exit 2
}
# Outside Actions (a verify from a workstation) the host and repository default as in
# kernel-artifacts.sh.
identity="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-ars-regia/athanor}/.github/workflows/promote-stable.yml@refs/heads/${RELEASE_BRANCH:-iso-v0}"
issuer=https://token.actions.githubusercontent.com

create() {
    [[ -f $1/promotion.json ]] || {
        echo "${0##*/}: $1/promotion.json is missing: run scripts/ci/promotion-plan.sh first" >&2
        exit 1
    }
    [[ -f $1/release.json ]] || {
        echo "${0##*/}: $1/release.json is missing: run system/github-release.sh plan first" >&2
        exit 1
    }
    mkdir -p "$2"
    cp "$1/promotion.json" "$1/release.json" "$1/image-digests.txt" "$2/"
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

# The promotion and its evidence, from promotion.json and the evidence alone: the part of the
# summary the Release notes repeat, so github-release.sh plan and publish build the same bytes.
promotion_summary() { # promotion_summary ARTIFACTS_DIR
    # A missing field is an error, never "none": the text is what the approval covers.
    jq -r 'def must(k): .[k] // error("promotion.json has no \(k)");
    "### Promotion plan of build run \(must("run_id"))", "",
    "Version: \(must("version")) (commit \(must("revision")))", "",
    "| Image | Digest | Previous stable |", "| --- | --- | --- |",
    (must("images")[] | "| \(must("name")) | `\(must("digest"))` | `\(.previous_stable // "none")` |"), "",
    "Installer ISO (athanor-iso:stable after the system images): `\(must("iso"))`, built from athanor-system `\(must("iso_system"))`", "",
    "Skipped: " + ([must("skipped")[] | "\(must("name")) (\(must("reason")))"] | if length == 0 then "none" else join("; ") end), "",
    "Hardware override: " + (must("overrides") | if length == 0 then "none" else join(", ") end), "",
    "Dwell: \(must("dwell_hours")) h (UD5)", "",
    "Signatures verified in: athanor-builder:\(must("builder_content_hash"))", ""' "$1/promotion.json"
    jq -r -s 'def must(k): .[k] // error("an evidence file has no \(k)");
    "| Evidence | Image | Verdict | Finished |", "| --- | --- | --- | --- |",
    (.[] | "| \(must("gate")) | \(must("image")) | \(must("verdict")) | \(must("finished_at")) |")' "$1"/evidence/*.json
}

summary() { # summary ARTIFACTS_DIR: Markdown for the job summary the release approval covers (PL60)
    promotion_summary "$1"
    [[ -f $1/release.json ]] || {
        echo "${0##*/}: $1/release.json is missing: run system/github-release.sh plan first" >&2
        exit 1
    }
    # The GitHub Release github-release.sh plan chose, and the fallback it may use (ADR-0131).
    jq -r 'def must(k): if has(k) and .[k] != null then .[k] else error("release.json has no \(k)") end;
    def release: "tag `\(must("tag"))`, title \(must("title")), latest \(must("latest")), notes SHA-256 `\(must("notes_sha256"))`";
    "", "GitHub Release (doc_pipeline.md PL42): " + release, "",
    if has("fallback") then "If that Release holds the evidence of another build: " + (.fallback | release), "" else empty end' "$1/release.json"
}

# promotion_notes ARTIFACTS_DIR TAG: the build, then how to verify each promotion's asset on the
# Release at TAG. A function of the build and TAG only, never of the promotion run: every promotion
# of a build computes the same notes, so a second one finds the Release exactly as it approved it
# (ADR-0131 decision 6). What a promotion published is in its signed bundle.
promotion_notes() {
    local run
    run=$(jq -er '.run_id' "$1/promotion.json") || {
        echo "${0##*/}: $1/promotion.json names no build run_id" >&2
        exit 1
    }
    jq -r 'def must(k): .[k] // error("promotion.json has no \(k)");
    "### Build run \(must("run_id"))", "",
    "Version: \(must("version")) (commit \(must("revision")))", "",
    "Each promotion of this build attaches its signed evidence bundle, `evidence-\(must("run_id"))-<promotion run>.tar`; its `promotion.json` names the images, digests and evidence that promotion published.", ""' "$1/promotion.json"
    printf '%s\n' "Verify each evidence bundle against its keyless signature:" "" '```sh' \
        "gh release download $2 -R ${GITHUB_REPOSITORY:-ars-regia/athanor} -p 'evidence-$run-*.tar'" \
        "for asset in evidence-$run-*.tar; do" \
        "  mkdir \"\${asset%.tar}\" && tar -xf \"\$asset\" -C \"\${asset%.tar}\" && (cd \"\${asset%.tar}\" &&" \
        "    cosign verify-blob --bundle bundle.sha256.sigstore.json --certificate-identity $identity --certificate-oidc-issuer $issuer bundle.sha256 &&" \
        "    sha256sum --strict -c bundle.sha256)" \
        "done" '```'
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
    if [[ $# -eq 4 && $2 == --promotion ]]; then
        promotion_notes "$3" "$4"
    else
        [[ $# -eq 2 ]] || usage
        summary "$2"
    fi
    ;;
*) usage ;;
esac
