# driver: the name RPM Fusion and negativo17 give the branch's kernel module, nvidia for the open
# branch, nvidia-580xx for the legacy one on Fedora 45 (build-rpms.sh).
%{!?driver:%global driver nvidia}
Name:           azoth-nvidia-kmod
Version:        %{nvidia_version}
Release:        1%{?dist}
Summary:        Declares the signed NVIDIA modules of the Azoth kernel as the installed %{driver}-kmod
License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor
BuildArch:      noarch
# The modules themselves come from ghcr.io/ars-regia/azoth-nvidia:<kernel-nvr>-<branch>, built
# and signed by nvidia-kmod.yml; the driver packages require a package providing the module
# at their exact version (doc_system_image.md, S4 and S5).
Provides:       %{driver}-kmod = 3:%{nvidia_version}
Conflicts:      akmod-%{driver}
Conflicts:      kmod-%{driver}
Conflicts:      dkms-%{driver}

%description
Satisfies the %{driver}-kmod requirement of the NVIDIA driver packages with the signed modules
that the Athanor system image copies next to the Azoth kernel. It installs no files.

%prep

%build

%install

%files

%changelog
* Wed Sep 16 2026 Athanor Forge <forge@athanor.os> - %{nvidia_version}-1
- Shim for the signed NVIDIA modules of the Azoth kernel
