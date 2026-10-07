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

In 1.0 an encrypted volume unlocks with its passphrase (ADR-0064; A2-27, D42 in
doc_kernel_profile.md), and nothing in the image enrols the TPM 2.0 by itself.
`athanor-tpm-luks-seal`, which sealed the volume at first boot to PCRs 0, 2, 7 and 11, was
removed with the other TPM units that acted without the user (issue #148), and
`verify.py shipped` fails if it is shipped again. An administrator may run
`athanor-uki-enroll <device>`: it binds a TPM keyslot to PCR 7 and to the machine's
`systemd-pcrlock` policy (`/var/lib/systemd/pcrlock.json`), enrols a recovery key first
when the volume has none, and keeps the passphrase. It does not create that policy, nothing
updates the policy after a firmware or Secure Boot database change, and nothing hands it to
the initrd before the system volume is unlocked; until those exist (P4b), TPM unlock of the
system volume is not supported, and the passphrase stays the way in.

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
