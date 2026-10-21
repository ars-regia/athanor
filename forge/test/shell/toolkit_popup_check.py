#!/usr/bin/python3
"""toolkit_popup_check.py - the dock plan, task 4: athanor-toolkit's popups against sway on its
headless backend.

Runs inside the rig. Starts sway, then `athanor-toolkit-popup` with WAYLAND_DEBUG: one layer
surface along the bottom edge, holding a button with a menu and a tooltip. The rig has no pointer
device, so `athanor-toolkit-pointer` adds a virtual one. From the client's own trace and output:

- resting on the button shows its tooltip, an `xdg_popup` of the layer surface without a grab,
  placed under the button's bounds grown by 4 pixels;
- a click on the button opens its menu, an `xdg_popup` of the layer surface with a grab, placed
  above the button's bounds;
- a click on the layer surface outside the menu closes it, without the compositor's popup_done;
- a click outside the client closes it through the compositor's popup_done;
- the secondary button opens it, the down arrow moves past the separator and Return activates
  the second item; Escape closes the next one; a click activates an item.
"""

import math
import os
import re
import subprocess
import sys
import time
from pathlib import Path

CLIENT = "/out/bin/athanor-toolkit-popup"
POINTER = "/out/bin/athanor-toolkit-pointer"
TRACE = Path("/out/toolkit-popup.trace")
OUTPUT = Path("/out/toolkit-popup.out")
WIDTH, HEIGHT = 1280, 800
TIMEOUT = 10
# The rig has no keyboard: the seat gains wtype's virtual one only while wtype runs, so wtype
# waits before its keys until the client has bound the keyboard.
TYPE = ["wtype", "-s", "500"]

POSITIONER = re.compile(
    r"-> xdg_wm_base#\d+\.create_positioner\(new id xdg_positioner#(\d+)\)"
)
SET = re.compile(
    r"-> xdg_positioner#(\d+)\.(set_size|set_anchor_rect|set_anchor|set_gravity|set_constraint_adjustment)\(([^)]*)\)"
)
GET_POPUP = re.compile(
    r"-> xdg_surface#\d+\.get_popup\(new id xdg_popup#(\d+), nil, xdg_positioner#(\d+)\)"
)
LAYER_PARENT = re.compile(r"-> zwlr_layer_surface_v1#\d+\.get_popup\(xdg_popup#(\d+)\)")
GRAB = re.compile(r"-> xdg_popup#(\d+)\.grab\(")
DONE = re.compile(r"(?<!-> )xdg_popup#(\d+)\.popup_done\(\)")
DESTROY = re.compile(r"-> xdg_popup#(\d+)\.destroy\(\)")
CONFIGURE = re.compile(
    r"(?<!-> )xdg_popup#(\d+)\.configure\((-?\d+), (-?\d+), (\d+), (\d+)\)"
)

# xdg_positioner's enums.
TOP, BOTTOM = 1, 2
SLIDE_X, FLIP_Y = 1, 8


def popups(lines):
    """Every xdg_popup the client made, in order: its positioner's settings, whether its parent is
    a layer surface, whether it took a grab, where the compositor put it, and how it ended. An id
    is reused once its popup is destroyed, so each line goes to the latest popup of its id."""
    positioners, made, live = {}, [], {}
    for line in lines:
        if m := POSITIONER.search(line):
            positioners[m[1]] = {}
        elif m := SET.search(line):
            values = tuple(int(v) for v in m[3].split(", "))
            positioners.setdefault(m[1], {})[m[2]] = values
        elif m := GET_POPUP.search(line):
            popup = {
                "positioner": positioners.get(m[2], {}),
                "layer": False,
                "grab": False,
                "position": None,
                "done": False,
                "destroyed": False,
            }
            made.append(popup)
            live[m[1]] = popup
        elif (m := LAYER_PARENT.search(line)) and m[1] in live:
            live[m[1]]["layer"] = True
        elif (m := GRAB.search(line)) and m[1] in live:
            live[m[1]]["grab"] = True
        elif (m := CONFIGURE.search(line)) and m[1] in live:
            live[m[1]]["position"] = tuple(int(v) for v in m.groups()[1:])
        elif (m := DONE.search(line)) and m[1] in live:
            live[m[1]]["done"] = True
        elif (m := DESTROY.search(line)) and m[1] in live:
            live.pop(m[1])["destroyed"] = True
    return made


def rects(text):
    """The rectangles the client printed: {name: (x, y, w, h)} in logical pixels."""
    out = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) == 6 and parts[0] == "rect":
            out[parts[1]] = tuple(float(v) for v in parts[2:])
    return out


def whole(x, y, w, h, pad=0):
    """A rectangle grown by `pad` in the whole pixels that cover it, as the driver anchors a
    popup: the origin down, the far edge up."""
    left, top = math.floor(x - pad), math.floor(y - pad)
    return (left, top, math.ceil(x + w + pad) - left, math.ceil(y + h + pad) - top)


def until(what, done):
    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        if done():
            return
        time.sleep(0.2)
    raise SystemExit(f"toolkit_popup_check: {what}")


def main():
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    runtime.mkdir(mode=0o700, parents=True, exist_ok=True)
    env = dict(
        os.environ,
        WLR_BACKENDS="headless",
        WLR_RENDERER="pixman",
        WLR_LIBINPUT_NO_DEVICES="1",
    )
    config = runtime / "sway.conf"
    config.write_text(f"output HEADLESS-1 mode {WIDTH}x{HEIGHT}\n")
    with open("/out/toolkit-popup-sway.log", "w") as log:
        compositor = subprocess.Popen(
            ["sway", "-c", str(config)], env=env, stdout=log, stderr=log
        )
    until("sway did not start", lambda: list(runtime.glob("sway-ipc.*.sock")))
    until("sway offers no Wayland socket", lambda: list(runtime.glob("wayland-[0-9]")))
    display = next(runtime.glob("wayland-[0-9]")).name
    wayland = dict(os.environ, WAYLAND_DISPLAY=display)
    with open(TRACE, "w") as trace, open(OUTPUT, "w") as output:
        client = subprocess.Popen(
            [CLIENT],
            env=dict(wayland, WAYLAND_DEBUG="1"),
            stdout=output,
            stderr=trace,
        )
    pointer = subprocess.Popen(
        [POINTER],
        env=wayland,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
    )

    def send(command):
        """One command to the virtual pointer, once the compositor has received it."""
        pointer.stdin.write(command + "\n")
        pointer.stdin.flush()
        if pointer.stdout.readline().strip() != "ok":
            raise SystemExit(f"toolkit_popup_check: the pointer refused {command!r}")

    def made():
        return popups(TRACE.read_text().splitlines())

    def printed():
        return OUTPUT.read_text()

    def step(what, done):
        until(what, lambda: client.poll() is None and done())
        print(what)

    try:
        until(
            "the client printed no rectangles",
            lambda: {"surface", "button", "item"} <= rects(printed()).keys(),
        )
        r = rects(printed())
        # The layer surface's size in whole logical pixels, as the driver asks for it.
        sw, sh = math.ceil(r["surface"][2]), math.ceil(r["surface"][3])
        # Anchored to the bottom edge alone: centred along it.
        left, top = (WIDTH - sw) // 2, HEIGHT - sh
        bx, by, bw, bh = r["button"]
        button = (int(left + bx + bw / 2), int(top + by + bh / 2))

        send(f"move {button[0]} {button[1]}")
        step(
            "resting on the button shows its tooltip",
            lambda: len(made()) == 1 and made()[0]["position"] is not None,
        )
        tip = made()[0]
        expect = {
            "set_anchor_rect": whole(bx, by, bw, bh, 4),
            "set_anchor": (BOTTOM,),
            "set_gravity": (BOTTOM,),
            "set_constraint_adjustment": (FLIP_Y | SLIDE_X,),
        }
        for key, value in expect.items():
            if tip["positioner"].get(key) != value:
                raise SystemExit(
                    f"toolkit_popup_check: tooltip {key} {tip['positioner'].get(key)}, expected {value}"
                )
        if not tip["layer"] or tip["grab"]:
            raise SystemExit(f"toolkit_popup_check: tooltip {tip}")

        send("click 272")
        step(
            "a click on the button opens its menu",
            lambda: len(made()) == 2 and made()[1]["position"] is not None,
        )
        menu = made()[1]
        expect = {
            "set_anchor_rect": whole(bx, by, bw, bh),
            "set_anchor": (TOP,),
            "set_gravity": (TOP,),
            "set_constraint_adjustment": (FLIP_Y | SLIDE_X,),
        }
        for key, value in expect.items():
            if menu["positioner"].get(key) != value:
                raise SystemExit(
                    f"toolkit_popup_check: menu {key} {menu['positioner'].get(key)}, expected {value}"
                )
        if not (menu["layer"] and menu["grab"] and made()[0]["destroyed"]):
            raise SystemExit(f"toolkit_popup_check: {made()}")

        # The bottom start corner of the layer surface: under the button, never under the menu.
        send(f"move {left + 2} {HEIGHT - 2}")
        send("click 272")
        step(
            "a click on the surface outside the menu closes it",
            lambda: made()[1]["destroyed"],
        )
        if made()[1]["done"]:
            raise SystemExit(
                "toolkit_popup_check: the compositor closed the menu, not the client"
            )

        send(f"move {button[0]} {button[1]}")
        send("click 272")
        step("the menu opens again", lambda: len(made()) == 3)
        send("move 20 20")
        send("click 272")
        step(
            "a click outside the client closes it through popup_done",
            lambda: made()[2]["done"] and made()[2]["destroyed"],
        )

        send(f"move {button[0]} {button[1]}")
        send("click 273")
        step(
            "the secondary button opens it",
            lambda: len(made()) == 4 and made()[3]["position"] is not None,
        )
        subprocess.run([*TYPE, "-k", "Down", "-k", "Return"], env=wayland, check=True)
        step(
            "down and Return activate the item after the separator",
            lambda: "activated Pin to Dock" in printed(),
        )
        step("activating an item closes the menu", lambda: made()[3]["destroyed"])

        send("click 272")
        step(
            "the menu opens a fourth time",
            lambda: len(made()) == 5 and made()[4]["position"] is not None,
        )
        subprocess.run([*TYPE, "-k", "Escape"], env=wayland, check=True)
        step(
            "Escape closes it", lambda: made()[4]["destroyed"] and not made()[4]["done"]
        )

        send("click 272")
        step(
            "the menu opens a fifth time",
            lambda: len(made()) == 6 and made()[5]["position"] is not None,
        )
        mx, my = made()[5]["position"][:2]
        ix, iy, iw, ih = r["item"]
        send(f"move {int(left + mx + ix + iw / 2)} {int(top + my + iy + ih / 2)}")
        send("click 272")
        step(
            "a click activates the first item",
            lambda: "activated New Window" in printed(),
        )
        if client.poll() is not None:
            raise SystemExit(
                f"toolkit_popup_check: the client exited with {client.returncode}"
            )
    finally:
        pointer.terminate()
        client.terminate()
        compositor.terminate()


if __name__ == "__main__":
    sys.exit(main())
