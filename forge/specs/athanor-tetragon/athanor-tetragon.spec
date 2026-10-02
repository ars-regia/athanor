Name:           athanor-tetragon
Version:        1.7.1
Release:        1%{?dist}
Summary:        Cilium Tetragon eBPF Runtime Security

License:        Apache-2.0
URL:            https://github.com/cilium/tetragon
Source0:        tetragon-v1.7.1-amd64.tar.gz
Source1:        tetragon.service
Source2:        tetragon.yaml

BuildRequires:  tar
Requires:       systemd

%description
Cilium Tetragon eBPF Runtime Security engine, packaged for Athanor OS.

%prep
%setup -q -c

%build
# Offline hermetic build using pre-fetched Source0 tarball

%install
mkdir -p %{buildroot}/usr/bin
mkdir -p %{buildroot}%{_sharedstatedir}/tetragon
mkdir -p %{buildroot}/etc/tetragon/tetragon.tp.d
mkdir -p %{buildroot}/usr/lib/systemd/system

# Copy binaries
install -m 0755 tetragon-v%{version}-amd64/usr/local/bin/tetragon %{buildroot}/usr/bin/tetragon
install -m 0755 tetragon-v%{version}-amd64/usr/local/bin/tetra %{buildroot}/usr/bin/tetra

# Bytecode eBPF: tetragon.yaml punta bpf-lib a questa directory
cp -r tetragon-v%{version}-amd64/usr/local/lib/tetragon/bpf %{buildroot}%{_sharedstatedir}/tetragon/

# Unità systemd e configurazione del daemon. Le tracing policy sono iniettate a
# runtime dal policy injector: tetragon.tp.d resta vuota e di proprietà del pacchetto.
install -m 0644 %{SOURCE1} %{buildroot}/usr/lib/systemd/system/tetragon.service
install -m 0644 %{SOURCE2} %{buildroot}/etc/tetragon/tetragon.yaml

%post
%systemd_post tetragon.service

%preun
%systemd_preun tetragon.service

%postun
%systemd_postun_with_restart tetragon.service

%files
/usr/bin/tetragon
/usr/bin/tetra
%{_sharedstatedir}/tetragon/bpf/
/usr/lib/systemd/system/tetragon.service
%config(noreplace) /etc/tetragon/tetragon.yaml
%dir /etc/tetragon/tetragon.tp.d

%changelog
* Thu Sep 03 2026 Athanor Forge <forge@athanor.os> - 1.3.0-2
- Dichiara tetragon.service e tetragon.yaml come Source1/Source2
- Configurazione del daemon in YAML, il formato che tetragon legge da /etc/tetragon/tetragon.yaml
- Rimossa la policy statica sys_execve.yaml, sostituita dal policy injector a runtime
- La directory tetragon.tp.d appartiene al pacchetto

* Mon Aug 03 2026 Athanor Forge <forge@athanor.os> - 1.3.0-1
- Initial release for Athanor OS
