Name:           azoth-nvidia-kmod
Version:        %{nvidia_version}
Release:        1%{?dist}
Summary:        Declares the signed NVIDIA modules of the Azoth kernel as the installed nvidia-kmod
License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor
BuildArch:      noarch
# The modules themselves come from ghcr.io/hr-mes/azoth-nvidia:<kernel-nvr>-<branch>, built
# and signed by nvidia-kmod.yml; the driver packages require a package providing the module
# at their exact version (doc_system_image.md, S4 and S5).
Provides:       nvidia-kmod = 3:%{nvidia_version}
Conflicts:      akmod-nvidia
Conflicts:      kmod-nvidia
Conflicts:      dkms-nvidia

%description
Satisfies the nvidia-kmod requirement of the NVIDIA driver packages with the signed modules
that the Athanor system image copies next to the Azoth kernel. It installs no files.

%prep

%build

%install

%files

%changelog
* Wed Sep 16 2026 Athanor Forge <forge@athanor.os> - %{nvidia_version}-1
- Shim for the signed NVIDIA modules of the Azoth kernel
