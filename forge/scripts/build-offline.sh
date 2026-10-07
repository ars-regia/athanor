#!/usr/bin/env bash
# ==============================================================================
# 🌋 Athanor OS - Local Offline Bootc Build Fallback Script
# ==============================================================================
# Used when GitHub Actions or Cloud CI is in an outage or network is offline.
# Builds the bootc system container image locally with podman without touching
# or overwriting the primary Cloud infrastructure (GHCR registry).
# ==============================================================================

set -euo pipefail

IMAGE_NAME="${1:-localhost/athanor-system}"
IMAGE_TAG="${2:-offline}"
FULL_IMAGE_REF="${IMAGE_NAME}:${IMAGE_TAG}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SYSTEM_DIR="${SCRIPT_DIR}/../../system"

echo "======================================================================"
echo "🌋 Athanor OS — Local Offline Fallback Build"
echo "======================================================================"
echo "🎯 Target Local Image: ${FULL_IMAGE_REF}"
echo "📁 Context Directory:  ${SYSTEM_DIR}"
echo "======================================================================"

# Safety check: Ensure local tag does not overwrite production GHCR image unintentionally
if [[ "${IMAGE_NAME}" == *"ghcr.io"* ]] && [[ "${IMAGE_TAG}" == "latest" ]]; then
    echo "⚠️  WARNING: Target image is set to production cloud ref '${FULL_IMAGE_REF}'."
    echo "   Using local isolation prefix 'localhost/' to protect primary cloud infrastructure."
    IMAGE_NAME="localhost/athanor-system"
    FULL_IMAGE_REF="${IMAGE_NAME}:${IMAGE_TAG}"
    echo "   Updated Target: ${FULL_IMAGE_REF}"
fi

# Tool checks
if ! command -v podman &>/dev/null; then
    echo "❌ Error: 'podman' is required but not installed or not in PATH." >&2
    exit 1
fi

# Detect network availability to select pull policy
PULL_FLAG="--pull=newer"
echo "🌐 Checking network connectivity for container base images..."
if ! ping -c 1 -w 2 8.8.8.8 &>/dev/null && ! ping -c 1 -w 2 1.1.1.1 &>/dev/null; then
    echo "🔌 Network offline detected. Switching podman pull policy to '--pull=never'."
    PULL_FLAG="--pull=never"
else
    echo "⚡ Network available. Podman pull policy set to '${PULL_FLAG}'."
fi

# Gather Git metadata if available
BUILD_ARGS=()
if command -v git &>/dev/null && git rev-parse --is-inside-work-tree &>/dev/null; then
    SHA_SHORT=$(git rev-parse --short HEAD 2>/dev/null || echo "offline-local")
    BUILD_ARGS+=("--build-arg" "SHA_HEAD_SHORT=${SHA_SHORT}")
fi

# No key reaches the image build: the vmlinuz arrives already signed in azoth-boot, signed by
# the sign-only job of nvidia-kmod (docs/architecture/doc_ci.md, D43; ADR-0037).

BUILD_DATE=$(date -u +%Y-%m-%d\T%H:%M:%SZ)

echo "🏗️  Starting local podman build..."
podman build \
    ${PULL_FLAG} \
    --tag "${FULL_IMAGE_REF}" \
    --label "org.opencontainers.image.created=${BUILD_DATE}" \
    --label "org.opencontainers.image.title=athanor-system-offline" \
    --label "org.opencontainers.image.description=Athanor OS - Immutable Bootc System (Local Offline Fallback)" \
    --label "containers.bootc=1" \
    --label "athanor.build.type=offline-fallback" \
    "${BUILD_ARGS[@]}" \
    -f "${SYSTEM_DIR}/Containerfile" \
    .

echo ""
echo "🔍 Validating local bootc container image structure..."
if podman run --rm "${FULL_IMAGE_REF}" bootc container lint; then
    echo "✅ Bootc container lint check passed successfully!"
else
    echo "⚠️  Bootc container lint check emitted warnings or failed."
fi

echo ""
echo "======================================================================"
echo "🎉 LOCAL OFFLINE BUILD COMPLETE!"
echo "======================================================================"
echo "📦 Image available in local storage:"
podman images "${FULL_IMAGE_REF}"
echo ""
echo "💡 Next Steps:"
echo "   • Test container locally:"
echo "     podman run --rm -it ${FULL_IMAGE_REF} /bin/bash"
echo ""
echo "   • Build local QCOW2 VM image from this fallback build:"
echo "     just disk-qcow2 target_image=${IMAGE_NAME} tag=${IMAGE_TAG}"
echo "======================================================================"
