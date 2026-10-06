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

# Verifica con skopeo (se skopeo non è installato, tenta di installarlo o usa fallback)
if ! command -v skopeo >/dev/null 2>&1; then
  if command -v dnf >/dev/null 2>&1; then
    sudo -n dnf install -y skopeo >&2 || dnf install -y skopeo >&2 || :
  fi
fi

if ! command -v skopeo >/dev/null 2>&1; then
  echo "check_idempotency.sh: skopeo non trovato, impossibile interrogare il registro" >&2
  exit 1
fi

# skopeo legge la configurazione dei registri sotto $HOME prima ancora di aprire la rete.
# Il job lo esegue con --userns=keep-id, quindi non è root, mentre HOME resta /root: il
# risultato è "open /root/.config/containers/registries.d: permission denied" in 34
# millisecondi, senza che nessuna richiesta parta. Una home scrivibile evita l'errore.
if [[ ! -w "${HOME:-/root}" ]]; then
  HOME=$(mktemp -d)
  export HOME
fi

# Prima senza credenziali, poi con. Le immagini della forge sono pubbliche e si leggono
# anonimamente; passare --creds a un registro che poi rifiuta quelle credenziali fa
# fallire skopeo con 403 anche su un'immagine leggibile da chiunque. Quel fallimento era
# indistinguibile da "immagine assente" e ricostruiva l'intero DAG a ogni run: tutti e 47
# i nodi, 4,6 ore di runner, per un push che non toccava nessuno di quei pacchetti.
#
# L'esito viene distinto in tre casi, perché "non c'è" e "non sono riuscito a chiedere"
# richiedono risposte diverse: la prima è una build da fare, la seconda è un guasto.
inspect_status=""
for attempt in anonymous authenticated; do
  INSPECT_ARGS=("--no-tags")
  if [[ "$attempt" == "authenticated" ]]; then
    [[ -n "${GITHUB_TOKEN:-}" ]] || continue
    INSPECT_ARGS+=("--creds" "${OWNER}:${GITHUB_TOKEN}")
  fi

  set +e
  inspect_err=$(skopeo inspect "${INSPECT_ARGS[@]}" "${IMAGE_URL_LOWER}" 2>&1 >/dev/null)
  rc=$?
  set -e

  if [[ $rc -eq 0 ]]; then
    inspect_status="found"
    break
  fi
  # Il registro risponde "manifest unknown" quando il tag non esiste: è una risposta, non
  # un guasto, e non serve riprovare autenticati.
  if grep -qi 'manifest unknown\|name unknown\|not found' <<< "$inspect_err"; then
    inspect_status="absent"
    break
  fi
  inspect_status="error"
  last_error=$inspect_err
done

case "$inspect_status" in
  found) CACHE_HIT="true" ;;
  absent) CACHE_HIT="false" ;;
  *)
    # Né presente né assente: il registro non ha risposto. Costruire sarebbe uno spreco
    # silenzioso, dichiarare la cache valida sarebbe peggio: si ferma e lo dice.
    echo "check_idempotency.sh: il registro non ha risposto per ${IMAGE_URL_LOWER}" >&2
    echo "${last_error:-nessun dettaglio}" >&2
    exit 1
    ;;
esac

echo "CACHE_HIT=${CACHE_HIT}"
echo "CONTENT_HASH=${CONTENT_HASH}"
if [[ "$CACHE_HIT" == "true" ]]; then
  echo ">>> Cache Hit! L'immagine esiste già su GHCR." >&2
else
  echo ">>> Cache Miss. Procedo con la build." >&2
fi
