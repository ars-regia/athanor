#!/usr/bin/env python3
"""A GTK4 window that draws a new frame at every tick of its frame clock.

athanor-compositor's headless soak (doc_own_stack.md, C1 task 2) runs it beside a Vulkan or EGL
client: each frame is a commit the compositor times. GTK chooses its renderer, so on a GPU the
frames reach the compositor through dmabuf, and on llvmpipe through wl_shm.
"""

import math
import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk


def draw(area, cr, width, height):
    phase = area.get_frame_clock().get_frame_time() / 1_000_000
    cr.set_source_rgb(0.5 + 0.5 * math.sin(phase), 0.3, 0.5 + 0.5 * math.cos(phase))
    cr.paint()


def activate(app):
    window = Gtk.ApplicationWindow(application=app, title="gtk-animation")
    # Wider and shorter than eglgears_wayland's 300x300 and vkcube's 500x500, and centred like
    # them, the two windows cross: whichever maps on top, the other keeps a visible part, so the
    # compositor throttles neither as occluded, whatever order they start in.
    window.set_default_size(640, 120)
    area = Gtk.DrawingArea()
    area.set_draw_func(draw)
    area.add_tick_callback(lambda widget, clock: widget.queue_draw() or True)
    window.set_child(area)
    window.present()


app = Gtk.Application(application_id="os.athanor.GtkAnimation")
app.connect("activate", activate)
sys.exit(app.run([]))
