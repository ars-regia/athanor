# Athanor: Platform Experience (Boot, Disk Encryption and the Desktop)

This document links the platform's trust chain to what the user sees, from power-on to the
desktop. Each area has a specification of its own, named below; this one only says how
they meet.

---

## 1. Boot and disk encryption

### Unified Kernel Image

The kernel, its initramfs and its command line are one signed file, a Unified Kernel Image
(UKI). `system/build-image.sh` builds it and signs it with the project's Secure Boot key;
an image whose UKI is signed with a throwaway key is never published. The kernel and its
command line are specified in [doc_kernel_profile.md](doc_kernel_profile.md).

### Home encryption and the TPM

The installer leaves the disk layout to Anaconda (`system/athanor-install.ks`).
systemd-homed encrypts the user's home with LUKS2. In 1.0 the LUKS device unlocks with the
passphrase only: nothing in the image enrols it in the TPM 2.0 (decision A2-27; D42 in
doc_kernel_profile.md). `athanor-tpm-luks-seal`, which sealed it at first boot to PCRs 0,
2, 7 and 11, was removed with the other TPM units that acted without the user (issue #148),
and `verify.py shipped` fails if it is shipped again. The TPM seal returns in 1.1 with the
UKI and a signed PCR 11 policy, so that a kernel update does not break the unlock.

---

## 2. First run and the greeter

The first run is specified in doc_first_run.md.

The login screen is `athanor-greeter-ui`, a GTK4 client of greetd. `athanor-greeter-session`
starts it on a `cosmic-comp` instance of its own, inside a bubblewrap sandbox, and the
greeter confines its own writes with Landlock before anything else. After login the session
runs on `cosmic-comp` ([doc_shell.md](doc_shell.md)).

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
