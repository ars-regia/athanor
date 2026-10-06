#!/bin/bash

# Deterministic Build Timestamp (Reproducible Builds)
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-1723320000}
set -euo pipefail
# Bedrock Pure Bash Idempotency Checker
# Replaces python3 idempotency_checker.py with native system tools (find, sha256sum, skopeo)

PACKAGE=""
REGISTRY=""
OWNER=""
IMAGE_NAME=""

BASE_DIGEST=""
HASH_ONLY=false

while [[ $# -gt 0 ]]; do
  case $1 in
    --package) PACKAGE="$2"; shift 2 ;;
    --registry) REGISTRY="$2"; shift 2 ;;
    --owner) OWNER="$2"; shift 2 ;;
    --image-name) IMAGE_NAME="$2"; shift 2 ;;
    --base-digest) BASE_DIGEST="$2"; shift 2 ;;
    --hash-only) HASH_ONLY=true; shift ;;
    *) echo "Argomento sconosciuto: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$IMAGE_NAME" ]]; then
  IMAGE_NAME="athanor-forge-${PACKAGE}"
fi

# Package images carry their content hash as hash-<hash> (UD41), the tag the DAG
# orchestrator asks for. The builder keeps the bare hash its consumers pull by.
HASH_TAG_PREFIX="hash-"
[[ "$PACKAGE" == "builder" ]] && HASH_TAG_PREFIX=""

# Determina directory o seed per il calcolo dell'hash
if [[ "$PACKAGE" == "builder" ]]; then
  DIR="builder"
elif [[ -d "specs/athanor-${PACKAGE}" ]]; then
  DIR="specs/athanor-${PACKAGE}"
elif [[ -d "specs/${PACKAGE}" ]]; then
  DIR="specs/${PACKAGE}"
else
  DIR=""
fi

if [[ -n "$DIR" && -d "$DIR" ]]; then
  # Hash SHA-256 deterministico dei path relativi e dei contenuti
  CONTENT_HASH=$({
    find "$DIR" -type f -print0 | sort -z | xargs -0 sha256sum
    # The crates of a package build from their Cargo path dependencies outside the spec
    # directory, so those sources belong to its hash (one implementation, shared with the
    # orchestrator: dag_orchestrator.py --path-dependencies).
    if [[ "$PACKAGE" != "builder" ]]; then
      while IFS= read -r dependency; do
        find "$dependency" -type f -not -path '*/target/*' -print0 | sort -z | xargs -0 sha256sum
      done < <(python3 "$(dirname "${BASH_SOURCE[0]}")/dag_orchestrator.py" --path-dependencies "$DIR")
    fi
    if [[ -f "config/rpmmacros" ]]; then
      echo -n "config/rpmmacros"
      cat "config/rpmmacros"
    fi
    if [[ -f "builder/Containerfile" ]]; then
      echo -n "builder/Containerfile"
      cat "builder/Containerfile"
    fi
    if [[ -f "builder/rpmfusion-custom.repo" ]]; then
      echo -n "builder/rpmfusion-custom.repo"
      cat "builder/rpmfusion-custom.repo"
    fi
    if [[ "$PACKAGE" != "builder" && -f "config/packages.json" ]]; then
      # Not part of the builder's hash: the image is built by flake.nix, which never reads
      # the package lists, so editing them must not rebuild it (UD42).
      echo -n "config/packages.json"
      cat "config/packages.json"
    fi
    if [[ "$PACKAGE" == "builder" ]]; then
      # L'immagine builder è definita dal flake: senza queste righe una modifica a
      # flake.nix o al lock darebbe CACHE_HIT e un builder stantio.
      for f in ../flake.nix ../flake.lock; do
        if [[ -f "$f" ]]; then
          echo -n "$f"
          cat "$f"
        fi
      done
    fi
    echo -n "CACHE_EPOCH=v12"
  } | sha256sum | awk '{print $1}')
else
  # Pacchetti upstream senza spec locale
  if command -v dnf >/dev/null 2>&1; then
    # Cerchiamo la versione effettiva nei repository abilitati
    UPSTREAM_VER=$(dnf repoquery --qf "%{VERSION}-%{RELEASE}\n" --arch x86_64,noarch "$PACKAGE" 2>/dev/null | sort -V | tail -n 1 || true)
  else
    UPSTREAM_VER=""
  fi
  
  # Invalidiamo la cache degli upstream (compilati da zero) ad ogni aggiornamento della Base Image
  # per prevenire desincronizzazione librerie (es. libx265 per ffmpeg).
  #
  # The digest is no longer fetched from the registry: it already lives in the base-atomic
  # FROM line of system/Containerfile, moved there by the kernel bump bot on every bump
  # (forge/specs/azoth/bump.py). Reading it from the checkout avoids both the network call
  # and the ghcr.io/${OWNER}/ermete-base-nvidia:latest tag, an image this repository no
  # longer publishes since the rename.
  if [[ -z "${BASE_DIGEST:-}" ]]; then
    SYSTEM_CONTAINERFILE="$(dirname "${BASH_SOURCE[0]}")/../../system/Containerfile"
    if ! BASE_DIGEST=$(grep -m1 -oP '^FROM \S*base-atomic\S*@\Ksha256:[0-9a-f]{64}' "$SYSTEM_CONTAINERFILE"); then
      echo "check_idempotency.sh: no base-atomic digest found in ${SYSTEM_CONTAINERFILE}" >&2
      exit 1
    fi
  fi
  
  VERSION=${UPSTREAM_VER:-unknown}
  if [[ -n "$UPSTREAM_VER" ]]; then
    CONTENT_HASH=$(echo -n "${PACKAGE}-${UPSTREAM_VER}-${BASE_DIGEST}-v12" | sha256sum | awk '{print $1}')
  else
    CONTENT_HASH=$(echo -n "${PACKAGE}-${VERSION}-upstream-v12-${BASE_DIGEST}" | sha256sum | awk '{print $1}')
  fi
fi

echo ">>> Content Hash calcolato per ${PACKAGE}: ${CONTENT_HASH}" >&2

# --hash-only: the caller asks the registry itself (dag_orchestrator.py, UD41).
if [[ "$HASH_ONLY" == "true" ]]; then
  echo "CONTENT_HASH=${CONTENT_HASH}"
  exit 0
fi

# Costruisce URL immagine GHCR
IMAGE_URL="docker://${REGISTRY}/${OWNER}/${IMAGE_NAME}:${HASH_TAG_PREFIX}${CONTENT_HASH}"
IMAGE_URL_LOWER=$(echo "$IMAGE_URL" | tr '[:upper:]' '[:lower:]')

echo ">>> Verifica esistenza su GHCR: ${IMAGE_URL_LOWER}..." >&2

# One reading of the registry for the whole pipeline: registry_probe.sh answers present or
# absent (including ghcr's 403 for a never-published package) and fails on anything else,
# which stops here rather than reading as a hit or a miss. Anonymous: the forge images are
# public, and credentials a registry rejects fail even a public read.
if ! PROBE=$(bash "$(dirname "${BASH_SOURCE[0]}")/registry_probe.sh" "${IMAGE_URL_LOWER#docker://}"); then
  echo "check_idempotency.sh: il registro non ha risposto per ${IMAGE_URL_LOWER}" >&2
  exit 1
fi
[[ "$PROBE" == "present" ]] && CACHE_HIT="true" || CACHE_HIT="false"

echo "CACHE_HIT=${CACHE_HIT}"
echo "CONTENT_HASH=${CONTENT_HASH}"
if [[ "$CACHE_HIT" == "true" ]]; then
  echo ">>> Cache Hit! L'immagine esiste già su GHCR." >&2
else
  echo ">>> Cache Miss. Procedo con la build." >&2
fi
