#!/usr/bin/env bash
# in-unit.sh <unit> <binary> [args...]: runs <binary> as the rig's athanor-compositor sees a
# client of <unit>, the shell form of bar_session.py's as_unit. A binary outside /usr is copied
# to /usr/bin/<its name> first, under a temporary name renamed over the old one, so a respawn
# never writes a file a dying client still executes (ETXTBSY). The shell then joins the cgroup
# /sys/fs/cgroup/<unit>, made if it is missing (grim runs again in scene.sh's RIG_STABLE loop),
# and execs the binary from /usr/bin, so the pid stays the client's. The cgroup tree is
# writable only in the container's systemd mode, which rig.sh's in_rig sets with
# RIG_COMPOSITOR: callers use this script only under RIG_COMPOSITOR=athanor.
set -euo pipefail

unit=${1:?usage: in-unit.sh <unit> <binary> [args...]}
binary=${2:?usage: in-unit.sh <unit> <binary> [args...]}
shift 2
case $binary in
/usr/*) executable=$binary ;;
*)
    executable=/usr/bin/${binary##*/}
    cp -p "$binary" "/usr/bin/.${binary##*/}.new"
    mv -f "/usr/bin/.${binary##*/}.new" "$executable"
    ;;
esac
cgroup=/sys/fs/cgroup/$unit
mkdir -p "$cgroup"
echo $$ > "$cgroup/cgroup.procs"
exec "$executable" "$@"
