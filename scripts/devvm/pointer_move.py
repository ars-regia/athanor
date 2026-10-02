"""pointer_move.py: run as root in the guest by launcher-acceptance.sh (stage hotplug), as
`python3 - DX < pointer_move.py`.

Moves the pointer DX device units to the right through a throwaway uinput relative mouse, so
that the compositor's pointer crosses onto the next output and the next window is mapped
there. An absolute pointer (pointer_click.py) is mapped onto the first output only.
"""

import fcntl
import os
import struct
import sys
import time

EV_SYN, EV_KEY, EV_REL = 0, 1, 2
REL_X, REL_Y, BTN_LEFT = 0, 1, 0x110
UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_RELBIT = 0x40045564, 0x40045565, 0x40045566
UI_DEV_CREATE, UI_DEV_DESTROY = 0x5501, 0x5502
ABS_CNT, BUS_VIRTUAL = 64, 0x06
# How long the compositor takes to add a new input device.
SETTLE_S = 1.5
STEP = 20


def emit(fd, kind, code, value):
    os.write(fd, struct.pack("llHHi", 0, 0, kind, code, value))


def main():
    dx = int(sys.argv[1])
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    for ioctl, bit in ((UI_SET_EVBIT, EV_REL), (UI_SET_EVBIT, EV_KEY), (UI_SET_EVBIT, EV_SYN), (UI_SET_RELBIT, REL_X), (UI_SET_RELBIT, REL_Y), (UI_SET_KEYBIT, BTN_LEFT)):
        fcntl.ioctl(fd, ioctl, bit)
    zeros = [0] * ABS_CNT
    os.write(fd, struct.pack("80sHHHHi" + "i" * (4 * ABS_CNT), b"athanor-acceptance-rel", BUS_VIRTUAL, 0, 0, 1, 0, *zeros, *zeros, *zeros, *zeros))
    fcntl.ioctl(fd, UI_DEV_CREATE)
    time.sleep(SETTLE_S)
    for _ in range(abs(dx) // STEP):
        emit(fd, EV_REL, REL_X, STEP if dx > 0 else -STEP)
        emit(fd, EV_SYN, 0, 0)
        time.sleep(0.005)
    time.sleep(0.3)
    fcntl.ioctl(fd, UI_DEV_DESTROY)


main()
