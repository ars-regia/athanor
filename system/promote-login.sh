#!/usr/bin/env bash
# Logs the promotion jobs in to the image registry and to the registry off GitHub that keeps
# the evidence bundle (ADR-0103 D23, ADR-0106). EVIDENCE_REGISTRY has no default: unset, the
# promotion fails closed.
# Environment: GITHUB_TOKEN, GITHUB_ACTOR, REGISTRY_HOST, EVIDENCE_REGISTRY,
#              EVIDENCE_REGISTRY_USER, EVIDENCE_REGISTRY_TOKEN.
set -euo pipefail
for var in EVIDENCE_REGISTRY EVIDENCE_REGISTRY_USER EVIDENCE_REGISTRY_TOKEN; do
    [[ -n ${!var:-} ]] || {
        echo "::error::$var is not set: no promotion without the copy off GitHub (D23, ADR-0106)"
        exit 1
    }
done
echo "$GITHUB_TOKEN" | skopeo login "$REGISTRY_HOST" -u "$GITHUB_ACTOR" --password-stdin
echo "$EVIDENCE_REGISTRY_TOKEN" | podman login "${EVIDENCE_REGISTRY%%/*}" -u "$EVIDENCE_REGISTRY_USER" --password-stdin
