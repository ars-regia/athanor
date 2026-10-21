# Athanor's greenboot: the upstream release at Fedora's version, built from upstream's archive
# with three patches of ours: one adds GREENBOOT_AUTO_REBOOT, so that a failed health check marks
# the deployment and reboots nothing (docs/architecture/doc_recovery.md, R5); one sets no
# rollback trigger for a download-only staged deployment, which a poweroff discards; one does
# not take a rollback made on purpose for a GRUB fallback. Fedora's own
# downstream changes are not carried; License, Requires, the install steps and the file lists
# were copied from Fedora's spec, so a bump diffs them by hand. Same Name and subpackages so
# that they replace Fedora's build instead of sitting beside it (like forge/specs/cosmic-comp).
# The release spells out Fedora's, so that 0.16.4-0.fc43.athanor1 sorts above 0.16.4-0.fc43
# and below 0.16.4-1.fc43: the Nix builder defines no %%dist.
%global fedora_release 0.fc45
%global pkgname greenboot

Name:           greenboot-rs
Version:        0.16.4
Release:        %{fedora_release}.athanor1
Summary:        Generic Health Check Framework for systemd
# Fedora's expression, from the crates it links statically.
License:        BSD-3-Clause AND ISC AND MIT AND Unicode-DFS-2016 AND (Apache-2.0 OR BSL-1.0) AND (Apache-2.0 OR MIT) AND (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT) AND (Unlicense OR MIT)
URL:            https://github.com/fedora-iot/greenboot-rs
Source0:        https://github.com/fedora-iot/greenboot-rs/archive/refs/tags/v%{version}.tar.gz#/greenboot-rs-%{version}.tar.gz
# Upstream does not commit a Cargo.lock and Fedora builds against its packaged crates; this
# lock pins the crates the fetch stage downloads and the offline build uses.
Source1:        Cargo.lock
# Written for upstream, not yet submitted; drop it when a release carries it.
Patch0:         0001-feat-add-GREENBOOT_AUTO_REBOOT-to-roll-back-without-.patch
# Written for upstream, not yet submitted: a locked deployment of `bootc upgrade --download-only`
# is no update that boots next, so it arms no rollback trigger (#454).
Patch1:         0002-fix-set-no-rollback-trigger-for-a-download-only-stag.patch
# Written for upstream, not yet submitted: the boot after `bootc rollback` starts the default
# deployment, so a trigger its health check had no time to clear is no GRUB fallback (#521).
Patch2:         0003-fix-do-not-take-a-rollback-made-on-purpose-for-a-GRU.patch

ExcludeArch:    %{ix86}

BuildRequires:  cargo rust
BuildRequires:  systemd-rpm-macros

%description
Greenboot is a generic health check framework for systemd allowing
the use of healthchecks to check the state of the system post
upgrade to ensure the system is in a known-good state and to allow
automated rollback actions if it's not.

%package -n %{pkgname}
Summary:        %{summary}
%{?systemd_ordering}
Requires:       systemd >= 240
Requires:       rpm-ostree
Requires:       pam >= 1.4.0
Recommends:     openssh

%description -n %{pkgname}
%{description}

This build adds GREENBOOT_AUTO_REBOOT: when it is false, a failed required health check
rolls back to the previous deployment without rebooting, it sets no rollback trigger
for a download-only staged deployment, and it does not undo a rollback made on purpose.

%package -n %{pkgname}-default-health-checks
Summary:        Series of optional and curated health checks
License:        BSD-3-Clause
Requires:       %{pkgname} = %{version}-%{release}
Requires:       util-linux
Requires:       jq

%description -n %{pkgname}-default-health-checks
%{description}

This package adds some default healthchecks for greenboot.

%prep
%autosetup -p1 -n greenboot-rs-%{version}
cp %{SOURCE1} Cargo.lock

%build
%set_build_flags
cargo build --release --locked

%check
# The tests of the patches: the bootc status parsing (handler) and the configuration
# (the binary). Upstream's grub and mount tests call grub2-editenv and need a /boot mount,
# which the builder has neither of.
cargo test --release --locked --lib handler::tests
cargo test --release --locked --bin greenboot

%install
mkdir -p %{buildroot}%{_libexecdir}/%{pkgname}
install -Dpm0755 target/release/greenboot %{buildroot}%{_libexecdir}/%{pkgname}/%{pkgname}
install -Dpm0644 -t %{buildroot}%{_unitdir} usr/lib/systemd/system/*.service
install -Dpm0644 -t %{buildroot}%{_unitdir} usr/lib/systemd/system/*.target
mkdir -p %{buildroot}%{_exec_prefix}/lib/motd.d/
install -Dpm0644 -t %{buildroot}%{_sysconfdir}/%{pkgname} etc/greenboot/greenboot.conf
install -D -t %{buildroot}%{_prefix}/lib/bootupd/grub2-static/configs.d grub2/08_greenboot.cfg
mkdir -p %{buildroot}%{_sysconfdir}/%{pkgname}/check/required.d
mkdir    %{buildroot}%{_sysconfdir}/%{pkgname}/check/wanted.d
mkdir    %{buildroot}%{_sysconfdir}/%{pkgname}/green.d
mkdir    %{buildroot}%{_sysconfdir}/%{pkgname}/red.d
mkdir -p %{buildroot}%{_prefix}/lib/%{pkgname}/check/required.d
mkdir    %{buildroot}%{_prefix}/lib/%{pkgname}/check/wanted.d
mkdir    %{buildroot}%{_prefix}/lib/%{pkgname}/green.d
mkdir    %{buildroot}%{_prefix}/lib/%{pkgname}/red.d
install -DpZm 0755 usr/lib/greenboot/check/required.d/* %{buildroot}%{_prefix}/lib/%{pkgname}/check/required.d
install -DpZm 0755 usr/lib/greenboot/check/wanted.d/* %{buildroot}%{_prefix}/lib/%{pkgname}/check/wanted.d
install -DpZm 0644 usr/lib/systemd/system/greenboot-healthcheck.service.d/10-network-online.conf %{buildroot}%{_unitdir}/greenboot-healthcheck.service.d/10-network-online.conf

%post -n %{pkgname}
if [ -d /run/systemd/system ]; then
%systemd_post greenboot-healthcheck.service
%systemd_post greenboot-set-rollback-trigger.service
%systemd_post greenboot-success.target
fi

%preun -n %{pkgname}
if [ -d /run/systemd/system ]; then
%systemd_preun greenboot-healthcheck.service
%systemd_preun greenboot-set-rollback-trigger.service
%systemd_preun greenboot-success.target
fi

%postun -n %{pkgname}
%systemd_postun greenboot-healthcheck.service
%systemd_postun greenboot-set-rollback-trigger.service
%systemd_postun greenboot-success.target

%files -n %{pkgname}
%license LICENSE
%doc README.md
%dir %{_libexecdir}/%{pkgname}
%{_libexecdir}/%{pkgname}/%{pkgname}
%{_unitdir}/greenboot-healthcheck.service
%{_unitdir}/greenboot-set-rollback-trigger.service
%{_unitdir}/greenboot-success.target
%config(noreplace) %{_sysconfdir}/%{pkgname}/greenboot.conf
%{_prefix}/lib/bootupd/grub2-static/configs.d/08_greenboot.cfg
%dir %{_prefix}/lib/%{pkgname}
%dir %{_prefix}/lib/%{pkgname}/check
%dir %{_prefix}/lib/%{pkgname}/check/required.d
%dir %{_prefix}/lib/%{pkgname}/check/wanted.d
%dir %{_prefix}/lib/%{pkgname}/green.d
%dir %{_prefix}/lib/%{pkgname}/red.d
%dir %{_sysconfdir}/%{pkgname}
%dir %{_sysconfdir}/%{pkgname}/check
%dir %{_sysconfdir}/%{pkgname}/check/required.d
%dir %{_sysconfdir}/%{pkgname}/check/wanted.d
%dir %{_sysconfdir}/%{pkgname}/green.d
%dir %{_sysconfdir}/%{pkgname}/red.d

%files -n %{pkgname}-default-health-checks
%dir %{_unitdir}/greenboot-healthcheck.service.d
%{_prefix}/lib/%{pkgname}/check/wanted.d/01_update_platforms_check.sh
%{_prefix}/lib/%{pkgname}/check/required.d/02_watchdog.sh
%{_prefix}/lib/%{pkgname}/check/required.d/01_repository_dns_check.sh
%{_unitdir}/greenboot-healthcheck.service.d/10-network-online.conf

%changelog
* Wed Oct 21 2026 Athanor Forge <forge@athanor.os> - 0.16.4-0.fc45.athanor1
- Fedora's greenboot-rs 0.16.4-0.fc45, for the Fedora 45 base; the three patches are unchanged.

* Fri Oct 16 2026 Athanor Forge <forge@athanor.os> - 0.16.4-0.fc43.athanor3
- Do not take a rollback made on purpose for a GRUB fallback: when the health check of a new
  deployment did not finish before `bootc rollback`, the next boot was taken for a fallback and
  rolled back to the deployment left (#521).

* Wed Oct 14 2026 Athanor Forge <forge@athanor.os> - 0.16.4-0.fc43.athanor2
- Set no rollback trigger for a download-only staged deployment: a poweroff discards it, and
  the next boot was taken for a GRUB fallback and rolled back to the previous deployment
  (#454). %%check runs the tests of both patches.

* Wed Oct 07 2026 Athanor Forge <forge@athanor.os> - 0.16.4-0.fc43.athanor1
- Fedora's greenboot-rs 0.16.4-0.fc43 built from the upstream archive, with the patch that
  adds GREENBOOT_AUTO_REBOOT.
