#!/usr/bin/env bash
# The scene's compositor. cosmic-comp is started by sway, so that it inherits sway's
# WAYLAND_DISPLAY and nests in it. With RIG_COMPOSITOR=athanor, scene.sh starts this script
# itself, and athanor-compositor runs on its headless backend with the arguments given, one
# --output per virtual output (doc_own_stack.md, OS6).
set -euo pipefail
if [ "${RIG_COMPOSITOR:-cosmic}" = athanor ]; then
    # The allow-list of OS4 where the compositor reads it at start, so that a client run as its
    # unit (bar_session.py --unit, in-unit.sh) is granted what the image would grant it; the
    # rig's own lines (grim) follow the shipped ones, in the rig only.
    install -d /usr/share/athanor/compositor
    cat /repo/forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients \
        /repo/forge/test/shell/rig-privileged-clients > /usr/share/athanor/compositor/privileged-clients
    chmod 0644 /usr/share/athanor/compositor/privileged-clients
    exec /out/bin/athanor-compositor --headless "$@" &> "/out/${RIG_TAG:-scene}-compositor.log"
fi
export COSMIC_BACKEND=winit
exec cosmic-comp --no-xwayland &> "/out/${RIG_TAG:-scene}-cosmic-comp.log"
