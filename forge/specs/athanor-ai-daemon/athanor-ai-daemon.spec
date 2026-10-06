%global debug_package %{nil}
%global spec_dir forge/specs/%{name}
Name:           athanor-ai-daemon
Version:        1.0.0
Release:        3%{?dist}
Summary:        Athanor OS Local AI & Machine Learning Inference Daemon

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor


BuildRequires:  rust >= 1.80.0
BuildRequires:  cargo
BuildRequires:  systemd-rpm-macros
BuildRequires:  gcc gcc-c++ pkgconf-pkg-config openssl-devel

%description
Local AI inference service for Athanor OS, exposed over D-Bus (os.athanor.AiDaemon).
The Candle-based model loader parses the GGUF header only and builds placeholder
weights: inference is not implemented yet.

%prep
# Built in place from the workspace checkout: nothing to unpack.

%build
%set_build_flags
# cargo generate-lockfile // FORBIDDEN BY RULE 4 (Offline Build)
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/%{name} %{buildroot}/usr/bin/%{name}
install -D -m 0644 %{spec_dir}/athanor-ai-daemon.service %{buildroot}/usr/lib/systemd/system/%{name}.service

%post
%systemd_post %{name}.service

%preun
%systemd_preun %{name}.service

%postun
%systemd_postun_with_restart %{name}.service

%files
/usr/bin/%{name}
/usr/lib/systemd/system/%{name}.service

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Point URL at the project repository
- Correct the %description: inference is a stub

* Sun Sep 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Install the unit from the spec directory instead of an empty placeholder file

* Wed Aug 05 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial release of athanor-ai-daemon spec
