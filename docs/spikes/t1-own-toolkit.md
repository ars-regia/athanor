# Spike T1: the layout chooser on the own toolkit

Results of milestone T1 of `doc_own_stack.md` (section 5), plan
`docs/superpowers/plans/2026-10-17-spike-t1-toolkit.md`. Task 8 adds the tables of tasks 4 and 5,
the renderer recommendation, the AccessKit gaps and what revision 2 must change. This file
holds task 7's measurements so far.

## Task 7: measurements

Measured on 2026-10-17 on the tree of branch `toolkit-t1-gpu` (T1 task 4, PR #589, the
measurement script and the toolkit's `vello_gpu` path), with
`forge/test/shell/rig.sh measure-chooser` after `rig.sh build-layout` and `rig.sh build-toolkit`.
The script is `forge/test/shell/measure_chooser.py`; its unit test is
`forge/test/shell/tests/test_measure_chooser.py`.
`ATHANOR_TOOLKIT_RENDERER` picks the toolkit's renderer at start-up: `cpu`, the default, or `gpu`.

### Method

| Quantity | How it is measured |
| --- | --- |
| Stripped binary | `strip -o` of the installed binary, then its size. The release profile already strips (`strip = true`), so this equals the installed file. The GTK chooser links GTK4 and its stack dynamically, the toolkit's chooser links its Rust dependencies statically: the last column of the table counts the shared objects each process maps, so the two sizes are read together. The toolkit's binary holds both renderers, so both of its rows are one file |
| Start-up time to the first frame | From the exec of the binary to the first `wl_surface.commit` of the window's surface (the surface given to `xdg_wm_base.get_xdg_surface`) after an `attach` of a non-null `wl_buffer`. The client's own `WAYLAND_DEBUG=1` trace reports the request; libwayland prints it for both choosers, as the toolkit speaks Wayland through libwayland-client. The clock is the script's monotonic clock when that line reaches the client's stderr. Tracing slows both clients by a similar amount, which is not measured. Five launches in a row in one scene; the table gives the median, the minimum and the maximum, and the report names the request each time ends on |
| Window buffer | The size of the buffer the first frame attaches, from the `wl_shm_pool.create_buffer` or `create_immed` request that made it, in buffer pixels (logical pixels at scale 1.0) |
| Memory after one minute | `VmRSS` (`/proc/<pid>/status`) and `Pss` (`/proc/<pid>/smaps_rollup`) of a sixth launch, without `WAYLAND_DEBUG`, 60 s after its exec, with no input |
| Shared objects mapped, their on-disk size | The distinct `*.so*` files in `/proc/<pid>/maps` at the same moment, one per device and inode, deleted files left out, and the sum of their sizes on disk |
| Lines | A tokei-like count by the same script (`measure_chooser.py lines`): blank lines, comment lines (`//`, `///`, `//!`, `/* */`), and code; "of which tests" is the code from the file's first top-level `#[cfg(test)]` to its end. Neither the rig image nor the desktop has `tokei`, so the script counts instead; it does not parse strings, so a `//` at the start of a line inside a multi-line string would count as a comment (the crate has none) |

The scene: the rig's headless sway on Pixman, cosmic-comp nested in it, one output of
1280x800 at scale 1.0, `en_US.UTF-8`, no assistive technology running. The rig passes no render
node to the container, so everything draws on the CPU: the toolkit with `vello_cpu` into `wl_shm`,
or with `vello_gpu` through wgpu, which picked its GL backend on Mesa's llvmpipe; GTK with
`GSK_RENDERER=cairo`, which the rig image sets, into `wl_shm` as well. Both open as floating windows
of about the same size, the window buffer column. The choosers run outside the scene's `faketime`.

### Laptop

Reference laptop, Intel Core i7-8550U, kernel `7.2.9-100.azoth.fc43`, in the rig's podman image.

| Chooser | Stripped binary | Start-up to first frame, median (min-max, runs) | Window buffer | RSS after 1 min | PSS after 1 min | Shared objects mapped, their on-disk size |
| --- | --- | --- | --- | --- | --- | --- |
| GTK (`athanor-layout-chooser`, cairo) | 1.34 MiB | 186 ms (167-376, 5) | 448x326 | 46.0 MiB | 33.1 MiB | 120, 77.0 MiB |
| Toolkit, `vello_cpu` (`athanor-toolkit-chooser`) | 10.18 MiB | 81 ms (65-95, 5) | 420x293 | 16.4 MiB | 11.2 MiB | 20, 11.6 MiB |
| Toolkit, `vello_gpu`, wgpu GL on llvmpipe | 10.18 MiB | 199 ms (176-655, 5) | 420x293 | 131.0 MiB | 87.9 MiB | 50, 207.2 MiB |

The adapter wgpu picked: `llvmpipe (LLVM 21.1.8, 256 bits)`, backend `Gl`, device type `Cpu`,
driver `4.5 (Core Profile) Mesa 25.3.6`. The `vello_gpu` row therefore measures Mesa's software
rasteriser and its LLVM, which account for most of its memory and shared objects; it says what
the GPU path costs where no GPU is present, not on one.

Before the `vello_gpu` path, on branch `toolkit-t1-measure` (1d96c554), the toolkit's chooser was
5.84 MiB stripped, drew its first frame in 63 ms (63-78, 5), and held 14.5 MiB RSS, 9.2 MiB PSS
and 18 shared objects of 11.5 MiB. The path adds 4.34 MiB to the stripped binary whichever
renderer runs, wgpu, naga and `vello_gpu` linked statically, and two shared objects to the `cpu`
row, as the toolkit now loads libwayland-client at start-up.

wgpu reaches the window through libwayland-client's pointers, so the toolkit enables
wayland-backend's `client_system` feature. Cargo unifies features across a build, so every
Wayland client crate built with the toolkit speaks Wayland through libwayland-client too. That
was already so before the toolkit enabled it: `gdk4-wayland`, under `athanor-compositor-client`,
enables the feature in any `--workspace` build such as `just check-rust`, and
`athanor-compositor` enables it alone, through Smithay's `wayland-egl` and `winit-wayland`.
`cargo test -p athanor-toolkit -p athanor-compositor -p athanor-compositor-client`, one
invocation in the rig, passes. The feature changes the transport under `wayland-client`, not its
interface, and libwayland is the implementation GTK and Mesa already use in the same session.

The table predates the frame pacing added after it: the toolkit now draws a frame only after the
compositor's callback for the last one, on both paths. `vello_gpu` presents with Mailbox where
Vulkan offers it, otherwise with Fifo, whose wait the pacing keeps short because the toolkit's
callback and Mesa's arrive together; on GLES, the rig's llvmpipe included, wgpu offers only Fifo.
The first frame, which the start-up column measures, does not wait on a callback.

With `ATHANOR_TOOLKIT_RENDERER=gpu`, `rig.sh surface toolkit-chooser` passes against the `vello_cpu`
goldens in all six cases, with no pixel beyond `compare.py`'s 2 % colour distance. Compared
exactly, 2147 to 3803 pixels per image differ, by one or two levels of 255 at most, on the
anti-aliased edges of shapes and glyphs: the two rasterisers round partial coverage differently.
There is no gamma difference, as both write sRGB-encoded values into a linear 8-bit target. No
`vello_gpu` goldens are needed. `rig.sh atspi toolkit-chooser`, `rig.sh orca chooser` and
`rig.sh orca chooser it` with `ATHANOR_CHOOSER_BIN` set to the toolkit's chooser, and
`rig.sh input-method` pass with `gpu` as with `cpu`.

### Development VM and NVIDIA desktop

Not run yet. They run on the desktop when its cores are free, with the same script:

- in the rig, on either machine, from a copy of this tree with the rig's binaries built:
  `forge/test/shell/rig.sh build-layout && forge/test/shell/rig.sh build-toolkit && forge/test/shell/rig.sh measure-chooser`
  (the tables land in `<out>/measure/report.md`; the rig has no render node, so `vello_gpu` runs
  on llvmpipe there);
- nested in the live session of the NVIDIA desktop or of the VM, with both binaries copied out
  of `<out>/bin`, which is where `vello_gpu` meets the real GPU:

  ```sh
  python3 -B forge/test/shell/measure_chooser.py run <out>/bin/athanor-layout-chooser layout.json
  ATHANOR_TOOLKIT_RENDERER=cpu python3 -B forge/test/shell/measure_chooser.py run <out>/bin/athanor-toolkit-chooser toolkit-cpu.json
  ATHANOR_TOOLKIT_RENDERER=gpu python3 -B forge/test/shell/measure_chooser.py run <out>/bin/athanor-toolkit-chooser toolkit-gpu.json
  python3 -B forge/test/shell/measure_chooser.py report system/athanor-toolkit layout.json toolkit-cpu.json toolkit-gpu.json
  ```

  There GTK takes the session's default renderer unless `GSK_RENDERER` is set, and wgpu picks the
  backend unless `WGPU_BACKEND` (`vulkan` or `gl`) is set; the JSON records both, and the report
  lists the adapter.

| Chooser | VM (virgl or llvmpipe) | NVIDIA desktop, nested |
| --- | --- | --- |
| GTK | not run yet | not run yet |
| Toolkit, `vello_cpu` | not run yet | not run yet |
| Toolkit, `vello_gpu` | not run yet | not run yet |

### Lines of the toolkit

`system/athanor-toolkit` at the same tree. Each file belongs to one area in `measure_chooser.py`;
a new file fails the count until it is placed. The driver is `driver.rs`; drawing is counted apart
from it, `render.rs` (`vello_cpu`, and the scene both renderers paint) and `gpu.rs` (`vello_gpu`);
the widgets are `widget.rs`, `focus.rs`, `theme.rs` and `lib.rs`.

| Area | Code | Of which tests | Comments | Blanks |
| --- | --- | --- | --- | --- |
| driver | 653 | 38 | 61 | 62 |
| rendering (`vello_cpu`, and the scene `vello_gpu` shares) | 321 | 69 | 17 | 19 |
| rendering (`vello_gpu`) | 142 | 0 | 24 | 6 |
| widgets | 714 | 156 | 72 | 64 |
| accessibility | 311 | 180 | 20 | 22 |
| text | 142 | 33 | 19 | 26 |
| chooser (example) | 102 | 30 | 16 | 12 |
| test clients (examples: text field, input method) | 111 | 0 | 12 | 11 |
| total | 2496 | 506 | 241 | 222 |

### Open

- The `vello_gpu` path is in every build of the toolkit: 4.34 MiB of the stripped binary even when
  `cpu` runs. A cargo feature could leave it out of builds that never use it; that is a decision
  for task 8's renderer recommendation.
- The rig measures `vello_gpu` on llvmpipe only. Its real cost, and whether it beats `vello_cpu`
  at this window size, is in the VM and NVIDIA desktop columns.
- With `gpu`, a frame wgpu cannot acquire (`Timeout`, `Occluded`, `Outdated` or `Lost`) still
  ends the client with the cause named, as no other error is hidden; a long-lived client would
  skip the frame, or reconfigure or recreate the surface, instead.
- GTK's figures are with cairo, as the rig sets it. A GTK run on its default renderer (GL on
  llvmpipe in the rig) would add a row; it is left to the desktop columns, where the session's
  renderer is the real one.
