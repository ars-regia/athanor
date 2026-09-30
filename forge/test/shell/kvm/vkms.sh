#!/usr/bin/env bash
# vkms.sh - a vkms device with two connected outputs, built through configfs
# (Documentation/gpu/vkms.rst, "Configuring with configfs"; Linux 6.19 and later). Runs as
# root in the throwaway guest of guest.sh, before the two-output layout cases. Prints the
# DRM card it created, and fails unless that card has exactly two connected connectors.
#
# The attribute names below are the ones the vkms module of Azoth 7.2.5 exports
# (planes/crtcs/encoders/connectors, type, possible_crtcs, possible_encoders, enabled);
# a plane of type 1 is a primary plane, and a new connector is connected by default.
set -euo pipefail

name=athanor
root=/sys/kernel/config/vkms/$name

# create_default_dev=0: only the device below exists, so cosmic-comp finds one card.
modprobe vkms create_default_dev=0
mountpoint -q /sys/kernel/config || mount -t configfs none /sys/kernel/config
if [ ! -d /sys/kernel/config/vkms ]; then
    echo "vkms.sh: $(uname -r) has no configfs support in vkms (Linux 6.19 or later needed)" >&2
    exit 1
fi

mkdir "$root"
for i in 0 1; do
    mkdir "$root/planes/primary$i" "$root/crtcs/crtc$i" "$root/encoders/enc$i" "$root/connectors/con$i"
    echo 1 > "$root/planes/primary$i/type"
    ln -s "$root/crtcs/crtc$i" "$root/planes/primary$i/possible_crtcs/"
    ln -s "$root/crtcs/crtc$i" "$root/encoders/enc$i/possible_crtcs/"
    ln -s "$root/encoders/enc$i" "$root/connectors/con$i/possible_encoders/"
done
echo 1 > "$root/enabled"
udevadm settle

# A configfs device is a faux device named after its configfs directory.
card=
for dev in /sys/class/drm/card[0-9]*; do
    case ${dev##*/} in *-*) continue ;; esac
    if [ "$(basename "$(readlink -f "$dev/device")")" = "$name" ]; then
        card=${dev##*/}
    fi
done
if [ -z "$card" ]; then
    drm=(/sys/class/drm/*)
    echo "vkms.sh: no DRM card belongs to the vkms device $name; /sys/class/drm holds: ${drm[*]##*/}" >&2
    exit 1
fi
connected=$(grep -lx connected /sys/class/drm/"$card"-*/status | wc -l)
if [ "$connected" != 2 ]; then
    echo "vkms.sh: $card has $connected connected connectors, not 2" >&2
    exit 1
fi
echo "$card"
