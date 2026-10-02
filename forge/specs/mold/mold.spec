Name:           mold
Version:        2.42.1
Release:        1%{?dist}
Summary:        Athanor OS Core Component - mold

License:        GPLv3
URL:            https://github.com/rui314/mold
Source0:        mold-2.42.1.tar.gz

%description
Core component implementation for mold.

%prep
%autosetup -n %{name}-%{version}

%build
cmake -B build -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_COMPILER=g++ -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build %{?_smp_mflags}

%install
rm -rf %{buildroot}
DESTDIR=%{buildroot} cmake --install build

%files
/usr/bin/mold

