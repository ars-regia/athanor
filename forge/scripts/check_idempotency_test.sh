#!/bin/bash
# Checks that the registry probe in check_idempotency.sh tells its three answers apart,
# against images that really are and really are not in the registry.
#
# Run it from anywhere with skopeo available:
#   bash forge/scripts/check_idempotency_test.sh
# The probed image is $REGISTRY_HOST/$GITHUB_REPOSITORY_OWNER/athanor-forge-tetragon (defaults
# ghcr.io and ars-regia), the variables the workflows and clean_ghcr.sh read.
#
# The case that matters is the first one. The probe used to pass --creds whenever a token
# was set, and a token the registry rejects makes skopeo fail with 403 even on an image
# anyone can read anonymously. That failure was read as "image absent", so every package
# rebuilt on every run: 47 nodes and 4.6 hours of runner time for a push that touched none
# of them. Anonymous first, credentials only as a fallback, and a registry that does not
# answer at all is an error rather than a silent rebuild.
set -u
owner=${GITHUB_REPOSITORY_OWNER:-ars-regia}
image=${REGISTRY_HOST:-ghcr.io}/${owner,,}/athanor-forge-tetragon
# A tag every kept package carries; the hash tags of a package change on each rebuild.
h_present=latest
h_absent=0000000000000000000000000000000000000000000000000000000000000000

fail=0
check() { # check LABEL EXPECTED ACTUAL
  if [[ "$2" == "$3" ]]; then echo "  ok  $1 -> $3"; else echo "  FAIL $1: expected $2, got $3"; fail=1; fi
}

# First, the environment fault itself, because it has to be measured before it is worked
# around. The job runs the check with --userns=keep-id: not root, while HOME is still
# /root. skopeo reads its registry configuration from HOME before it opens any socket, so
# an unwritable HOME fails in milliseconds and no request is ever made. That failure read
# as "image absent" and rebuilt the whole graph.
if [[ $(id -u) -ne 0 && ! -w ${HOME:-/root} ]]; then
  if skopeo inspect --no-tags "docker://${image}:${h_present}" > /dev/null 2>&1; then
    echo "  FAIL unwritable HOME: skopeo unexpectedly succeeded, the workaround is now moot"
    fail=1
  else
    echo "  ok  unwritable HOME breaks a bare skopeo (what the script works around)"
  fi
else
  echo "  skip HOME fault: needs a non-root user with an unwritable HOME"
fi

# From here on, the same guard check_idempotency.sh applies, so the probe answers are
# about the registry rather than about the home directory.
[[ -w ${HOME:-/root} ]] || { HOME=$(mktemp -d); export HOME; }

probe() { # probe TAG TOKEN
  local url="docker://${image}:$1" token=$2
  local status="" err rc
  for attempt in anonymous authenticated; do
    local args=("--no-tags")
    if [[ $attempt == authenticated ]]; then
      [[ -n $token ]] || continue
      args+=("--creds" "${owner}:${token}")
    fi
    err=$(skopeo inspect "${args[@]}" "$url" 2>&1 >/dev/null); rc=$?
    if [[ $rc -eq 0 ]]; then status=found; break; fi
    if grep -qi 'manifest unknown\|name unknown\|not found' <<< "$err"; then status=absent; break; fi
    status=error
  done
  echo "$status"
}

# The exact CI failure: a real image, a token the registry rejects.
check "present image + rejected token" found "$(probe "$h_present" not-a-real-token)"
# No token at all, the way a local run works.
check "present image, no token"        found "$(probe "$h_present" "")"
# A tag that genuinely does not exist must read as absent, not as an error.
check "missing image"                  absent "$(probe "$h_absent" "")"
check "missing image + rejected token" absent "$(probe "$h_absent" not-a-real-token)"

exit $fail
