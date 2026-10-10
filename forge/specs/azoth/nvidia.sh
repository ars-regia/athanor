#!/usr/bin/env bash
# NVIDIA kernel modules for the Athanor kernel (docs/architecture/doc_kernel_build.md,
# section 10, phase K4). Runs in the nvidia/Containerfile image. Two branches:
#   open    the open modules (Turing and later) from the GitHub repository
#           NVIDIA/open-gpu-kernel-modules, at the commit pinned in pins.env;
#   legacy  the proprietary modules of the 580 branch (Maxwell, Pascal, Volta) from the
#           .run of download.nvidia.com, pinned by sha256 in nvidia/sources.sha256.
# Builds against the kernel-devel tree with the kernel toolchain (clang, LLVM): Kbuild
# applies the kernel flags to the modules on its own; the RM part of the open modules,
# which NVIDIA builds outside Kbuild, receives the same flags (kCFI, retpoline, return
# thunk, SLS, IBT) through EXTRA_CFLAGS: objtool must find neither naked rets nor calls
# without retpoline in the C code (that is how a missing flag shows up); the rest is only
# counted. In the legacy branch the RM part is NVIDIA's nv-kernel.o_binary blob, without
# kCFI or return thunks: the calls into it are a known risk, verifiable only on hardware.
#
# Usage: nvidia.sh build --driver open|legacy --devel DIR --out DIR
#        nvidia.sh manifest --out DIR
#   --devel    directory holding kernel-devel-*.rpm (the out/devel of build.sh or the image)
#   --out      build: the .ko files in OUT/<driver>/lib/modules/<kver>/extra/nvidia/, the
#              layout the system image copies and syft catalogs, with `version` and
#              `kver` in OUT/<driver>/, unsigned (sign-kernel.sh modules signs them in the
#              signer image); manifest: downloads the
#              .run of the legacy branch pinned in pins.env, compares it with the hash
#              NVIDIA publishes next to it and writes OUT/sources.sha256 (the bump bot
#              copies it to nvidia/sources.sha256)
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
CACHE=${AZOTH_CACHE:-/var/cache/azoth}
STAGE=${1:-}
[[ $# -gt 0 ]] && shift
DRIVER='' DEVEL='' OUT=''
while [[ $# -gt 0 ]]; do
  case $1 in
    --driver) DRIVER=$2; shift 2 ;;
    --devel) DEVEL=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
usage() {
  echo "usage: nvidia.sh build --driver open|legacy --devel DIR --out DIR" >&2
  echo "       nvidia.sh manifest --out DIR" >&2
  exit 2
}
[[ $OUT ]] || usage
case $STAGE in
  build) [[ $DEVEL && ( $DRIVER == open || $DRIVER == legacy ) ]] || usage ;;
  manifest) ;;
  *) usage ;;
esac

die() { echo "error: $*" >&2; exit 1; }
step() { echo; echo "== $*"; }
fetch() { # fetch URL: into the cache, when missing
  local url=$1 name=${1##*/}
  [[ -s $CACHE/$name ]] || { mkdir -p "$CACHE"; curl -fsSL --retry 3 -o "$CACHE/$name.part" "$url" && mv "$CACHE/$name.part" "$CACHE/$name"; }
}
# shellcheck source=pins.env
. "$HERE/pins.env"

WORK=$(mktemp -d)
mkdir -p "$OUT"

devel_tree() { # the kernel-devel tree extracted from the RPM: Kbuild for external modules
  # does not need the installation, so neither network nor dnf is required.
  mapfile -t DEVEL_RPM < <(find "$DEVEL" -name 'kernel-devel-[0-9]*.rpm')
  [[ ${#DEVEL_RPM[@]} -eq 1 ]] || die "expected exactly one kernel-devel-*.rpm in $DEVEL, found ${#DEVEL_RPM[@]}"
  KVER=$(rpm -qp --qf '%{VERSION}-%{RELEASE}.%{ARCH}' "${DEVEL_RPM[0]}")
  mkdir -p "$WORK/devel" && (cd "$WORK/devel" && rpm2cpio "${DEVEL_RPM[0]}" | cpio -idm --quiet)
  SYSSRC="$WORK/devel/usr/src/kernels/$KVER"
  [[ -f $SYSSRC/Makefile && -f $SYSSRC/.config ]] || die "kernel-devel tree not found for $KVER"
  echo "kernel $KVER"
}

cfg() { grep -q "^$1=y" "$SYSSRC/.config"; } # cfg CONFIG_X: the option is on in the kernel
cfg_val() { sed -n "s/^$1=//p" "$SYSSRC/.config"; } # cfg_val CONFIG_X: the value of the option
cc_option() { # cc_option FLAG...: the flags if clang accepts them, like cc-option of Kbuild
  if clang -Werror "$@" -c -x c /dev/null -o /dev/null 2> /dev/null; then echo " $*"; fi
}

# The clang warnings for the two ways a function reaches an indirect call with a type
# other than its own: a cast, and an assignment C accepts because an enum is compatible
# with its underlying integer while kCFI hashes them apart. kcfi_check.py reads them.
KCFI_WARNINGS="-Wcast-function-type-strict -Wincompatible-function-pointer-types-strict"

build() {
  local src kodir version flags rm_targets=()
  devel_tree
  grep -q '^CONFIG_CC_VERSION_TEXT="clang' "$SYSSRC/.config" || die "the kernel is not built with clang: the modules must use the same toolchain"
  cfg CONFIG_CFI || die "the kernel lacks CONFIG_CFI"
  # The flags Kbuild builds the modules with, read from .config in clang spelling
  # (Makefile and arch/x86/Makefile of the kernel), for the code NVIDIA builds outside
  # Kbuild: its Makefiles only try the gcc spellings, which clang silently drops.
  flags="-fsanitize=kcfi"
  cfg CONFIG_CFI_ICALL_NORMALIZE_INTEGERS && flags+=" -fsanitize-cfi-icall-experimental-normalize-integers"
  cfg CONFIG_FINEIBT_BHI && flags+=" -fsanitize-kcfi-arity"
  cfg CONFIG_MITIGATION_RETPOLINE && flags+=" -mretpoline-external-thunk$(cc_option -mindirect-branch-cs-prefix)"
  cfg CONFIG_MITIGATION_RETHUNK && flags+=" -mfunction-return=thunk-extern"
  cfg CONFIG_MITIGATION_SLS && flags+=" -mharden-sls=all"
  cfg CONFIG_X86_KERNEL_IBT && flags+=" -fcf-protection=branch -fno-jump-tables"
  # CALL_PADDING: the kCFI preamble sits 16 bytes before the entry (movl of the hash and
  # 11 nops) and the callers read the hash at -15. Without the same padding the kernel,
  # applying FineIBT/kCFI at load time, does not find the hashes and the module fails to
  # load.
  cfg CONFIG_CALL_PADDING && flags+=" -fpatchable-function-entry=$(cfg_val CONFIG_FUNCTION_PADDING_BYTES),$(cfg_val CONFIG_FUNCTION_PADDING_BYTES)"
  local align alignflag
  align=$(cfg_val CONFIG_FUNCTION_ALIGNMENT)
  if [[ ${align:-0} -gt 0 ]]; then # like the kernel Makefile: -fmin-function-alignment when available, else -falign-functions
    alignflag=$(cc_option -fmin-function-alignment="$align"); flags+="${alignflag:- -falign-functions=$align}"
  fi

  case $DRIVER in
    open)
      step "open modules $NVIDIA_OPEN_VERSION, commit $NVIDIA_OPEN_COMMIT"
      git -c advice.detachedHead=false clone -q --depth 1 --branch "$NVIDIA_OPEN_VERSION" \
        https://github.com/NVIDIA/open-gpu-kernel-modules "$WORK/src" 2> /dev/null
      [[ $(git -C "$WORK/src" rev-parse HEAD) == "$NVIDIA_OPEN_COMMIT" ]] \
        || die "tag $NVIDIA_OPEN_VERSION does not point to the pinned commit $NVIDIA_OPEN_COMMIT"
      src="$WORK/src"; kodir="$src/kernel-open"; version=$NVIDIA_OPEN_VERSION
      # NVIDIA does not build the RM with kCFI: the patches give the functions it calls
      # indirectly the type of their call sites. A patch that no longer applies after a
      # bump stops the build here, before a module that would trap at boot.
      local patch
      for patch in "$HERE"/nvidia/patches/open/*.patch; do
        git -C "$src" apply "$patch" || die "${patch##*/} does not apply to $NVIDIA_OPEN_VERSION"
        echo "applied ${patch##*/}"
      done
      # The RM part (nv-kernel.o, nv-modeset-kernel.o), built by NVIDIA outside Kbuild:
      # EXTRA_CFLAGS is the hook of utils.mk, and the kernel flags go through it.
      # KCFI_WARNINGS feed kcfi_check.py after the Kbuild part.
      rm_targets=(kernel-open/nvidia/nv-kernel.o_binary kernel-open/nvidia-modeset/nv-modeset-kernel.o_binary)
      step "RM part with clang and the kernel flags ($flags)"
      make -C "$src" -j"$(nproc)" -Otarget CC=clang CXX=clang++ LD=ld.lld AR=llvm-ar EXTRA_CFLAGS="$flags $KCFI_WARNINGS" "${rm_targets[@]}" \
        > "$OUT/$DRIVER-rm.log" 2>&1 || { tail -n 30 "$OUT/$DRIVER-rm.log"; die "RM part failed, log in $OUT/$DRIVER-rm.log"; }
      ;;
    legacy)
      local run="NVIDIA-Linux-x86_64-$NVIDIA_LEGACY_VERSION-no-compat32.run"
      step "proprietary modules $NVIDIA_LEGACY_VERSION from the .run"
      fetch "https://download.nvidia.com/XFree86/Linux-x86_64/$NVIDIA_LEGACY_VERSION/$run"
      (cd "$CACHE" && grep " $run\$" "$HERE/nvidia/sources.sha256" | sha256sum --check --quiet --strict)
      sh "$CACHE/$run" --extract-only --target "$WORK/run" > /dev/null
      src="$WORK/run/kernel"; kodir="$src"; version=$NVIDIA_LEGACY_VERSION
      ;;
  esac

  step "modules against $KVER with Kbuild (clang, LLVM, kCFI from the kernel)"
  # IGNORE_CC_MISMATCH: NVIDIA's conftest wants the same compiler version string, and the
  # Fedora clang gets updated between the kernel build and this one. Being clang is
  # enough: the kCFI hashes depend on the types, not on the version.
  # -Otarget: the output of each object as a block, so the objtool warnings stay attributable.
  make -C "$src" -j"$(nproc)" -Otarget modules SYSSRC="$SYSSRC" CC=clang LD=ld.lld LLVM=1 LLVM_IAS=1 IGNORE_CC_MISMATCH=1 KCFLAGS="$KCFI_WARNINGS" \
    > "$OUT/$DRIVER-build.log" 2>&1 || { tail -n 40 "$OUT/$DRIVER-build.log"; die "build failed, log in $OUT/$DRIVER-build.log"; }
  if [[ $DRIVER == open ]]; then
    step "kCFI types of the open modules"
    python3 "$HERE/nvidia/kcfi_check.py" "$src" "$OUT/$DRIVER-rm.log" "$OUT/$DRIVER-build.log" \
      || die "kCFI type mismatches, logs in $OUT/$DRIVER-rm.log and $OUT/$DRIVER-build.log"
  fi
  local objtool unmitigated
  objtool=$(awk '/warning: objtool:/ { n++ } END { print n + 0 }' "$OUT/$DRIVER-build.log")
  if [[ $DRIVER == open ]]; then
    # In the open branch all the code is built here with the kernel flags, and a missing
    # flag shows up in objtool as a naked ret or an indirect call/jump without retpoline
    # in a C function. The rest is a property of NVIDIA's code, not of the flags, and is
    # only counted: clang extends neither kCFI to virtual calls nor the return thunks to
    # the thunks (_Z names) of the DisplayPort C++ in nvidia-modeset.o, and the RM keeps
    # unreachable function tails ("falls through").
    unmitigated=$(awk '/warning: objtool:/ && /MITIGATION_(RETHUNK|RETPOLINE) build/ && !/objtool: _Z/' "$OUT/$DRIVER-build.log")
    [[ -z $unmitigated ]] || { head -n 20 <<< "$unmitigated"; die "objtool finds C code without return thunk or retpoline in the open branch, log in $OUT/$DRIVER-build.log"; }
    echo "objtool: $objtool warnings, none for missing flags (DisplayPort C++ and unreachable RM tails)"
  else
    echo "objtool: $objtool warnings, from NVIDIA's RM blob and the DisplayPort C++ (expected in the legacy branch)"
  fi

  local ko dest="$OUT/$DRIVER/lib/modules/$KVER/extra/nvidia"
  mkdir -p "$dest"
  for ko in "$kodir"/*.ko; do
    [[ $(modinfo -F vermagic "$ko") == "$KVER "* ]] || die "${ko##*/}: vermagic $(modinfo -F vermagic "$ko") is not of kernel $KVER"
    # The kCFI preambles (__cfi_<function>) prove that the kernel flags got through.
    # Not `nm | grep -q`: with pipefail, grep closes the pipe at the first match and nm
    # dies of SIGPIPE on large modules, so the check would fail at random.
    grep -q ' __cfi_' <(nm "$ko") || die "${ko##*/}: no kCFI preamble"
    install -m 644 "$ko" "$dest/"
  done
  echo "$version" > "$OUT/$DRIVER/version"
  echo "$KVER" > "$OUT/$DRIVER/kver"
  echo "$DRIVER modules $version: $(find "$OUT/$DRIVER" -name '*.ko' -printf '%f ')"
}

manifest() {
  local run="NVIDIA-Linux-x86_64-$NVIDIA_LEGACY_VERSION-no-compat32.run"
  local url="https://download.nvidia.com/XFree86/Linux-x86_64/$NVIDIA_LEGACY_VERSION"
  step "manifest of the .run $NVIDIA_LEGACY_VERSION"
  fetch "$url/$run"
  fetch "$url/$run.sha256sum"
  # The hash NVIDIA publishes next to the file, from the same host: an integrity check of
  # the download, not of authenticity (the .run is not signed).
  (cd "$CACHE" && sha256sum --check --quiet --strict "$run.sha256sum")
  (cd "$CACHE" && sha256sum "$run") > "$OUT/sources.sha256"
  echo "manifest in $OUT/sources.sha256"
}

case $STAGE in build) build ;; manifest) manifest ;; esac
step "done: $(find "$OUT" -type f -name '*.ko' -printf '%P ')"
