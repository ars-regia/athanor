#!/usr/bin/env bash
# layout_session.sh [client...] - the stage 1c part of a session, in the order the user
# units give it (athanor-layout.service is Before=cosmic-panel.service): the translator
# applies the layout, then cosmic-panel starts on it. Without a client, the panel is the
# scene's process; with one, the panel runs beside it and the client is.
set -euo pipefail
log=/out/${RIG_TAG:-scene}
/out/bin/athanor-layout-translator &> "$log-translator.log" &
record=$XDG_STATE_HOME/athanor/layout-cosmic-panel
for _ in $(seq 40); do
    [ -e "$record" ] && break
    sleep 0.25
done
if [ ! -e "$record" ]; then
    echo "layout_session.sh: the translator wrote no configuration; see $log-translator.log" >&2
    exit 1
fi
if [ $# -eq 0 ]; then
    exec cosmic-panel
fi
cosmic-panel &> "$log-panel.log" &
exec "$@"
