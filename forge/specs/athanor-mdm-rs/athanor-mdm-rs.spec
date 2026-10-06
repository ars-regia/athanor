%global debug_package %{nil}
Name:           athanor-mdm-rs
Version:        1.0.0
Release:        3%{?dist}
Summary:        Athanor OS Device Policy Daemon

License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor
Requires:       polkit cryptsetup systemd


BuildRequires:  rust cargo systemd-rpm-macros pkgconf-pkg-config openssl-devel

%description
Athanor OS MDM Daemon. Applies pushed device policies (USB storage lock and forced
VPN) through a polkit-gated D-Bus interface. It does not track devices and exposes no
remote wipe action.

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
SRC_OS_ATHANOR_MDM_CONF=forge/specs/athanor-mdm-rs/athanor-mdm-rs-1.0.0/os.athanor.Mdm.conf
[ -f "$SRC_OS_ATHANOR_MDM_CONF" ] || SRC_OS_ATHANOR_MDM_CONF=os.athanor.Mdm.conf
install -D -m 0644 "$SRC_OS_ATHANOR_MDM_CONF" %{buildroot}%{_datadir}/dbus-1/system.d/os.athanor.Mdm.conf

# Install Polkit policy
SRC_OS_ATHANOR_MDM_POLICY=forge/specs/athanor-mdm-rs/athanor-mdm-rs-1.0.0/os.athanor.mdm.policy
[ -f "$SRC_OS_ATHANOR_MDM_POLICY" ] || SRC_OS_ATHANOR_MDM_POLICY=os.athanor.mdm.policy
install -D -m 0644 "$SRC_OS_ATHANOR_MDM_POLICY" %{buildroot}%{_datadir}/polkit-1/actions/os.athanor.mdm.policy

# Create a systemd service file
mkdir -p %{buildroot}/usr/lib/systemd/system
cat <<EOF > %{buildroot}/usr/lib/systemd/system/%{name}.service
[Unit]
Description=Athanor OS Anti-Theft & MDM Daemon
After=network-online.target dbus.service
Requires=dbus.service

[Service]
CPUWeight=50
MemoryHigh=96M
MemoryMax=128M
OOMScoreAdjust=-100
Type=dbus
BusName=os.athanor.Mdm
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
%{_datadir}/dbus-1/system.d/os.athanor.Mdm.conf
%{_datadir}/polkit-1/actions/os.athanor.mdm.policy

%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Point URL and the polkit vendor_url at the project repository
- Correct Summary and %description: the daemon applies USB and VPN policies only
- Require authentication on every device wipe (auth_admin, no cached grant)

* Thu Jul 16 2026 Athanor <athanor@athanor.os> - 1.0.0-1
- Initial release

