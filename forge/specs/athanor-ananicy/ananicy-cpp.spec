%global debug_package %{nil}
# Snapshot di master: l'ultimo tag (1.1.1, 2023) non compila con glibc 2.42 e clang 22
# (std::int32_t/std::strerror senza <cstdint>/<cstring>, struct sched_attr ridefinita);
# il merge "fix-glibc-2.42-headers" del 2026-08-18 li sistema. Versione post-release
# nella notazione di Fedora: <ultimo tag>^<data>git<commit>.
%global commit 3554447c1ca495478bd00e002078847dfd2205d6
%global snapdate 20260818

Name:           ananicy-cpp
Version:        1.1.1^%{snapdate}git%(echo %{commit} | cut -c1-7)
Release:        1%{?dist}
Summary:        Ananicy rewritten in C++

License:        GPL-3.0-only
URL:            https://gitlab.com/ananicy-cpp/ananicy-cpp
Source0:        https://gitlab.com/ananicy-cpp/ananicy-cpp/-/archive/%{commit}/ananicy-cpp-%{commit}.tar.gz

BuildRequires:  cmake
BuildRequires:  gcc-c++
BuildRequires:  spdlog-devel
BuildRequires:  fmt-devel
BuildRequires:  systemd-devel
BuildRequires:  nlohmann-json-devel
Requires:       systemd

%description
Ananicy-cpp is a rewrite of ananicy in C++ for lower resource usage and faster
startup. It applies nice, ionice, cgroup and OOM score rules to processes as
they are spawned. Rules are read from /etc/ananicy.d; the package ships none.

%prep
%autosetup -n ananicy-cpp-%{commit}

%build
%set_build_flags
# Librerie di sistema invece dei download di CPM; backend netlink (proc connector),
# non quello eBPF, così la build non dipende dal BTF del kernel del builder.
cmake -S . -B build \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX=%{_prefix} \
  -DUSE_EXTERNAL_SPDLOG=ON \
  -DUSE_EXTERNAL_FMTLIB=ON \
  -DUSE_EXTERNAL_JSON=ON \
  -DENABLE_SYSTEMD=ON \
  -DUSE_BPF_PROC_IMPL=OFF
cmake --build build %{?_smp_mflags}

%install
DESTDIR=%{buildroot} cmake --install build
mkdir -p %{buildroot}%{_sysconfdir}/ananicy.d

%post
%systemd_post ananicy-cpp.service

%preun
%systemd_preun ananicy-cpp.service

%postun
%systemd_postun_with_restart ananicy-cpp.service

%files
%license LICENSE
%{_bindir}/ananicy-cpp
%{_unitdir}/ananicy-cpp.service
%dir %{_sysconfdir}/ananicy.d

%changelog
* Thu Sep 03 2026 Athanor Forge <forge@athanor.os> - 1.1.1^20260818git3554447-1
- Spec riscritta: Source0 dall'archivio upstream GitLab verificato da
  SOURCES/sources.sha256, cmake esplicito al posto delle macro %%cmake di Fedora,
  unità systemd upstream installata da cmake invece del file vuoto creato con touch
- Scriptlet systemd per l'unità; /etc/ananicy.d è una directory del pacchetto
- Snapshot di master 3554447c (2026-08-18): compila con glibc 2.42 e clang 22

* Mon Aug 03 2026 Athanor Forge <forge@athanor.os> - 1.1.1-1
- Initial packaging
