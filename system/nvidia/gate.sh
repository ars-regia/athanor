#!/usr/bin/env bash
# Build gate of the system image (docs/architecture/doc_system_image.md, S3, S6, S8): run on the
# finished image root, as the last step of system/Containerfile.
#   none           no NVIDIA module, kernel argument, modprobe option, dracut configuration,
#                  negativo17 repository or RPM Fusion NVIDIA driver repository
#   nvidia         modules, driver packages, shim and GSP firmware at NVIDIA_OPEN_VERSION,
#                  the EGL, GBM and Vulkan files of the userspace, and nouveau blacklisted
#                  in modprobe.d and on the kernel command line
#   nvidia-legacy  the same at NVIDIA_LEGACY_VERSION, with the RPM Fusion packages, no GSP check
# Usage: gate.sh none|nvidia|nvidia-legacy PINS_ENV [ROOT]
set -euo pipefail

GPU=${1:?usage: gate.sh none|nvidia|nvidia-legacy PINS_ENV [ROOT]}
PINS=${2:?usage: gate.sh none|nvidia|nvidia-legacy PINS_ENV [ROOT]}
ROOT=${3:-/}
bad=0
violation() { echo "nvidia gate: $*" >&2; bad=1; }

# Fail closed: a missing or misnamed modules tree must not pass as "no NVIDIA module".
[[ -d $ROOT/usr/lib/modules ]] || violation "/usr/lib/modules: not a directory under $ROOT"
# The modules of the NVIDIA driver itself wherever they lie (extra/, updates/, kernel/), by
# exact base name: in-tree modules such as nvidiafb or nvidia-wmi-ec-backlight are not the driver.
mapfile -t modules < <(find "$ROOT/usr/lib/modules" -regextype posix-extended \
  -regex '.*/nvidia([-_](drm|modeset|uvm|peermem))?\.ko(\.(xz|zst|gz))?' 2> /dev/null | sort)

if [[ $GPU == none ]]; then
  for ko in "${modules[@]}"; do violation "${ko#"$ROOT"}: NVIDIA module in the default image"; done
  while IFS= read -r f; do
    violation "${f#"$ROOT"}: NVIDIA configuration in the default image"
  # Word-bounded: underscores and hyphens are boundaries; Fedora's dist-blacklist.conf names the unrelated framebuffer driver nvidiafb.
  done < <(grep -rlE '(^|[^[:alnum:]])nvidia([^[:alnum:]]|$)' "$ROOT/usr/lib/bootc/kargs.d" "$ROOT/usr/lib/modprobe.d" "$ROOT/etc/modprobe.d" "$ROOT/usr/lib/dracut/dracut.conf.d" 2> /dev/null | sort)
  while IFS= read -r f; do
    violation "${f#"$ROOT"}: NVIDIA driver repository in the default image"
  done < <({
    grep -rlE 'negativo17|^\[rpmfusion[^]]*nvidia' "$ROOT/etc/yum.repos.d"
    find "$ROOT/etc/yum.repos.d" -name '*rpmfusion*nvidia*'
  } 2> /dev/null | sort -u)
  exit "$bad"
fi

case $GPU in
  nvidia) expected=$(sed -n 's/^NVIDIA_OPEN_VERSION=//p' "$PINS"); packages=(nvidia-driver nvidia-driver-libs nvidia-kmod-common azoth-nvidia-kmod) ;;
  nvidia-legacy) expected=$(sed -n 's/^NVIDIA_LEGACY_VERSION=//p' "$PINS"); packages=(xorg-x11-drv-nvidia xorg-x11-drv-nvidia-libs azoth-nvidia-kmod) ;;
  *) echo "gate.sh: unknown GPU $GPU" >&2; exit 2 ;;
esac
[[ -n $expected ]] || { echo "gate.sh: no NVIDIA version for $GPU in $PINS" >&2; exit 2; }

[[ ${#modules[@]} -gt 0 ]] || violation "no NVIDIA module under /usr/lib/modules"
for ko in "${modules[@]}"; do
  if got=$(modinfo -F version "$ko" 2>&1); then
    [[ $got == "$expected" ]] || violation "${ko##*/}: module $got, pin $expected"
  else
    violation "${ko##*/}: modinfo failed: $got"
  fi
done
for pkg in "${packages[@]}"; do
  if got=$(rpm --root "$ROOT" -q --qf '%{VERSION}' "$pkg" 2>&1); then
    [[ $got == "$expected" ]] || violation "$pkg: package $got, pin $expected"
  else
    violation "$pkg: not installed or unreadable: $got"
  fi
done
if [[ $GPU == nvidia ]]; then
  for fw in gsp_ga10x.bin gsp_tu10x.bin; do
    [[ -f $ROOT/usr/lib/firmware/nvidia/$expected/$fw ]] || violation "/usr/lib/firmware/nvidia/$expected/$fw missing"
  done
fi
for f in usr/share/glvnd/egl_vendor.d/10_nvidia.json usr/lib64/gbm/nvidia-drm_gbm.so; do
  [[ -e $ROOT/$f ]] || violation "/$f missing"
done
compgen -G "$ROOT/usr/share/vulkan/icd.d/nvidia_icd*.json" > /dev/null || violation "/usr/share/vulkan/icd.d/nvidia_icd*.json missing"
# nouveau must never claim the GPU: the modprobe.d blacklist covers the root filesystem, the
# kernel argument also covers loads requested before any modprobe.d file is read.
grep -qsE '^[[:space:]]*blacklist[[:space:]]+nouveau[[:space:]]*$' "$ROOT"/usr/lib/modprobe.d/*.conf "$ROOT"/etc/modprobe.d/*.conf ||
  violation "no modprobe.d file blacklists nouveau"
grep -qsE '(^|[^[:alnum:]._-])modprobe\.blacklist=([^[:space:]",]*,)*nouveau([,"[:space:]]|$)' "$ROOT"/usr/lib/bootc/kargs.d/*.toml ||
  violation "no kargs.d file sets modprobe.blacklist=nouveau"
exit "$bad"
