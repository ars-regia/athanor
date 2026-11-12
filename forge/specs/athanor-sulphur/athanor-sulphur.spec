# repo-input: system/athanor-sulphur-client/protocols/athanor-toplevel-state-v1.xml
# repo-input: system/athanor-sulphur-client/protocols/athanor-keyboard-group-v1.xml
# repo-input: system/athanor-sulphur-client/protocols/athanor-a11y-v1.xml
# repo-input: system/athanor-sulphur-client/protocols/athanor-workspace-tiling-v1.xml
# repo-input: system/athanor-sulphur-client/protocols/athanor-dock-overlap-v1.xml
# repo-input: system/athanor-sulphur-client/protocols/athanor-material-v1.xml
%global debug_package %{nil}
Name:           athanor-sulphur
Version:        1.0.0
Release:        23%{?athanor_content}%{?dist}
Summary:        Athanor's own Wayland compositor and its session
License:        GPL-3.0-or-later
Obsoletes:      athanor-compositor < %{version}-%{release}
Provides:       athanor-compositor = %{version}-%{release}

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
default session "Athanor" (doc_session.md, SN14 and SN15): athanor-sulphur-session starts
it as the user unit athanor-sulphur.service, which starts athanor-session.target once
the display is ready. The allow-list of the clients granted its privileged globals is
/usr/share/athanor/sulphur/privileged-clients. cosmic-comp's session stays the second
entry, and takes the sign-in when this one cannot start.

%prep

%build
%set_build_flags
cargo build --release --locked -p %{name}

%install
install -D -m 0755 target/release/athanor-sulphur %{buildroot}/usr/bin/athanor-sulphur
install -D -m 0755 forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/data/athanor-sulphur-session \
    %{buildroot}/usr/bin/athanor-sulphur-session
install -D -m 0644 forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/data/athanor-sulphur.service \
    %{buildroot}/usr/lib/systemd/user/athanor-sulphur.service
install -D -m 0644 forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/data/privileged-clients \
    %{buildroot}/usr/share/athanor/sulphur/privileged-clients

%files
/usr/bin/athanor-sulphur
/usr/bin/athanor-sulphur-session
/usr/lib/systemd/user/athanor-sulphur.service
%dir /usr/share/athanor
%dir /usr/share/athanor/sulphur
/usr/share/athanor/sulphur/privileged-clients

%changelog
* Sat Nov 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-23
- Keep the name of a withdrawn wl_output global reserved for five seconds, as wlroots does:
  the global is disabled, which sends global_remove, and destroyed only after the grace, so a
  client that bound the old name before it read the removal is not given another output or
  another interface.

* Sat Nov 07 2026 Athanor Forge <forge@athanor.os> - 1.0.0-22
- Apply modes, positions, enabling and disabling and adaptive sync through
  wlr-output-management, besides scales and transforms: a configuration is checked whole
  before any of it is applied, one the outputs cannot take or that leaves no output enabled
  fails and changes nothing, a step the GPU refuses undoes those taken, and every manager is
  sent the result (doc_session_daemons.md SD23).
- Keep what the user configured: an output given no position keeps its own, and a monitor
  whose modes change while it stays plugged in keeps its place and, while it still offers
  it, its mode.
- Keep the modes of a disabled monitor's head up to date when its connector's modes change.
- Refuse an output configuration that places an output more than 2^24 pixels from the
  origin, past which the sums of the space would overflow.
- Place an output enabled again at the origin when the right of the others would take it
  past 2^24 pixels.
- Give the keyboard to a lock surface still shown when the output whose lock surface had it
  is disabled.
- Check a tested output configuration as an applied one, as far as the backend can tell
  before applying it: adaptive sync on an output without it, and any step on the nested
  output, fail both; a step a monitor refuses at apply still fails and undoes the others.
- Confirm the lock only once every step of an output configuration is taken, not while
  they are taken.
- Keep an output the power client turned off off when a refused configuration drives it
  again.
- Refuse a layer surface, a lock surface and a power control asked for on a stale wl_output
  of an output no longer shown, and give a full screen asked for on one the window's own
  output.
- Withdraw the global an output still has before it is offered again, so that no client
  sees two globals for one output.
* Fri Nov 06 2026 Athanor Forge <forge@athanor.os> - 1.0.0-21
- Report a monitor's make, model and serial from the EDID blob of its DRM connector, read
  with athanor-edid: wl_output and xdg_output carry the make and model, and the serial
  reaches only wlr-output-management; a monitor with no valid EDID keeps make Athanor, model
  DRM and an empty serial (doc_osd.md OD4).

* Thu Nov 05 2026 Athanor Forge <forge@athanor.os> - 1.0.0-20
- At a fractional scale a client's opaque region keeps only the output's pixels it covers
  whole, for windows, popups, layer surfaces, the lock surface and the cursor: a pixel the
  region covers by half, which the client antialiases, is blended with what lies beneath it
  instead of being drawn over nothing (#978).

* Thu Nov 05 2026 Athanor Forge <forge@athanor.os> - 1.0.0-19
- Clear the outputs to cosmic-comp's clear colour, srgb(39,41,42), in place of Calmo's
  dark bg1, so that the shell's surfaces match the cosmic-comp goldens where no surface
  is drawn (#736).

* Wed Nov 04 2026 Athanor Forge <forge@athanor.os> - 1.0.0-18
- Rename the package, its binaries, its user unit and its session script from
  athanor-compositor to athanor-sulphur, and its data directory from
  /usr/share/athanor/compositor to /usr/share/athanor/sulphur (ADR-0134). The package
  obsoletes and provides athanor-compositor, so that an installed system updates to it.

* Wed Nov 04 2026 Athanor Forge <forge@athanor.os> - 1.0.0-17
- On a tiled workspace a dialog, or a window of one size, floats above the tiles instead
  of taking one, and a tile dropped on another swaps places with it; a client's resize of
  a tile moves the split beside the edge it moved, no further than the area (#894).

* Wed Nov 04 2026 Athanor Forge <forge@athanor.os> - 1.0.0-16
- A confined pointer keeps its confinement, a oneshot one included, when it moves over a
  subsurface of its window: the window keeps the pointer's focus, and the pointer stays on
  the window's own surface and stops at the edge of its input region, or the region's,
  instead of short of it; a confinement that activates over a subsurface gives the window
  the pointer's focus at once (#860).

* Wed Nov 04 2026 Athanor Forge <forge@athanor.os> - 1.0.0-15
- Tile the windows of a workspace while its tiling is on, each new window beside the one
  focused last, with gaps of 8, and return each to its floating place when it is turned
  off (doc_own_stack.md OS12, #894). athanor_workspace_tiling_v1, granted to the bar,
  reads and sets the state of each workspace.

* Sun Nov 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-14
- A frame that a refused mode test forgot, whose VBlank still comes, no longer counts as
  a lock frame on screen (#813).

* Sun Nov 01 2026 Athanor Forge <forge@athanor.os> - 1.0.0-13
- The session's end removes only the names of the display from the user manager, as the
  readiness gate does (doc_session.md, SN2 and SN15, #825).

* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-12
- Apply sticky, slow and bounce keys in front of every key's reader, the bindings, the
  focused client and the keyboard monitor, slow and bounce keys to the hardware keyboards
  only and sticky keys to a virtual keyboard's keys too; the protected keys pass slow and
  bounce keys, and nothing is told while the session is locked or a
  trusted prompt has the keyboard (doc_accessibility.md AX11, #865). athanor_a11y_v1 sets
  them and tells the modifiers held, a key rejected, a gesture and an aid turned off
- Bind Super+A to the launcher until athanor-library exists, so that Super then A opens it
  under sticky keys (doc_accessibility.md AX11, #865)

* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-11
- athanor-keyboard-group-v1 version 2 names the seat's layouts, which a virtual keyboard's
  keymap sent to every wl_keyboard no longer hides from the bar: Tab reaches past the input
  source under wtype (#876)

* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-10
- Grant the output manager to cosmic-randr in the units of Settings, which shows no window
  while it waits on it (#880)

* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-9
- The magnifier and the colour filters (doc_accessibility.md AX10, revision 3): the
  privileged athanor_a11y_manager_v1 for the bar, Super+= and Super+- to zoom, Super with
  the scroll wheel when accessibility_zoom allows it, a view that follows the pointer and any
  keyboard focus the pointer did not move, the colour inversion and filters on
  each output's frame, and cosmic-comp's a11y_screen_filter.ron read at start and written
  at each change, so both sessions show the same state. athanor-compositor-session no longer
  starts cosmic-comp's session for the magnifier or a filter, and --screen-filter-on is gone.

* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-8
- Serve the keyboard monitor in a `--headless` run of the binary too, so that Orca's own
  commands reach Orca in the shell rig's scenes (#841, #863)

* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-7
- Stack a toplevel with a parent, set through xdg-foreign or xdg_toplevel.set_parent, above
  that parent and its other dialogs on its workspace, and give it the keyboard at its first
  placement when the parent's own surface holds it (#872). The exported handle is the
  exporter's grant: its holder may keep a dialog above the exporter's window while the handle
  lives, and a new window parented through it takes the keyboard from the parent's surface when
  it is placed, which the portal's file chooser needs. Every focus now raises the focused
  window's dialogs with it, on all focus paths.

* Fri Oct 30 2026 Athanor Forge <forge@athanor.os> - 1.0.0-6
- Take a window's place as its geometry's top left in restore, re-centring and reachability,
  so that a client-side shadow is not applied a second time and a restored window no longer
  drifts by the shadow (#853)

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
