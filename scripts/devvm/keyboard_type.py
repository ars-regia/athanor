"""keyboard_type.py: run as root in the guest by launcher-acceptance.sh, as
`python3 - TOKEN... < keyboard_type.py`.

Types through a throwaway uinput keyboard, so the keys reach the compositor as a real
keyboard's would: Super reaches cosmic-comp's shortcuts and text reaches the focused
surface. A token is text, typed with the US layout the dev VM uses, or a key named after
an @: @super, @enter, @escape, @tab, @down, @up, @left, @right, which presses that key
alone, or several joined by +, which presses them together (@super+shift+ctrl+right; the
modifiers are @super, @shift, @ctrl, @alt). A key may be followed by :COUNT[:SECONDS],
which holds it down for COUNT autorepeats SECONDS apart (@down:20:0.25), as a key held
over the list does. A key followed by *COUNT:SECONDS is tapped COUNT times, SECONDS apart
(@down*4:0.25): a selection that moves more slowly than a held key's autorepeat, which the
compositor hands to the client at the client's own rate.
"""

import fcntl
import os
import struct
import sys
import time

EV_SYN, EV_KEY = 0, 1
SYN_REPORT = 0
UI_SET_EVBIT, UI_SET_KEYBIT = 0x40045564, 0x40045565
UI_DEV_CREATE, UI_DEV_DESTROY = 0x5501, 0x5502
ABS_CNT, BUS_VIRTUAL = 64, 0x06
SHIFT = 42
NAMED = {
    "super": 125, "shift": SHIFT, "ctrl": 29, "alt": 56, "enter": 28, "escape": 1, "tab": 15,
    "down": 108, "up": 103, "left": 105, "right": 106,
}
# Linux input event codes of the US layout: unshifted characters, then shifted ones.
PLAIN = dict(zip("1234567890-=qwertyuiopasdfghjklzxcvbnm./ ", [
    2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    30, 31, 32, 33, 34, 35, 36, 37, 38,
    44, 45, 46, 47, 48, 49, 50, 52, 53, 57,
]))
SHIFTED = {"+": 13, "*": 9, "_": 12, "#": 4, "%": 6, "&": 8, "?": 53} | {c.upper(): k for c, k in PLAIN.items() if c.isalpha()}
# How long the compositor takes to add a new input device.
SETTLE_S = 1.5
# The delay between two repeats of a held key.
REPEAT_S = 0.03


def emit(fd, kind, code, value):
    os.write(fd, struct.pack("llHHi", 0, 0, kind, code, value))


def press(fd, codes, repeats=0, interval=REPEAT_S):
    """Keys down in order, `repeats` autorepeat events of the last one (value 2), keys up in
    reverse: a chord, or a key held and not only tapped."""
    for code in codes:
        emit(fd, EV_KEY, code, 1)
        emit(fd, EV_SYN, SYN_REPORT, 0)
        time.sleep(0.03)
    for _ in range(repeats):
        emit(fd, EV_KEY, codes[-1], 2)
        emit(fd, EV_SYN, SYN_REPORT, 0)
        time.sleep(interval)
    for code in reversed(codes):
        emit(fd, EV_KEY, code, 0)
        emit(fd, EV_SYN, SYN_REPORT, 0)
        time.sleep(0.03)


def strokes(tokens):
    """(codes, repeats, interval, pause) for each stroke."""
    for token in tokens:
        if token.startswith("@") and "*" in token:
            name, _, rest = token[1:].partition("*")
            count, _, seconds = rest.partition(":")
            if name not in NAMED:
                raise SystemExit(f"keyboard_type.py: no key named {token!r}")
            for _ in range(int(count)):
                yield [NAMED[name]], 0, REPEAT_S, float(seconds)
            continue
        if token.startswith("@"):
            name, _, rest = token[1:].partition(":")
            count, _, seconds = rest.partition(":")
            if not all(key in NAMED for key in name.split("+")):
                raise SystemExit(f"keyboard_type.py: no key named {token!r}")
            yield [NAMED[key] for key in name.split("+")], int(count or 0), float(seconds or REPEAT_S), 0.0
            continue
        for char in token:
            if char in PLAIN:
                yield [PLAIN[char]], 0, REPEAT_S, 0.0
            elif char in SHIFTED:
                yield [SHIFT, SHIFTED[char]], 0, REPEAT_S, 0.0
            else:
                raise SystemExit(f"keyboard_type.py: no key for {char!r}")


def main():
    keys = list(strokes(sys.argv[1:]))  # refuse an untypable token before creating the device
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    try:
        fcntl.ioctl(fd, UI_SET_EVBIT, EV_SYN)
        fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
        for code in {*NAMED.values(), *PLAIN.values(), *SHIFTED.values()}:
            fcntl.ioctl(fd, UI_SET_KEYBIT, code)
        zeros = [0] * ABS_CNT
        # struct uinput_user_dev: name, input_id, ff_effects_max, absmax, absmin, absfuzz, absflat
        os.write(fd, struct.pack(
            f"80sHHHHi{4 * ABS_CNT}i", b"athanor-acceptance-keyboard",
            BUS_VIRTUAL, 0, 0, 1, 0, *zeros, *zeros, *zeros, *zeros,
        ))
        fcntl.ioctl(fd, UI_DEV_CREATE)
        try:
            time.sleep(SETTLE_S)
            for codes, repeats, interval, pause in keys:
                press(fd, codes, repeats, interval)
                time.sleep(pause)
            time.sleep(0.3)
        finally:
            fcntl.ioctl(fd, UI_DEV_DESTROY)
    finally:
        os.close(fd)
    return 0


if __name__ == "__main__":
    sys.exit(main())
