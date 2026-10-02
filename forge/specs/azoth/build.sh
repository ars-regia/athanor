#!/usr/bin/env bash
# Athanor kernel: from the pins to the RPMs, inside builder/Containerfile.
# Specification: docs/architecture/doc_kernel_build.md. Everything it downloads is pinned
# in pins.env, verified against SOURCES/sources.sha256 and against the signatures of the
# keys in SOURCES/keys. Every failing check stops the build. The manifest stage downloads
# the pinned sources and writes their manifest: that is how the bump bot (bump.py)
# regenerates SOURCES/sources.sha256. The refresh stage stops after the patches and writes
# to DIR/refreshed the copies of the CachyOS patches that no longer apply without fuzz, for
# a human to review and commit under patches/refreshed (section 8). The microvm stage runs prep and compiles only the
# MicroVM guest kernel (section 9); build compiles both kernels. --variant NAME
# (variants/NAME) is the same build with a fragment overriding kernel-local, for the A/B
# comparison of the benchmark (kernel-weekly.yml): buildid .azoth.NAME, never published.
set -euo pipefail

usage() { echo "usage: ${0##*/} --stage manifest|prep|refresh|microvm|build --out DIR [--variant NAME]" >&2; exit 2; }
STAGE='' OUT='' VARIANT=''
while [[ $# -gt 0 ]]; do
  case $1 in
    --stage) STAGE=${2:?}; shift 2 ;;
    --variant) VARIANT=${2:?}; shift 2 ;;
    --out) OUT=${2:?}; shift 2 ;;
    *) usage ;;
  esac
done
[[ ( $STAGE == manifest || $STAGE == prep || $STAGE == refresh || $STAGE == microvm || $STAGE == build ) && -n $OUT ]] || usage

die() { echo "build.sh: $*" >&2; exit 1; }
step() { echo; echo ">>> $*"; }

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=pins.env
source "$HERE/pins.env"
CACHE=${AZOTH_CACHE:-/var/cache/azoth}
TOP=$HOME/rpmbuild
SRC=$TOP/SOURCES
mkdir -p "$CACHE" "$OUT" "$TOP"
OUT=$(cd "$OUT" && pwd)     # absolute: the patch step applies files from inside $WORK/b
# A fresh work area on every run: the git repository of the merge does not survive a
# second pass on the same index (the patches show up as already applied).
WORK=$(mktemp -d "$TOP/athanor.XXXXXX")
# The effective config delta: kernel-local, or kernel-local with the lines of the symbols
# redefined by the variant replaced by the variants/NAME fragment.
LOCAL=$HERE/kernel-local
if [[ $VARIANT ]]; then
  [[ -f $HERE/variants/$VARIANT ]] || die "unknown variant: $VARIANT (variants/)"
  LOCAL=$WORK/kernel-local
  {
    grep -vEf <(grep -oE 'CONFIG_\w+' "$HERE/variants/$VARIANT" | sed 's/.*/^(# )?&[= ]/') "$HERE/kernel-local"
    echo; echo "# --- variant $VARIANT (variants/$VARIANT) ---"; cat "$HERE/variants/$VARIANT"
  } > "$LOCAL"
fi

# Names and URLs derived from the pins.
SRPM=kernel-$FEDORA_KERNEL_NVR.src.rpm
KOJI=https://kojipkgs.fedoraproject.org/packages/kernel/${FEDORA_KERNEL_NVR%%-*}/${FEDORA_KERNEL_NVR#*-}
# koji prunes the signed copies of the builds that are no longer the latest but keeps the
# signature header in data/sigcache: spliced onto the SRPM by the koji library it gives
# the signed file back.
SRPM_URL=$KOJI/src/$SRPM
SRPM_SIG_URL=$KOJI/data/sigcache/${FEDORA_KEY_FPR: -8}/src/$SRPM.sig
FEDORA_KEY=/etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-${FEDORA_KERNEL_NVR##*.fc}-primary
KVER=${CACHYOS_RELEASE#cachyos-}; KVER=${KVER%-*}     # cachyos-7.1.8-1 -> 7.1.8
SERIES=$(cut -d. -f1,2 <<< "$KVER")                     # 7.1
CACHY_TAR=$CACHYOS_RELEASE.tar.gz
CACHY_URL=https://github.com/CachyOS/linux/releases/download/$CACHYOS_RELEASE/$CACHY_TAR
VANILLA_TAR=linux-$KVER.tar.xz
VANILLA_SIGN=linux-$KVER.tar.sign
VANILLA_URL=https://cdn.kernel.org/pub/linux/kernel/v${KVER%%.*}.x
CACHY_CONFIG=cachyos-config-${CACHYOS_CONFIG_COMMIT:0:12}
CACHY_CONFIG_URL=https://raw.githubusercontent.com/CachyOS/linux-cachyos/$CACHYOS_CONFIG_COMMIT/linux-cachyos/config
PATCHES_URL=https://raw.githubusercontent.com/CachyOS/kernel-patches/$CACHYOS_PATCHES_COMMIT/$SERIES
mapfile -t PATCHES < <(grep -vE '^\s*(#|$)' "$HERE/patches.list")
mapfile -t ATHANOR_PATCHES < <(find "$HERE/patches" -maxdepth 1 -name '*.patch' | sort)
# patches/redhat/: Athanor patches to code that only the Red Hat patch adds. They go on the
# Fedora merge alone, because the CachyOS tree of the config derivation lacks that code, so
# they must not touch Kconfig.
mapfile -t REDHAT_PATCHES < <(find "$HERE/patches/redhat" -name '*.patch' | sort)
# In the cache the CachyOS patches carry the commit prefix: the same file name comes back
# with different content at every kernel-patches commit, and the cache is persistent.
patch_file() { echo "${CACHYOS_PATCHES_COMMIT:0:12}-${1##*/}"; }
mapfile -t FEDORA_WINS < <(grep -vE '^\s*(#|$)' "$HERE/fedora-wins.list")
[[ -z $(printf '%s\n' "${PATCHES[@]##*/}" | sort | uniq -d) ]] || die "patches.list: duplicate file names"

# shellcheck source=bconds.sh
source "$HERE/bconds.sh"
MAKE_OPTS=(HOSTCC=clang CC=clang LLVM=1 LLVM_IAS=1)      # %{clang_make_opts} of kernel.spec

# --- sources ------------------------------------------------------------------------

fetch() { # fetch FILE URL: into the cache, once
  [[ -f $CACHE/$1 ]] && return
  echo "downloading $1"
  curl -fsSL --retry 3 -o "$CACHE/$1.part" "$2" && mv "$CACHE/$1.part" "$CACHE/$1"
}

step "pinned sources (pins.env)"
fetch "$SRPM" "$SRPM_URL"
fetch "$SRPM.sig" "$SRPM_SIG_URL"
fetch "$CACHY_TAR" "$CACHY_URL"
fetch "$CACHY_TAR.asc" "$CACHY_URL.asc"
fetch "$VANILLA_TAR" "$VANILLA_URL/$VANILLA_TAR"
fetch "$VANILLA_SIGN" "$VANILLA_URL/$VANILLA_SIGN"
fetch "$CACHY_CONFIG" "$CACHY_CONFIG_URL"
for p in "${PATCHES[@]}"; do fetch "$(patch_file "$p")" "$PATCHES_URL/$p"; done

if [[ $STAGE == manifest ]]; then
  # The manifest of these pins, from the files just downloaded. The signatures are
  # verified by prep, which the bot runs right after on the same manifest.
  files=("$SRPM" "$SRPM.sig" "$CACHY_TAR" "$CACHY_TAR.asc" "$VANILLA_TAR" "$VANILLA_SIGN" "$CACHY_CONFIG")
  for p in "${PATCHES[@]}"; do files+=("$(patch_file "$p")"); done
  (cd "$CACHE" && sha256sum "${files[@]}") > "$OUT/sources.sha256"
  echo "manifest of ${#files[@]} files in $OUT/sources.sha256"
  exit 0
fi

step "toolchain (builder/toolchain.lock)"
bash "$HERE/builder/lock.sh" check

step "hashes (SOURCES/sources.sha256)"
(cd "$CACHE" && sha256sum --check --quiet --strict "$HERE/SOURCES/sources.sha256")

verify_gpg() { # verify_gpg KEYDIR SIGNATURE DATA: a good signature from one of the vendored keys
  local home
  home=$(mktemp -d)
  gpg --homedir "$home" --batch --quiet --import "$HERE/SOURCES/keys/$1"/*.asc
  gpg --homedir "$home" --batch --status-fd 1 --verify "$2" "$3" 2>/dev/null \
    | grep '^\[GNUPG:\] GOODSIG ' > /dev/null
}

step "signatures"
verify_gpg cachyos "$CACHE/$CACHY_TAR.asc" "$CACHE/$CACHY_TAR" || die "invalid CachyOS signature: $CACHY_TAR"
xz -dc "$CACHE/$VANILLA_TAR" | verify_gpg kernel.org "$CACHE/$VANILLA_SIGN" - || die "invalid kernel.org signature: $VANILLA_TAR"
SIGNED_SRPM=$WORK/$SRPM
python3 -c 'import sys, koji; koji.splice_rpm_sighdr(open(sys.argv[1], "rb").read(), sys.argv[2], sys.argv[3])' "$CACHE/$SRPM.sig" "$CACHE/$SRPM" "$SIGNED_SRPM"
rpmkeys --import "$FEDORA_KEY"
rpmkeys --checksig --verbose "$SIGNED_SRPM" | grep "signature, key fingerprint: $FEDORA_KEY_FPR: OK" > /dev/null \
  || die "SRPM not signed by the Fedora key $FEDORA_KEY_FPR: $SRPM"

# --- tree ---------------------------------------------------------------------------

step "kernel.spec and Fedora sources in $TOP"
printf '%%_topdir %s\n%%buildid .azoth%s\n' "$TOP" "${VARIANT:+.$VARIANT}" > "$HOME/.rpmmacros"
rpm -i "$SIGNED_SRPM"

step "CachyOS base: three-way merge of vanilla, CachyOS and the Red Hat patch, then patches.list and patches/"
tar -C "$WORK" -xf "$CACHE/$VANILLA_TAR" && mv "$WORK/linux-$KVER" "$WORK/a"
tar -C "$WORK" -xzf "$CACHE/$CACHY_TAR" && mv "$WORK/$CACHYOS_RELEASE" "$WORK/b"
tar -C "$WORK" -xf "$SRC/$VANILLA_TAR" && mv "$WORK/linux-$KVER" "$WORK/fedora-vanilla"
REDHAT_PATCH=("$SRC"/patch-*-redhat.patch)
[[ ${#REDHAT_PATCH[@]} -eq 1 ]] || die "Red Hat patch: expected one, found ${#REDHAT_PATCH[@]}"
# kernel.spec applies the Red Hat patch and then linux-kernel-test.patch (Patch999999)
# with `git --work-tree=. apply`: the test patch is the diff between the Fedora tree and
# the three-way merge (vanilla base) of the CachyOS base onto that tree, so it applies by
# construction and the identical additions (backports present in both) merge on their
# own. Git plumbing only: the index is the work area, no checkout.
export GIT_AUTHOR_NAME=athanor GIT_AUTHOR_EMAIL=kernel@athanor.os GIT_COMMITTER_NAME=athanor GIT_COMMITTER_EMAIL=kernel@athanor.os
g() { git -C "$WORK/a" "$@"; }
g init -q
g add -Af . && VANILLA=$(g commit-tree -m vanilla "$(g write-tree)")
# Fedora regenerates the tarball with git archive: different bytes from the signed
# upstream, content that must be identical. The same git tree proves it.
git --git-dir="$WORK/a/.git" -C "$WORK/fedora-vanilla" add -Af .
[[ $(g write-tree) == $(g rev-parse "$VANILLA^{tree}") ]] || die "the tarball in the SRPM does not match the signed vanilla $VANILLA_TAR"
git --git-dir="$WORK/a/.git" -C "$WORK/b" add -Af . && CACHY=$(g commit-tree -p "$VANILLA" -m cachyos "$(g write-tree)")
g read-tree "$VANILLA" && g apply --cached "${REDHAT_PATCH[0]}" && FEDORA=$(g commit-tree -p "$VANILLA" -m redhat "$(g write-tree)")
# shellcheck disable=SC2053  # the pattern comparison is intended
fedora_wins() { local pat; for pat in "${FEDORA_WINS[@]}"; do [[ $1 == $pat ]] && return; done; false; }
if MERGED=$(g merge-tree --write-tree --name-only --no-messages "$FEDORA" "$CACHY"); then
  g read-tree "$MERGED"
else
  mapfile -t CONFLICTS < <(tail -n +2 <<< "$MERGED" | grep -v "^$")
  for path in "${CONFLICTS[@]}"; do
    fedora_wins "$path" || die "conflict between the CachyOS base and the Red Hat patch outside fedora-wins.list: $path"
  done
  g read-tree "${MERGED%%$'\n'*}"
  g restore --staged --source="$FEDORA" -- "${CONFLICTS[@]}"
  echo "conflicts resolved with the Fedora tree (fedora-wins.list): ${CONFLICTS[*]}"
fi
# patches/refreshed/ (doc_kernel_build.md, sections 1 and 8): a CachyOS patch that no
# longer applies without fuzz is refreshed once with --stage refresh, where GNU patch
# proposes a copy on the real merged tree and a human reviews the fuzzed hunks before
# committing it. The copy records the hash of the upstream file it comes from. prep and
# build apply everything with git apply, without fuzz, and stop when a copy is needed,
# stale (refreshed from another upstream file) or obsolete (the upstream file applies again).
REFRESHED=$HERE/patches/refreshed
sha256_of() { sha256sum "$1" | cut -d' ' -f1; }
recorded_sha256() { sed -n 's/^Upstream-SHA256: //p' "$1"; }
applies() { # applies PATCH: PATCH applies without fuzz to the merged index and to the CachyOS tree
  g apply --cached --check "$1" 2> /dev/null && (cd "$WORK/b" && git apply --check "$1" 2> /dev/null)
}
applies_verbose() { # applies_verbose PATCH: like applies(), but leaves git's own diagnostics (file and line) on stderr
  g apply --cached --check "$1" && (cd "$WORK/b" && git apply --check "$1")
}
refresh_copy() { # refresh_copy PATH UPSTREAM: GNU patch proposes the refreshed copy; sets REFRESH_DEST
  local p=$1 upstream=$2 dest=$OUT/refreshed/$1 tmp=$WORK/refresh idx=$WORK/refresh.index
  local log before after f mode files present
  mapfile -t files < <(g apply --numstat "$upstream" | cut -f3)
  [[ ${#files[@]} -gt 0 ]] || die "$p: the patch lists no files"
  [[ -z $(printf '%s\n' "${files[@]}" | awk '/ => /') ]] || die "$p: a patch with renames cannot be refreshed, refresh it by hand"
  rm -rf "$tmp" && mkdir -p "$tmp"
  mapfile -t present < <(git --literal-pathspecs -C "$WORK/a" ls-files -- "${files[@]}")
  [[ ${#present[@]} -eq 0 ]] || g checkout-index --prefix="$tmp/" -- "${present[@]}"
  if ! log=$(patch -d "$tmp" -p1 --forward --fuzz=2 --no-backup-if-mismatch --reject-file=- < "$upstream" 2>&1); then
    printf '%s\n' "$log" >&2
    die "$p: GNU patch cannot apply it even with fuzz 2, refresh it by hand"
  fi
  # The copy is the diff between the index and the files GNU patch produced, written through
  # a second index so the real one moves only when the copy is applied below.
  before=$(g write-tree)
  cp "$WORK/a/.git/index" "$idx"
  for f in "${files[@]}"; do
    if [[ -f $tmp/$f ]]; then
      mode=100644
      [[ ! -x $tmp/$f ]] || mode=100755
      GIT_INDEX_FILE=$idx g update-index --add --cacheinfo "$mode,$(g hash-object -w "$tmp/$f"),$f"
    else
      GIT_INDEX_FILE=$idx g update-index --force-remove -- "$f"
    fi
  done
  after=$(GIT_INDEX_FILE=$idx g write-tree)
  mkdir -p "$(dirname "$dest")"
  {
    echo "Refreshed-From: CachyOS/kernel-patches $CACHYOS_PATCHES_COMMIT $SERIES/$p"
    echo "Upstream-SHA256: $(sha256_of "$upstream")"
    echo "Refreshed-On: $CACHYOS_RELEASE merged with the Red Hat patch of kernel-$FEDORA_KERNEL_NVR"
    echo
    echo "GNU patch output: review every hunk applied with fuzz before committing this copy."
    # shellcheck disable=SC2001  # per-line indentation of a multi-line string, not a literal substitution
    sed 's/^/  /' <<< "$log"
    echo
    g diff --binary "$before" "$after"
  } > "$dest"
  applies "$dest" || die "$p: the proposed copy does not apply without fuzz to both the merged index and the CachyOS tree: refresh it by hand (doc_kernel_build.md, section 8)"
  echo "$p: refreshed copy written to $dest" >&2
  REFRESH_DEST=$dest
}
# Orphan copies: a file under patches/refreshed with no matching patches.list entry would
# never be checked by the loop below and could drift unnoticed from what the build applies.
if [[ -d $REFRESHED ]]; then
  mapfile -t REFRESHED_FILES < <(find "$REFRESHED" -name '*.patch' | sort)
  for f in "${REFRESHED_FILES[@]}"; do
    rel=${f#"$REFRESHED"/}
    printf '%s\n' "${PATCHES[@]}" | grep -qxF "$rel" || die "patches/refreshed/$rel matches no entry of patches.list: delete it"
  done
fi
[[ $STAGE != refresh ]] || rm -rf "$OUT/refreshed"
for p in "${PATCHES[@]}"; do
  upstream=$CACHE/$(patch_file "$p")
  copy=$REFRESHED/$p
  if [[ $STAGE == refresh ]]; then
    if applies "$upstream"; then
      use=$upstream
      [[ ! -f $copy ]] || echo "$p: the upstream patch applies without fuzz, delete patches/refreshed/$p"
    elif [[ -f $copy && $(recorded_sha256 "$copy") == "$(sha256_of "$upstream")" ]] && applies "$copy"; then
      use=$copy
      echo "$p: patches/refreshed/$p is up to date"
    else
      refresh_copy "$p" "$upstream"
      use=$REFRESH_DEST
    fi
  elif [[ -f $copy ]]; then
    [[ $(recorded_sha256 "$copy") == "$(sha256_of "$upstream")" ]] \
      || die "refresh needed: $p (patches/refreshed/$p was refreshed from another upstream file)"
    ! applies "$upstream" || die "patches/refreshed/$p is obsolete: the upstream patch applies without fuzz again, delete the copy"
    applies_verbose "$copy" || die "refresh needed: $p (patches/refreshed/$p no longer applies without fuzz)"
    use=$copy
  else
    applies_verbose "$upstream" || die "refresh needed: $p (it no longer applies without fuzz)"
    use=$upstream
  fi
  g apply --cached "$use"
  (cd "$WORK/b" && git apply "$use")     # the CachyOS tree serves the config derivation
done
if [[ $STAGE == refresh ]]; then
  step "refresh done: the copies to review are in $OUT/refreshed (none if nothing needed a refresh)"
  exit 0
fi
# The Athanor patches (patches/), in name order, after the CachyOS ones.
for p in "${ATHANOR_PATCHES[@]}"; do
  g apply --cached "$p"
  (cd "$WORK/b" && git apply "$p")
done
for p in "${REDHAT_PATCHES[@]}"; do
  [[ -z $(git apply --numstat "$p" | awk '$3 ~ /(^|\/)Kconfig/') ]] || die "${p##*/}: a patch in patches/redhat must not touch Kconfig"
  g apply --cached "$p"
done
# The certificates kernel-local compiles in (CONFIG_SYSTEM_TRUSTED_KEYS and
# CONFIG_SYSTEM_REVOCATION_KEYS, paths relative to the tree): added to the index, so
# linux-kernel-test.patch carries them into the tree rpmbuild prepares.
mapfile -t MODULE_CERTS < <(find "$HERE/keys/modules" -name '*.pem' | sort)
mapfile -t REVOKED_CERTS < <(find "$HERE/keys/revoked" -name '*.pem' | sort)
[[ ${#MODULE_CERTS[@]} -eq 1 ]] || die "keys/modules: expected one certificate, found ${#MODULE_CERTS[@]}"
[[ ${#REVOKED_CERTS[@]} -ge 1 ]] || die "keys/revoked: no certificate, CONFIG_SYSTEM_REVOCATION_KEYS needs one"
# awk 1 rather than cat ends every file with a newline: a PEM without one would glue its
# END line to the next BEGIN, and extract-cert stops at the first unparsable block without
# an error, dropping the remaining certificates.
add_to_tree() { # add_to_tree PATH FILE...: the concatenation of FILE... at PATH in the index
  g update-index --add --cacheinfo "100644,$(awk 1 "${@:2}" | g hash-object -w --stdin),$1"
}
add_to_tree certs/athanor-modules.pem "${MODULE_CERTS[@]}"
add_to_tree certs/athanor-revoked.pem "${REVOKED_CERTS[@]}"
g diff --binary "$FEDORA" "$(g write-tree)" -- . ':!.github' > "$SRC/linux-kernel-test.patch"

# Before the config derivation: listnewconfig must see the same toolchain as rpmbuild
# (rust-src, bindgen, pahole), otherwise RUST_IS_AVAILABLE and the options depending on
# it change between the pre-pass and the Fedora gate. After the patches: a patch that does
# not apply stops prep in seconds, and the refresh stage, which ends there, needs no toolchain.
# The builder installed them from builder/toolchain.lock: with every repository disabled
# this is a check, and a spec whose BuildRequires the lock does not cover stops here.
step "BuildRequires of kernel.spec (builder/toolchain.lock)"
dnf -y builddep --disablerepo='*' "${DEFINES[@]}" "$TOP/SPECS/kernel.spec" \
  || die "BuildRequires outside builder/toolchain.lock: run builder/lock.sh generate"

# --- config -------------------------------------------------------------------------

step "kernel-local: the Athanor delta, then the options new to the tree with the CachyOS values"
# The same merge kernel.spec performs: Fedora x86_64 config, clang and clang_lto snippets,
# kernel-local. On that config listnewconfig lists the options the tree introduces.
merged=$WORK/merged.config
cp "$SRC/kernel-x86_64-fedora.config" "$merged"
for snip in "$SRC/partial-clang-snip.config" "$SRC/partial-clang_lto-x86_64-snip.config" "$LOCAL"; do
  python3 "$SRC/merge.py" "$snip" "$merged" > "$merged.tmp" && mv "$merged.tmp" "$merged"
done
derived=$WORK/derived.config
: > "$derived"
for _ in 1 2 3 4 5; do
  make -s -C "$WORK/b" ARCH=x86_64 "${MAKE_OPTS[@]}" KCONFIG_CONFIG="$merged" listnewconfig > "$WORK/listnew"
  grep '^CONFIG_' "$WORK/listnew" > "$WORK/new" || break
  while IFS= read -r line; do
    name=${line%%=*}
    grep -E "^($name=|# $name is not set)" "$CACHE/$CACHY_CONFIG" || echo "$line"
  done < "$WORK/new" | sed -E 's/^(CONFIG_\w+)=n$/# \1 is not set/' >> "$derived"
  python3 "$SRC/merge.py" "$derived" "$merged" > "$merged.tmp" && mv "$merged.tmp" "$merged"
done
[[ ! -s $WORK/new ]] || die "config derivation does not converge: $(head -3 "$WORK/new" | tr '\n' ' ')"
echo "derived options: $(wc -l < "$derived")"; cat "$derived"
{
  cat "$LOCAL"
  echo
  echo "# Options introduced by the tree, with the values of $CACHY_CONFIG (derived by build.sh)."
  cat "$derived"
} > "$SRC/kernel-local"

step "x86_64 only: the other Fedora configs become '# EMPTY' and process_configs.sh skips them"
for f in "$SRC"/kernel-*-fedora.config; do
  [[ $f == */kernel-x86_64-fedora.config ]] || printf '# EMPTY\n' > "$f"
done

check_delta() { # check_delta CONFIG FRAGMENT: every line of the fragment must hold in the config
  local bad=0 line name
  while IFS= read -r line; do
    name=$(grep -oE 'CONFIG_\w+' <<< "$line")
    if [[ $line == CONFIG_* ]]; then
      grep -qxF "$line" "$1" || { echo "  required $line, generated: $(grep -E "^(# )?${name}[= ]" "$1" || echo missing)"; bad=1; }
    else
      ! grep -qE "^$name=" "$1" || { echo "  required $line, generated: $(grep -E "^$name=" "$1")"; bad=1; }
    fi
  done < <(grep -E '^(CONFIG_\w+=|# CONFIG_\w+ is not set)' "$2")
  [[ $bad -eq 0 ]] || die "the generated config does not honour ${2#"$HERE"/}"
}

# --- rpmbuild -----------------------------------------------------------------------

# Reproducibility (spec, section 3 step 8): fixed build user, host and date. The date is
# the one of the kernel.spec changelog, the same SOURCE_DATE_EPOCH rpm uses for the
# package timestamps; the kernel writes it into UTS_VERSION (linux_banner in .rodata,
# init_uts_ns in .data, `uname -v`) and without this variable it would use the current
# time: two builds of the same pin differed exactly there (repro.py, K7).
EPOCH=$(rpmspec -q --srpm --qf '[%{changelogtime} ]' "$TOP/SPECS/kernel.spec" | cut -d' ' -f1)
[[ $EPOCH =~ ^[0-9]+$ ]] || die "kernel.spec changelog without a date"
KBUILD_BUILD_TIMESTAMP=$(date -u -d "@$EPOCH" '+%a %b %e %H:%M:%S UTC %Y')
export KBUILD_BUILD_USER=azoth KBUILD_BUILD_HOST=forge KBUILD_BUILD_TIMESTAMP

step "rpmbuild -bp: patches and config gate (process_configs.sh -w -n -c)"
rpmbuild -bp --target x86_64 "${BCONDS[@]}" "$TOP/SPECS/kernel.spec"
CONFIG=$(find "$TOP/BUILD" -path '*/configs/kernel-*-x86_64.config' -print -quit)
[[ -n $CONFIG ]] || die "generated config not found under $TOP/BUILD"
check_delta "$CONFIG" "$LOCAL"
# kernel-local pins the random kmalloc partition mode only because the compiler has no
# allocation tokens. The generated config says whether it has them (CC_HAS_ALLOC_TOKEN, the
# same cc-option Kconfig uses): once it does, the pin is obsolete and the build stops.
if grep -qx 'CONFIG_CC_HAS_ALLOC_TOKEN=y' "$CONFIG" && grep -qx 'CONFIG_KMALLOC_PARTITION_RANDOM=y' "$LOCAL"; then
  die "kernel-local: the compiler now supports allocation tokens (CC_HAS_ALLOC_TOKEN=y): remove the three KMALLOC_PARTITION lines from kernel-local, Fedora already selects KMALLOC_PARTITION_TYPED (section 13, decision 6)"
fi
cp "$CONFIG" "$SRC/kernel-local" "$OUT/"

# --- MicroVM guest kernel (section 9) -----------------------------------------------------

# Same source (the tree prepared by rpmbuild -bp) and same pin, second config:
# x86_64_defconfig + kvm_guest.config + microvm/kernel-local, in a separate object
# directory (O=), so the tree stays clean for rpmbuild -bb. The fragment gate
# (check_delta) runs in every stage, the compilation only in microvm and build.
TREE=$(dirname "$(dirname "$CONFIG")")
[[ -f $TREE/Makefile ]] || die "kernel tree not found next to $CONFIG"
MICROVM_OBJ=$WORK/microvm
step "MicroVM config: x86_64_defconfig + kvm_guest.config + microvm/kernel-local"
# rpmbuild -bp leaves include/config and include/generated in the tree
# (process_configs.sh) and O= demands a clean tree; mrproper is also the first step of
# InitBuildVars in kernel.spec (from which BuildKernel restarts with configs/), so
# nothing changes for the main kernel.
make -s -C "$TREE" mrproper
mkdir -p "$MICROVM_OBJ"
make -s -C "$TREE" O="$MICROVM_OBJ" "${MAKE_OPTS[@]}" x86_64_defconfig kvm_guest.config
"$TREE/scripts/kconfig/merge_config.sh" -m -O "$MICROVM_OBJ" "$MICROVM_OBJ/.config" "$HERE/microvm/kernel-local"
make -s -C "$TREE" O="$MICROVM_OBJ" "${MAKE_OPTS[@]}" olddefconfig
check_delta "$MICROVM_OBJ/.config" "$HERE/microvm/kernel-local"
cp "$MICROVM_OBJ/.config" "$OUT/microvm.config"
echo "MicroVM config: $(grep -c '=y$' "$MICROVM_OBJ/.config") built-in options, $(grep -c '=m$' "$MICROVM_OBJ/.config") modules"

# A variant is only the main kernel to be measured: no guest kernel.
if [[ ( $STAGE == microvm || $STAGE == build ) && -z $VARIANT ]]; then
  NVR=$(bash "$HERE/nvr.sh")
  step "MicroVM kernel: rpmbuild -bb microvm/azoth-microvm.spec (O=$MICROVM_OBJ)"
  # The same SOURCE_DATE_EPOCH as the main kernel, which rpm takes from the kernel.spec
  # changelog: the guest spec has none, and without an epoch the package timestamps would
  # not be reproducible (KBUILD_BUILD_TIMESTAMP is already in the environment).
  SOURCE_DATE_EPOCH=$EPOCH rpmbuild -bb --target x86_64 --define "source_date_epoch_from_changelog 0" --define "use_source_date_epoch_as_buildtime 1" \
    --define "kernel_tree $TREE" --define "objdir $MICROVM_OBJ" \
    --define "kversion ${NVR%%-*}" --define "krelease ${NVR#*-}" \
    --define "make_opts ${MAKE_OPTS[*]}" "$HERE/microvm/azoth-microvm.spec"
  mkdir -p "$OUT/microvm"
  mv "$TOP"/RPMS/x86_64/azoth-microvm-*.rpm "$OUT/microvm/"
fi

if [[ $STAGE == build ]]; then
  step "rpmbuild -bb"
  rpmbuild -bb --noprep --target x86_64 "${BCONDS[@]}" "$TOP/SPECS/kernel.spec"
  # Three distinct OCI images (kernel-build.yml): binaries, devel for the external kmods,
  # debuginfo with its own retention. The classification is by package name.
  mkdir -p "$OUT/kernel" "$OUT/devel" "$OUT/debuginfo"
  for rpm in "$TOP"/RPMS/x86_64/*.rpm; do
    case ${rpm##*/} in
      *debuginfo*) cp "$rpm" "$OUT/debuginfo/" ;;
      *devel*) cp "$rpm" "$OUT/devel/" ;;
      *) cp "$rpm" "$OUT/kernel/" ;;
    esac
  done
  rpm -qp --qf '%{VERSION}-%{RELEASE}' "$TOP"/RPMS/x86_64/kernel-core-*.rpm > "$OUT/nvr"
fi

step "done: $(find "$OUT" -type f -printf "%P ")"
