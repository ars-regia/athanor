# Athanor: Platform Experience (Boot, Disk Encryption and the Desktop)

This document links the platform's trust chain to what the user sees, from power-on to the
desktop. Each area has a specification of its own, named below; this one only says how
they meet.

---

## 1. Boot and disk encryption

### Boot chain

Release 1.0 has no Unified Kernel Image (ADR-0037, ADR-0043). The firmware starts shim,
shim verifies GRUB and the kernel, and the kernel (`vmlinuz`) is signed with the project's
Secure Boot key in a sign-only CI job and trusted on the machine through a MOK the owner
enrols (ADR-0064). The initramfs and the kernel command line are not signed. The UKI, with
a signed PCR 11 policy, arrives with the sealed composefs of release 1.1 (ADR-0043). The
kernel and its command line are specified in [doc_kernel_profile.md](doc_kernel_profile.md).

### Disk encryption and the TPM

In release 1.0, disk encryption is a choice the person makes in the installer, not a
default (maintainer decision of 2026-10-08). The installer offers LUKS2 encryption, and the
person may decline it.

What the installer does today. The ISO is bootc-image-builder's Anaconda installer with the
kickstart in `system/disk_config/iso.toml`; the manual build uses `system/athanor-install.ks`.
Neither kickstart carries `clearpart`, `part` or `autopart`, so Anaconda opens its storage
screen and leaves the disk layout and the encryption choice to the person. Anaconda's
encryption option is off until the person turns it on. When it is on, Anaconda asks for a
passphrase and, with automatic partitioning, puts the btrfs file system that holds `/`,
`/var` and `/home` in one LUKS2 volume; `/boot` and the EFI system partition stay
unencrypted. When it is off, user data is stored unencrypted.

Gaps, named here so that this text claims no more than the tree does:

- The ISO carries Anaconda's GTK interface. The web interface that ADR-0059 chooses for 1.0
  is not built yet, and its encryption screen has not been checked.
- No acceptance run installs with encryption on. The encrypted layout above is Anaconda's
  behaviour, not one the project's tests verify.
- The installer enrols neither a recovery key nor the TPM: the volume it creates has the
  passphrase keyslot only.

Accounts are classic accounts in `/etc/passwd`, and systemd-homed is disabled by preset
(ADR-0045), so nothing encrypts a home directory by itself: the home is encrypted exactly
when the system volume is.

In 1.0 an encrypted volume unlocks with its passphrase, or with TPM plus PIN once an
administrator runs `athanor-uki-enroll`; TPM-only unlocking needs the signed UKI (ADR-0064;
A2-27 as amended on 2026-10-08, D42 in doc_kernel_profile.md). Nothing in the image enrols
the TPM 2.0 by itself.
`athanor-tpm-luks-seal`, which sealed the volume at first boot to PCRs 0, 2, 7 and 11, was
removed with the other TPM units that acted without the user (issue #148), and
`verify.py shipped` fails if it is shipped again. An administrator may run
`athanor-uki-enroll <device>` (A2-27 as amended on 2026-10-08): it adds a keyslot that needs
both the TPM and a PIN, bound to the value of PCR 7, enrols a recovery key first when the
volume has none, and keeps the passphrase. PCR 7 holds the Secure Boot state, the
firmware's PK, KEK, db and dbx, the db certificate that verified shim and, as shim's
[README.tpm](https://github.com/rhboot/shim/blob/15.8/README.tpm) states (lines 9-22), the
certificate from db, MokList or shim's own list that matched each binary shim verifies,
GRUB and the kernel, plus SBAT and MokSBState. It does not hold the initrd or the kernel
command line. The PIN is required in 1.0 because neither is signed and GRUB has no
password, so the TPM alone cannot tell the boot Athanor ships from another one that
Secure Boot also accepts. TPM-only unlocking waits for the signed UKI (D42). The PIN has
a limit: it protects a machine taken while powered off, not one whose `/boot` someone
changes and leaves for its owner to start, since a boot prepared that way can ask for the
PIN itself and PCR 7 does not change; closing that needs the signed UKI (P4b). The tool
refuses when Secure Boot does not verify the boot chain (it reads `mokutil --sb-state`),
since PCR 7 then binds nothing, and when PCR 7 reads all zeros, since firmware that never
measured it leaves the keyslot on the PIN alone. It changes no boot configuration: with no `tpm2-device=`
option, systemd-cryptsetup tries the volume's LUKS2 tokens before the passphrase, and the
generic initramfs carries the TPM2 token plugin, so the next boot asks for the PIN. Kernel
updates leave PCR 7 alone, and so do most firmware updates; an update of the Secure Boot
databases (db, dbx or KEK, which fwupd applies), of shim, or a kernel signed with a new
Secure Boot key (whose certificate shim measures from MokList) can change it, and the next boot then asks for the passphrase or the recovery key;
running the tool again binds the new value. After repeated wrong PINs the boot asks for the
passphrase or the recovery key. In the swtpm acceptance run that happened when the TPM's
dictionary-attack lockout engaged, after three failures; on a physical TPM with a higher
limit systemd-cryptsetup's own retry count may end it first. While a lockout lasts the right
PIN fails too. A `systemd-pcrlock` policy, which survives
announced updates of that kind, arrives with the UKI (P4b, D42).

---

## 2. First run and the greeter

The first run is specified in doc_first_run.md.

The login screen is `athanor-greeter-ui`, a GTK4 client of greetd. `athanor-greeter-session`
starts it on a `cosmic-comp` instance of its own, inside a bubblewrap sandbox, and the
greeter confines its own writes with Landlock before anything else. After login the session
runs on `cosmic-comp` ([doc_shell.md](doc_shell.md)).
First run is specified in [doc_first_run.md](doc_first_run.md).

---

## 3. Applications and the desktop

The root filesystem is immutable: applications are not installed with `dnf`. Graphical
applications are Flatpaks, confined by bubblewrap, and reach the user's files through the
XDG portals; the image ships `xdg-desktop-portal-athanor` for the file chooser. How
applications are found, installed and updated is specified in
[doc_software.md](doc_software.md), which also records that no Flathub remote is configured
today.

- **Interface:** COSMIC on cosmic-comp, with Athanor's own surfaces replacing COSMIC's one
  stage at a time. See [doc_shell.md](doc_shell.md), which supersedes the niri and Relm4
  panel described here before 2026-09-18.
- **Development:** the image carries Nix (`athanor-nix-support`); the developer mode is
  part of [doc_software.md](doc_software.md).
