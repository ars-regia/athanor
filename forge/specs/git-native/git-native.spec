Name:           git-native
Version:        2.56.0
Release:        1%{?dist}
Summary:        Assimilated Git Version Control System built natively from source for Athanor OS
License:        GPL-2.0-only
URL:            https://git-scm.com/
Source0:        git-2.56.0.tar.gz


Provides:       git = %{version}-%{release}
Provides:       git-core = %{version}-%{release}
Obsoletes:      git < %{version}-%{release}
Obsoletes:      git-core < %{version}-%{release}

BuildRequires:  gcc
BuildRequires:  make
BuildRequires:  gettext
BuildRequires:  curl-devel
BuildRequires:  expat-devel
BuildRequires:  openssl-devel
BuildRequires:  zlib-devel

%description
Git version control system compiled natively from source for Athanor OS Forge.

%prep
%autosetup -n git-%{version}

%build
%set_build_flags
make %{?_smp_mflags} prefix=/usr all

%install
mkdir -p %{buildroot}

rm -rf %{buildroot}
make %{?_smp_mflags} prefix=/usr DESTDIR=%{buildroot} install

%files
/usr/bin/git*
%{_libexecdir}/git-core
%{_datadir}/git-core

%changelog
* Sat Aug 08 2026 Athanor Forge <forge@athanor.os> - 2.48.1-1
- Assimilated git native source build for Athanor OS.

