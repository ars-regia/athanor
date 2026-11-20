"""inject.py: run as root on the reference machine by the shell bench, as
`sudo -n python3 - WIDTH HEIGHT OP... < inject.py`.

Gives input through two throwaway uinput devices, an absolute pointer and a keyboard, which
the compositor reads through libinput like real ones (doc_shell_standard.md, spike Q2). The
pointer's range is WIDTH x HEIGHT, mapped onto the one output. Each OP is one argument:
`move X Y`, `press`, `press right`, `release`, `release right`, `click`, `rclick`,
`key CODE` (an evdev key code: 1 is Escape), `wait MS`. After each OP that sends input it
prints {"op": OP, "t_ns": T}, T being CLOCK_MONOTONIC right after the event's SYN_REPORT:
the clock of GTK's frame timings (spike Q3). Modelled on scripts/devvm/pointer_click.py.
"""

import fcntl
import json
import os
import struct
import sys
import time

EV_SYN, EV_KEY, EV_ABS = 0, 1, 3
SYN_REPORT, BTN_LEFT, BTN_RIGHT, ABS_X, ABS_Y = 0, 0x110, 0x111, 0, 1
UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_ABSBIT = 0x40045564, 0x40045565, 0x40045567
UI_DEV_CREATE, UI_DEV_DESTROY = 0x5501, 0x5502
ABS_CNT, BUS_VIRTUAL = 64, 0x06
# KEY_ESC to KEY_MICMUTE: enough keys for libinput to class the device as a keyboard.
KEYS = range(1, 249)
# How long the compositor takes to add a new input device.
SETTLE_S = 1.5


def device(name, keys, width=0, height=0):
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    for kind in (EV_SYN, EV_KEY, EV_ABS) if width else (EV_SYN, EV_KEY):
        fcntl.ioctl(fd, UI_SET_EVBIT, kind)
    for key in keys:
        fcntl.ioctl(fd, UI_SET_KEYBIT, key)
    absmax = [0] * ABS_CNT
    if width:
        for axis in (ABS_X, ABS_Y):
            fcntl.ioctl(fd, UI_SET_ABSBIT, axis)
        absmax[ABS_X], absmax[ABS_Y] = width - 1, height - 1
    zeros = [0] * ABS_CNT
    # struct uinput_user_dev: name, input_id, ff_effects_max, absmax, absmin, absfuzz, absflat
    os.write(
        fd,
        struct.pack(
            f"80sHHHHi{4 * ABS_CNT}i", name, BUS_VIRTUAL, 0, 0, 1, 0, *absmax, *zeros, *zeros, *zeros
        ),
    )
    fcntl.ioctl(fd, UI_DEV_CREATE)
    return fd


def send(fd, *events):
    for kind, code, value in (*events, (EV_SYN, SYN_REPORT, 0)):
        os.write(fd, struct.pack("llHHi", 0, 0, kind, code, value))
    return time.clock_gettime_ns(time.CLOCK_MONOTONIC)


def main():
    width, height = int(sys.argv[1]), int(sys.argv[2])
    ops = [arg.split() for arg in sys.argv[3:]]
    pointer = device(b"athanor-bench-pointer", (BTN_LEFT, BTN_RIGHT), width, height)
    keyboard = device(b"athanor-bench-keyboard", KEYS)
    try:
        time.sleep(SETTLE_S)
        for op in ops:
            t_ns = None
            match op:
                case ["move", x, y]:
                    if not (0 <= int(x) < width and 0 <= int(y) < height):
                        raise SystemExit(f"{x},{y} lies outside the {width}x{height} output")
                    t_ns = send(pointer, (EV_ABS, ABS_X, int(x)), (EV_ABS, ABS_Y, int(y)))
                case ["press" | "release" as what, *side] if side in ([], ["right"]):
                    button = BTN_RIGHT if side else BTN_LEFT
                    t_ns = send(pointer, (EV_KEY, button, int(what == "press")))
                case ["click" | "rclick" as what]:
                    button = BTN_LEFT if what == "click" else BTN_RIGHT
                    t_ns = send(pointer, (EV_KEY, button, 1))
                    time.sleep(0.05)
                    send(pointer, (EV_KEY, button, 0))
                case ["key", code]:
                    t_ns = send(keyboard, (EV_KEY, int(code), 1))
                    time.sleep(0.03)
                    send(keyboard, (EV_KEY, int(code), 0))
                case ["wait", ms]:
                    time.sleep(int(ms) / 1000)
                case _:
                    raise SystemExit(f"unknown op {' '.join(op)!r}")
            if t_ns is not None:
                print(json.dumps({"op": " ".join(op), "t_ns": t_ns}), flush=True)
    finally:
        for fd in (pointer, keyboard):
            fcntl.ioctl(fd, UI_DEV_DESTROY)
            os.close(fd)
    return 0


if __name__ == "__main__":
    sys.exit(main())
