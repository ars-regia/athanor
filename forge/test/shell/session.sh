#!/usr/bin/env bash
# Started by sway, so that the nested compositor inherits sway's WAYLAND_DISPLAY and nests in
# it: cosmic-comp, or athanor-compositor with RIG_COMPOSITOR=athanor (doc_own_stack.md, C1).
set -euo pipefail
if [ "${RIG_COMPOSITOR:-cosmic}" = athanor ]; then
    # The allow-list of OS4 where the compositor reads it at start, so that a client run as its
    # unit runs it (bar_session.py --unit) is granted what the image would grant it.
    install -D -m 0644 /repo/forge/specs/athanor-compositor/athanor-compositor-1.0.0/data/privileged-clients \
        /usr/share/athanor/compositor/privileged-clients
    exec /out/bin/athanor-compositor --scale "${RIG_SCALE:-1}" &> "/out/${RIG_TAG:-scene}-compositor.log"
fi
export COSMIC_BACKEND=winit
exec cosmic-comp --no-xwayland &> "/out/${RIG_TAG:-scene}-cosmic-comp.log"
