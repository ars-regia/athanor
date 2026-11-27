#!/usr/bin/env bash
# The scene's compositor. cosmic-comp is started by sway, so that it inherits sway's
# WAYLAND_DISPLAY and nests in it. With RIG_COMPOSITOR=athanor, scene.sh starts this script
# itself, and athanor-sulphur runs on its headless backend with the arguments given, one
# --output per virtual output (doc_own_stack.md, OS6).
set -euo pipefail
if [ "${RIG_COMPOSITOR:-cosmic}" = athanor ]; then
    # The allow-list of OS4 where the compositor reads it at start, so that a client run as its
    # unit (bar_session.py --unit, in-unit.sh) is granted what the image would grant it; the
    # rig's own lines (grim) follow the shipped ones, in the rig only, after a newline of their
    # own: a shipped list without a final newline would otherwise join its last line to the
    # rig's first. The compositor skips the blank line this can leave (identity.rs).
    install -d /usr/share/athanor/sulphur
    {
        cat /repo/forge/specs/athanor-sulphur/athanor-sulphur-1.0.0/data/privileged-clients
        echo
        cat /repo/forge/test/shell/rig-privileged-clients
    } > /usr/share/athanor/sulphur/privileged-clients
    chmod 0644 /usr/share/athanor/sulphur/privileged-clients
    # The compositor grants a line outside its transient ones only once the user's systemd says
    # the client is the main process of the line's unit, defined in the image, and the system's
    # systemd vouches for the user's (OS4, #1560). The rig has neither: a system bus at the fixed
    # path the compositor asks, and fake_systemd.py --origin on both buses, answer for the units
    # in-unit.sh and bar_session.py --unit make, from the list above.
    install -d /run/dbus
    printf '%s\n' '<busconfig><include>/usr/share/dbus-1/session.conf</include></busconfig>' > /tmp/rig-system-bus.conf
    dbus-daemon --config-file=/tmp/rig-system-bus.conf --nofork \
        --address=unix:path=/run/dbus/system_bus_socket &> "/out/${RIG_TAG:-scene}-system-bus.log" &
    ready=/tmp/rig-fake-systemd.ready
    for _ in $(seq 100); do
        [ -S /run/dbus/system_bus_socket ] && break
        sleep 0.1
    done
    python3 /repo/forge/test/shell/fake_systemd.py --origin /usr/share/athanor/sulphur/privileged-clients \
        "/out/${RIG_TAG:-scene}-systemd.log" > "$ready" 2> "/out/${RIG_TAG:-scene}-fake-systemd.log" &
    for _ in $(seq 100); do
        grep -qx ready "$ready" && break
        sleep 0.1
    done
    if ! grep -qx ready "$ready"; then
        echo "session.sh: fake_systemd.py --origin did not start; see /out/${RIG_TAG:-scene}-fake-systemd.log" >&2
        exit 1
    fi
    exec /out/bin/athanor-sulphur --headless "$@" &> "/out/${RIG_TAG:-scene}-compositor.log"
fi
export COSMIC_BACKEND=winit
exec cosmic-comp --no-xwayland &> "/out/${RIG_TAG:-scene}-cosmic-comp.log"
