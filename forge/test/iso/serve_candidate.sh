#!/usr/bin/env bash
# serve_candidate.sh REPOSITORY DIGEST WORKDIR: serves the candidate of the upgrade case
# (doc_update_delivery.md UD18 step 3, ADR-0130) as REPOSITORY's path :stable from a registry
# on the runner's 127.0.0.1:5000, which the guest reaches through QEMU's gateway as
# 10.0.2.2:5000. The candidate's signature (sha256-<hex>.sig) and attestation
# (sha256-<hex>.att) are served beside it, and the run fails without either: the client
# reads both, and without the attestation it would only warn. The attestation is verified
# first with the public keys of the checkout (scripts/ci/release_attestation.py class, as
# system/verify-images.sh verifies it; VERIFY_KEYS_DIR, default system/keys), and the class it
# names lands in WORKDIR/class: the class the guest must settle on.
#
# The registry is left running in the background: WORKDIR/registry.pid names it, for the
# caller to stop, and WORKDIR/mirror holds the location the guest's registries.conf mirror
# names. On a failure the registry is stopped here.
set -euo pipefail

[[ $# -eq 3 ]] || {
    echo "usage: serve_candidate.sh REPOSITORY DIGEST WORKDIR" >&2
    exit 2
}
repository=$1 digest=$2 work=$3
[[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || {
    echo "not a digest: ${digest}" >&2
    exit 2
}
[[ $repository =~ ^[^/]+/[^:@]+$ ]] || {
    echo "not a repository: ${repository}" >&2
    exit 2
}
path=${repository#*/}
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
retry=${here}/../../scripts/retry.sh
local_repository=127.0.0.1:5000/${path}

mkdir -p "${work}/store"
"$retry" python3 -B "${here}/../../../scripts/ci/release_attestation.py" class \
    --keys-dir "${VERIFY_KEYS_DIR:-${here}/../../../system/keys}" "${repository}@${digest}" < /dev/null > "${work}/class"
cat > "${work}/registry.yml" << EOF
version: 0.1
log:
  level: error
  accesslog:
    disabled: true
storage:
  filesystem:
    rootdirectory: ${work}/store
http:
  addr: 127.0.0.1:5000
EOF
registry serve "${work}/registry.yml" > "${work}/registry.log" 2>&1 &
registry_pid=$!
echo "$registry_pid" > "${work}/registry.pid"
stop_on_failure() {
    local status=$?
    if [[ $status -ne 0 ]]; then
        kill "$registry_pid"
        echo "registry log:" >&2
        cat "${work}/registry.log" >&2
    fi
}
trap stop_on_failure EXIT
python3 -c '
import socket, sys, time
for _ in range(100):
    try:
        socket.create_connection(("127.0.0.1", 5000), 1).close()
        sys.exit(0)
    except OSError:
        time.sleep(0.2)
sys.exit("the registry did not start on 127.0.0.1:5000")'

# The registry serves plain HTTP on the loopback: --dest-tls-verify=false lets skopeo use it.
"$retry" skopeo copy --preserve-digests --dest-tls-verify=false \
    "docker://${repository}@${digest}" "docker://${local_repository}:stable"
for suffix in sig att; do
    tag=sha256-${digest#sha256:}.${suffix}
    "$retry" skopeo copy --preserve-digests --dest-tls-verify=false \
        "docker://${repository}:${tag}" "docker://${local_repository}:${tag}" ||
        {
            echo "the candidate ${digest} has no ${tag} in ${repository} (ADR-0130)" >&2
            exit 1
        }
done

served=sha256:$(skopeo inspect --raw --tls-verify=false "docker://${local_repository}:stable" | sha256sum | cut -d' ' -f1)
[[ $served == "$digest" ]] || {
    echo "the mirror serves ${served} as :stable, not ${digest}" >&2
    exit 1
}
echo "10.0.2.2:5000/${path}" > "${work}/mirror"
echo "=== candidate ${digest} served as 10.0.2.2:5000/${path}:stable, with its .sig and its .att of the $(< "${work}/class") class"
