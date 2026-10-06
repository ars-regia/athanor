%global debug_package %{nil}
Name:           athanor-system-tweaks
Version:        1.0.0
Release:        7%{?dist}
Summary:        Athanor OS athanor-system-tweaks
License:        GPL-3.0-or-later
URL:            https://github.com/hr-mes/athanor
BuildArch:      noarch

%description
Provides athanor-system-tweaks for Athanor OS.

%prep
# Nothing to prep

%build
# Nothing to build

%install
mkdir -p %{buildroot}/usr/share/athanor-system-tweaks
mkdir -p %{buildroot}/usr/lib/environment.d
mkdir -p %{buildroot}/usr/share/pipewire/pipewire.conf.d
mkdir -p %{buildroot}/etc/NetworkManager/conf.d
mkdir -p %{buildroot}/usr/lib/NetworkManager/conf.d
mkdir -p %{buildroot}/usr/lib/systemd/resolved.conf.d
mkdir -p %{buildroot}/usr/share/polkit-1/rules.d
mkdir -p %{buildroot}/usr/lib/sysctl.d
mkdir -p %{buildroot}/usr/lib/tmpfiles.d
cp -a %{_sourcedir}/usr/lib/environment.d/10-athanor-wayland.conf %{buildroot}/usr/lib/environment.d/
cp -a %{_sourcedir}/usr/share/pipewire/pipewire.conf.d/10-low-latency.conf %{buildroot}/usr/share/pipewire/pipewire.conf.d/
cp -a %{_sourcedir}/etc/polkit-1/rules.d/10-athanor-wheel-admin.rules %{buildroot}/usr/share/polkit-1/rules.d/
cp -a %{_sourcedir}/usr/lib/sysctl.d/99-bore.conf %{buildroot}/usr/lib/sysctl.d/
cp -a %{_sourcedir}/usr/lib/sysctl.d/99-network-security.conf %{buildroot}/usr/lib/sysctl.d/
cp -a %{_sourcedir}/usr/lib/tmpfiles.d/99-azoth-sysfs.conf %{buildroot}/usr/lib/tmpfiles.d/
cp -a %{_sourcedir}/etc/NetworkManager/conf.d/99-mac-randomization.conf %{buildroot}/etc/NetworkManager/conf.d/
cp -a %{_sourcedir}/usr/lib/systemd/resolved.conf.d/50-athanor-dns.conf %{buildroot}/usr/lib/systemd/resolved.conf.d/
cp -a %{_sourcedir}/usr/lib/NetworkManager/conf.d/50-athanor-hostname.conf %{buildroot}/usr/lib/NetworkManager/conf.d/

%post

%files
%dir /usr/share/athanor-system-tweaks
%config(noreplace) /etc/NetworkManager/conf.d/99-mac-randomization.conf
/usr/lib/systemd/resolved.conf.d/50-athanor-dns.conf
/usr/lib/environment.d/10-athanor-wayland.conf
/usr/share/pipewire/pipewire.conf.d/10-low-latency.conf
/usr/share/polkit-1/rules.d/10-athanor-wheel-admin.rules
/usr/lib/sysctl.d/99-bore.conf
/usr/lib/sysctl.d/99-network-security.conf
/usr/lib/tmpfiles.d/99-azoth-sysfs.conf
/usr/lib/NetworkManager/conf.d/50-athanor-hostname.conf
%changelog
* Tue Oct 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-7
- Point URL at the project repository

* Mon Oct 05 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- DNS: strict DNS over TLS to Quad9 (9.9.9.9, 149.112.112.112, 2620:fe::fe,
  2620:fe::9, TLS name dns.quad9.net), routing domain "~." so the global resolver
  wins over DHCP DNS, DNSSEC=allow-downgrade. The file moves to
  /usr/lib/systemd/resolved.conf.d/50-athanor-dns.conf so an administrator
  overrides it with a later-sorting drop-in in /etc/systemd/resolved.conf.d/.
- NetworkManager pushes link DNS to systemd-resolved (dns=systemd-resolved). With
  "~." link and VPN DNS servers serve only their own domains (a VPN uses a
  route-only search domain or a negative dns-priority). DNS over TLS stays strict for that
  server, so it needs DoT with a valid certificate or connection.dns-over-tls set
  to opportunistic/no on the VPN profile (untested on a real VPN). Strict DNS over TLS needs
  a correct clock. NetworkManager's connectivity check cannot detect a captive
  portal that blocks port 853 (tracked in #168); see 50-athanor-hostname.conf.
- Wi-Fi uses wifi.cloned-mac-address=stable-ssid. Ethernet keeps its hardware MAC
  address; a random wired address cut the Hyper-V test VM off the network.
- NetworkManager no longer sends or accepts a hostname over DHCP
  (dhcp-send-hostname=0 for IPv4 and IPv6, hostname-mode=none); the system
  uses DEFAULT_HOSTNAME from os-release until the user names the device.

* Thu Sep 17 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Stop setting net.ipv4.tcp_congestion_control = bbr in 99-bore.conf. Azoth builds
  BBRv3 in as the default congestion control; the setting loaded tcp_bbr.ko and
  replaced BBRv3 with BBRv1.

* Fri Jul 10 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Added native sysctl tuning for BORE scheduler (99-bore.conf)

* Wed Jul 08 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Add Wayland environment variables and PipeWire low latency config
* Wed Jul 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- Initial Bedrock encapsulation
