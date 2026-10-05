%global debug_package %{nil}
Name:           athanor-nvidia-config
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor configuration for the NVIDIA image variants
License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor
BuildArch:      noarch
Requires:       azoth-nvidia-kmod

%description
Kernel arguments, the nouveau blacklist, nvidia-drm options, module loading, the udev
seat rule and the persistence daemon preset for the NVIDIA driver. Power management and the
initrd policy stay with the vendor packages of each branch. Installed only by
athanor-system-nvidia and athanor-system-nvidia-legacy (docs/architecture/doc_system_image.md,
S4 and S5).

%prep

%build

%install
mkdir -p %{buildroot}
cp -a %{_sourcedir}/usr %{buildroot}/

%files
/usr/lib/bootc/kargs.d/01-nvidia.toml
/usr/lib/modprobe.d/athanor-nvidia-blacklist-nouveau.conf
/usr/lib/modprobe.d/nvidia-drm.conf
/usr/lib/modules-load.d/10-nvidia.conf
/usr/lib/systemd/system-preset/70-athanor-nvidia.preset
%dir /usr/lib/systemd/system/nvidia-powerd.service.d
/usr/lib/systemd/system/nvidia-powerd.service.d/laptop-only.conf
/usr/lib/udev/rules.d/71-nvidia-uaccess.rules

%changelog
* Wed Sep 16 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- NVIDIA configuration moved out of athanor-base-config (doc_system_image.md, S4)
