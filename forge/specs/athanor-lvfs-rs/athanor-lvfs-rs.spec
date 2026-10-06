%global debug_package %{nil}
Name:           athanor-lvfs-rs
Version:        1.0.0
Release:        2%{?dist}
Summary:        Athanor OS Firmware Automation Daemon

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor


BuildRequires:  rust cargo systemd-rpm-macros pkgconf-pkg-config openssl-devel
Requires:       fwupd

%description
Athanor OS LVFS Daemon for automated background UEFI/BIOS firmware updates via fwupdmgr.

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
SRC_OS_ATHANOR_LVFS_CONF=forge/specs/athanor-lvfs-rs/athanor-lvfs-rs-1.0.0/os.athanor.Lvfs.conf
[ -f "$SRC_OS_ATHANOR_LVFS_CONF" ] || SRC_OS_ATHANOR_LVFS_CONF=os.athanor.Lvfs.conf
install -D -m 0644 "$SRC_OS_ATHANOR_LVFS_CONF" %{buildroot}%{_datadir}/dbus-1/system.d/os.athanor.Lvfs.conf

# Install Polkit policy
SRC_OS_ATHANOR_LVFS_POLICY=forge/specs/athanor-lvfs-rs/athanor-lvfs-rs-1.0.0/os.athanor.lvfs.policy
[ -f "$SRC_OS_ATHANOR_LVFS_POLICY" ] || SRC_OS_ATHANOR_LVFS_POLICY=os.athanor.lvfs.policy
install -D -m 0644 "$SRC_OS_ATHANOR_LVFS_POLICY" %{buildroot}%{_datadir}/polkit-1/actions/os.athanor.lvfs.policy

# Create a systemd service file
mkdir -p %{buildroot}/usr/lib/systemd/system
cat <<EOF > %{buildroot}/usr/lib/systemd/system/%{name}.service
[Unit]
Description=Athanor OS Firmware Automation Daemon
After=network-online.target dbus.service fwupd.service
Requires=dbus.service fwupd.service

[Service]
MemoryDenyWriteExecute=true
CPUWeight=30
CPUQuota=50%
MemoryHigh=192M
MemoryMax=256M
OOMScoreAdjust=300
CapabilityBoundingSet=CAP_SYS_ADMIN
AmbientCapabilities=CAP_SYS_ADMIN
Type=dbus

BusName=os.athanor.Lvfs
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
%{_datadir}/dbus-1/system.d/os.athanor.Lvfs.conf
%{_datadir}/polkit-1/actions/os.athanor.lvfs.policy

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Point URL and the polkit vendor_url at the project repository

* Thu Jul 16 2026 Athanor <athanor@athanor.os> - 1.0.0-1
- Initial release

