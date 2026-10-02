"""pointer_click.py: run as root in the guest by switch-acceptance.sh (stage orca), as
`python3 - X Y WIDTH HEIGHT < pointer_click.py`.

Clicks the left button at X, Y on a WIDTH x HEIGHT output through a throwaway uinput
absolute pointer, so the click reaches the compositor as a real input device's would: a
layer surface with on-demand keyboard focus takes the focus only from such a click. QEMU's
monitor cannot stand in for it: under egl-headless its mouse_move reaches no input device.
udev classes absolute X/Y with a mouse button and no touch as a mouse (as VMware's), and the
compositor maps its range onto the one output.
"""

import fcntl
import os
import struct
import sys
import time

EV_SYN, EV_KEY, EV_ABS = 0, 1, 3
SYN_REPORT, BTN_LEFT, ABS_X, ABS_Y = 0, 0x110, 0, 1
UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_ABSBIT = 0x40045564, 0x40045565, 0x40045567
UI_DEV_CREATE, UI_DEV_DESTROY = 0x5501, 0x5502
ABS_CNT, BUS_VIRTUAL = 64, 0x06
# How long the compositor takes to add a new input device, and to handle a press.
SETTLE_S = 1.5


def emit(fd, kind, code, value):
    os.write(fd, struct.pack("llHHi", 0, 0, kind, code, value))


def main():
    x, y, width, height = (int(arg) for arg in sys.argv[1:5])
    if not (0 <= x < width and 0 <= y < height):
        print(f"{x},{y} lies outside the {width}x{height} output", file=sys.stderr)
        return 2
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    try:
        for kind in (EV_SYN, EV_KEY, EV_ABS):
            fcntl.ioctl(fd, UI_SET_EVBIT, kind)
        fcntl.ioctl(fd, UI_SET_KEYBIT, BTN_LEFT)
        for axis in (ABS_X, ABS_Y):
            fcntl.ioctl(fd, UI_SET_ABSBIT, axis)
        absmax = [0] * ABS_CNT
        absmax[ABS_X], absmax[ABS_Y] = width - 1, height - 1
        zeros = [0] * ABS_CNT
        # struct uinput_user_dev: name, input_id, ff_effects_max, absmax, absmin, absfuzz, absflat
        os.write(
            fd,
            struct.pack(
                f"80sHHHHi{4 * ABS_CNT}i",
                b"athanor-acceptance-pointer",
                BUS_VIRTUAL,
                0,
                0,
                1,
                0,
                *absmax,
                *zeros,
                *zeros,
                *zeros,
            ),
        )
        fcntl.ioctl(fd, UI_DEV_CREATE)
        try:
            time.sleep(SETTLE_S)
            emit(fd, EV_ABS, ABS_X, x)
            emit(fd, EV_ABS, ABS_Y, y)
            emit(fd, EV_SYN, SYN_REPORT, 0)
            time.sleep(0.2)
            for value in (1, 0):
                emit(fd, EV_KEY, BTN_LEFT, value)
                emit(fd, EV_SYN, SYN_REPORT, 0)
                time.sleep(0.1)
            time.sleep(SETTLE_S)
        finally:
            fcntl.ioctl(fd, UI_DEV_DESTROY)
    finally:
        os.close(fd)
    print(f"clicked {x},{y}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
