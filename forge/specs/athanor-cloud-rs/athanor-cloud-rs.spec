%global debug_package %{nil}
Name:           athanor-cloud-rs
Version:        1.0.0
Release:        3%{?dist}
Summary:        Athanor OS Continuity & Local P2P Sync Daemon

License:        GPL-3.0-or-later
URL:            https://github.com/ars-regia/athanor
Requires:       wl-clipboard


BuildRequires:  rust cargo systemd-rpm-macros pkgconf-pkg-config openssl-devel

%description
Athanor OS Cloud Daemon for Universal Clipboard and Local P2P synchronization.

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
SRC_OS_ATHANOR_CLOUD_CONF=forge/specs/athanor-cloud-rs/athanor-cloud-rs-1.0.0/os.athanor.Cloud.conf
[ -f "$SRC_OS_ATHANOR_CLOUD_CONF" ] || SRC_OS_ATHANOR_CLOUD_CONF=os.athanor.Cloud.conf
install -D -m 0644 "$SRC_OS_ATHANOR_CLOUD_CONF" %{buildroot}%{_datadir}/dbus-1/system.d/os.athanor.Cloud.conf

# Install Polkit policy
SRC_OS_ATHANOR_CLOUD_POLICY=forge/specs/athanor-cloud-rs/athanor-cloud-rs-1.0.0/os.athanor.cloud.policy
[ -f "$SRC_OS_ATHANOR_CLOUD_POLICY" ] || SRC_OS_ATHANOR_CLOUD_POLICY=os.athanor.cloud.policy
install -D -m 0644 "$SRC_OS_ATHANOR_CLOUD_POLICY" %{buildroot}%{_datadir}/polkit-1/actions/os.athanor.cloud.policy

# Create a systemd service file
mkdir -p %{buildroot}/usr/lib/systemd/system
cat <<EOF > %{buildroot}/usr/lib/systemd/system/%{name}.service
[Unit]
Description=Athanor OS Continuity Daemon
After=network-online.target dbus.service graphical.target
Requires=dbus.service

[Service]
MemoryDenyWriteExecute=true
CPUWeight=50
CPUQuota=100%
MemoryHigh=384M
MemoryMax=512M
OOMScoreAdjust=100
CapabilityBoundingSet=CAP_NET_ADMIN
AmbientCapabilities=CAP_NET_ADMIN
Type=dbus

BusName=os.athanor.Cloud
ExecStart=/usr/bin/%{name}
Restart=always
RestartSec=5s
DynamicUser=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=true
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
WantedBy=graphical.target
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
%{_datadir}/dbus-1/system.d/os.athanor.Cloud.conf
%{_datadir}/polkit-1/actions/os.athanor.cloud.policy

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Point URL and the polkit vendor_url at the project repository

* Thu Jul 16 2026 Athanor <athanor@athanor.os> - 1.0.0-1
- Initial release

