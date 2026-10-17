#!/usr/bin/env bash
# Started by sway, so that the nested compositor inherits sway's WAYLAND_DISPLAY and nests in
# it: cosmic-comp, or athanor-compositor with RIG_COMPOSITOR=athanor (doc_own_stack.md, C1).
if [ "${RIG_COMPOSITOR:-cosmic}" = athanor ]; then
    exec /out/bin/athanor-compositor --scale "${RIG_SCALE:-1}" &> "/out/${RIG_TAG:-scene}-compositor.log"
fi
export COSMIC_BACKEND=winit
exec cosmic-comp --no-xwayland &> "/out/${RIG_TAG:-scene}-cosmic-comp.log"
