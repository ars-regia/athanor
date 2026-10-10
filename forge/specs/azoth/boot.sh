#!/usr/bin/env bash
# Boot matrix of the Athanor kernel (docs/architecture/doc_kernel_build.md, section 7, gate 3).
# Runs in the boot/Containerfile image. Extracts vmlinuz from the kernel-core RPM, builds a
# test initramfs (busybox, bpftool, boot/init) and a UKI signed with an ephemeral MOK,
# enrols the MOK in the OVMF varstore with Secure Boot on and boots QEMU six times:
# firmware {SeaBIOS, OVMF+Secure Boot via shim} x CPU {Penryn, host}, plus an Intel and an
# AMD IOMMU case. Penryn (x86-64-v1, no POPCNT or SSE4.2; D14) proves that no instruction
# beyond the baseline made it into the kernel. Every boot must end
# with `K3 RESULT ok` on the serial console (the assertions are in boot/init).
# Every boot also checks that the certificates of keys/modules and keys/revoked are
# compiled into the kernel. With --insmod it exercises the external module chain (section
# 7, gate 4) in every case: a .ko signed with the module signing key compiled into the
# kernel must load (ENODEV: good signature, no GPU), and any other must be rejected
# (EKEYREJECTED).
#
# Usage: boot.sh --rpms DIR --out DIR [--accel kvm|tcg] [--case NAME]... [--mok CERT]...
#                [--mok-ca CERT]... [--ima-key CERT] [--insmod FILE.ko:ERRNO[:BIOS_ERRNO]]...
#   --rpms   directory to search for kernel-core-*.rpm (the out of build.sh or the artifact)
#   --out    serial logs, summary and test material
#   --accel  kvm (default, needs /dev/kvm) or tcg (emulation: slow, `host` becomes `max`)
#   --case   restricts the matrix (repeatable): bios-penryn bios-host uefi-penryn uefi-host iommu-intel iommu-amd
#   --mok    certificate (PEM) to enrol in MokList besides the ephemeral one of the UKI,
#            to prove an enrolled MOK does not authorise modules
#   --mok-ca user CA (PEM) to enrol in MokList and trust for the machine keyring, as
#            `mokutil --trust-mok` does (D40)
#   --ima-key certificate (PEM) the guest offers to the .ima keyring, which must refuse it
#            (D40, D46)
#   --insmod module to load in the guest and the errno expected from insmod (ENODEV,
#            EKEYREJECTED, or 0); BIOS_ERRNO, when given, applies to the bios-* and iommu-*
#            cases, the first errno to the uefi-* ones
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
RPMS='' OUT='' ACCEL=kvm CASES=() MOKS=() MOK_CAS=() IMA_KEY='' INSMOD=()
while [[ $# -gt 0 ]]; do
  case $1 in
    --rpms) RPMS=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    --accel) ACCEL=$2; shift 2 ;;
    --case) CASES+=("$2"); shift 2 ;;
    --mok) MOKS+=("$2"); shift 2 ;;
    --mok-ca) MOK_CAS+=("$2"); shift 2 ;;
    --ima-key) IMA_KEY=$2; shift 2 ;;
    --insmod) INSMOD+=("$2"); shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ $RPMS && $OUT ]] || { echo "usage: boot.sh --rpms DIR --out DIR [--accel kvm|tcg] [--case NAME]... [--mok CERT]... [--mok-ca CERT]... [--ima-key CERT] [--insmod FILE.ko:ERRNO[:BIOS_ERRNO]]..." >&2; exit 2; }
[[ ${#CASES[@]} -gt 0 ]] || CASES=(bios-penryn bios-host uefi-penryn uefi-host iommu-intel iommu-amd)
[[ $ACCEL == kvm && ! -w /dev/kvm ]] && { echo "/dev/kvm not accessible: use --accel tcg" >&2; exit 2; }

die() { echo "error: $*" >&2; exit 1; }
step() { echo; echo "== $*"; }
insmod_spec() { # insmod_spec FILE.ko:ERRNO[:BIOS_ERRNO]: print "file uefi-errno bios-errno"
  local ko=${1%%:*} rest=${1#*:} uefi bios
  [[ $1 == *:* && $ko ]] || die "--insmod expects FILE.ko:ERRNO[:BIOS_ERRNO], got: $1"
  uefi=${rest%%:*}; bios=$uefi; [[ $rest == *:* ]] && bios=${rest#*:}
  for e in "$uefi" "$bios"; do
    [[ $e =~ ^(0|ENODEV|EKEYREJECTED)$ ]] || die "--insmod: unknown errno '$e' in $1"
  done
  echo "$ko $uefi $bios"
}

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
# The builtin keyring holds exactly the certificates of keys/modules and the key the kernel
# build generates for its own modules (measured on the 7.2 series: "Loading compiled-in X.509
# certificates" loads the Fedora-generated signing key and the Athanor module signing key).
K3_BUILTIN=$(( $(find "$HERE/keys/modules" -name '*.pem' | wc -l) + 1 ))
TEST_CMDLINE="$CMDLINE console=ttyS0,115200 panic=-1 k3.uname=$KVER k3.certs=$K3_CERTS k3.builtin=$K3_BUILTIN"

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
# Binaries with their libraries, all in /lib64, the default path of the loader: the guest
# has no ld.so.cache and libLLVM lives in a directory that on the host is reachable only
# through ld.so.conf.d.
install_binary() { # install_binary PATH: the binary and its libraries
  install -D -m 755 "$1" "$R$1"
  ldd "$1" | awk '/=> \//{print $3} /^\s*\/lib64\/ld-linux/{print $1}' \
    | while read -r lib; do install -D "$lib" "$R/lib64/${lib##*/}"; done
}
# bpftool: the Fedora one drags libLLVM along (140 MB uncompressed, 39 MB compressed), the
# price of `bpftool feature probe` done with the real tool. keyctl: the keyring assertions.
install_binary /usr/sbin/bpftool
install_binary /usr/bin/keyctl
install -m 755 "$HERE/boot/init" "$R/init"
# The modules under test, numbered: two branches share the same nvidia.ko. The k3.insmod
# parameter lists file:errno and goes into the command line of every case.
for s in "${INSMOD[@]}"; do insmod_spec "$s" > /dev/null; done # the process substitution below hides a failure
K3_INSMOD_UEFI='' K3_INSMOD_BIOS=''
for i in "${!INSMOD[@]}"; do
  read -r ko uefi bios < <(insmod_spec "${INSMOD[$i]}")
  [[ -f $ko ]] || die "--insmod: no such file: $ko"
  install -D -m 644 "$ko" "$R/modules/$i-${ko##*/}"
  K3_INSMOD_UEFI+="${K3_INSMOD_UEFI:+,}$i-${ko##*/}:$uefi"
  K3_INSMOD_BIOS+="${K3_INSMOD_BIOS:+,}$i-${ko##*/}:$bios"
done
if [[ $IMA_KEY ]]; then
  install -d "$R/ima" && openssl x509 -in "$IMA_KEY" -outform DER -out "$R/ima/key.der"
  TEST_CMDLINE+=" k3.imakey=1"
fi
BIOS_CMDLINE="$TEST_CMDLINE${K3_INSMOD_BIOS:+ k3.insmod=$K3_INSMOD_BIOS}"
UEFI_CMDLINE="$TEST_CMDLINE${K3_INSMOD_UEFI:+ k3.insmod=$K3_INSMOD_UEFI} k3.sb=1"
# The user CAs the UEFI cases must find in the machine keyring, by subject key identifier.
K3_MOKCA=''
for cert in "${MOK_CAS[@]}"; do K3_MOKCA+="${K3_MOKCA:+,}$(skid "$cert")"; done
UEFI_CMDLINE+="${K3_MOKCA:+ k3.mokca=$K3_MOKCA}"
(cd "$R" && find . | cpio -o -H newc --quiet | zstd -q -T0 -19 -o "$WORK/initramfs.img")
echo "initramfs: $(du -sh "$R" | cut -f1) uncompressed, $(du -h "$WORK/initramfs.img" | cut -f1) compressed"

step "UKI signed with an ephemeral MOK, enrolled in the OVMF varstore"
# The Secure Boot profile of the project key: the MOK the matrix enrols has its shape.
openssl req -x509 -newkey rsa:2048 -nodes -days 2 -config "$HERE/keys/profiles/secureboot.cnf" \
  -subj '/CN=Athanor OS K3 test MOK/' -keyout "$WORK/mok.key" -out "$OUT/mok.pem" 2> /dev/null
ukify build --linux "$VMLINUZ" --initrd "$WORK/initramfs.img" --uname "$KVER" \
  --cmdline "$UEFI_CMDLINE" --stub /usr/lib/systemd/boot/efi/linuxx64.efi.stub \
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
# D40: a user CA enrolled in MokList, trusted for the machine keyring. Since 15.6 shim
# mirrors MokListTrustedRT, the trust the kernel reads, unless MokListTrusted exists
# (mokutil --untrust-mok creates it): the variable is left unset, so the CA is trusted.
for cert in "${MOK_CAS[@]}"; do ADD_MOK+=(--add-mok "$(< /proc/sys/kernel/random/uuid)" "$cert"); done
virt-fw-vars -i /usr/share/edk2/ovmf/OVMF_VARS.secboot.fd -o "$WORK/vars.fd" "${ADD_MOK[@]}" > "$OUT/varstore.log"
# ESP: shim at the removable path, the UKI where shim looks for the second stage.
mkdir -p "$WORK/esp/EFI/BOOT"
cp /boot/efi/EFI/fedora/shimx64.efi "$WORK/esp/EFI/BOOT/BOOTX64.EFI"
cp "$WORK/uki.efi" "$WORK/esp/EFI/BOOT/grubx64.efi"

case_args() { # case_args NAME: the QEMU arguments of one case, in the global array CASE_ARGS
  local name=$1 cpu machine=q35,smm=on iommu='' fw
  case $name in
    bios-penryn|uefi-penryn) cpu=Penryn ;;
    bios-host|uefi-host) cpu=host ;;
    # Section 12 item 2: the interrupt remapping of both IOMMUs needs the split irqchip.
    iommu-intel) cpu=host iommu=intel-iommu,intremap=on machine+=,kernel-irqchip=split ;;
    iommu-amd) cpu=host iommu=amd-iommu,intremap=on machine+=,kernel-irqchip=split ;;
    *) die "unknown case: $name" ;;
  esac
  [[ $cpu == host && $ACCEL == tcg ]] && cpu=max
  fw=${name%%-*}; [[ $fw == iommu ]] && fw=bios
  CASE_ARGS=(-machine "$machine" -accel "$ACCEL" -cpu "$cpu" -smp 2 -m 2048
             -display none -monitor none -serial "file:$OUT/$name.log" -no-reboot)
  # The IOMMU comes before every other PCI device, as QEMU requires.
  [[ $iommu ]] && CASE_ARGS+=(-device "$iommu")
  CASE_ARGS+=(-device virtio-rng-pci)
  case $fw in
    bios) CASE_ARGS+=(-kernel "$VMLINUZ" -initrd "$WORK/initramfs.img"
                      -append "$BIOS_CMDLINE${iommu:+ k3.iommu=${name#iommu-}}") ;;
    uefi) CASE_ARGS+=(-global "driver=cfi.pflash01,property=secure,value=on"
                      -drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE"
                      -drive "if=pflash,format=raw,file=$WORK/vars-$name.fd"
                      -drive "if=virtio,format=raw,readonly=on,file=fat:ro:$WORK/esp") ;;
  esac
}

run_case() { # run_case NAME
  local name=$1 log="$OUT/$1.log"
  case_args "$name"
  [[ $name == uefi-* ]] && cp "$WORK/vars.fd" "$WORK/vars-$name.fd"
  step "$name (accel $ACCEL)"
  timeout 900 qemu-system-x86_64 "${CASE_ARGS[@]}" || echo "qemu: exit $?"
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
