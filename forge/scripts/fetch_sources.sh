#!/usr/bin/env bash
# =============================================================================
# ATHANOR FORGE - fetch_sources.sh
# Scarica le sorgenti remote dichiarate in una spec (Source*/Patch* con URL)
# in <sourcedir> e ne verifica il checksum contro SOURCES/sources.sha256.
#
# Sostituisce `spectool -g -R`: non esiste in nixpkgs e non verifica nulla.
# Dipendenze: rpmspec, curl, sha256sum (tutte nel builder).
#
# Uso: fetch_sources.sh [--pin] <spec-dir> <sourcedir>
#
# --pin downloads every remote source again and rewrites the manifest from what it
# downloaded: the spec updater runs it after it moves Version, so the new hashes come
# from the same URL and file-name rules the build verifies against.
#
# Convenzioni:
#   - URL con frammento `#/nome.tar.gz` (stile Fedora): il file locale prende
#     quel nome; altrimenti il basename dell'URL.
#   - Le sorgenti senza URL sono file tracciati in git, già copiati in
#     <sourcedir> dal chiamante: non vengono toccate.
#   - Manifest: righe `sha256  nomefile`, il formato di `sha256sum`.
#
# Esce con errore se un URL non è scaricabile, se un file manca nel manifest
# o se il checksum non corrisponde. Nessun fallimento è silenzioso.
# =============================================================================
set -euo pipefail

pin=false
if [[ ${1:-} == --pin ]]; then
  pin=true
  shift
fi
SPEC_DIR="${1:?uso: fetch_sources.sh [--pin] <spec-dir> <sourcedir>}"
SOURCE_DIR="${2:?uso: fetch_sources.sh [--pin] <spec-dir> <sourcedir>}"
MANIFEST="$SPEC_DIR/SOURCES/sources.sha256"

spec=$(find "$SPEC_DIR" -maxdepth 1 -name '*.spec' | head -n 1)
if [[ -z "$spec" ]]; then
  echo "fetch_sources: nessuna .spec in $SPEC_DIR" >&2
  exit 1
fi
mkdir -p "$SOURCE_DIR"

mapfile -t remote < <(rpmspec -P "$spec" \
  | sed -nE 's/^(Source|Patch)[0-9]*:[[:space:]]*((https?|ftp):\/\/[^[:space:]]+).*/\2/p')

if [[ ${#remote[@]} -eq 0 ]]; then
  echo "fetch_sources: nessuna sorgente remota in $(basename "$spec")"
  exit 0
fi
if [[ $pin == true ]]; then
  mkdir -p "$(dirname "$MANIFEST")"
  pinned=$(mktemp)
  trap 'rm -f "$pinned"' EXIT
elif [[ ! -f "$MANIFEST" ]]; then
  echo "fetch_sources: $(basename "$spec") dichiara sorgenti remote ma manca $MANIFEST" >&2
  exit 1
fi

for entry in "${remote[@]}"; do
  url="${entry%%#*}"
  if [[ "$entry" == *'#/'* ]]; then
    file="${entry##*#/}"
  else
    file=$(basename "$url")
  fi
  # Il nome viene da un frammento libero della spec: deve restare un singolo
  # componente di percorso dentro <sourcedir>, mai "..", mai un percorso.
  if [[ -z "$file" || "$file" == */* || "$file" == .* ]]; then
    echo "fetch_sources: nome file non valido derivato da $entry: '$file'" >&2
    exit 1
  fi

  if [[ $pin == true ]]; then
    echo "fetch_sources: scarico $file da $url"
    curl -fsSL --retry 3 --retry-delay 5 -o "$SOURCE_DIR/$file" "$url"
    (cd "$SOURCE_DIR" && sha256sum -- "$file") >> "$pinned"
    continue
  fi

  expected=$(awk -v f="$file" '$2 == f { print $1 }' "$MANIFEST")
  if [[ -z "$expected" ]]; then
    echo "fetch_sources: $file non è nel manifest $MANIFEST" >&2
    exit 1
  fi

  if [[ ! -f "$SOURCE_DIR/$file" ]]; then
    echo "fetch_sources: scarico $file da $url"
    curl -fsSL --retry 3 --retry-delay 5 -o "$SOURCE_DIR/$file" "$url"
  fi

  if ! echo "$expected  $SOURCE_DIR/$file" | sha256sum -c --quiet -; then
    echo "fetch_sources: checksum errato per $file (atteso $expected)" >&2
    exit 1
  fi
  echo "fetch_sources: $file verificato"
done

if [[ $pin == true ]]; then
  mv "$pinned" "$MANIFEST"
  chmod 644 "$MANIFEST"
  trap - EXIT
  echo "fetch_sources: $MANIFEST riscritto"
fi
