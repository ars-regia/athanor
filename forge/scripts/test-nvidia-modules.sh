#!/bin/bash
set -euo pipefail

MODULES_DIR="${1:-/usr/lib/modules}"

if [ ! -d "$MODULES_DIR" ]; then
    echo "ERROR: Directory $MODULES_DIR does not exist." >&2
    exit 1
fi

# Find target kernel directory
KDIR=""
if [ -d "$MODULES_DIR/$(uname -r 2>/dev/null)" ]; then
    KDIR="$MODULES_DIR/$(uname -r)"
else
    for d in "$MODULES_DIR"/*; do
        if [ -d "$d" ] && { [ -e "$d/vmlinuz" ] || [ -L "$d/vmlinuz" ]; }; then
            KDIR="$d"
            break
        fi
    done
    [ -z "$KDIR" ] && for d in "$MODULES_DIR"/*; do [ -d "$d" ] && KDIR="$d" && break; done
fi

if [ -z "$KDIR" ] || [ ! -d "$KDIR" ]; then
    echo "ERROR: No kernel directory found in $MODULES_DIR." >&2
    exit 1
fi

echo "Verifying NVIDIA kernel modules in: $KDIR"

MISSING=0
for mod in nvidia nvidia-drm nvidia-modeset; do
    alt_mod="${mod//-/_}"
    MATCH=$(find "$KDIR" \( -name "${mod}.ko*" -o -name "${alt_mod}.ko*" \) -print -quit 2>/dev/null)
    if [ -n "$MATCH" ]; then
        echo "  [OK] Found ${mod}: ${MATCH}"
    else
        echo "  [FAIL] Missing module: ${mod}" >&2
        MISSING=1
    fi
done

if [ "$MISSING" -ne 0 ]; then
    echo "ERROR: Missing required NVIDIA kernel modules." >&2
    exit 1
fi

echo "SUCCESS: All required NVIDIA kernel modules are present."
