# Spike T1: the layout chooser on the own toolkit

Results of milestone T1 of `doc_own_stack.md` (section 5), plan
`docs/superpowers/plans/2026-10-17-spike-t1-toolkit.md`: the chooser against the GTK goldens
(task 4), the AT-SPI and Orca comparison (task 5), the measurements (task 7), and, for revision 2
(task 8), the renderer recommendation, the AccessKit gaps met and what revision 2 must change.

## Summary

- **Renderer: `vello_cpu`.** It drew the chooser faster and in less memory than `vello_gpu` on
  every machine measured, the NVIDIA desktop included. `vello_gpu` stays in the crate behind the
  `gpu` cargo feature, off by default.
- **Accessibility:** the chooser passes the AT-SPI and Orca comparisons in English and Italian.
  One gap is AccessKit's: a group is exposed as `panel`, where GTK exposes `grouping`, so Orca says
  "panel". The comparison counts the two roles as equal by the maintainer's decision of
  2026-10-17, not because Orca speaks them alike (section "AccessKit gaps").
- **Cost:** 2532 lines of Rust, 510 of them tests. Against GTK, the chooser starts in 22 ms
  against 302 ms and holds 18 MiB against 270 MiB on the NVIDIA desktop. The binary is larger,
  as the toolkit links its Rust dependencies statically where GTK maps 150 shared objects.

## Task 4: the chooser against the GTK goldens

The toolkit chooser was drawn in the GTK chooser's twelve cases (English, German and right to
left; scales 1.0 and 1.5; light and dark). It differs in each of them; its own goldens are
`forge/test/shell/golden/toolkit-chooser`. The visible differences, which the styling work closes:

| # | Toolkit | GTK |
| --- | --- | --- |
| 1 | No compositor border or shadow around the window | Border and shadow |
| 2 | A white title bar, regular-weight title, round grey Minimize and Close buttons | A grey-beige bar, bold title, bare icons |
| 3 | Headings in regular weight, dark ink | 11.5 px semi-bold, grey ink |
| 4 | Toggle buttons: grey fill, no border, radius about 6, height 30 | Near-white fill, 1 px border, radius 8, minimum height 34 |
| 5 | The selected purple differs slightly | Calmo's accent |
| 6 | Off-white background | White |
| 7 | The system sans | Calmo's font |
| 8 | Wider buttons, larger padding: the German window about 650 px wide | 14 px padding, about 614 px |
| 9 | No dark variant: the six dark cases draw light | Dark variant |
| 10 | Right to left: rows of 42 px, as Parley's locale fallback font gives a Latin line 29 px of height under Arabic | Rows of 30 px |
| 11 | The window sits lower on the screen | (a different compositor places it) |

## Task 5: AT-SPI and Orca

Run again on 2026-10-18 on the desktop, in the rig, with `ATHANOR_CHOOSER_BIN` set to the toolkit's
chooser: `rig.sh atspi chooser`, `rig.sh atspi chooser it`, `rig.sh orca chooser`,
`rig.sh orca chooser it`. All four pass. The references are the GTK chooser's, under
`forge/test/shell/golden/atspi` and the Orca recipe's.

| Element (English / Italian) | GTK | Toolkit | Result |
| --- | --- | --- | --- |
| Window "Layout" / "Disposizione" | frame | frame, also `focusable` and `focused` | same |
| Groups "Style", "Panel", "Dock" / "Stile", "Pannello", "Dock" | grouping, `vertical` | panel, `vertical` | note: role; Orca says "Style panel" for "Style grouping", "Stile pannello" for "Stile raggruppamento" |
| Group headings (three) | label | label | same |
| Eight choices (Island ... None / Isola ... Nessuno) | toggle button, `pressed` on the current pick | toggle button, `pressed` on the current pick | same; Orca says "toggle button pressed" / "pulsante di commutazione premuto" as for GTK |
| Window title "Layout" / "Disposizione" | label | label | same |
| Minimize, Close | button | button | same; both choosers name them in English under Italian |

Every element adds the `enabled` state, which GTK leaves out; an extra state is not a loss. No
name, role, state or value of the GTK transcript is missing, with two exceptions the comparison
makes on purpose: the momentary states `focused` and `active` are not compared (GTK's "Island"
has `focused`, the toolkit puts focus on the frame), and the role `grouping` is matched by
`panel` under the maintainer's decision of 2026-10-17.


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
row, as the toolkit now loads libwayland-client at start-up. These rows are the build with the
`gpu` feature. The default build, without it, was measured only for its size, 5.85 MiB stripped
(section "Renderer"); its start-up, memory and shared objects were not measured.

wgpu reaches the window through libwayland-client's pointers, so the `gpu` feature enables
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

### NVIDIA desktop, nested in the session

Measured on 2026-10-18 on the maintainer's desktop (RTX 4070 Ti SUPER, NVIDIA 615.71.09, image
`athanor-system-nvidia` 43.20261017.393), nested in the live session on cosmic-comp, on four cores
(`taskset -c 0-3`), with the binaries of `rig.sh build-toolkit` at 526536e7 and the commands of
the development VM section below. GTK took the session's default renderer, Vulkan on the NVIDIA
GPU; wgpu picked `NVIDIA GeForce RTX 4070 Ti SUPER` (Vulkan, discrete GPU). The window buffers
are as tall as in the rig and wider (695 against 448 pixels for GTK, 709 against 420 for the
toolkit); the cause of the extra width was not investigated.

| Chooser | Stripped binary | Start-up to first frame, median (min-max, runs) | Window buffer | RSS after 1 min | PSS after 1 min | Shared objects mapped, their on-disk size |
| --- | --- | --- | --- | --- | --- | --- |
| GTK (`athanor-layout-chooser`, Vulkan) | 1.34 MiB | 302 ms (293-328, 5) | 695x326 | 269.9 MiB | 132.6 MiB | 150, 444.5 MiB |
| Toolkit, `vello_cpu` | 10.18 MiB | 22 ms (21-22, 5) | 709x293 | 18.4 MiB | 10.6 MiB | 20, 11.6 MiB |
| Toolkit, `vello_gpu`, wgpu Vulkan on the RTX 4070 Ti SUPER | 10.18 MiB | 296 ms (283-510, 5) | 709x293 | 228.4 MiB | 112.6 MiB | 77, 474.5 MiB |

The same day, the rig on the desktop (llvmpipe, as on the laptop) gave GTK 109 ms and 48.1 MiB,
`vello_cpu` 63 ms and 16.9 MiB, `vello_gpu` 121 ms and 142.1 MiB: the laptop's order again.

On the real GPU, `vello_gpu`'s start-up and memory are those of a Vulkan device: the NVIDIA
driver's libraries and the device's setup, which `vello_cpu` never loads. At the chooser's size the
CPU draws a frame well inside the first-frame time; the GPU path pays for its device before its
first frame.

### Development VM

Not measured. The rig (Mesa's llvmpipe, no GPU) on the laptop and on the desktop already gives the
case without a GPU, and the VM's virgl adds a host GPU behind a paravirtual driver, which no
Athanor user meets on hardware. Booting the VM on the desktop costs memory and cores the 0.9
hardware matrix needs; the column is measured if revision 2 asks for virgl figures. The commands,
run inside the guest's session (or any live session):

```sh
python3 -B forge/test/shell/measure_chooser.py run <out>/bin/athanor-layout-chooser layout.json
ATHANOR_TOOLKIT_RENDERER=cpu python3 -B forge/test/shell/measure_chooser.py run <out>/bin/athanor-toolkit-chooser toolkit-cpu.json
ATHANOR_TOOLKIT_RENDERER=gpu python3 -B forge/test/shell/measure_chooser.py run <out>/bin/athanor-toolkit-chooser toolkit-gpu.json
python3 -B forge/test/shell/measure_chooser.py report system/athanor-toolkit layout.json toolkit-cpu.json toolkit-gpu.json
```

GTK takes the session's default renderer unless `GSK_RENDERER` is set, and wgpu picks the backend
unless `WGPU_BACKEND` (`vulkan` or `gl`) is set; the JSON records both, and the report lists the
adapter. The binaries come from `rig.sh build-toolkit`, which builds them with the `gpu` feature.

### Lines of the toolkit

`system/athanor-toolkit` at 526536e7, before the `gpu` feature. Each file belongs to one area in
`measure_chooser.py`; a new file fails the count until it is placed. The driver is `driver.rs`;
drawing is counted apart from it, `render.rs` (`vello_cpu`, and the scene both renderers paint) and
`gpu.rs` (`vello_gpu`); the widgets are `widget.rs`, `focus.rs`, `theme.rs` and `lib.rs`.

| Area | Code | Of which tests | Comments | Blanks |
| --- | --- | --- | --- | --- |
| driver | 679 | 42 | 71 | 62 |
| rendering (`vello_cpu`, and the scene `vello_gpu` shares) | 321 | 69 | 17 | 19 |
| rendering (`vello_gpu`) | 152 | 0 | 31 | 7 |
| widgets | 714 | 156 | 72 | 64 |
| accessibility | 311 | 180 | 20 | 22 |
| text | 142 | 33 | 19 | 26 |
| chooser (example) | 102 | 30 | 16 | 12 |
| test clients (examples: text field, input method) | 111 | 0 | 12 | 11 |
| total | 2532 | 510 | 258 | 223 |

## Task 8: for revision 2

### The renderer

**Recommendation: `vello_cpu` by default; `vello_gpu` behind the `gpu` cargo feature, off by
default** (maintainer decision, 2026-10-18). This settles the open decision of `doc_own_stack.md`
section 7, which recommended `vello_gpu` where T1 measured it faster: it was faster nowhere.

- On the NVIDIA desktop `vello_cpu` drew its first frame 13 times sooner (22 ms against 296 ms)
  and held 12 times less memory (18.4 MiB against 228.4 MiB RSS). In the rig, on llvmpipe, the
  margin is smaller and in the same direction.
- Without the feature the toolkit links neither wgpu, naga nor `vello_gpu`: the chooser built
  without it is 5.85 MiB stripped, against 10.18 MiB with it.
- `rig.sh build-toolkit` builds and tests both, and builds the rig's binaries with the feature, so
  `ATHANOR_TOOLKIT_RENDERER=gpu` keeps working in the rig and in `measure_chooser.py`. Without the
  feature the variable's `gpu` is refused with the reason. CI's clippy (`--all-features`) lints
  the GPU path; no CI job runs its tests.
- The feature is turned on for a surface only when that surface is measured faster with it: a
  large or animated surface, a full-screen one at a high scale. The chooser's figures do not
  carry over.

### A frame wgpu cannot acquire

With `gpu`, a frame wgpu cannot acquire (`Timeout`, `Occluded`, `Outdated` or `Lost`) ends the
client with the cause named. It stays so while the feature is off by default (maintainer
decision, 2026-10-18). Turning the feature on by default for any surface requires first: skip the
frame on `Timeout` and `Occluded`, reconfigure the surface on `Outdated`, recreate it on `Lost`,
each with a test.

### AccessKit gaps

| Gap | Met where | Upstream | Effect |
| --- | --- | --- | --- |
| `Role::Group` (and `Role::RadioGroup`) map to AT-SPI `panel`, not `grouping` (`accesskit_atspi_common` 0.21.0, `node.rs`) | Task 5: the chooser's three groups | No issue found on 2026-10-18; to be opened | Orca says "panel" / "pannello" where GTK gives "grouping" / "raggruppamento" |
| No Collection interface: Orca's structural navigation fails | Not exercised by the chooser | AccessKit#603 (pull request #758) | Known from revision 1; meets the first surface with headings or lists to jump between |
| A combo box's value and expanded state do not reach the reader | Not exercised: the chooser has no combo box | AccessKit#824 | Known from revision 1 |

`localized_role_name` returns the node's role description or an empty string; Orca speaks its own
translated role names, as the Italian run shows, so this was not a loss here.

### What revision 2 must change

1. Section 7: the toolkit's renderer is decided, `vello_cpu`, with `vello_gpu` behind a feature
   and the conditions above for turning it on.
2. Section 1.3: the Vello paragraph says what T1 measured on NVIDIA's driver and on llvmpipe.
3. Section 4 (OS7) and section 8: the group role joins the AccessKit gaps carried until upstream
   takes them. OS8 says whether a role spoken differently ("panel" for "grouping") holds a surface
   back; T1 counted it as a note, by the maintainer's decision of 2026-10-17 recorded in
   `atspi_compare.py`, although Orca speaks the two roles differently.
4. The title bar's Minimize and Close are named in English under Italian in both choosers; the
   toolkit's title bar, which it draws itself (maintainer decision of 2026-10-17), takes its strings from
   `athanor-i18n` before the first surface moves.
5. The styling differences of task 4, and the right-to-left line height, are work for the first
   surface, not decisions; revision 2 lists them as its acceptance.
