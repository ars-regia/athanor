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
systemd-homed encrypts the user's home with LUKS2. At first boot
`athanor-tpm-luks-seal.service` enrolls the LUKS device behind `/var/home` in the TPM 2.0
with `systemd-cryptenroll`, bound to PCRs 0 (firmware), 2 (option ROMs), 7 (Secure Boot
state) and 11 (the UKI). The TPM releases the key only when those measurements match the
ones it was sealed to.

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
