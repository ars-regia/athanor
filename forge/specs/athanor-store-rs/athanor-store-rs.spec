%global debug_package %{nil}
Name:           athanor-store-rs
Version:        1.0.0
Release:        4%{?dist}
Summary:        Athanor OS Universal App Store Daemon


License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor-forge


BuildRequires:  rust cargo systemd-rpm-macros pkgconf-pkg-config openssl-devel gtk4-devel

Requires:       flatpak

%description
Athanor OS Universal App Store Daemon for Flatpak and OCI container management.

%prep
# Stub prep

%build
%set_build_flags
# cargo generate-lockfile // FORBIDDEN BY RULE 4 (Offline Build)
cargo build --release --locked -p %{name}

%install
mkdir -p %{buildroot}

install -D -m 0755 target/release/%{name} %{buildroot}/usr/bin/%{name}

# Install D-Bus system configuration
SRC_OS_ATHANOR_STORE_CONF=forge/specs/athanor-store-rs/athanor-store-rs-1.0.0/os.athanor.Store.conf
[ -f "$SRC_OS_ATHANOR_STORE_CONF" ] || SRC_OS_ATHANOR_STORE_CONF=os.athanor.Store.conf
install -D -m 0644 "$SRC_OS_ATHANOR_STORE_CONF" %{buildroot}%{_datadir}/dbus-1/system.d/os.athanor.Store.conf

# Install Polkit policy
SRC_OS_ATHANOR_STORE_POLICY=forge/specs/athanor-store-rs/athanor-store-rs-1.0.0/os.athanor.store.policy
[ -f "$SRC_OS_ATHANOR_STORE_POLICY" ] || SRC_OS_ATHANOR_STORE_POLICY=os.athanor.store.policy
install -D -m 0644 "$SRC_OS_ATHANOR_STORE_POLICY" %{buildroot}%{_datadir}/polkit-1/actions/os.athanor.store.policy

# Create a systemd service file
mkdir -p %{buildroot}/usr/lib/systemd/system
cat <<EOF > %{buildroot}/usr/lib/systemd/system/%{name}.service
[Unit]
Description=Athanor OS Universal App Store Daemon
After=network-online.target dbus.service
Requires=dbus.service

[Service]
CPUWeight=50
MemoryHigh=512M
MemoryMax=768M
OOMScoreAdjust=200
Type=dbus
BusName=os.athanor.Store
ExecStart=/usr/bin/%{name}
Restart=always
RestartSec=5s
DynamicUser=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=true
MemoryDenyWriteExecute=true
NoNewPrivileges=yes
SystemCallFilter=@system-service
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectKernelLogs=true
ProtectControlGroups=true
RestrictRealtime=true
RestrictSUIDSGID=true
LockPersonality=true

[Install]
WantedBy=multi-user.target
EOF

%post
%systemd_post %{name}.service

%preun
%systemd_preun %{name}.service

%postun
%systemd_postun_with_restart %{name}.service

%files
/usr/bin/%{name}
/usr/lib/systemd/system/%{name}.service
%{_datadir}/dbus-1/system.d/os.athanor.Store.conf
%{_datadir}/polkit-1/actions/os.athanor.store.policy

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- Point the polkit vendor_url at the project repository.

* Thu Jul 16 2026 Athanor <athanor@athanor.os> - 1.0.0-1
- Initial release

