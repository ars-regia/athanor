#!/usr/bin/env bash
# Key-based signature of the published system images, and the verification a machine will
# make (docs/architecture/doc_update_trust.md, UT2). Runs in a job that holds the key and
# does nothing else: it reads the digests file the build job wrote, signs, verifies, ends.
#
# The signature must be the classic cosign attachment at <repo>:sha256-<hex>.sig, the only
# format containers/image reads, so it is made with `skopeo copy
# --sign-by-sigstore-private-key` and never with cosign 3, which writes a bundle every
# Athanor machine treats as no signature. Each image is then pulled through the policy
# rendered from this checkout (`skopeo copy --policy`), not checked with `cosign verify`:
# that also catches a wrong registries.d entry before a machine meets it.
#
# The key and its passphrase arrive in COSIGN_PRIVATE_KEY and COSIGN_PASSWORD. skopeo takes
# both as files only, so they are written by the shell's own printf, under umask 077, into a
# private directory on tmpfs that is removed on exit, and the variables are unset before the
# first child process starts: neither value is ever on a command line, in a world-readable
# file, or in the environment of skopeo.
#
# Usage: sign-images.sh DIGESTS_FILE      (lines: "REPOSITORY TAG DIGEST", image-digests.sh)
# Environment: COSIGN_PRIVATE_KEY, COSIGN_PASSWORD; SIGN_KEYS_DIR (default system/keys);
#              the registry login is the caller's business.
set -euo pipefail

[[ $# -eq 1 && -s $1 ]] || { echo "usage: ${0##*/} DIGESTS_FILE" >&2; exit 2; }
digests=$1
[[ -n ${COSIGN_PRIVATE_KEY:-} ]] || { echo "${0##*/}: COSIGN_PRIVATE_KEY is not available to this job: check the signing environment" >&2; exit 2; }
[[ -n ${COSIGN_PASSWORD+set} ]] || { echo "${0##*/}: COSIGN_PASSWORD is not available to this job: check the signing environment" >&2; exit 2; }

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

registry=''
while read -r repository tag digest; do
  [[ $digest =~ ^sha256:[0-9a-f]{64}$ && -n $tag ]] || { echo "${0##*/}: malformed line in $digests: '$repository $tag $digest'" >&2; exit 2; }
  [[ -z $registry || $registry == "${repository%/*}" ]] || { echo "${0##*/}: $digests names two registries" >&2; exit 2; }
  registry=${repository%/*}
done < "$digests"
bash "$root/forge/specs/athanor-update/SOURCES/usr/libexec/athanor-update/render-policy" \
  --registry "$registry" --keys-dir "$keys_dir" --out "$work/policy"

while read -r repository tag digest; do
  # The signature covers a digest; the tag is only how skopeo addresses the copy. A tag that
  # no longer names the digest the build job recorded is not signed.
  now=$(bash "$retry" skopeo inspect --format '{{.Digest}}' "docker://$repository:$tag")
  [[ $now == "$digest" ]] || { echo "${0##*/}: $repository:$tag is $now, the build job recorded $digest" >&2; exit 1; }
  # skopeo writes the sigstore attachment only where registries.d enables it: the rendered
  # one does, for exactly these repositories, and the runner's default does not.
  bash "$retry" skopeo --registries.d "$work/policy/registries.d" copy --preserve-digests --sign-by-sigstore-private-key "$work/key" --sign-passphrase-file "$work/passphrase" \
    "docker://$repository:$tag" "docker://$repository:$tag"
done < "$digests"

n=0
while read -r repository _ digest; do
  n=$((n + 1))
  bash "$retry" skopeo --registries.d "$work/policy/registries.d" copy --policy "$work/policy/policy.json" \
    "docker://$repository@$digest" "dir:$pulls/verified-$n"
  rm -rf "$pulls/verified-$n"
  echo "signed and verified with the shipped policy: $repository@$digest"
done < "$digests"
