#!/usr/bin/env bash
# Points the `stable` tag of the three system images at the digests of one pipeline run
# (docs/architecture/doc_update_trust.md, D1). Users follow :stable; :latest stays for
# testing. The signature is by digest, so it carries: nothing is signed here and no private
# key is needed. Nothing moves unless all three images pass every check:
#   - the acceptance evidence of the run (acceptance-<run>.json, written by
#     .github/workflows/iso-acceptance.yml) says "pass" and names this exact digest: a digest
#     the acceptance test never installed is never promoted, whatever its tag says now;
#   - the evidence is at least PROMOTE_DWELL_HOURS old: the automatic promotion waits a dwell
#     time after acceptance (promote-stable.yml), the manual override and the out-of-band
#     security path do not;
#   - the run's image exists and carries the classic cosign attachment machines verify;
#   - that signature verifies: the image is pulled through the policy a machine has,
#     rendered from the public keys under system/keys, as system/sign-images.sh does. A
#     stable a machine refuses would leave every machine without updates, silently;
#   - its build time is newer than the current stable's: a machine never follows a tag
#     backwards (UT5), so an older promotion would only strand the channel.
# Besides `stable`, each image gets `stable-previous` (the digest stable pointed at) and
# `stable-<YYYYMMDD>`; forge/scripts/clean_ghcr.sh keeps all three (UT10).
# Usage: promote.sh RUN_ID EVIDENCE
# Environment: REGISTRY (default ghcr.io/<GITHUB_REPOSITORY_OWNER>); PROMOTE_KEYS_DIR
#              (default system/keys); PROMOTE_DWELL_HOURS (default 0); skopeo logged in.
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 2 && $1 =~ ^[0-9]+$ ]] || { echo "usage: ${0##*/} RUN_ID EVIDENCE" >&2; exit 2; }
run=$1 evidence=$2
dwell_hours=${PROMOTE_DWELL_HOURS:-0}
[[ $dwell_hours =~ ^[0-9]+$ ]] || { echo "${0##*/}: PROMOTE_DWELL_HOURS must be a whole number of hours" >&2; exit 2; }
# The evidence as a whole: well formed, about this run, a pass, and old enough.
jq -e --arg run "$run" '.schema == 1 and .run_id == $run and (.images | type) == "object"
  and (.finished_at | type) == "string"' "$evidence" > /dev/null \
  || { echo "${0##*/}: $evidence is not acceptance evidence for run $run" >&2; exit 1; }
result=$(jq -r .result "$evidence")
[[ $result == pass ]] || { echo "${0##*/}: run $run did not pass acceptance (result: $result)" >&2; exit 1; }
finished=$(date -u -d "$(jq -r .finished_at "$evidence")" +%s)
if (( $(date -u +%s) - finished < dwell_hours * 3600 )); then
  echo "${0##*/}: run $run passed acceptance less than ${dwell_hours} h ago" >&2
  exit 1
fi
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

declare -A has_stable=() digests=()
for name in athanor-system athanor-system-nvidia athanor-system-nvidia-legacy; do
  repository=$REGISTRY/$name
  digest=$(skopeo inspect --format '{{.Digest}}' "docker://$repository:$run")
  [[ $(jq -r --arg name "$name" '.images[$name] // empty' "$evidence") == "$digest" ]] \
    || { echo "${0##*/}: $repository@$digest is not the digest the acceptance evidence names" >&2; exit 1; }
  digests[$name]=$digest
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
  # By digest: the one checked above, even if the run tag were moved in between.
  bash "$retry" skopeo copy --preserve-digests "docker://$repository@${digests[$name]}" "docker://$repository:stable-$day"
  bash "$retry" skopeo copy --preserve-digests "docker://$repository@${digests[$name]}" "docker://$repository:stable"
  echo "stable -> $repository:$run"
done
