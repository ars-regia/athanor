Name:           athanor-secure-boot
Version:        1.0.0
Release:        3%{?dist}
Summary:        Athanor OS TPM sealing and rollback protection

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor-forge
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  systemd-rpm-macros
Requires:       systemd-ukify systemd-boot-unsigned sbsigntools tpm2-tools cryptsetup

%description
Athanor OS cryptographic scripts for Unified Kernel Image (UKI) generation using systemd-stub and ukify,
UEFI Secure Boot signing, and TPM 2.0 PCR 0, 2, 7, 11 measurement/sealing.

%prep
# Nothing to prep, just source files

%build
# Nothing to build

%install
mkdir -p %{buildroot}%{_libexecdir}/athanor
install -m 0755 %{_sourcedir}/usr/libexec/athanor-tpm-luks-seal.sh %{buildroot}%{_libexecdir}/athanor-tpm-luks-seal.sh
install -m 0755 %{_sourcedir}/usr/libexec/athanor/athanor-tpm-rollback-check.sh %{buildroot}%{_libexecdir}/athanor/athanor-tpm-rollback-check.sh
install -m 0755 %{_sourcedir}/usr/libexec/athanor/athanor-tpm-rollback-update.sh %{buildroot}%{_libexecdir}/athanor/athanor-tpm-rollback-update.sh

# Install systemd services
mkdir -p %{buildroot}/usr/lib/systemd/system
install -m 0644 %{_sourcedir}/usr/lib/systemd/system/athanor-tpm-rollback-check.service %{buildroot}/usr/lib/systemd/system/athanor-tpm-rollback-check.service
install -m 0644 %{_sourcedir}/usr/lib/systemd/system/athanor-tpm-rollback-update.service %{buildroot}/usr/lib/systemd/system/athanor-tpm-rollback-update.service
install -m 0644 %{_sourcedir}/usr/lib/systemd/system/athanor-tpm-luks-seal.service %{buildroot}/usr/lib/systemd/system/athanor-tpm-luks-seal.service

mkdir -p %{buildroot}/usr/lib/systemd/system/systemd-pcrphase-sysinit.service.d
install -m 0644 %{_sourcedir}/usr/lib/systemd/system/systemd-pcrphase-sysinit.service.d/10-rollback-check.conf %{buildroot}/usr/lib/systemd/system/systemd-pcrphase-sysinit.service.d/10-rollback-check.conf

%files
%{_libexecdir}/athanor-tpm-luks-seal.sh
%{_libexecdir}/athanor/athanor-tpm-rollback-check.sh
%{_libexecdir}/athanor/athanor-tpm-rollback-update.sh
/usr/lib/systemd/system/athanor-tpm-luks-seal.service
/usr/lib/systemd/system/athanor-tpm-rollback-check.service
/usr/lib/systemd/system/athanor-tpm-rollback-update.service
/usr/lib/systemd/system/systemd-pcrphase-sysinit.service.d/10-rollback-check.conf

%changelog
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Retire athanor-secure-boot.service and its measure script (doc_update_trust.md, D2): the
  unit was never enabled, and the script generated a signing key on the machine. The four
  Secure Boot readings are published by athanor-update. The TPM sealing, rollback check and
  pcrphase drop-in stay.

* Fri Aug 07 2026 Athanor <athanor@athanor.os> - 1.0.0-2
- Add UKI assembly via systemd-stub and ukify, and TPM 2.0 PCR 0,2,7,11 LUKS sealing service
