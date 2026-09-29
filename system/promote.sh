#!/usr/bin/env bash
# Points the `stable` tag of the three system images at the digests of one pipeline run
# (docs/architecture/doc_update_trust.md, D1). Users follow :stable; :latest stays for
# testing. The signature is by digest, so it carries: nothing is signed here and no private
# key is needed. Nothing moves unless all three images pass every check:
#   - the run's image exists and carries the classic cosign attachment machines verify;
#   - that signature verifies: the image is pulled through the policy a machine has,
#     rendered from the public keys under system/keys, as system/sign-images.sh does. A
#     stable a machine refuses would leave every machine without updates, silently;
#   - its build time is newer than the current stable's: a machine never follows a tag
#     backwards (UT5), so an older promotion would only strand the channel.
# Besides `stable`, each image gets `stable-previous` (the digest stable pointed at) and
# `stable-<YYYYMMDD>`; forge/scripts/clean_ghcr.sh keeps all three (UT10).
# Usage: promote.sh RUN_ID
# Environment: REGISTRY (default ghcr.io/<GITHUB_REPOSITORY_OWNER>); PROMOTE_KEYS_DIR
#              (default system/keys); skopeo logged in.
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 1 && $1 =~ ^[0-9]+$ ]] || { echo "usage: ${0##*/} RUN_ID" >&2; exit 2; }
run=$1
owner=${GITHUB_REPOSITORY_OWNER:-}
REGISTRY=${REGISTRY:-${owner:+ghcr.io/${owner,,}}}
[[ -n $REGISTRY ]] || { echo "${0##*/}: set REGISTRY or GITHUB_REPOSITORY_OWNER" >&2; exit 2; }
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
retry="$root/forge/scripts/retry.sh"
keys_dir=${PROMOTE_KEYS_DIR:-$root/system/keys}
SIMPLE_SIGNING=application/vnd.dev.cosign.simplesigning.v1+json
work=$(mktemp -d)
trap 'rm -r "$work"' EXIT
err=$work/err
bash "$root/forge/specs/athanor-update/SOURCES/usr/libexec/athanor-update/render-policy" \
  --registry "$REGISTRY" --keys-dir "$keys_dir" --out "$work/policy"

created() { # created REPOSITORY DIGEST -> seconds since the epoch
  local label
  label=$(skopeo inspect --config "docker://$1@$2" | jq -r '.config.Labels["org.opencontainers.image.created"] // empty')
  [[ -n $label ]] || { echo "${0##*/}: $1@$2 has no org.opencontainers.image.created label" >&2; return 1; }
  date -u -d "$label" +%s
}

declare -A has_stable=()
for name in athanor-system athanor-system-nvidia athanor-system-nvidia-legacy; do
  repository=$REGISTRY/$name
  digest=$(skopeo inspect --format '{{.Digest}}' "docker://$repository:$run")
  skopeo inspect --raw "docker://$repository:sha256-${digest#sha256:}.sig" \
    | jq -e --arg type "$SIMPLE_SIGNING" '.layers | any(.mediaType == $type)' > /dev/null \
    || { echo "${0##*/}: $repository@$digest has no signature a machine can verify (sha256-<hex>.sig)" >&2; exit 1; }
  bash "$retry" skopeo --registries.d "$work/policy/registries.d" copy --policy "$work/policy/policy.json" \
    "docker://$repository@$digest" "dir:$work/pull" \
    || { echo "${0##*/}: $repository@$digest does not verify with the keys under system/keys: machines would refuse it" >&2; exit 1; }
  rm -r "$work/pull"
  if stable=$(skopeo inspect --format '{{.Digest}}' "docker://$repository:stable" 2> "$err"); then
    has_stable[$name]=1
    if [[ $stable != "$digest" && $(created "$repository" "$digest") -le $(created "$repository" "$stable") ]]; then
      echo "${0##*/}: $repository:$run is not newer than the current stable: machines would not follow it" >&2
      exit 1
    fi
  elif ! grep -q 'manifest unknown' "$err"; then
    # Anything but "there is no stable tag yet" is a real failure.
    cat "$err" >&2
    exit 1
  fi
done

day=$(date -u +%Y%m%d)
for name in athanor-system athanor-system-nvidia athanor-system-nvidia-legacy; do
  repository=$REGISTRY/$name
  if [[ -n ${has_stable[$name]:-} ]]; then
    bash "$retry" skopeo copy --preserve-digests "docker://$repository:stable" "docker://$repository:stable-previous"
  fi
  bash "$retry" skopeo copy --preserve-digests "docker://$repository:$run" "docker://$repository:stable-$day"
  bash "$retry" skopeo copy --preserve-digests "docker://$repository:$run" "docker://$repository:stable"
  echo "stable -> $repository:$run"
done
