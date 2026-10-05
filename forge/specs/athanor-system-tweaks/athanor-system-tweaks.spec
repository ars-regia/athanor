%global debug_package %{nil}
Name:           athanor-system-tweaks
Version:        1.0.0
Release:        6%{?dist}
Summary:        Athanor OS athanor-system-tweaks
License:        MIT
URL:            https://github.com/hr-mes/athanor-forge
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
mkdir -p %{buildroot}/etc/systemd/resolved.conf.d
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
cp -a %{_sourcedir}/etc/systemd/resolved.conf.d/99-dns-tls.conf %{buildroot}/etc/systemd/resolved.conf.d/
cp -a %{_sourcedir}/usr/lib/NetworkManager/conf.d/50-athanor-hostname.conf %{buildroot}/usr/lib/NetworkManager/conf.d/

%post

%files
%dir /usr/share/athanor-system-tweaks
%config(noreplace) /etc/NetworkManager/conf.d/99-mac-randomization.conf
%config(noreplace) /etc/systemd/resolved.conf.d/99-dns-tls.conf
/usr/lib/environment.d/10-athanor-wayland.conf
/usr/share/pipewire/pipewire.conf.d/10-low-latency.conf
/usr/share/polkit-1/rules.d/10-athanor-wheel-admin.rules
/usr/lib/sysctl.d/99-bore.conf
/usr/lib/sysctl.d/99-network-security.conf
/usr/lib/tmpfiles.d/99-azoth-sysfs.conf
/usr/lib/NetworkManager/conf.d/50-athanor-hostname.conf
%changelog
* Mon Oct 05 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- DNS-over-TLS becomes opportunistic. Strict mode resolved nothing on networks
  that block port 853.
- Ethernet keeps its hardware MAC address. A random wired address cut the
  Hyper-V test VM off the network; Wi-Fi stays random per connection.
- NetworkManager no longer sets the hostname from DHCP; the system uses
  DEFAULT_HOSTNAME from os-release until the user names the device.

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
