#!/usr/bin/env bash
# Builds the kernel builder image on the self-hosted runner, for every event every layer again
# from a freshly pulled base, so a build starts from the checkout alone. The pull request's
# build is the one the push to iso-v0 may publish (ADR-0110), so it gets no cached layers either.
# Run from the repository root.
set -euo pipefail

podman build --no-cache --pull=always -t localhost/azoth-builder -f forge/specs/azoth/builder/Containerfile forge/specs/azoth
