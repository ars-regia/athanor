#!/usr/bin/env bash
# =============================================================================
# ATHANOR FORGE - fetch_sources.sh
# Downloads the remote sources a spec declares (Source*/Patch* with a URL) into
# <sourcedir> and verifies their checksums against SOURCES/sources.sha256.
#
# Replaces `spectool -g -R`: it is not in nixpkgs and it verifies nothing.
# Dependencies: rpmspec, curl, sha256sum (all in the builder).
#
# Usage: fetch_sources.sh [--pin] <spec-dir> <sourcedir>
#
# --pin downloads every remote source again and rewrites the manifest from what it
# downloaded: the spec updater runs it after it moves Version, so the new hashes come
# from the same URL and file-name rules the build verifies against.
#
# Conventions:
#   - A URL with a fragment `#/name.tar.gz` (Fedora style): the local file takes
#     that name; otherwise the basename of the URL.
#   - Sources without a URL are files tracked in git, already copied into
#     <sourcedir> by the caller: they are not touched.
#   - Manifest: lines `sha256  filename`, the format of `sha256sum`.
#
# Exits with an error if rpmspec cannot parse the spec, if a URL cannot be
# downloaded, if a file is missing from the manifest or if a checksum does not
# match. No failure is silent.
# =============================================================================
set -euo pipefail

pin=false
if [[ ${1:-} == --pin ]]; then
  pin=true
  shift
fi
SPEC_DIR="${1:?usage: fetch_sources.sh [--pin] <spec-dir> <sourcedir>}"
SOURCE_DIR="${2:?usage: fetch_sources.sh [--pin] <spec-dir> <sourcedir>}"
MANIFEST="$SPEC_DIR/SOURCES/sources.sha256"

spec=$(find "$SPEC_DIR" -maxdepth 1 -name '*.spec' | head -n 1)
if [[ -z "$spec" ]]; then
  echo "fetch_sources: no .spec in $SPEC_DIR" >&2
  exit 1
fi
mkdir -p "$SOURCE_DIR"

# Command substitutions, not `mapfile < <(...)`: a process substitution hides the exit
# status of its command, and a spec that does not parse would read as one without sources.
if ! expanded=$(rpmspec -P "$spec"); then
  echo "fetch_sources: rpmspec cannot parse $(basename "$spec")" >&2
  exit 1
fi
urls=$(sed -nE 's/^(Source|Patch)[0-9]*:[[:space:]]*((https?|ftp):\/\/[^[:space:]]+).*/\2/p' <<<"$expanded")
remote=()
if [[ -n $urls ]]; then
  mapfile -t remote <<<"$urls"
fi

if [[ ${#remote[@]} -eq 0 ]]; then
  echo "fetch_sources: no remote source in $(basename "$spec")"
  exit 0
fi
if [[ $pin == true ]]; then
  mkdir -p "$(dirname "$MANIFEST")"
  pinned=$(mktemp)
  trap 'rm -f "$pinned"' EXIT
elif [[ ! -f "$MANIFEST" ]]; then
  echo "fetch_sources: $(basename "$spec") declares remote sources but the manifest is missing: $MANIFEST" >&2
  exit 1
fi

for entry in "${remote[@]}"; do
  url="${entry%%#*}"
  if [[ "$entry" == *'#/'* ]]; then
    file="${entry##*#/}"
  else
    file=$(basename "$url")
  fi
  # The name comes from a free fragment of the spec: it must stay a single path
  # component inside <sourcedir>, never "..", never a path.
  if [[ -z "$file" || "$file" == */* || "$file" == .* ]]; then
    echo "fetch_sources: invalid file name derived from $entry: '$file'" >&2
    exit 1
  fi

  if [[ $pin == true ]]; then
    echo "fetch_sources: downloading $file from $url"
    curl -fsSL --retry 3 --retry-delay 5 -o "$SOURCE_DIR/$file" "$url"
    (cd "$SOURCE_DIR" && sha256sum -- "$file") >> "$pinned"
    continue
  fi

  expected=$(awk -v f="$file" '$2 == f { print $1 }' "$MANIFEST")
  if [[ -z "$expected" ]]; then
    echo "fetch_sources: $file is not in the manifest $MANIFEST" >&2
    exit 1
  fi

  if [[ ! -f "$SOURCE_DIR/$file" ]]; then
    echo "fetch_sources: downloading $file from $url"
    curl -fsSL --retry 3 --retry-delay 5 -o "$SOURCE_DIR/$file" "$url"
  fi

  if ! echo "$expected  $SOURCE_DIR/$file" | sha256sum -c --quiet -; then
    echo "fetch_sources: checksum mismatch for $file (expected $expected)" >&2
    exit 1
  fi
  echo "fetch_sources: $file verified"
done

if [[ $pin == true ]]; then
  mv "$pinned" "$MANIFEST"
  chmod 644 "$MANIFEST"
  trap - EXIT
  echo "fetch_sources: $MANIFEST rewritten"
fi
