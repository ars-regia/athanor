#!/usr/bin/env bash
# Publishes the installer ISO forge/scripts/build_iso.sh built, as an OCI image (docs/
# architecture/doc_system_image.md, section 8): the ISO carries the whole system image and is
# far past the 2 GB an Actions artifact takes, so it ships the way kernel-build.yml publishes
# the kernel artifacts, one file in a scratch image.
# The image's labels name the run, the commit and the digest of each system image as pushed
# (DIGESTS, system/image-digests.sh); forge/test/iso/evidence.py binds the acceptance evidence
# to them. The pushed digest is then signed keyless: the certificate names the workflow, the
# branch and the commit that built it, and forge/test/iso/attest.sh attests nothing unless the
# ISO it tested carries that signature. The labels are part of the signed image, so a signed
# ISO cannot name images its build did not push.
# Usage: publish-iso.sh --registry REG --run RUN_ID --revision SHA --source URL --digests FILE
#                       [--tag TAG]... OUTPUT_DIR
#   --registry  registry host and owner (e.g. ghcr.io/owner); the image is REG/athanor-iso
#   --tag       further tags of the same digest (latest on the release branch)
# Prints a Markdown summary on stdout. Environment: podman and cosign logged in to the registry,
# and cosign able to sign keyless (id-token: write).
set -euo pipefail
shopt -s inherit_errexit

usage() {
    echo "usage: ${0##*/} --registry REG --run RUN_ID --revision SHA --source URL --digests FILE [--tag TAG]... OUTPUT_DIR" >&2
    exit 2
}
registry='' run='' revision='' source='' digests='' tags=()
while [[ $# -gt 1 ]]; do
    [[ -n $2 ]] || usage
    case $1 in
    --registry) registry=$2 ;; --run) run=$2 ;; --revision) revision=$2 ;; --source) source=$2 ;;
    --digests) digests=$2 ;; --tag) tags+=("$2") ;; *) usage ;;
    esac
    shift 2
done
[[ $# -eq 1 && -n $registry && -n $source && -f $digests ]] || usage
[[ $run =~ ^[0-9]+$ && $revision =~ ^[0-9a-f]{40}$ ]] || usage
output=$1
retry="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/forge/scripts/retry.sh"

mapfile -t isos < <(find "$output" -name '*.iso' -type f)
[[ ${#isos[@]} -eq 1 ]] || {
    echo "${0##*/}: expected one .iso under $output, found ${#isos[@]}" >&2
    exit 1
}
iso=${isos[0]}
image=$registry/athanor-iso
labels=(--label "io.athanor.run-id=$run" --label "org.opencontainers.image.revision=$revision")
while read -r repository _ digest; do
    [[ ${repository##*/} =~ ^[a-z0-9-]+$ && $digest =~ ^sha256:[0-9a-f]{64}$ ]] ||
        {
            echo "${0##*/}: malformed line in $digests: '$repository $digest'" >&2
            exit 1
        }
    labels+=(--label "io.athanor.image-digest.${repository##*/}=$digest")
done < "$digests"

work=$(mktemp -d "${TMPDIR:-/var/tmp}/publish-iso.XXXXXX")
trap 'rm -r "$work"' EXIT
# A Containerfile of its own keeps the build context to the ISO alone.
printf '%s\n' 'FROM scratch' \
    'LABEL org.opencontainers.image.title="Athanor installer ISO"' \
    "LABEL org.opencontainers.image.version=\"$run\"" \
    "COPY $(basename "$iso") /athanor-$run.iso" > "$work/Containerfile"
podman build --format docker -t "$image:$run" "${labels[@]}" \
    --annotation "org.opencontainers.image.revision=$revision" \
    --annotation "org.opencontainers.image.source=$source" \
    -f "$work/Containerfile" "$(dirname "$iso")"
bash "$retry" podman push --digestfile "$work/digest" "$image:$run"
digest=$(< "$work/digest")
[[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || {
    echo "${0##*/}: podman push reported no digest" >&2
    exit 1
}
bash "$retry" cosign sign --yes "$image@$digest"
for tag in "${tags[@]}"; do
    podman tag "$image:$run" "$image:$tag"
    bash "$retry" podman push "$image:$tag"
done

echo "### Installer ISO"
echo
echo "\`$image:$run\` (\`$digest\`), $(du -h "$iso" | cut -f1)"
