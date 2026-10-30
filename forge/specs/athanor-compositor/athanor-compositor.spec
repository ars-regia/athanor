# repo-input: system/athanor-compositor-client/protocols/athanor-toplevel-state-v1.xml
# repo-input: system/athanor-compositor-client/protocols/athanor-keyboard-group-v1.xml
# repo-input: system/athanor-compositor-client/protocols/athanor-a11y-v1.xml
%global debug_package %{nil}
Name:           athanor-compositor
Version:        1.0.0
Release:        6%{?dist}
Summary:        Athanor's own Wayland compositor and its session
License:        GPL-3.0-or-later

BuildRequires:  rust cargo gcc pkgconf-pkg-config systemd-devel libxkbcommon-devel mesa-libgbm-devel pixman-devel libseat-devel libinput-devel
# The renderer loads libEGL at run time; the session script calls systemctl, loginctl,
# busctl and gsettings, starts athanor-session.target (athanor-system-services), and sources
# the supervisor of the locker and the idle daemon, the reader of the import list and the
# user's language, region and cursor, reads that list, runs the USBGuard hook and hands the
# sign-in to athanor-session (athanor-system-config).
Requires:       systemd glib2 mesa-libEGL mesa-dri-drivers athanor-system-services
# 74 ships the supervisor, the reader of the import list and the user's environment it sources.
Requires:       athanor-system-config >= 1.0.0-74
# The compositor starts xwayland-satellite on the first X11 client (doc_own_stack.md, OS4).
Requires:       xwayland-satellite

%description
Athanor's own Wayland compositor on Smithay (doc_own_stack.md, milestone C1), the greeter's
default session "Athanor" (doc_session.md, SN14 and SN15): athanor-compositor-session starts
it as the user unit athanor-compositor.service, which starts athanor-session.target once
the display is ready. The allow-list of the clients granted its privileged globals is
/usr/share/athanor/compositor/privileged-clients. cosmic-comp's session stays the second
entry, and takes the sign-in when this one cannot start.

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
* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- The magnifier and the colour filters (doc_accessibility.md AX10, revision 3): the
  privileged athanor_a11y_manager_v1 for the bar, Super+= and Super+- to zoom, Super with
  the scroll wheel when accessibility_zoom allows it, the colour inversion and filters on
  each output's frame, and cosmic-comp's a11y_screen_filter.ron read at start and written
  at each change, so both sessions show the same state. athanor-compositor-session no longer
  starts cosmic-comp's session for the magnifier or a filter, and --screen-filter-on is gone.

* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-5
- Serve org.freedesktop.a11y.KeyboardMonitor to Orca alone, admitted at every call by its
  unit and executable, so that Orca's own commands reach it, and send it every key but the
  protected ones (doc_accessibility.md AX4, #863)

* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-4
- Tablets, pointer gestures, xdg-decoration and xdg-foreign (decision 30): drawing tablets,
  their tools and their cursors; swipe, pinch and hold to the focused surface; every window
  decorated by its client; an exported toplevel imported as the parent of another, valid
  while the exported toplevel lives.

* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-3
- Pointer constraints, relative pointer motion and keyboard-shortcuts inhibitors for
  every client, under doc_own_stack.md decision 30: a lock or a confinement holds only
  while its surface has the keyboard, the user's way out stays the compositor's under an
  inhibitor, and Ctrl+Alt+Tab suspends the focused client's until a click or a key gives
  it the keyboard back.

* Thu Oct 29 2026 Athanor Forge <forge@athanor.os> - 1.0.0-2
- Offer primary selection to every client, following the keyboard focus, and data control
  (ext and wlr) to the clients the allow-list grants it, which grants it to none yet
  (doc_own_stack.md decision 30)

* Thu Oct 22 2026 Athanor Forge <forge@athanor.os> - 1.0.0-1
- First package (doc_session.md revision 4, SN13 step 6): the compositor, its user unit,
  the session's script, the greeter's default, which hands the sign-in to cosmic-comp's
  session when it cannot start, and the allow-list of the bar, the dock, the control
  center, the launcher, the locker, the idle daemon and cosmic-osd.
