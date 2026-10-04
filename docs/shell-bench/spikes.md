# Spikes of the shell standard

Answers to the open questions of `doc_shell_standard.md` (section 3), measured on the
reference machine described in `machine.md`. A spike that fails stops the plan until ST9
is revised.

## Q1: capture

**Question.** Whether `grim`, run as the session user against the session's compositor
socket, captures the screen on the reference machine: it does in the VM
(`scripts/devvm/screenshot.sh`) and was refused on the maintainer's desktop on 2026-10-04
("compositor doesn't support the screen capture protocol"); if refused, which capture
protocol cosmic-comp offers there.

**Run.** `ssh athanor-ref 'export XDG_RUNTIME_DIR=/run/user/$(id -u); ls $XDG_RUNTIME_DIR | grep -E "^wayland-[0-9]+$"; WAYLAND_DISPLAY=wayland-1 grim -t ppm - | head -c 20 | od -c | head -2'`

**Answer.** The session's socket is `wayland-1` and `grim` returns a full 1920x1080 PPM
(`P6\n1920 1080\n255\n`). The refusal seen on the desktop does not occur here.
`Machine(display="wayland-1")`, the default, is right.

**Passed.**

## Q2: input

**Question.** Whether a throwaway `uinput` pointer and keyboard, created as root over SSH
as `scripts/devvm` does in the VM, move cosmic-comp's pointer and type into a surface on
the reference machine's own libinput stack, beside its touchpad and keyboard.

**Run.** `machine/inject.py` as root with `move 960 540`, `key 125` (Super), `key 1`
(Escape), a `grim` shot after each key; `machine/tree.py` for `athanor-bar` and
`athanor-dock`; then clicks on a bar status button and right-clicks on a dock
application button, each followed by a shot.

**Answer.** The pointer moves and clicks, and the keys type: Super opens the launcher and
Escape closes it. `inject.py` prints one line per input op, with increasing `t_ns`. The
applications are on the accessibility bus under their unit names, `athanor-bar` and
`athanor-dock`. The bar's tree lists its status buttons (Accessibility, Tiling, Sound,
Network, Battery, Notifications, Power, the update state, the clock) with
`"popup": true`; the dock's tree lists its pinned application buttons with
`"popup": true` and Launcher, Workspaces, Applications with `"popup": false`.

**Passed.**

### Closing a popover (Task 6 reads this)

- **Escape does not close** a bar popover or a dock menu opened by the pointer: the
  popover stays open after `key 1` (seen on Sound and Tiling in the bar, on the Files menu
  in the dock). Escape closes the launcher, which takes the keyboard.
- **What closes them:** clicking the same bar button again closes its popover; a click
  on an empty point of the desktop (960, 540) closes a bar popover and a dock menu; a
  second right-click on the same dock button closes its menu.
- **The response stage closes** a bar popover by clicking its button again, and a dock
  menu by clicking an empty point of the desktop, as the plan foresaw.

### Findings outside the spikes

These are defects of today's shell, recorded for the register and the first measurement;
the spikes do not fix them.

1. **Opening a popover of one program while a popover of the other is open kills the
   second program.** A right-click on a dock button with a bar popover open, or a click on
   a bar button with a dock menu open, makes cosmic-comp log
   `Failed to grab popup: NotTheTopmostPopup` and raise a protocol error on the client,
   which exits (`Error 71 (Protocol error) dispatching to Wayland display`); systemd
   restarts it about 1.5 s later. Reproduced three times (the dock twice, the bar once).
   cosmic-comp also logs `surface missing from known popups` on most popover closes. The
   bench must close every popover before it acts on the other program.
2. **A dock application launched by a click elsewhere.** After the Files menu of the dock
   was closed by a second right-click, the next left click, on a bar button, launched
   COSMIC Files (`Starting app-athanor-com.system76.CosmicFiles@...` 72 ms after the
   press). Seen once.
3. **The dock's accessibility extents look offset.** The tree nests the dock's boxes at
   (18, 14) and then (36, 28), as if the inner margin were counted twice, and puts the
   Files button's centre at (160, 44) in a 392x60 surface. If that surface is centred on
   the bottom edge, the centre lands at (924, 1064), while the screenshot shows the icon at
   about (910, 1054). The bar's extents match its screenshot. The surface origin from
   Task 3's geometry line settles it; until then the bench must not take the dock's
   extents at face value (Task 5).

## Q3: presentation time

**Question.** Whether GTK's frame clock on the reference machine gets real presentation
times from cosmic-comp (`wp_presentation`), on `CLOCK_MONOTONIC`, so that a frame line can
be set against the input's `t_ns`.

**Run.** The dock of this branch (Task 3's timing lines) restarted with
`ATHANOR_SHELL_BENCH=1 WAYLAND_DEBUG=1`; then `machine/inject.py` right-clicked the
dock's first application button (COSMIC Files) ten times, each menu closed by a click on
the desktop (Escape does not close it, see Q2), and the dock's frame lines were read from
the journal.

**Answer.**

- `wp_presentation` announces `clock_id(1)`, `CLOCK_MONOTONIC`, the clock `inject.py`
  stamps its input with.
- All 79 frame lines carry a non-zero `presented_us`, different from `predicted_us` in
  79 of 79.
- From each right-click to the next presented frame of the `popover` surface: 71.8, 35.1,
  15.5, 11.8, 9.1, 22.2, 35.3, 15.1, 12.4, 9.3 ms. All are within the plausible 5-200 ms.
- `refresh_us` is 16667 on every line: the panel runs at 60 Hz (`machine.md`).
- The dock's surface line puts it at (764, 1020), 392x60, on a 1920x1080 output. This
  settles finding 3 of Q2: the Files button's accessible centre (160, 44) maps to
  (924, 1064), but the icon is drawn at about (910, 1054). The accessibility extents count
  the dock's inner margin (18, 14) twice.

**Passed.**

## Q4: overlay

**Question.** Whether the bar and dock packages of a branch can be put on the reference
machine's transient `/usr` overlay (`bootc usr-overlay`) and run without SELinux or IMA
refusals, and whether a reboot discards them.

**Run.** The two RPMs of this branch built in `athanor-builder` with
`forge/scripts/build_spec.sh`, as the DAG builds them. Then on the machine:
`bootc usr-overlay`, `rpm -Uvh --replacepkgs`, a restart of both units, the units' warnings
and `ausearch -m AVC,INTEGRITY_DATA,INTEGRITY_RULE -ts recent`. Finally a reboot.

**Answer.**

- Before the reboot:
  - both units are `active`;
  - the units log no warning;
  - `ausearch` reports `<no matches>`;
  - `rpm -V` is clean;
  - the installed binaries carry the timing code (the NVR is the image's, 1.0.0-4, so
    the binary is what tells them apart).
- After the reboot:
  - `rpm -q` gives the image's packages again;
  - the binaries no longer carry the timing code;
  - `/usr` lies on the read-only composefs root (`findmnt -T /usr`: `overlay ro`) and
    refuses a write, even as root.

**Passed.**
