Name:           athanor-qa
Version:        1.0.0
Release:        2%{?dist}
Summary:        Athanor OS Quality Assurance Scripts

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor

BuildArch:      noarch

%description
Installs /usr/bin/test-nvidia-modules.sh, a diagnostic that checks the NVIDIA kernel
modules load on the running system.

%prep
# No prep

%build
# No build

%install
mkdir -p %{buildroot}/usr/bin
install -m 0755 %{_sourcedir}/test-nvidia-modules.sh %{buildroot}/usr/bin/test-nvidia-modules.sh

%files
/usr/bin/test-nvidia-modules.sh

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Point URL at the project repository
- Describe the one script the package installs

* Mon Aug 03 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial release
