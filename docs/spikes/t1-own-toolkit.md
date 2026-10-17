# Spike T1: the layout chooser on the own toolkit

Results of milestone T1 of `doc_own_stack.md` (section 5), plan
`docs/superpowers/plans/2026-10-17-spike-t1-toolkit.md`. Task 8 adds the tables of tasks 4 and 5,
the renderer recommendation, the AccessKit gaps and what revision 2 must change. This file
holds task 7's measurements so far.

## Task 7: measurements

Measured on 2026-10-17 on the tree of branch `toolkit-t1-measure` (T1 task 4, PR #589, plus the
measurement script), with `forge/test/shell/rig.sh measure-chooser` after `rig.sh build-layout` and
`rig.sh build-toolkit`. The script is `forge/test/shell/measure_chooser.py`; its unit test is
`forge/test/shell/tests/test_measure_chooser.py`.

### Method

| Quantity | How it is measured |
| --- | --- |
| Stripped binary | `strip -o` of the installed binary, then its size. The release profile already strips (`strip = true`), so this equals the installed file. The GTK chooser links GTK4 and its stack dynamically, the toolkit's chooser links its Rust dependencies statically: the last column of the table counts the shared objects each process maps, so the two sizes are read together |
| Start-up time to the first frame | From the exec of the binary to the first `wl_surface.commit` of the window's surface (the surface given to `xdg_wm_base.get_xdg_surface`) after an `attach` of a non-null `wl_buffer`. The client's own `WAYLAND_DEBUG=1` trace reports the request: libwayland's for GTK, wayland-backend's for the toolkit. The clock is the script's monotonic clock when that line reaches the client's stderr, so both clients pay the same tracing cost. Five launches in a row in one scene; the table gives the median, the minimum and the maximum |
| Memory after one minute | `VmRSS` (`/proc/<pid>/status`) and `Pss` (`/proc/<pid>/smaps_rollup`) of the fifth launch, 60 s after its exec, with no input |
| Shared objects mapped | The distinct `*.so*` files in `/proc/<pid>/maps` at the same moment, and the sum of their sizes on disk |
| Lines | A tokei-like count by the same script (`measure_chooser.py lines`): blank lines, comment lines (`//`, `///`, `//!`, `/* */`), and code; "of which tests" is the code from the file's first top-level `#[cfg(test)]` to its end. Neither the rig image nor the desktop has `tokei`, so the script counts instead; it does not parse strings, so a `//` at the start of a line inside a multi-line string would count as a comment (the crate has none) |

The scene: the rig's headless sway on Pixman, cosmic-comp nested in it, one output of
1280x800 at scale 1.0, `en_US.UTF-8`, no assistive technology running. Both choosers draw on the
CPU: the toolkit with `vello_cpu` into `wl_shm`, GTK with `GSK_RENDERER=cairo`, which the rig image
sets, into `wl_shm` as well. Both open as floating windows of about the same size (GTK 448x326
logical pixels, the toolkit 420x293). The choosers run outside the scene's `faketime`.

### Laptop

Reference laptop, Intel Core i7-8550U, kernel `7.2.9-100.azoth.fc43`, in the rig's podman image.

| Chooser | Stripped binary | Start-up to first frame, median (min-max, runs) | RSS after 1 min | PSS after 1 min | Shared objects mapped |
| --- | --- | --- | --- | --- | --- |
| GTK (`athanor-layout-chooser`, cairo) | 1.34 MiB | 185 ms (169-331, 5) | 46.1 MiB | 32.9 MiB | 120, 77.0 MiB |
| Toolkit, `vello_cpu` (`athanor-toolkit-chooser`) | 5.84 MiB | 63 ms (63-78, 5) | 14.5 MiB | 9.2 MiB | 18, 11.5 MiB |
| Toolkit, `vello_gpu` | not run yet: the toolkit has no `vello_gpu` path | | | | |

### Development VM and NVIDIA desktop

Not run yet. They run on the desktop when its cores are free, with the same script:

- in the rig, on either machine, from a copy of this tree with the rig's binaries built:
  `forge/test/shell/rig.sh build-layout && forge/test/shell/rig.sh build-toolkit && forge/test/shell/rig.sh measure-chooser`
  (the tables land in `<out>/measure/report.md`);
- nested in the live session of the NVIDIA desktop or of the VM, with both binaries copied out
  of `<out>/bin`: `python3 -B forge/test/shell/measure_chooser.py run <out>/bin/athanor-layout-chooser layout.json`,
  the same for `athanor-toolkit-chooser`, then
  `python3 -B forge/test/shell/measure_chooser.py report system/athanor-toolkit layout.json toolkit.json`.
  There GTK takes the session's default renderer unless `GSK_RENDERER` is set; the JSON records it.

| Chooser | VM (virgl or llvmpipe) | NVIDIA desktop, nested |
| --- | --- | --- |
| GTK | not run yet | not run yet |
| Toolkit, `vello_cpu` | not run yet | not run yet |
| Toolkit, `vello_gpu` | not run yet, no path | not run yet, no path |

### Lines of the toolkit

`system/athanor-toolkit` at the same tree. Each file belongs to one area in `measure_chooser.py`;
a new file fails the count until it is placed. The driver is `driver.rs`; drawing (`render.rs`)
is counted apart from it; the widgets are `widget.rs`, `focus.rs`, `theme.rs` and `lib.rs`.

| Area | Code | Of which tests | Comments | Blanks |
| --- | --- | --- | --- | --- |
| driver | 614 | 29 | 53 | 59 |
| rendering (`vello_cpu`) | 246 | 69 | 11 | 14 |
| widgets | 713 | 156 | 72 | 64 |
| accessibility | 311 | 180 | 20 | 22 |
| text | 142 | 33 | 19 | 26 |
| chooser (example) | 102 | 30 | 16 | 12 |
| test clients (examples: text field, input method) | 111 | 0 | 12 | 11 |
| total | 2239 | 497 | 203 | 208 |

### Open

- The `vello_gpu` column waits on a `vello_gpu` path in the toolkit (a wgpu surface beside the
  `wl_shm` one), which the toolkit does not have; building it is a decision, not part of this task.
- GTK's figures are with cairo, as the rig sets it. A GTK run on its default renderer (GL on
  llvmpipe in the rig) would add a row; it is left to the desktop columns, where the session's
  renderer is the real one.
