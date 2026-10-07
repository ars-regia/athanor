# Defects found by the first measurement, 2026-10-04, 1b4c5f7

The bar and the dock of commit 1b4c5f7 on the reference machine, measured on the
transient `/usr` overlay (spike Q4) over image `athanor-system:37188690695`. This
measurement is not a gate (ST9). Both surfaces are enabled by default, so every defect
below belongs to an enabled surface (ST2). No limit is relaxed.

## Defects

1. **Dock auto-hide (ST6): all four scenarios fail.** `dock-autohide/switch`, `leave`
   and `menu` never settle (14391 pixels differ from the reference). `rest` never settles
   either: the island is drawn at (18, 1034, 373, 1074), 764.5 px left of the centre.
   This is the defect the plan expected; Task 11 repairs it.
2. **No pressed state (Response, in place, limit 50 ms).** In every bar button and every
   dock application button, the 95th percentile is 180-194 ms (bar) and 157-159 ms
   (dock). The press itself presents no frame. The first frame comes after the release,
   which the bench sends 150 ms later. No stylesheet gives a shell button an `:active`
   rule, so pressing it changes nothing on screen.
3. **Recovery (limit 1 s).** After a `SIGKILL`, the bar presents its surface again in
   5.61 s and the dock in 5.79 s, both in the same state. The units' `RestartSec=1s`,
   which the plan expected to fix (Task 12), accounts for 1 s of that. The rest is the
   restarted process's own path to its first frame, which this measurement does not
   break down.
4. **Start (limit 1.5 s).** The bar and the dock present their first frame 3.70 s after
   the session's `cosmic-comp` started. Both values are the same to the nanosecond:
   their first frames were presented in the same refresh.
5. **Idle (limit 0.1% of one CPU).** The shell's processes together use 0.1033% over
   10 minutes, 3% above the limit.

## What passed

- **Opening.** Every popover opens within 29-43 ms at the 95th percentile (limit 100 ms).
- **Memory.** Bar 30.0 MiB (limit 64), dock 25.0 MiB (48), `athanor-shelld` 4.0 MiB (16).
- **Same state after recovery.** Both surfaces come back in the same state.
- **Smoothness.** The two dock menus that animated (COSMIC Files, COSMIC Terminal) are
  on time, with a worst interval of 1 refresh.

## Not measured

**Smoothness of the bar's popovers and of two dock menus** (COSMIC Settings, COSMIC
Text Editor). No opening window held the five frames the bench needs to call it an
animation. The report shows these rows as failing because the bench treats a missing
value as a failure.

Before the next run, one of two things must be settled:

- whether these popovers open with no animation, in which case there is nothing to judge;
- or whether their animation runs on a surface the bench does not follow.

## Known defects the bench does not measure

- **`athanor-shelld` loses the unread notifications on a restart.** The state file
  required by ST5 is not implemented.
- **Two popovers across programs.** Opening a popover of the bar while a dock menu is
  open, or the reverse, makes cosmic-comp raise a protocol error on the client, which
  exits (spikes, Q2, finding 1).
- **Dock accessibility extents.** The dock reports its accessibility extents with its
  inner margin counted twice (spikes, Q2 finding 3, and Q3).
- **Focus outline.** The `*:focus-visible` rule in
  `system/athanor-style/calmo/templates/surfaces.css.in` draws an outline on every
  container around the focused widget, not only on the widget. Seen in the greeter.
- **Reference machine configuration (ST9).** The reference machine's `sudo` rule is
  `NOPASSWD: ALL`, wider than ST9 allows.
- **Autologin.** It is configured but did not start the session at the start stage's
  reboot; the maintainer logged in by hand. The start measure counts from the session
  compositor's start, so the manual login does not change the result.
