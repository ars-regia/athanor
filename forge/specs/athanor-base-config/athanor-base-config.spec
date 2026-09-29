%global debug_package %{nil}
Name:           athanor-base-config
Version:        43.0.0
Release:        10%{?dist}
Summary:        Athanor OS Base Configuration (Systemd, Branding, GPG)

License:        MIT
URL:            https://github.com/hr-mes/athanor-forge
BuildArch:      noarch

Requires:       glibc-langpack-it glibc-langpack-en
Provides:       fedora-logos = 43
Provides:       fedora-logos = %{version}-%{release}
Obsoletes:      fedora-logos < 43
Provides:       fedora-logos-httpd = 43
Provides:       fedora-logos-httpd = %{version}-%{release}
Obsoletes:      fedora-logos-httpd < 43
Provides:       system-logos = 43
Provides:       system-logos = %{version}-%{release}
Obsoletes:      system-logos < 43
Provides:       system-logos-httpd = 43
Provides:       system-logos-httpd = %{version}-%{release}
Obsoletes:      system-logos-httpd < 43
Provides:       fedora-release = 43
Provides:       fedora-release = %{version}-%{release}
Obsoletes:      fedora-release < 43
Provides:       fedora-release-systemd = 43
Provides:       fedora-release-systemd = %{version}-%{release}
Obsoletes:      fedora-release-systemd < 43
Provides:       fedora-release-identity = 43
Provides:       fedora-release-identity = %{version}-%{release}
Obsoletes:      fedora-release-identity < 43
Provides:       fedora-release-common = 43
Provides:       fedora-release-common = %{version}-%{release}
Obsoletes:      fedora-release-common < 43
Provides:       system-release = 43
Provides:       system-release = %{version}-%{release}
Provides:       system-release(43)
Obsoletes:      system-release < 43
%description
This package provides the foundational configuration for Athanor Base.
It includes Dracut configurations,
Systemd presets, custom Plymouth/GDM branding, Polkit rules, and GPG keys.

%prep
# No extraction needed, files are injected in install phase.

%build
# Nothing to build

%install
mkdir -p %{buildroot}
find %{_sourcedir} -mindepth 1 -maxdepth 1 ! -name "*.spec" -exec cp -a {} %{buildroot}/ \;
mkdir -p %{buildroot}/usr/lib/tmpfiles.d
mv %{buildroot}/etc/tmpfiles.d/* %{buildroot}/usr/lib/tmpfiles.d/
rm -rf %{buildroot}/etc/tmpfiles.d

%files
/etc/pki/rpm-gpg/*
/etc/selinux/config
/etc/yum.repos.d/*
/etc/ssh/sshd_config.d/*
/etc/systemd/coredump.conf.d/*
/usr/lib/systemd/system/*
/usr/lib/tmpfiles.d/*
/usr/lib/systemd/journald.conf.d/*
/usr/lib/fedora-release
/usr/lib/os-release
/usr/lib/bootc/kargs.d/02-hardening.toml
/usr/lib/bootc/kargs.d/03-ima-evm.toml
/usr/lib/bootc/kargs.d/04-confidential-compute.toml
/usr/lib/bootc/kargs.d/05-dma-protection.toml
/usr/lib/bootc/kargs.d/06-mte-lam.toml
/etc/grub.d/01_athanor_grub_auth
/usr/lib/dracut/dracut.conf.d/*
/usr/lib/systemd/system-preset/*
/usr/lib/systemd/system/scx_loader.service.d/*
/usr/lib/sysusers.d/*
/usr/share/pixmaps/*
/usr/share/plymouth/themes/spinner/watermark.png
/usr/share/polkit-1/rules.d/*

%changelog
* Thu Sep 24 2026 Athanor Forge <forge@athanor.os> - 43.0.0-10
- Drop the override of bootc-fetch-apply-updates.service: it called `bootc upgrade --stage`,
  a flag bootc 1.16 does not have. The stock timer stays disabled by athanor-update's preset.

* Fri Sep 18 2026 Athanor Forge <forge@athanor.os> - 43.0.0-9
- Rename 99-Athanor-Base.preset to 80-athanor-base.preset, so mcelog.service is really
  disabled. systemd takes the FIRST preset line that matches a unit, in lexicographic
  order of file name, and Fedora's 90-default.preset carries "enable mcelog.*": at 99
  this file was read after it and the disable line was never reached, so preset-all
  left the enablement symlink base-atomic ships and every installation booted a failing
  mcelog.service. The same shadowing hid "disable NetworkManager-wait-online.service",
  which 90-default.preset enables; both now take effect.
- Drop the preset lines for plymouth-quit-wait.service and systemd-udev-settle.service.
  Neither unit has an [Install] section, so neither can be enabled or disabled and the
  lines never did anything.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 43.0.0-8
- Disable mcelog.service, which base-atomic ships, through the preset. The kernel
  reports machine checks on every vendor; mcelog is deprecated upstream and fails on
  AMD family 17h and later.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 43.0.0-7
- Remove kargs.d/07-btrfs-root.toml. A rootflags=compress=zstd:1 applied to every
  installation would keep an ext4 or xfs root from mounting; the installer now adds the
  option to the installed deployment only when its root is btrfs.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 43.0.0-6
- Remove athanor-journal-seal.service, its preset line and Seal=yes from
  99-immutable.conf. Fedora builds systemd without gcrypt, so journalctl has no
  forward-secure sealing, --setup-keys fails and the unit failed on every boot while
  sealing was never in effect. Storage=persistent stays.
* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 43.0.0-5
- Mount the btrfs root with compress=zstd:1 through rootflags= in
  kargs.d/07-btrfs-root.toml. The installer now removes the / line Anaconda writes to
  /etc/fstab, which systemd-remount-fs cannot apply to a composefs root, and that line
  was the only place the option lived; the initrd never applied it.
- Drop "disable systemd-remount-fs.service" from the preset: the unit has no [Install]
  section, so the line never had an effect.
* Wed Sep 16 2026 Athanor Forge <forge@athanor.os> - 43.0.0-4
- Drop the nvidia-powerd/nvidia-persistenced preset enables and the nvidia-persistenced
  sysusers entry: the vendor packages own them in the NVIDIA images, and the default image
  never installs the NVIDIA driver (doc_system_image.md, S3)
* Wed Sep 16 2026 Athanor Forge <forge@athanor.os> - 43.0.0-3
- Move the NVIDIA configuration and the negativo17 repository to athanor-nvidia-config (doc_system_image.md, S3)
* Fri Jul 31 2026 Athanor <athanor@customer.mlnnita1.isp.starlink.com> - 1.0.0-6
- Add Provides and Obsoletes for fedora-logos to avoid conflicts during system image build
* Thu Jul 30 2026 Athanor <athanor@customer.mlnnita1.isp.starlink.com> - 1.0.0-5
- Trigger rebuild to execute deduplication fix
* Tue Jul 14 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Require glibc-langpack-it and glibc-langpack-en to guarantee Bedrock locale availability across all apps when glibc-all-langpacks is pruned

* Mon Jul 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Add enable nvidia-persistenced.service to systemd preset for deterministic GPU node creation

* Wed Jul 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial encapsulation of raw files into RPM for Bedrock logic
