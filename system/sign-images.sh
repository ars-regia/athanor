#!/usr/bin/env bash
# Key-based signature of the published system images, and the verification a machine will
# make (docs/architecture/doc_update_trust.md, UT2). Runs in a job that holds the key and
# does nothing else: it reads the digests file the build job wrote, signs, verifies, ends.
#
# The signature must be the classic cosign attachment at <repo>:sha256-<hex>.sig, the only
# format containers/image reads, so it is made with `skopeo copy
# --sign-by-sigstore-private-key` and never with cosign 3, which writes a bundle every
# Athanor machine treats as no signature. It copies repo@digest onto itself: the digest the
# build job recorded is what is signed, and no tag is read. Each image is then pulled through
# the policy rendered from this checkout (`skopeo copy --policy`), not checked with `cosign
# verify`: that also catches a wrong registries.d entry before a machine meets it.
#
# The key and its passphrase arrive in COSIGN_PRIVATE_KEY and COSIGN_PASSWORD. skopeo takes
# both as files only, so they are written by the shell's own printf, under umask 077, into a
# private directory on tmpfs that is removed on exit, and the variables are unset before the
# first child process starts: neither value is ever on a command line, in a world-readable
# file, or in the environment of skopeo.
#
# The digests file comes from the build job, so it is not trusted for what to sign: the
# registry is given by the caller, and the file must name exactly the three shipped
# repositories under it, once each. A build job cannot steer the key onto another repository.
#
# The shipped policy names its keys with keyPaths, which a skopeo older than 1.15 rejects
# (the runner's is 1.13). With SIGN_VERIFY_BUILDER, the content hash of the builder image of
# the run, the verification runs that image's skopeo under podman, after the key files are
# removed. The container receives the rendered policy and the public keys, read-only, and no
# registry login: it pulls anonymously, as a machine does, so the job's token, which can write
# packages, never reaches code built from the repository. Without it the host's skopeo verifies.
#
# Usage: sign-images.sh --registry REGISTRY/OWNER DIGESTS_FILE
#        (lines: "REPOSITORY TAG DIGEST", image-digests.sh)
# Environment: COSIGN_PRIVATE_KEY, COSIGN_PASSWORD; SIGN_KEYS_DIR (default system/keys);
#              SIGN_VERIFY_BUILDER (REGISTRY/OWNER/athanor-builder:HASH verifies);
#              the registry login is the caller's business.
set -euo pipefail

[[ $# -eq 3 && $1 == --registry && -n $2 && -s $3 ]] || { echo "usage: ${0##*/} --registry REGISTRY/OWNER DIGESTS_FILE" >&2; exit 2; }
registry=$2
digests=$3
shipped=(athanor-system athanor-system-nvidia athanor-system-nvidia-legacy)
[[ -n ${COSIGN_PRIVATE_KEY:-} ]] || { echo "${0##*/}: COSIGN_PRIVATE_KEY is not available to this job: check the signing-images environment" >&2; exit 2; }
[[ -n ${COSIGN_PASSWORD+set} ]] || { echo "${0##*/}: COSIGN_PASSWORD is not available to this job: check the signing-images environment" >&2; exit 2; }
if [[ -n ${SIGN_VERIFY_BUILDER:-} ]]; then
  [[ $SIGN_VERIFY_BUILDER =~ ^[0-9a-f]{64}$ ]] || { echo "${0##*/}: SIGN_VERIFY_BUILDER is not a content hash: '$SIGN_VERIFY_BUILDER'" >&2; exit 2; }
fi

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
retry="$root/forge/scripts/retry.sh"
keys_dir=${SIGN_KEYS_DIR:-$root/system/keys}

umask 077
work=$(mktemp -d -p "${XDG_RUNTIME_DIR:-/dev/shm}" sign-images.XXXXXX)
# The verification pulls whole images and holds no secret: it goes to disk, not to the
# memory-backed directory that holds the key.
pulls=$(mktemp -d)
trap 'rm -rf "$work" "$pulls"' EXIT
printf '%s' "$COSIGN_PRIVATE_KEY" > "$work/key"
printf '%s' "$COSIGN_PASSWORD" > "$work/passphrase"
unset COSIGN_PRIVATE_KEY COSIGN_PASSWORD

declare -A seen=()
while read -r repository tag digest; do
  [[ $digest =~ ^sha256:[0-9a-f]{64}$ && -n $tag ]] || { echo "${0##*/}: malformed line in $digests: '$repository $tag $digest'" >&2; exit 2; }
  [[ ${repository%/*} == "$registry" && " ${shipped[*]} " == *" ${repository##*/} "* ]] ||
    { echo "${0##*/}: $digests names $repository, not a shipped repository under $registry" >&2; exit 2; }
  [[ -z ${seen[$repository]:-} ]] || { echo "${0##*/}: $digests names $repository twice" >&2; exit 2; }
  seen[$repository]=1
done < "$digests"
[[ ${#seen[@]} -eq ${#shipped[@]} ]] || { echo "${0##*/}: $digests names ${#seen[@]} of the ${#shipped[@]} shipped repositories" >&2; exit 2; }
keys_dir=$(cd "$keys_dir" && pwd)
bash "$root/forge/specs/athanor-update/SOURCES/usr/libexec/athanor-update/render-policy" \
  --registry "$registry" --keys-dir "$keys_dir" --out "$work/policy"
verifier=()
if [[ -n ${SIGN_VERIFY_BUILDER:-} ]]; then
  # The policy names the keys by absolute path, so they are mounted where the policy says.
  verifier=(podman run --rm --cap-drop=all --security-opt no-new-privileges --security-opt label=disable
    -v "$work/policy:$work/policy:ro" -v "$keys_dir:$keys_dir:ro"
    "${registry,,}/athanor-builder:$SIGN_VERIFY_BUILDER")
fi

while read -r repository _ digest; do
  # skopeo writes the sigstore attachment only where registries.d enables it: the rendered
  # one does, for exactly these repositories, and the runner's default does not. A copy onto
  # a digest reference fails unless the manifest still has that digest.
  bash "$retry" skopeo --registries.d "$work/policy/registries.d" copy --preserve-digests --sign-by-sigstore-private-key "$work/key" --sign-passphrase-file "$work/passphrase" \
    "docker://$repository@$digest" "docker://$repository@$digest"
done < "$digests"

# The key is no longer needed: nothing after this point, and no container, can read it.
unlink "$work/key"
unlink "$work/passphrase"
n=0
while read -r repository _ digest; do
  n=$((n + 1))
  # In the builder image the pull lands in the container, which --rm removes.
  target="$pulls/verified-$n"
  [[ ${#verifier[@]} -eq 0 ]] || target=/var/tmp/verified
  bash "$retry" "${verifier[@]}" skopeo --registries.d "$work/policy/registries.d" copy --policy "$work/policy/policy.json" \
    "docker://$repository@$digest" "dir:$target"
  rm -rf "$pulls/verified-$n"
  echo "signed and verified with the shipped policy: $repository@$digest"
done < "$digests"
