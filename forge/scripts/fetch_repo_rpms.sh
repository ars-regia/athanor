#!/bin/bash
set -euo pipefail

mkdir -p repo-cache/repo \
         repo-cache/repo-tier0 \
         repo-cache/repo-tier1 \
         repo-cache/repo-tier2 \
         repo-cache/repo-tier3

export STORAGE_DRIVER=vfs
export BUILDAH_ISOLATION=chroot
export XDG_DATA_HOME=/var/tmp
export XDG_CONFIG_HOME=/var/tmp
export XDG_CACHE_HOME=/var/tmp
export _CONTAINERS_USERNS_CONFIGURED=done

# Clean up any legacy flat .rpm files from previous runs to prevent stale packages
find repo-cache/ -maxdepth 2 -name "*.rpm" -type f -delete

TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

OWNER="${1:-hr-mes}"

# Fetch package lists dynamically from Single Source of Truth
readarray -t CUSTOM_TIER0 < <(jq -r '.custom_tier0[] // empty' config/packages.json)
readarray -t CUSTOM_TIER1 < <(jq -r '.custom_tier1[] // empty' config/packages.json)
readarray -t CUSTOM_TIER2 < <(jq -r '.custom_tier2[] // empty' config/packages.json)
readarray -t CUSTOM_TIER3 < <(jq -r '.custom_tier3[] // empty' config/packages.json)

readarray -t UPSTREAM_CORE < <(jq -r '.upstream_core[] // empty' config/packages.json)
readarray -t UPSTREAM_DESKTOP < <(jq -r '.upstream_desktop[] // empty' config/packages.json)
readarray -t UPSTREAM_MEDIA < <(jq -r '.upstream_media[] // empty' config/packages.json)
readarray -t UPSTREAM_CLI < <(jq -r '.upstream_cli[] // empty' config/packages.json)

# Per-tier package images. An entry without a tag means :latest. The kernel is the
# azoth image of the pins by the digest system/kernel-artifacts.sh verified, never by tag
# (docs/architecture/doc_build_ordering.md, O4): run its require-ready first. The NVIDIA
# modules are not RPMs: system/Containerfile copies them from their image by digest.
ARTIFACTS="$(dirname "${BASH_SOURCE[0]}")/../../system/kernel-artifacts.sh"
KERNEL_STATE=$(bash "$ARTIFACTS" get state)
[[ $KERNEL_STATE == ready ]] || { echo "[FATAL] kernel artifacts are ${KERNEL_STATE}, not ready: run system/kernel-artifacts.sh require-ready first" >&2; exit 1; }
KERNEL_NVR=$(bash "$ARTIFACTS" get nvr)
PINNED_NVR=$(bash "$(dirname "${BASH_SOURCE[0]}")/../specs/azoth/nvr.sh")
[[ $KERNEL_NVR == "$PINNED_NVR" ]] || { echo "[FATAL] kernel artifacts were resolved for ${KERNEL_NVR}, the pins give ${PINNED_NVR}: run system/kernel-artifacts.sh resolve again" >&2; exit 2; }
KERNEL_DIGEST=$(bash "$ARTIFACTS" get kernel_digest)

# Packages published by a dedicated workflow, not the DAG, so no athanor-forge-<pkg>
# image is ever built for them -- the same exclusion dag_orchestrator.py applies. The
# kernel is here as azoth@<digest> in tier 0; kernel-forge names it in custom_tier2 but
# has no image of its own. Skipping them keeps the fatal-on-missing check below true
# only for packages that really should have an image.
is_external() { case "$1" in kernel|kernel-forge) return 0 ;; *) return 1 ;; esac; }

TIER0_IMAGES=(
  "azoth@${KERNEL_DIGEST}"
)
for pkg in "${CUSTOM_TIER0[@]}"; do
  [[ -n "$pkg" ]] && ! is_external "$pkg" && TIER0_IMAGES+=("athanor-forge-$pkg")
done

TIER1_IMAGES=()
for pkg in "${CUSTOM_TIER1[@]}"; do
  [[ -n "$pkg" ]] && ! is_external "$pkg" && TIER1_IMAGES+=("athanor-forge-$pkg")
done

# The upstream_* packages (COSMIC, media, CLI) are NOT built into per-package rolling
# images: no athanor-forge-rolling-<pkg> is published, and pulling them here failed
# silently, so 50 of them were absent from the image while the build stayed green. They
# are installed as Fedora/RPM Fusion binaries directly in system/Containerfile. When the
# zero-trust rebuild-from-source lands (2026-09-11 decision), each will get a spec and a
# tier image, and its name moves into the custom_tier* lists above -- not back here as a
# rolling image.

TIER2_IMAGES=()
for pkg in "${CUSTOM_TIER2[@]}"; do
  [[ -n "$pkg" ]] && ! is_external "$pkg" && TIER2_IMAGES+=("athanor-forge-$pkg")
done

TIER3_IMAGES=()
for pkg in "${CUSTOM_TIER3[@]}"; do
  [[ -n "$pkg" ]] && ! is_external "$pkg" && TIER3_IMAGES+=("athanor-forge-$pkg")
done

declare -A OLD_DIGESTS
declare -A NEW_DIGESTS

pull_and_extract() {
  local img="$1"
  local target_dir="$2"
  local ref="$img"
  [[ "$ref" == *:* ]] || ref="$ref:latest"
  local IMAGE_LOWER=$(echo "ghcr.io/$OWNER/$ref" | tr '[:upper:]' '[:lower:]')
  
  local old_digest="${OLD_DIGESTS[$img]:-}"
  local new_digest=""
  local inspect_out
  if inspect_out=$(skopeo inspect --config "docker://$IMAGE_LOWER" 2>/dev/null); then
    new_digest=$(echo "$inspect_out" | jq -r '.config.Labels["org.opencontainers.image.revision"] // .config.Labels["tier.content.sha256"] // ""')
  fi
  if [ -z "$new_digest" ]; then
    if inspect_out=$(skopeo inspect "docker://$IMAGE_LOWER" 2>/dev/null); then
      new_digest=$(echo "$inspect_out" | jq -r '.Digest // ""')
    fi
  fi

  if [ -n "$old_digest" ] && [ -n "$new_digest" ] && [ "$old_digest" = "$new_digest" ]; then
    echo "    [CACHE HIT] $img hasn't changed. Skipping pull."
    echo "$new_digest" > "$TMP_DIR/digest_$img"
    return 0
  fi
  
  echo "    [CACHE MISS] Pulling $img (old: $old_digest, new: $new_digest)"
  echo "$new_digest" > "$TMP_DIR/digest_$img"

  local ctr=""
  if ctr=$(buildah from "$IMAGE_LOWER" 2>/dev/null); then
    local mnt
    mnt=$(buildah mount "$ctr")
    (
      flock 200
      # Prune old versions by wiping the image's dedicated subdirectory
      if [ -d "$target_dir/$img" ]; then
        rm -rf "$target_dir/$img"
      fi
      mkdir -p "$target_dir/$img"
      
      readarray -t mnt_rpms < <(find "$mnt" -maxdepth 1 -name "*.rpm" -type f)
      if [ ${#mnt_rpms[@]} -gt 0 ]; then
        cp -a "${mnt_rpms[@]}" "$target_dir/$img/"
      fi
    ) 200>"$target_dir/.lock"
    buildah umount "$ctr"
    buildah rm "$ctr"
  else
    # Every image in the TIER*_IMAGES lists is a custom package the DAG has just built and
    # published, so a missing one is a real failure -- a package that will be absent from
    # the system image. This used to be swallowed (return 0), which is how 50 packages went
    # missing while the build stayed green. Fail loudly instead.
    echo "    [FATAL] Image not found or pull failed: $IMAGE_LOWER" >&2
    return 1
  fi
}

echo "=== Restoring Aggregate Tier Repos (Caching) ==="
for tier in tier0 tier1 tier2 tier3; do
  img="athanor-forge-${tier}-repo:latest"
  IMAGE_LOWER=$(echo "ghcr.io/$OWNER/$img" | tr '[:upper:]' '[:lower:]')
  echo "    Pulling previous $IMAGE_LOWER..."
  ctr=""
  if ctr=$(buildah from "$IMAGE_LOWER" 2>/dev/null); then
    mnt=$(buildah mount "$ctr")
    # The published repository keeps one directory per package image: restore them as
    # they are, so that a cache hit below finds its RPMs in place and a package that
    # left the manifest can be pruned by name.
    readarray -t cached_dirs < <(find "$mnt" -mindepth 1 -maxdepth 1 -type d ! -name repodata)
    if [ ${#cached_dirs[@]} -gt 0 ]; then
      cp -a "${cached_dirs[@]}" "repo-cache/repo-${tier}/"
    fi
    if [ -f "$mnt/manifest.json" ]; then
      cp -a "$mnt/manifest.json" "repo-cache/repo-${tier}/"
    fi
    buildah umount "$ctr"
    buildah rm "$ctr"
  fi
  declare -n current_images="TIER${tier#tier}_IMAGES"
  for dir in "repo-cache/repo-${tier}"/*/; do
    [ -d "$dir" ] || continue
    name=$(basename "$dir")
    if [[ " ${current_images[*]} " != *" $name "* ]]; then
      echo "    [PRUNE] $name is no longer in the manifest: dropping its RPMs from $tier"
      rm -rf "$dir"
    fi
  done
  unset -n current_images
  
  # Load old digests
  if [ -f "repo-cache/repo-${tier}/manifest.json" ]; then
    while read -r k v; do
      OLD_DIGESTS["$k"]="$v"
    done < <(jq -r 'to_entries[] | "\(.key) \(.value)"' "repo-cache/repo-${tier}/manifest.json")
  fi
done

echo "=== Extracting Tier 0 RPMs ==="
for img in "${TIER0_IMAGES[@]}"; do
  pull_and_extract "$img" "repo-cache/repo-tier0" &
done

echo "=== Extracting Tier 1 RPMs ==="
for img in "${TIER1_IMAGES[@]}"; do
  pull_and_extract "$img" "repo-cache/repo-tier1" &
done

echo "=== Extracting Tier 2 RPMs ==="
for img in "${TIER2_IMAGES[@]}"; do
  pull_and_extract "$img" "repo-cache/repo-tier2" &
done

echo "=== Extracting Tier 3 RPMs ==="
for img in "${TIER3_IMAGES[@]}"; do
  pull_and_extract "$img" "repo-cache/repo-tier3" &
done

for pid in $(jobs -p); do
  wait $pid || { echo "FATAL: a parallel extraction job failed"; exit 1; }
done

echo "=== Post-Processing: Deduplicating RPMs (Keeping Latest) ==="
# In case of leftover duplicates from parallel jobs, keep only the latest version of each RPM
for tier in tier0 tier1 tier2 tier3; do
  # First pass: if azoth exists anywhere in this tier, purge all old 'kernel' packages
  for prefix in kernel kernel-core kernel-modules kernel-modules-core kernel-modules-extra kernel-modules-internal kernel-uki-virt kernel-uki-virt-addons kernel-devel kernel-devel-matched; do
    if find repo-cache/repo-${tier}/ -type f -name "azoth-[0-9]*.rpm" | grep -q .; then
      echo "    [DEDUPLICATION] Found azoth. Removing obsolete ${prefix}..."
      find repo-cache/repo-${tier}/ -type f -name "${prefix}-[0-9]*.rpm" -delete
    fi
  done

  # Second pass: generic deduplication across subdirectories
  # We group RPMs by package name, and if multiple versions exist, we keep the newest.
  declare -A seen_pkgs
  while IFS= read -r rpm_file; do
    [ -e "$rpm_file" ] || continue
    file_basename=$(basename "$rpm_file")
    pkg_name=$(echo "$file_basename" | sed -E 's/-[0-9].*//')
    if [ -n "$pkg_name" ]; then
      if [[ "$pkg_name" == kmod-nvidia-* ]]; then
        pkg_name="kmod-nvidia"
      fi
      # Collect all RPMs matching this package name recursively
      if [ -z "${seen_pkgs[$pkg_name]+x}" ]; then
        seen_pkgs[$pkg_name]=1
        readarray -t matching_rpms < <(find repo-cache/repo-${tier}/ -type f -name "${pkg_name}-[0-9]*.rpm")
        
        if [ ${#matching_rpms[@]} -gt 1 ]; then
          # Sort by version first (ascending)
          readarray -t sorted_rpms < <(ls -1v "${matching_rpms[@]}")
          
          latest_rpm="${sorted_rpms[${#sorted_rpms[@]}-1]}" # Default to highest version overall
          
          # If any RPMs are in subdirectories, they are fresh. Pick the highest version among fresh ones.
          fresh_rpms=()
          for f in "${sorted_rpms[@]}"; do
            if echo "$f" | grep -q "repo-cache/repo-${tier}/[^/]\+/."; then
              fresh_rpms+=("$f")
            fi
          done
          
          if [ ${#fresh_rpms[@]} -gt 0 ]; then
            latest_rpm="${fresh_rpms[${#fresh_rpms[@]}-1]}"
          fi

          for f in "${matching_rpms[@]}"; do
            if [ "$f" != "$latest_rpm" ]; then
              echo "    [DEDUPLICATION] Removing older duplicate: $f"
              rm -f "$f"
            fi
          done
        fi
      fi
    fi
  done < <(find repo-cache/repo-${tier}/ -type f -name "*.rpm")
  unset seen_pkgs
done

echo "=== Syncing tiered RPMs to aggregate repo ==="
for tier in tier0 tier1 tier2 tier3; do
  readarray -t tier_rpms < <(find "repo-cache/repo-${tier}/" -name "*.rpm" -type f)
  if [ ${#tier_rpms[@]} -gt 0 ]; then
    cp -a "${tier_rpms[@]}" repo-cache/repo/
  fi
done

for img in "${TIER0_IMAGES[@]}" "${TIER1_IMAGES[@]}" "${TIER2_IMAGES[@]}" "${TIER3_IMAGES[@]}"; do
  if [ -f "$TMP_DIR/digest_$img" ]; then
    NEW_DIGESTS["$img"]=$(cat "$TMP_DIR/digest_$img")
  fi
done

echo "=== Saving New Manifests ==="
for tier in tier0 tier1 tier2 tier3; do
  # We construct the manifest for the tier
  echo "{}" > "repo-cache/repo-${tier}/manifest.json"
  declare -n TIER_ARRAY="TIER${tier#tier}_IMAGES"
  for img in "${TIER_ARRAY[@]}"; do
    digest="${NEW_DIGESTS[$img]:-}"
    if [ -n "$digest" ]; then
      jq --arg k "$img" --arg v "$digest" '.[$k] = $v' "repo-cache/repo-${tier}/manifest.json" > tmp.json && mv tmp.json "repo-cache/repo-${tier}/manifest.json"
    fi
  done
  if [ -f "repo-cache/repo-${tier}/manifest.json" ]; then
    cp "repo-cache/repo-${tier}/manifest.json" "repo-cache/repo/manifest_${tier}.json"
  fi
done

count_rpms() {
  local dir="$1"
  if [ -d "$dir" ]; then
    find "$dir" -name '*.rpm' -type f | wc -l
  else
    echo 0
  fi
}

echo "--- Extracted RPMs Summary ---"
echo "Tier 0 count: $(count_rpms repo-cache/repo-tier0)"
echo "Tier 1 count: $(count_rpms repo-cache/repo-tier1)"
echo "Tier 2 count: $(count_rpms repo-cache/repo-tier2)"
echo "Tier 3 count: $(count_rpms repo-cache/repo-tier3)"
echo "Total repo count: $(count_rpms repo-cache/repo)"
