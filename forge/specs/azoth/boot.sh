#!/usr/bin/env bash
# Boot matrix of the Athanor kernel (docs/architecture/doc_kernel_build.md, section 7, gate 3).
# Runs in the boot/Containerfile image. Extracts vmlinuz from the kernel-core RPM, builds a
# test initramfs (busybox, bpftool, boot/init) and a UKI signed with an ephemeral MOK,
# enrols the MOK in the OVMF varstore with Secure Boot on and boots QEMU four times:
# firmware {SeaBIOS, OVMF+Secure Boot via shim} x CPU {Nehalem, host}. Nehalem proves that
# no instruction beyond the x86-64 baseline made it into the kernel. Every boot must end
# with `K3 RESULT ok` on the serial console (the assertions are in boot/init).
# Every boot also checks that the certificates of keys/modules and keys/revoked are
# compiled into the kernel. With --insmod it exercises the external module chain (section
# 7, gate 4) in every case: a .ko signed with the module signing key compiled into the
# kernel must load (ENODEV: good signature, no GPU), and any other must be rejected
# (EKEYREJECTED).
#
# Usage: boot.sh --rpms DIR --out DIR [--accel kvm|tcg] [--case NAME]... [--mok CERT]...
#                [--insmod FILE.ko:ERRNO]...
#   --rpms   directory to search for kernel-core-*.rpm (the out of build.sh or the artifact)
#   --out    serial logs, summary and test material
#   --accel  kvm (default, needs /dev/kvm) or tcg (emulation: slow, `host` becomes `max`)
#   --case   restricts the matrix (repeatable): bios-nehalem bios-host uefi-nehalem uefi-host
#   --mok    certificate (PEM) to enrol in MokList besides the ephemeral one of the UKI,
#            to prove an enrolled MOK does not authorise modules
#   --insmod module to load in the guest and the errno expected from insmod (ENODEV,
#            EKEYREJECTED, or 0), in every case
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
RPMS='' OUT='' ACCEL=kvm CASES=() MOKS=() INSMOD=()
while [[ $# -gt 0 ]]; do
  case $1 in
    --rpms) RPMS=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    --accel) ACCEL=$2; shift 2 ;;
    --case) CASES+=("$2"); shift 2 ;;
    --mok) MOKS+=("$2"); shift 2 ;;
    --insmod) INSMOD+=("$2"); shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ $RPMS && $OUT ]] || { echo "usage: boot.sh --rpms DIR --out DIR [--accel kvm|tcg] [--case NAME]... [--mok CERT]... [--insmod FILE.ko:ERRNO]..." >&2; exit 2; }
[[ ${#CASES[@]} -gt 0 ]] || CASES=(bios-nehalem bios-host uefi-nehalem uefi-host)
[[ $ACCEL == kvm && ! -w /dev/kvm ]] && { echo "/dev/kvm not accessible: use --accel tcg" >&2; exit 2; }

die() { echo "error: $*" >&2; exit 1; }
step() { echo; echo "== $*"; }

mapfile -t CORE < <(find "$RPMS" -name 'kernel-core-*.rpm')
[[ ${#CORE[@]} -eq 1 ]] || die "expected exactly one kernel-core-*.rpm in $RPMS, found ${#CORE[@]}"
KVER=$(rpm -qp --qf '%{VERSION}-%{RELEASE}.%{ARCH}' "${CORE[0]}")
CMDLINE=$(< "$HERE/cmdline")
# The base command line is the image's (generated from athanor-kernel-profile/profile.toml,
# ima_policy=tcb included, so IMA measures something). Test only: serial console, immediate
# reboot on panic (with -no-reboot QEMU exits) and the parameters read by boot/init.
# The certificates the kernel must have compiled in (kernel-local), by subject key
# identifier, the id the kernel logs them with: the module signing one and the revoked.
skid() { openssl x509 -in "$1" -noout -ext subjectKeyIdentifier | tail -n 1 | tr -d ' :' | tr 'A-F' 'a-f'; }
K3_CERTS=''
for cert in "$HERE"/keys/modules/*.pem "$HERE"/keys/revoked/*.pem; do
  K3_CERTS+="${K3_CERTS:+,}$(skid "$cert")"
done
TEST_CMDLINE="$CMDLINE console=ttyS0,115200 panic=-1 k3.uname=$KVER k3.certs=$K3_CERTS"

WORK=$(mktemp -d)
mkdir -p "$OUT"
step "kernel $KVER from ${CORE[0]##*/}"
mkdir -p "$WORK/rpm" && (cd "$WORK/rpm" && rpm2cpio "${CORE[0]}" | cpio -idm --quiet "./lib/modules/$KVER/vmlinuz")
VMLINUZ="$WORK/rpm/lib/modules/$KVER/vmlinuz"
[[ -s $VMLINUZ ]] || die "vmlinuz missing from the kernel-core"

step "test initramfs"
R="$WORK/initramfs"
mkdir -p "$R"/{bin,dev,proc,sys,tmp,usr/sbin}
install -m 755 /usr/sbin/busybox "$R/bin/busybox"
# Relative links: `busybox --install` would make them absolute towards $R, which does not
# exist in the guest.
for applet in $(/usr/sbin/busybox --list); do ln -s busybox "$R/bin/$applet"; done
# bpftool with its libraries: the Fedora one drags libLLVM along (140 MB uncompressed,
# 39 MB compressed), the price of `bpftool feature probe` done with the real tool.
install -m 755 /usr/sbin/bpftool "$R/usr/sbin/bpftool"
# All of them in /lib64, the default path of the loader: the guest has no ld.so.cache and
# libLLVM lives in a directory that on the host is reachable only through ld.so.conf.d.
ldd /usr/sbin/bpftool | awk '/=> \//{print $3} /^\s*\/lib64\/ld-linux/{print $1}' \
  | while read -r lib; do install -D "$lib" "$R/lib64/${lib##*/}"; done
install -m 755 "$HERE/boot/init" "$R/init"
# The modules under test, numbered: two branches share the same nvidia.ko. The k3.insmod
# parameter lists file:errno and goes into the command line of every case.
K3_INSMOD=''
for i in "${!INSMOD[@]}"; do
  ko=${INSMOD[$i]%%:*}; errno=${INSMOD[$i]##*:}
  [[ -f $ko && $errno && $errno != "$ko" ]] || die "--insmod expects FILE.ko:ERRNO, got: ${INSMOD[$i]}"
  install -D -m 644 "$ko" "$R/modules/$i-${ko##*/}"
  K3_INSMOD+="${K3_INSMOD:+,}$i-${ko##*/}:$errno"
done
TEST_CMDLINE+="${K3_INSMOD:+ k3.insmod=$K3_INSMOD}"
(cd "$R" && find . | cpio -o -H newc --quiet | zstd -q -T0 -19 -o "$WORK/initramfs.img")
echo "initramfs: $(du -sh "$R" | cut -f1) uncompressed, $(du -h "$WORK/initramfs.img" | cut -f1) compressed"

step "UKI signed with an ephemeral MOK, enrolled in the OVMF varstore"
# The Secure Boot profile of the project key: the MOK the matrix enrols has its shape.
openssl req -x509 -newkey rsa:2048 -nodes -days 2 -config "$HERE/keys/profiles/secureboot.cnf" \
  -subj '/CN=Athanor OS K3 test MOK/' -keyout "$WORK/mok.key" -out "$OUT/mok.pem" 2> /dev/null
ukify build --linux "$VMLINUZ" --initrd "$WORK/initramfs.img" --uname "$KVER" \
  --cmdline "$TEST_CMDLINE k3.sb=1" --stub /usr/lib/systemd/boot/efi/linuxx64.efi.stub \
  --signtool sbsign --secureboot-private-key "$WORK/mok.key" --secureboot-certificate "$OUT/mok.pem" \
  --output "$WORK/uki.efi" > "$OUT/ukify.log"
sbverify --cert "$OUT/mok.pem" "$WORK/uki.efi" >> "$OUT/ukify.log"
OVMF_CODE=/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd
# MokList: the ephemeral MOK of the UKI and those of --mok. shim copies it to MokListRT;
# none of them is a CA, so the kernel puts them in the platform keyring, which
# patches/redhat/0001 keeps out of module verification: they verify boot artefacts,
# never modules.
ADD_MOK=()
for cert in "$OUT/mok.pem" "${MOKS[@]}"; do ADD_MOK+=(--add-mok "$(< /proc/sys/kernel/random/uuid)" "$cert"); done
virt-fw-vars -i /usr/share/edk2/ovmf/OVMF_VARS.secboot.fd -o "$WORK/vars.fd" "${ADD_MOK[@]}" > "$OUT/varstore.log"
# ESP: shim at the removable path, the UKI where shim looks for the second stage.
mkdir -p "$WORK/esp/EFI/BOOT"
cp /boot/efi/EFI/fedora/shimx64.efi "$WORK/esp/EFI/BOOT/BOOTX64.EFI"
cp "$WORK/uki.efi" "$WORK/esp/EFI/BOOT/grubx64.efi"

run_case() { # run_case NAME  (NAME = <bios|uefi>-<nehalem|host>)
  local name=$1 fw=${1%-*} cpu=${1#*-} log="$OUT/$1.log" args
  [[ $cpu == host && $ACCEL == tcg ]] && cpu=max
  [[ $cpu == nehalem ]] && cpu=Nehalem
  args=(-machine "q35,smm=on" -accel "$ACCEL" -cpu "$cpu" -smp 2 -m 2048
        -display none -monitor none -serial "file:$log" -no-reboot
        -device virtio-rng-pci)
  case $fw in
    bios) args+=(-kernel "$VMLINUZ" -initrd "$WORK/initramfs.img" -append "$TEST_CMDLINE") ;;
    uefi) cp "$WORK/vars.fd" "$WORK/vars-$name.fd"
          args+=(-global "driver=cfi.pflash01,property=secure,value=on"
                 -drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE"
                 -drive "if=pflash,format=raw,file=$WORK/vars-$name.fd"
                 -drive "if=virtio,format=raw,readonly=on,file=fat:ro:$WORK/esp") ;;
    *) die "unknown case: $name" ;;
  esac
  step "$name (firmware $fw, cpu $cpu, accel $ACCEL)"
  timeout 900 qemu-system-x86_64 "${args[@]}" || echo "qemu: exit $?"
  if grep -q '^K3 RESULT ok' "$log"; then
    RESULTS+=("| $name | ok |"); echo "$name: ok"
  else
    RESULTS+=("| $name | FAIL |"); FAILED+=("$name")
    echo "$name: FAIL"; grep -E '^K3 (FAIL|RESULT)' "$log" || tail -n 20 "$log"
  fi
}

RESULTS=() FAILED=()
for c in "${CASES[@]}"; do run_case "$c"; done

{
  echo "## Boot matrix $KVER (accel $ACCEL)"; echo; echo "| case | result |"; echo "| --- | --- |"
  printf '%s\n' "${RESULTS[@]}"
} > "$OUT/summary.md"
step "summary"; cat "$OUT/summary.md"
[[ ${#FAILED[@]} -eq 0 ]] || die "failed cases: ${FAILED[*]}"
