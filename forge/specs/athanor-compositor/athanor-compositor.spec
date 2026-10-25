# repo-input: system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml
%global debug_package %{nil}
Name:           athanor-compositor
Version:        1.0.0
Release:        1%{?dist}
Summary:        Athanor's own Wayland compositor, as the preview session
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config systemd-devel libxkbcommon-devel mesa-libgbm-devel pixman-devel libseat-devel libinput-devel
# The renderer loads libEGL at run time; the session script calls systemctl, loginctl and
# busctl, starts athanor-session.target (athanor-system-services) and runs the USBGuard hook
# (athanor-system-config).
Requires:       systemd mesa-libEGL mesa-dri-drivers athanor-system-services athanor-system-config

%description
Athanor's own Wayland compositor on Smithay (doc_own_stack.md, milestone C1), offered by
the greeter as "Athanor on the new compositor (preview)" (doc_session.md, SN14 and SN15):
athanor-compositor-session starts it as the user unit athanor-compositor.service, which
starts athanor-session.target once the display is ready. The allow-list of the clients
granted its privileged globals is /usr/share/athanor/compositor/privileged-clients. The
default session stays on cosmic-comp.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-compositor %{buildroot}/usr/bin/athanor-compositor
install -D -m 0755 forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/athanor-compositor-session \
    %{buildroot}/usr/bin/athanor-compositor-session
install -D -m 0644 forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/athanor-compositor.service \
    %{buildroot}/usr/lib/systemd/user/athanor-compositor.service
install -D -m 0644 forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients \
    %{buildroot}/usr/share/athanor/compositor/privileged-clients

%files
/usr/bin/athanor-compositor
/usr/bin/athanor-compositor-session
/usr/lib/systemd/user/athanor-compositor.service
%dir /usr/share/athanor
%dir /usr/share/athanor/compositor
/usr/share/athanor/compositor/privileged-clients

%changelog
* Thu Oct 22 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First package (doc_session.md, SN13 step 6): the compositor, its user unit, the preview
  session's script and the allow-list of the bar, the dock, the control center and the
  launcher.
