#!/usr/bin/env bash
# Points the `stable` tag of the three system images at the digests of one pipeline run
# (docs/architecture/doc_update_trust.md, D1). Users follow :stable; :latest is the newest,
# untested build of the release branch. The signature is by digest, so it carries: nothing is signed here and
# no private key is needed. Nothing moves unless all three images pass every check:
#   - build provenance: RUN_ID is, by the GitHub API, a run of this repository's
#     athanor-forge-orchestrator.yml on a trusted branch, triggered by a push, a schedule or a
#     dispatch, completed with success, and every image's revision label is that run's head
#     commit. The run tag alone proves nothing: any branch build pushes one, and the
#     acceptance test installs whatever ISO it is given, so its evidence says the images
#     work, not where they came from;
#   - acceptance evidence: a cosign attestation on that exact digest, made keyless by
#     .github/workflows/iso-acceptance.yml running on a trusted branch (PROMOTE_TRUSTED_REFS,
#     default iso-v0). Its predicate (forge/test/iso/evidence.py) must say "pass" with every
#     check true, and name this run, this digest, the commit the image was built from (its
#     org.opencontainers.image.revision label), this repository, a trusted ref and a trusted
#     event. Where the evidence was stored or who runs this script does not matter: only the
#     signing identity and the predicate count, so a branch, a fork or an edited artifact
#     cannot produce evidence that passes;
#   - the newest such evidence is at least PROMOTE_DWELL_HOURS old (default 0): the automatic
#     promotion waits a dwell time after acceptance (promote-stable.yml), the manual override
#     and the out-of-band security path do not;
#   - the run's image carries the classic cosign attachment machines verify, and that
#     signature verifies: the image is pulled through the policy a machine has, rendered from
#     the public keys under system/keys, as system/sign-images.sh does. A stable a machine
#     refuses would leave every machine without updates, silently;
#   - its build time is newer than the current stable's: a machine never follows a tag
#     backwards (UT5), so an older promotion would only strand the channel.
# Besides `stable`, each image gets `stable-previous` (the digest stable pointed at) and
# `stable-<YYYYMMDD>`; forge/scripts/clean_ghcr.sh keeps all three (UT10).
# Usage: promote.sh RUN_ID
# Every check runs for all three images before any tag moves.
# Exit status: 0 promoted; 1 refused or failed (a signature machines
# would reject, a registry error); 2 usage; 3 the run is not newer than the current stable;
# 4 the run is not eligible yet or at all (no image tagged with the run, no passing acceptance
# evidence for it, or evidence younger than the dwell time). promote-auto.sh tells them apart.
# Environment: REGISTRY (default ghcr.io/<GITHUB_REPOSITORY_OWNER>); PROMOTE_KEYS_DIR
#              (default system/keys); PROMOTE_DWELL_HOURS (default 0); PROMOTE_TRUSTED_REFS
#              (default iso-v0); PROMOTE_EVIDENCE_OUT (optional: where to write the verified
#              predicate); GITHUB_SERVER_URL and GITHUB_REPOSITORY name the signing workflow
#              and the build run's repository; skopeo logged in, cosign on PATH, gh
#              authenticated (actions: read).
set -euo pipefail
shopt -s inherit_errexit

[[ $# -eq 1 && $1 =~ ^[0-9]+$ ]] || { echo "usage: ${0##*/} RUN_ID" >&2; exit 2; }
run=$1
dwell_hours=${PROMOTE_DWELL_HOURS:-0}
[[ $dwell_hours =~ ^[0-9]+$ ]] || { echo "${0##*/}: PROMOTE_DWELL_HOURS must be a whole number of hours" >&2; exit 2; }
owner=${GITHUB_REPOSITORY_OWNER:-}
REGISTRY=${REGISTRY:-${owner:+ghcr.io/${owner,,}}}
[[ -n $REGISTRY ]] || { echo "${0##*/}: set REGISTRY or GITHUB_REPOSITORY_OWNER" >&2; exit 2; }
repo=${GITHUB_REPOSITORY:-hr-mes/athanor}
server=${GITHUB_SERVER_URL:-https://github.com}
read -ra trusted <<< "${PROMOTE_TRUSTED_REFS:-iso-v0}"
refs=''
for ref in "${trusted[@]}"; do
  [[ $ref =~ ^[A-Za-z0-9._/-]+$ ]] || { echo "${0##*/}: PROMOTE_TRUSTED_REFS: '$ref' is not a branch name" >&2; exit 2; }
  refs+="${refs:+|}${ref//./\\.}"
done
[[ -n $refs ]] || { echo "${0##*/}: PROMOTE_TRUSTED_REFS names no branch" >&2; exit 2; }
workflows="$server/$repo/.github/workflows"
# Owner, repository and host names hold no regex metacharacter other than the dot.
IDENTITY="^${workflows//./\\.}/iso-acceptance\\.yml@refs/heads/(${refs})\$"
ISSUER=https://token.actions.githubusercontent.com
# cosign's verdicts for a missing or foreign attestation; anything else is an error (as in
# system/kernel-artifacts.sh, which explains the anchoring).
UNVERIFIED='no signatures found|no matching signatures: *$|no matching attestations: *$|no matching CertificateIdentity'
IMAGES=(athanor-system athanor-system-nvidia athanor-system-nvidia-legacy)
# The checks forge/test/iso/verdict.py records and forge/test/iso/evidence.py requires
# (system/tests/test_promote.py keeps the lists equal): evidence naming fewer is a partial pass.
CHECKS='["installed","kickstart-done","profile","karg","greeter","session","settings","no-guest-failure"]'
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
retry="$root/forge/scripts/retry.sh"
keys_dir=${PROMOTE_KEYS_DIR:-$root/system/keys}
SIMPLE_SIGNING=application/vnd.dev.cosign.simplesigning.v1+json
work=$(mktemp -d)
trap 'rm -r "$work"' EXIT
err=$work/err
bash "$root/forge/specs/athanor-update/SOURCES/usr/libexec/athanor-update/render-policy" \
  --registry "$REGISTRY" --keys-dir "$keys_dir" --out "$work/policy"

label() { # label REPOSITORY DIGEST LABEL -> the value, empty when absent
  skopeo inspect --config "docker://$1@$2" | jq -r --arg l "$3" '.config.Labels[$l] // empty'
}

created() { # created REPOSITORY DIGEST -> seconds since the epoch
  local value
  value=$(label "$1" "$2" org.opencontainers.image.created)
  [[ -n $value ]] || { echo "${0##*/}: $1@$2 has no org.opencontainers.image.created label" >&2; return 1; }
  date -u -d "$value" +%s
}

ineligible() { echo "${0##*/}: $*" >&2; exit 4; }

accepted() { # accepted REPOSITORY NAME DIGEST: writes the newest valid predicate to $work/NAME.json
  local revision status=0
  revision=$(label "$1" "$3" org.opencontainers.image.revision)
  [[ $revision =~ ^[0-9a-f]{40}$ ]] || ineligible "$1@$3 has no org.opencontainers.image.revision label: no evidence can be bound to it"
  [[ $revision == "$build_sha" ]] || ineligible "$1@$3 was built from $revision, not from run $run's commit $build_sha"
  # The identity is the workflow file and the branch it ran from; the repository extension
  # rules out another repository calling iso-acceptance.yml as a reusable workflow.
  cosign verify-attestation --type custom --certificate-identity-regexp "$IDENTITY" \
    --certificate-oidc-issuer "$ISSUER" --certificate-github-workflow-repository "$repo" \
    "$1@$3" > "$work/att" 2> "$err" || status=$?
  if [[ $status -ne 0 ]]; then
    grep -qE "$UNVERIFIED" "$err" && ineligible "$1@$3 has no acceptance evidence signed by iso-acceptance.yml on refs/heads/(${refs})"
    cat "$err" >&2
    exit 1
  fi
  # Of the attestations that passed the identity check, only those whose every field matches
  # the promotion target count: a fail or a partial pass, another run, digest, commit,
  # repository, branch or trigger, or a malformed field, does not.
  jq -s --arg run "$run" --arg name "$2" --arg digest "$3" --arg revision "$revision" \
    --arg repo "$repo" --arg refs "^refs/heads/(${refs})\$" --argjson checks "$CHECKS" \
    --argjson images "$(printf '%s\n' "${IMAGES[@]}" | jq -R . | jq -s 'sort')" '
    def sha256: type == "string" and test("^sha256:[0-9a-f]{64}$");
    [ .[] | (.payload | @base64d | fromjson | .predicate.Data | try fromjson catch null)
      | select(type == "object" and .schema == 1 and .kind == "athanor-acceptance"
          and .run_id == $run and .revision == $revision and .result == "pass"
          and .tested_image == "athanor-system"
          and (.images | type) == "object" and (.images | keys) == $images
          and all(.images[]; sha256) and .images[$name] == $digest
          and (.tests | type) == "object" and (.tests | keys) == ($checks | sort)
          and all(.tests[]; . == true)
          and (.finished_at | type) == "string" and (.finished_at | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"))
          and (.acceptance | type) == "object"
          and .acceptance.repository == $repo
          and (.acceptance.run_id | type) == "string" and (.acceptance.run_id | test("^[0-9]+$"))
          and (.acceptance.sha | type) == "string" and (.acceptance.sha | test("^[0-9a-f]{40}$"))
          and (.acceptance.ref | type) == "string" and (.acceptance.ref | test($refs))
          and (.acceptance.event | IN("schedule", "workflow_dispatch", "push"))
          and (.acceptance.ref as $ref | .acceptance.workflow_ref
               | IN($repo + "/.github/workflows/iso-acceptance.yml@" + $ref,
                    $repo + "/.github/workflows/athanor-forge-orchestrator.yml@" + $ref))) ]
    | sort_by(.finished_at) | last // empty' "$work/att" > "$work/$2.json" \
    || { echo "${0##*/}: $1@$3: malformed acceptance attestation" >&2; exit 1; }
  [[ -s $work/$2.json ]] || ineligible "$1@$3 has no passing acceptance evidence for run $run at commit $revision"
}

# The build run, from GitHub's own record of it: workflow file, branch, trigger, repository
# and outcome are GitHub's, not anything a job of the run wrote.
if ! build=$(gh api "/repos/$repo/actions/runs/$run" 2> "$err"); then
  grep -q 'Not Found' "$err" && ineligible "run $run is not a workflow run of $repo"
  cat "$err" >&2
  exit 1
fi
jq -e --arg repo "$repo" --args '
  .repository.full_name == $repo and .head_repository.full_name == $repo
  and .path == ".github/workflows/athanor-forge-orchestrator.yml"
  and (.head_branch | IN($ARGS.positional[])) and (.event | IN("push", "schedule", "workflow_dispatch"))
  and .status == "completed" and .conclusion == "success"
  and (.head_sha | test("^[0-9a-f]{40}$"))' "${trusted[@]}" <<< "$build" > /dev/null \
  || ineligible "run $run is not a successful Orchestrator run of a trusted branch of $repo: $(jq -c \
    '{path, head_branch, event, status, conclusion, repository: .head_repository.full_name}' <<< "$build")"
build_sha=$(jq -r .head_sha <<< "$build")

declare -A has_stable=() digests=()
now=$(date -u +%s)
for name in "${IMAGES[@]}"; do
  repository=$REGISTRY/$name
  if ! digest=$(skopeo inspect --format '{{.Digest}}' "docker://$repository:$run" 2> "$err"); then
    grep -q 'manifest unknown' "$err" && ineligible "$repository has no image tagged $run"
    cat "$err" >&2
    exit 1
  fi
  digests[$name]=$digest
  accepted "$repository" "$name" "$digest"
  finished=$(date -u -d "$(jq -r .finished_at "$work/$name.json")" +%s)
  (( now - finished >= dwell_hours * 3600 )) || ineligible "run $run passed acceptance less than ${dwell_hours} h ago"
  skopeo inspect --raw "docker://$repository:sha256-${digest#sha256:}.sig" \
    | jq -e --arg type "$SIMPLE_SIGNING" '.layers | any(.mediaType == $type)' > /dev/null \
    || { echo "${0##*/}: $repository@$digest has no signature a machine can verify (sha256-<hex>.sig)" >&2; exit 1; }
  bash "$retry" skopeo --registries.d "$work/policy/registries.d" copy --policy "$work/policy/policy.json" \
    "docker://$repository@$digest" "dir:$work/pull" \
    || { echo "${0##*/}: $repository@$digest does not verify with the keys under system/keys: machines would refuse it" >&2; exit 1; }
  rm -r "$work/pull"
  if stable=$(skopeo inspect --format '{{.Digest}}' "docker://$repository:stable" 2> "$err"); then
    has_stable[$name]=1
    if [[ $stable == "$digest" ]] || (( $(created "$repository" "$digest") <= $(created "$repository" "$stable") )); then
      echo "${0##*/}: $repository:$run is not newer than the current stable: machines would not follow it" >&2
      exit 3
    fi
  elif ! grep -q 'manifest unknown' "$err"; then
    # Anything but "there is no stable tag yet" is a real failure.
    cat "$err" >&2
    exit 1
  fi
done
if [[ -n ${PROMOTE_EVIDENCE_OUT:-} ]]; then
  cp "$work/athanor-system.json" "$PROMOTE_EVIDENCE_OUT"
fi

day=$(date -u +%Y%m%d)
for name in "${IMAGES[@]}"; do
  repository=$REGISTRY/$name
  if [[ -n ${has_stable[$name]:-} ]]; then
    bash "$retry" skopeo copy --preserve-digests "docker://$repository:stable" "docker://$repository:stable-previous"
  fi
  # By digest: the one checked above, even if the run tag were moved in between.
  bash "$retry" skopeo copy --preserve-digests "docker://$repository@${digests[$name]}" "docker://$repository:stable-$day"
  bash "$retry" skopeo copy --preserve-digests "docker://$repository@${digests[$name]}" "docker://$repository:stable"
  echo "stable -> $repository:$run (${digests[$name]})"
done
