%global debug_package %{nil}

Name:           athanor-cliphist
Version:        0.7.0
Release:        3%{?dist}
Summary:        Wayland clipboard manager

License:        GPL-3.0-only
URL:            https://github.com/sentriz/cliphist
Source0:        https://github.com/sentriz/cliphist/archive/refs/tags/v%{version}.tar.gz#/cliphist-%{version}.tar.gz

BuildRequires:  golang
Requires:       wl-clipboard
Provides:       cliphist = %{version}-%{release}

%description
Wayland clipboard manager: keeps a history of what wl-paste reports and
serves it back to pickers. Built from the upstream release with the modules
pinned by its go.sum.

%prep
%autosetup -n cliphist-%{version}

%build
# The build cache lives in the build tree; the modules come from the default module cache,
# which forge/scripts/build_spec.sh fills before the build runs without network, and are
# verified against go.sum (readonly mode, the default). Static binary: no C dependency.
export GOCACHE="$PWD/.gocache" GOFLAGS="-trimpath" CGO_ENABLED=0
go build -ldflags '-s -w' -o cliphist .

%install
install -D -m 0755 cliphist %{buildroot}%{_bindir}/cliphist

%files
%license LICENSE
%{_bindir}/cliphist

%changelog
* Mon Oct 05 2026 Athanor Forge <forge@athanor.os> - 0.7.0-3
- Take the Go modules from the default module cache, which the forge fills before
  the build runs without network

* Thu Sep 03 2026 Athanor Forge <forge@athanor.os> - 0.7.0-2
- Spec riscritta: Source0 dall'archivio upstream verificato da SOURCES/sources.sha256,
  build del modulo Go estratto invece di `go build` nella radice del repo seguito da
  un `touch cliphist` che spediva un binario vuoto
- Requires wl-clipboard, di cui cliphist invoca wl-paste a runtime

* Mon Aug 03 2026 Athanor Forge <forge@athanor.os> - 0.7.0-1
- Initial packaging
