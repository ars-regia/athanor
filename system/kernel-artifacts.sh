#!/usr/bin/env bash
# The kernel and NVIDIA module artifacts of the current pins, verified in the registry and
# identified by digest (docs/architecture/doc_build_ordering.md, O2-O5, O7). Every workflow of
# the build ordering runs this script and reads its file; none repeats its decisions.
#
#   resolve [--expect-kernel-digest D]  write kernel-artifacts.env: state=ready, modules-missing
#                                       or kernel-missing, then the verified digests. Exit 0 for
#                                       the three states; 1 on a registry, Rekor, network or data
#                                       error, or when azoth:<nvr> is still D's own NVR but no
#                                       longer resolves to D (republished or withdrawn since). D
#                                       is resolved against the NVR it was verified for (its own
#                                       attested pins, or the org.opencontainers.image.version
#                                       label as a fallback); when the current pins name a
#                                       different NVR, D no longer applies and this resolves
#                                       exactly as an unconstrained caller would, never as an
#                                       error just because the pins moved on (O5)
#   require-ready                       resolve, then exit 1 unless state=ready
#   cycle --event E [--before B] [--after A] [--sha S] [--head H]
#                                       after resolve, for an Orchestrator run (O4): append
#                                       cycle=build or cycle=defer; exit 1 when the kernel is
#                                       missing and no other cycle owns it
#   check-plan --base REV --head REV    after resolve, for System Image Check (O7): append
#                                       check_gpus and check_delta; exit 1 for a failing row
#   get KEY                             print the value of KEY; exit 1 when the file lacks it
#   has KEY                             exit 0 when KEY has a non-empty value
#   registry                            print the registry and owner this run would resolve
#                                       against (the same $REGISTRY resolve writes to the file);
#                                       no network call, so a caller can compose an image
#                                       reference before resolve has ever run
#   digest REF                          the digest of REF, empty when the tag does not exist
#   signed REF kernel|modules           signed or unsigned, by the workflow that publishes it
#   predicates REF modules              the custom predicates of REF, one JSON per line, or
#                                       unverified
#   probe digest|signed|predicates|config ...
#                                       one attempt of the four above (they retry it)
#
# The file is $KERNEL_ARTIFACTS_DIR/kernel-artifacts.env (default: kernel-artifacts/ at the
# repository root). KERNEL_REGISTRY is the registry and owner (default ghcr.io/ followed by
# GITHUB_REPOSITORY_OWNER, else ars-regia); GITHUB_SERVER_URL and GITHUB_REPOSITORY name the
# workflows whose signatures are trusted, and KERNEL_TRUSTED_REFS the branches they may have run
# on (space separated, default "iso-v0 main"): a kernel or a module signed by the same workflow on
# any other branch is not published, whoever pushed it. A kernel is ready only when its verified
# pins attestation carries the inputs of this checkout (forge/specs/azoth/build-inputs.py), as
# the modules' does. cycle and check-plan run git in the repository checkout that is the current
# directory. Needs skopeo, cosign and jq for the registry.
set -euo pipefail
shopt -s inherit_errexit

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SELF=$HERE/$(basename "${BASH_SOURCE[0]}")
ROOT=$(dirname "$HERE")
DIR=${KERNEL_ARTIFACTS_DIR:-$ROOT/kernel-artifacts}
FILE=$DIR/kernel-artifacts.env
PINS=$ROOT/forge/specs/azoth/pins.env
owner=${GITHUB_REPOSITORY_OWNER:-ars-regia}
REGISTRY=${KERNEL_REGISTRY:-ghcr.io/${owner,,}}
ISSUER=https://token.actions.githubusercontent.com
workflows="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-ars-regia/athanor}/.github/workflows"
# Owner, repository and host names hold no regex metacharacter other than the dot.
workflows=${workflows//./\\.}
# The branches whose runs may publish. A signature carries the ref of the run that made it, and
# a workflow_dispatch can be started from any branch: without this list any branch could sign a
# kernel the images then trust.
TRUSTED_REFS=${KERNEL_TRUSTED_REFS:-iso-v0 main}
refs=''
read -ra trusted <<< "$TRUSTED_REFS"
for ref in "${trusted[@]}"; do
  [[ $ref =~ ^[A-Za-z0-9._/-]+$ ]] || { echo "kernel-artifacts: KERNEL_TRUSTED_REFS: '$ref' is not a branch name" >&2; exit 1; }
  refs+="${refs:+|}${ref//./\\.}"
done
[[ -n $refs ]] || { echo "kernel-artifacts: KERNEL_TRUSTED_REFS names no branch" >&2; exit 1; }
declare -A IDENTITY=(
  [kernel]="^${workflows}/kernel-build\.yml@refs/heads/(${refs})\$"
  [modules]="^${workflows}/nvidia-kmod\.yml@refs/heads/(${refs})\$"
)
# cosign v3 reports a missing or foreign signature or attestation with these messages. Any
# other failure (registry, Rekor, TUF, network) is an error, never a missing artifact.
# "no matching signatures" and "no matching attestations" are anchored to the end of the
# line: cosign appends the last per-signature error to that same prefix when the real cause
# is transient (a Rekor or cert-chain fetch failure), and an unanchored match would swallow
# that outage as a plain "unsigned" verdict instead of failing (O3).
UNVERIFIED='no signatures found|no matching signatures: *$|no matching attestations: *$|no matching CertificateIdentity'
# The push paths of .github/workflows/kernel-build.yml (a unit test keeps them equal).
KERNEL_BUILD_PATHS=('forge/specs/azoth/*' '.github/workflows/kernel-build.yml' '.github/workflows/nvidia-build.yml')
# The files .github/workflows/kernel-bump.yml regenerates with the pins, except
# system/Containerfile: a base bump is reviewed with its package delta (O8).
NVIDIA_PIN_FILES=(forge/specs/azoth/pins.env forge/specs/azoth/KERNEL.md forge/specs/azoth/nvidia/sources.sha256 system/nvidia/locks/open.lock system/nvidia/locks/legacy.lock)
KERNEL_PIN_FILES=("${NVIDIA_PIN_FILES[@]}" forge/specs/azoth/SOURCES/sources.sha256 forge/specs/azoth/builder/Containerfile forge/specs/azoth/builder/toolchain.lock forge/specs/azoth/boot/Containerfile forge/specs/azoth/boot/schbench.lock forge/specs/azoth/boot/toolchain.lock forge/specs/azoth/nvidia/Containerfile forge/specs/azoth/nvidia/toolchain.lock)

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

usage() { sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "$SELF" >&2; exit 2; }
die() { echo "kernel-artifacts: $*" >&2; exit 1; }
annotate() { # annotate notice|warning MESSAGE
  if [[ ${GITHUB_ACTIONS:-} == true ]]; then echo "::$1 title=Kernel artifacts::$2"; else echo "kernel-artifacts: $1: $2"; fi
}
retry() { bash "$ROOT/forge/scripts/retry.sh" "$@"; }
ask() { retry bash "$SELF" probe "$@"; }
identity() { [[ -n ${IDENTITY[${1:-}]:-} ]] || usage; echo "${IDENTITY[$1]}"; }

probe_digest() {
  local out status=0
  out=$(skopeo inspect --format '{{.Digest}}' "docker://$1" 2> "$TMP/err") || status=$?
  if [[ $status -ne 0 ]]; then
    grep -q 'manifest unknown' "$TMP/err" && return 0
    # ghcr.io answers a package that was never published (azoth-nvidia before the first
    # NVIDIA build under an owner) by denying the anonymous bearer token, not with manifest
    # unknown. A private package gets the same answer and would be rebuilt and pushed
    # again: the kernel artifacts are public by design, so the log names both readings.
    if grep -qE 'Requesting bearer token: .*403' "$TMP/err"; then
      local repo=${1%@*}
      echo "kernel-artifacts: ${repo%:*}: denied, read as never published (or not public)" >&2
      return 0
    fi
    cat "$TMP/err" >&2
    return 1
  fi
  [[ $out =~ ^sha256:[0-9a-f]{64}$ ]] || die "$1: malformed digest '$out'"
  echo "$out"
}

probe_signed() {
  local status=0
  cosign verify --certificate-identity-regexp "$(identity "$2")" --certificate-oidc-issuer "$ISSUER" "$1" > /dev/null 2> "$TMP/err" || status=$?
  if [[ $status -eq 0 ]]; then
    echo signed
  elif grep -qE "$UNVERIFIED" "$TMP/err"; then
    # stderr: stdout is the verdict. The identity it did have is what a maintainer needs to see.
    sed -n "s|.*got \"\(.*\)\".*|kernel-artifacts: $1 is signed as \1, not by a trusted ref|p" "$TMP/err" >&2
    echo unsigned
  else
    cat "$TMP/err" >&2
    return 1
  fi
}

probe_predicates() {
  local status=0
  cosign verify-attestation --type custom --certificate-identity-regexp "$(identity "$2")" --certificate-oidc-issuer "$ISSUER" "$1" > "$TMP/out" 2> "$TMP/err" || status=$?
  if [[ $status -eq 0 ]]; then
    jq -ce '.payload | @base64d | fromjson | .predicate.Data | fromjson' "$TMP/out" || die "$1: malformed attestation"
  elif grep -qE "$UNVERIFIED" "$TMP/err"; then
    echo unverified
  else
    cat "$TMP/err" >&2
    return 1
  fi
}

probe_config() { # probe_config REF LABEL: the OCI config label, empty when the manifest or the label is absent
  local out status=0
  out=$(skopeo inspect --config "docker://$1" 2> "$TMP/err") || status=$?
  if [[ $status -ne 0 ]]; then
    grep -q 'manifest unknown' "$TMP/err" && return 0
    cat "$TMP/err" >&2
    return 1
  fi
  jq -r --arg label "$2" '(.config.Labels // {})[$label] // empty' <<< "$out"
}

get() {
  local line
  [[ -f $FILE ]] || die "$FILE does not exist: run kernel-artifacts.sh resolve first"
  line=$(grep -m1 "^$1=" "$FILE") || die "$FILE has no $1"
  echo "${line#*=}"
}

write() { # write STATE LINE...: replace the file in one step
  mkdir -p "$DIR"
  printf '%s\n' "state=$1" "${@:2}" > "$FILE.tmp"
  mv "$FILE.tmp" "$FILE"
  echo "kernel-artifacts: state=$1"
}

set_kv() { # set_kv KEY=VALUE...: idempotently set each key in $FILE, replacing a stale value
  local pattern status=0
  pattern=$(printf '%s\n' "$@" | sed -E 's/^([^=]+)=.*/^\1=/' | paste -sd'|' -)
  grep -vE "$pattern" "$FILE" > "$FILE.tmp" || status=$?
  [[ $status -eq 0 || $status -eq 1 ]] || die "$FILE: could not filter the keys of a previous run"
  printf '%s\n' "$@" >> "$FILE.tmp"
  mv "$FILE.tmp" "$FILE"
}

module_verdict() { # module_verdict REF BRANCH KERNEL_DIGEST DEVEL_DIGEST: verified or unverified
  local signed predicates pins
  signed=$(ask signed "$1" modules)
  [[ $signed == signed ]] || { echo unverified; return 0; }
  predicates=$(ask predicates "$1" modules)
  [[ $predicates != unverified ]] || { echo unverified; return 0; }
  pins=$(sed -n "s/^\(NVIDIA_${2^^}_[A-Z0-9_]*\)=\(.*\)$/\1\t\2/p" "$PINS" | jq -Rn '[inputs | split("\t") | {(.[0]): .[1]}] | add')
  jq -sr --arg branch "$2" --arg kernel "$3" --arg devel "$4" --argjson pins "$pins" '
    if any(.[]; .driver == $branch and .kernel_digest == $kernel and .devel_digest == $devel and (.pins as $p | $pins | to_entries | all(.value == $p[.key])))
    then "verified" else "unverified" end' <<< "$predicates"
}

kernel_verdict() { # kernel_verdict REF: verified when a pins/inputs attestation of REF, verified
                    # against the trusted refs, equals the inputs of this checkout, unverified
                    # otherwise. The signature says who built REF, this says from what: a kernel
                    # built from other patches or another Fedora pin is signed all the same. Like
                    # module_verdict it scans every verified entry. A failure of build-inputs.py
                    # or of the verification dies here: the caller checks the substitution.
  local predicates expected status=0
  predicates=$(ask predicates "$1" kernel) || exit 1
  [[ $predicates != unverified ]] || { echo unverified; return 0; }
  expected=$(python3 "$ROOT/forge/specs/azoth/build-inputs.py" | jq -cS .) || status=$?
  [[ $status -eq 0 && -n $expected ]] || die "build-inputs.py did not produce the inputs of this checkout"
  jq -sr --argjson expected "$expected" 'if any(.[]; . == $expected) then "verified" else "unverified" end' <<< "$predicates"
}

attested_nvr() { # attested_nvr REF NVR: whether REF's cosign-verified custom pins attestation
                  # confirms NVR (each candidate FEDORA_KERNEL_NVR run through nvr.sh's own
                  # formula and shape check, shared with the pinned value so both go through the
                  # same rule) -- "confirmed" when some verified entry derives NVR, "contested"
                  # when every verified entry that carries the pin derives something else, or
                  # "unattested" when the attestation is missing, unverified, or no entry
                  # carries the pin at all. Scans every verified entry, mirroring
                  # module_verdict's any(.[]; ...): the identity regex accepts kernel-build.yml
                  # on any branch, so several verified attestations can sit on one digest, and
                  # picking an arbitrary one (the first) could contest a confirmed NVR just
                  # because an unrelated entry sorts first, silently dropping the
                  # republish/withdrawal guard. A real verification error, or a signed
                  # FEDORA_KERNEL_NVR that does not even look like one, dies here after printing
                  # why; since this runs inside the command substitution of its own caller, that
                  # only ends this subshell, so the caller must still check the substitution's
                  # own exit status to see it.
  local predicates fedoras fedora candidate status=0
  predicates=$(ask predicates "$1" kernel) || status=$?
  [[ $status -eq 0 ]] || die "$1: could not verify its pins attestation to tell whether the pins moved on"
  [[ $predicates != unverified ]] || { echo unattested; return 0; }
  fedoras=$(jq -rs '[.[].pins.FEDORA_KERNEL_NVR // empty] | map(select(. != "")) | unique[]' <<< "$predicates")
  [[ -n $fedoras ]] || { echo unattested; return 0; }
  while IFS= read -r fedora; do
    candidate=$(bash "$ROOT/forge/specs/azoth/nvr.sh" <(echo "FEDORA_KERNEL_NVR=$fedora")) || exit 1
    [[ $candidate != "$2" ]] || { echo confirmed; return 0; }
  done <<< "$fedoras"
  echo contested
}

pins_moved() { # pins_moved NVR EXPECT: whether EXPECT names a different NVR than NVR, i.e. the
                # pins moved on since EXPECT was resolved rather than NVR's own tag being
                # mutated or withdrawn. Prefers EXPECT's cosign-verified pins attestation
                # (attested_nvr); the org.opencontainers.image.version OCI label is only a
                # fallback for when that attestation is missing or carries the pin nowhere at
                # all, and unlike the attestation it is not itself verified. EXPECT with neither
                # answers false: never the benign case. A real read error on either check exits
                # non-zero instead of being folded into that same false, which would misreport
                # it as a republish or withdrawal: attested_nvr's own die only ends its command
                # substitution's subshell, so its exit status is checked explicitly here rather
                # than trusted to propagate on its own.
  local verdict label status=0
  verdict=$(attested_nvr "$REGISTRY/azoth@$2" "$1") || exit 1
  if [[ $verdict != unattested ]]; then
    [[ $verdict == contested ]]
    return
  fi
  label=$(ask config "$REGISTRY/azoth@$2" org.opencontainers.image.version) || status=$?
  [[ $status -eq 0 ]] || die "$REGISTRY/azoth@$2: could not read its OCI config to tell whether the pins moved on"
  [[ -n $label && $label != "$1" ]]
}

resolve() {
  local expect='' nvr kernel devel kernel_signed devel_signed branch version tag digest verdict state=ready
  while [[ $# -gt 0 ]]; do
    case $1 in
      --expect-kernel-digest) [[ $# -ge 2 ]] || usage; expect=$2; shift 2 ;;
      *) usage ;;
    esac
  done
  rm -f "$FILE"
  nvr=$(bash "$ROOT/forge/specs/azoth/nvr.sh")
  local -a lines=("nvr=$nvr" "registry=$REGISTRY")
  # An expectation binds to the NVR it was resolved for. When the pins have since moved to a
  # different NVR, it no longer applies to this resolution at all: drop it and resolve exactly
  # as an unconstrained caller would (O5). Every path below still reports only the
  # freshly-verified digest of the CURRENT nvr, never $expect, so a caller is still never told
  # ready about a different kernel even with the expectation dropped.
  if [[ -n $expect ]] && pins_moved "$nvr" "$expect"; then
    expect=''
  fi
  kernel=$(ask digest "$REGISTRY/azoth:$nvr")
  devel=$(ask digest "$REGISTRY/azoth-devel:$nvr")
  if [[ -z $kernel || -z $devel ]]; then
    [[ -z $expect ]] || die "$REGISTRY/azoth:$nvr is no longer published, the caller resolved $expect: the kernel was republished or withdrawn since"
    write kernel-missing "${lines[@]}"
    return 0
  fi
  [[ -z $expect || $kernel == "$expect" ]] || die "$REGISTRY/azoth:$nvr is $kernel, the caller resolved $expect: the kernel was republished since"
  kernel_signed=$(ask signed "$REGISTRY/azoth@$kernel" kernel)
  devel_signed=$(ask signed "$REGISTRY/azoth-devel@$devel" kernel)
  if [[ $kernel_signed != signed || $devel_signed != signed ]]; then
    [[ -z $expect ]] || die "$REGISTRY/azoth:$nvr is no longer signed, the caller resolved $expect: the kernel was republished or its signature was revoked since"
    write kernel-missing "${lines[@]}"
    return 0
  fi
  verdict=$(kernel_verdict "$REGISTRY/azoth@$kernel") || exit 1
  if [[ $verdict != verified ]]; then
    [[ -z $expect ]] || die "$REGISTRY/azoth:$nvr is $kernel but was not built from the inputs of this checkout, the caller resolved $expect: the kernel was republished with other inputs"
    write kernel-missing "${lines[@]}"
    return 0
  fi
  lines+=("kernel_digest=$kernel" "devel_digest=$devel")
  for branch in open legacy; do
    version=$(sed -n "s/^NVIDIA_${branch^^}_VERSION=//p" "$PINS")
    [[ -n $version ]] || die "NVIDIA_${branch^^}_VERSION is not set in $PINS"
    tag="$nvr-k${kernel:7:12}-$branch-$version"
    lines+=("nvidia_${branch}_version=$version" "nvidia_${branch}_tag=$tag")
    digest=$(ask digest "$REGISTRY/azoth-nvidia:$tag")
    verdict=unverified
    [[ -z $digest ]] || verdict=$(module_verdict "$REGISTRY/azoth-nvidia@$digest" "$branch" "$kernel" "$devel")
    if [[ $verdict == verified ]]; then
      lines+=("nvidia_${branch}_digest=$digest")
    else
      state=modules-missing
    fi
  done
  write "$state" "${lines[@]}"
}

changed_files() { # changed_files BASE HEAD
  git cat-file -e "$1^{commit}" 2> /dev/null || retry git fetch --no-tags --depth=1 origin "$1"
  git diff --name-only "$1" "$2"
}

matches_any() { # matches_any PATH PATTERN...
  local path=$1 pattern
  shift
  for pattern in "$@"; do
    # shellcheck disable=SC2053 # the right-hand side is a glob on purpose
    [[ $path == $pattern ]] && return 0
  done
  return 1
}

kernel_build_touched() { # kernel_build_touched BASE HEAD: yes or no
  local files file
  files=$(changed_files "$1" "$2")
  while IFS= read -r file; do
    if [[ -n $file ]] && matches_any "$file" "${KERNEL_BUILD_PATHS[@]}"; then
      echo yes
      return 0
    fi
  done <<< "$files"
  echo no
}

cycle() {
  local event='' before='' after='' sha='' head='' state nvr decision=build touched
  while [[ $# -gt 0 ]]; do
    [[ $# -ge 2 ]] || usage
    case $1 in
      --event) event=$2 ;;
      --before) before=$2 ;;
      --after) after=$2 ;;
      --sha) sha=$2 ;;
      --head) head=$2 ;;
      *) usage ;;
    esac
    shift 2
  done
  # Argument shape is checked before the state, so a malformed call fails even on state=ready:
  # a short SHA or an empty --head must never be compared as if it settled who owns the cycle.
  case $event in
    push)
      [[ $before =~ ^[0-9a-f]{40}$ && $after =~ ^[0-9a-f]{40}$ ]] || die "cycle: --event push needs --before and --after, each a 40-hex-character commit SHA"
      ;;
    workflow_dispatch)
      [[ -z $sha ]] || [[ $sha =~ ^[0-9a-f]{40}$ && $head =~ ^[0-9a-f]{40}$ ]] || die "cycle: --sha and --head must each be a 40-hex-character commit SHA"
      ;;
    schedule) ;;
    *) die "cycle: --event must be push, workflow_dispatch or schedule, got '$event'" ;;
  esac
  state=$(get state)
  nvr=$(get nvr)
  if [[ $state != ready ]]; then
    case $event in
      push)
        if [[ $before =~ ^0+$ ]]; then
          [[ $state == modules-missing ]] || die "azoth:$nvr is not published, and a push without a previous commit (a new branch) cannot tell whether Kernel Build owns it"
        else
          touched=$(kernel_build_touched "$before" "$after")
          if [[ $touched == yes ]]; then
            decision=defer
            annotate notice "$state for azoth:$nvr, and this push starts Kernel Build, which dispatches the Orchestrator for it: no image in this run"
          else
            [[ $state == modules-missing ]] || die "azoth:$nvr is not published and this push does not start Kernel Build"
          fi
        fi
        ;;
      workflow_dispatch)
        if [[ $state == kernel-missing ]]; then
          [[ -n $sha && $sha != "$head" ]] || die "azoth:$nvr is not published"
          decision=defer
          annotate notice "azoth:$nvr is not published and the branch moved from $sha to $head: the newer push has its own cycle"
        fi
        ;;
      schedule)
        [[ $state == modules-missing ]] || die "azoth:$nvr is not published"
        ;;
    esac
  fi
  set_kv "cycle=$decision"
  echo "kernel-artifacts: cycle=$decision"
}

check_plan() {
  local base='' head='' state nvr files file keys key only_kernel_pins=true only_nvidia_pins=true nvidia_moved=false other_moved=false gpus delta
  while [[ $# -gt 0 ]]; do
    [[ $# -ge 2 ]] || usage
    case $1 in
      --base) base=$2 ;;
      --head) head=$2 ;;
      *) usage ;;
    esac
    shift 2
  done
  [[ -n $base && -n $head ]] || usage
  state=$(get state)
  nvr=$(get nvr)
  files=$(changed_files "$base" "$head")
  while IFS= read -r file; do
    [[ -n $file ]] || continue
    matches_any "$file" "${KERNEL_PIN_FILES[@]}" || only_kernel_pins=false
    matches_any "$file" "${NVIDIA_PIN_FILES[@]}" || only_nvidia_pins=false
  done <<< "$files"
  keys=$(git diff -U0 "$base" "$head" -- forge/specs/azoth/pins.env | sed -n 's/^[-+]\([A-Z_][A-Z0-9_]*\)=.*/\1/p' | sort -u)
  while IFS= read -r key; do
    [[ -n $key ]] || continue
    if [[ $key == NVIDIA_* ]]; then nvidia_moved=true; else other_moved=true; fi
  done <<< "$keys"
  case $state in
    ready)
      gpus='none nvidia nvidia-legacy' delta=true
      ;;
    kernel-missing)
      # only_kernel_pins alone would also be true when the diff moves NVIDIA pins alone: that
      # never explains a missing kernel, so this row requires an actual kernel pin to move too.
      [[ $only_kernel_pins == true && $other_moved == true ]] || die "azoth:$nvr is not published: a pin bump mixed with other changes cannot be checked, move the pins in their own pull request"
      gpus='' delta=false
      annotate warning "azoth:$nvr is not published yet: Kernel Build on this pull request proves the kernel and the modules build and boot; the images are built after the merge"
      ;;
    modules-missing)
      [[ $only_nvidia_pins == true && $nvidia_moved == true && $other_moved == false ]] || die "the NVIDIA modules of azoth:$nvr are not published: with unchanged NVIDIA pins publishing them is the Orchestrator's job (bootstrap or interrupted publication), and NVIDIA pins move in their own pull request"
      gpus=none delta=true
      annotate warning "the NVIDIA modules of the new pins are not published yet: only the default image is built; the variants are built and gated after the merge"
      ;;
    *) die "$FILE: unknown state '$state'" ;;
  esac
  set_kv "check_gpus=$gpus" "check_delta=$delta"
  echo "kernel-artifacts: check_gpus='$gpus' check_delta=$delta"
}

[[ $# -ge 1 ]] || usage
command=$1
shift
case $command in
  resolve) resolve "$@" ;;
  require-ready)
    [[ $# -eq 0 ]] || usage
    resolve
    state=$(get state)
    [[ $state == ready ]] || die "state=$state: the kernel of the pins and both NVIDIA module branches must be published, signed and attested"
    ;;
  cycle) cycle "$@" ;;
  check-plan) check_plan "$@" ;;
  get) [[ $# -eq 1 ]] || usage; get "$1" ;;
  has) [[ $# -eq 1 ]] || usage; [[ -f $FILE ]] && grep -q "^$1=." "$FILE" ;;
  registry) [[ $# -eq 0 ]] || usage; echo "$REGISTRY" ;;
  digest) [[ $# -eq 1 ]] || usage; ask digest "$1" ;;
  signed) [[ $# -eq 2 ]] || usage; ask signed "$1" "$2" ;;
  predicates) [[ $# -eq 2 ]] || usage; ask predicates "$1" "$2" ;;
  probe)
    [[ $# -ge 2 ]] || usage
    case $1 in
      digest) [[ $# -eq 2 ]] || usage; probe_digest "$2" ;;
      signed) [[ $# -eq 3 ]] || usage; probe_signed "$2" "$3" ;;
      predicates) [[ $# -eq 3 ]] || usage; probe_predicates "$2" "$3" ;;
      config) [[ $# -eq 3 ]] || usage; probe_config "$2" "$3" ;;
      *) usage ;;
    esac
    ;;
  *) usage ;;
esac
