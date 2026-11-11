# Spike VL-S5: the tier measurement of VL13

Measurement spike for VL-S5 of `docs/architecture/doc_visual_language.md` (revision 3, VL13),
run on 2026-11-11 against `origin/iso-v0` at `08cdd7c8`. The questions: which frame is timed,
and for how long; the thresholds between M0, M1 and M2; the cost of M2 per frame on an old
integrated GPU, measured rather than estimated; and whether a measurement on the client side may
offer M1 under cosmic-comp. Sections 1 to 3 are findings; a cause that the data suggest but do
not show is marked as inferred. Section 4 holds the proposals, which need the maintainer's
approval through a revision of `doc_visual_language.md`. Section 5 lists what the spike did not
measure.

## 1. Method

The tool is `athanor-sulphur-tier-bench`, a binary of the `athanor-sulphur` crate on the branch
`spike/vl-s5-tier`, kept as the tag `archive/spike/vl-s5-tier-copy` (`ead59e43`), which adds
the `probe-copy` mode of section 2.2 to the tag `archive/spike/vl-s5-tier-probe` (`61482b0c`).
It is not merged. It draws through Sulphur's renderer stack:

- Smithay's `GlesRenderer` on a render node, through GBM, at the pinned revision `023b78b1`;
- a custom texture shader compiled with `compile_custom_texture_shader`, as `look.rs` compiles
  the colour filter.

The scene is the size of an output, drawn whole every frame, so every frame has full damage. It
holds a wallpaper and three translucent windows (alpha 235/255) that move every frame. Both
textures have a pattern that no sampler can skip.

The tool measures seven cases:

- **`scene`:** the scene alone.
- **`dock-N`:** the scene, then a dual Kawase blur of the dock's region, composited back with
  `material.saturate` = 1.2 in the last pass. The region is 1280 × 96 px, centred, 8 px above
  the bottom edge. The blur runs N passes down from half resolution and N passes up, with an
  offset of 2.5 half-texels.
- **`full-N`:** the same blur over the whole output, as the application library would need
  it in M2.

N is 2, 3 or 4. Which N matches `material.blur` = 20 px was not derived nor checked by eye
(section 5).

A frame is timed from its first GL call until its sync point has signalled. There is one wait a
frame, on the last pass, as a compositor waits: the passes of one context complete in order. The
figure is therefore the frame's GPU time plus its submission, an upper bound of the GPU time.

The cases take turns, frame by frame. Each round draws every case once; 30 rounds of warm-up are
followed by 300 measured rounds, 200 on nouveau. With the argument `probe`, the tool draws only
`scene`, `dock-3` and `full-3`, for 30 and 60 rounds: the probe that section 4 proposes.

The turns matter on an integrated GPU, whose clock follows its load. A first run on the UHD 620
drew each case alone, 30 warm-up frames then 300 measured frames, one case after the other. It
timed `scene`, the first case, at 4.43 ms, and `dock-4`, the sixth, at 2.12 ms: less than the
scene alone. No clock was sampled during that run. The inferred cause is that the scene alone
kept the GPU partly idle between frames, so its clock stayed low, and that the clock had risen
by the later, heavier cases. In turns, every case is drawn under the same load.

| Machine | GPU | Driver | Render node |
| --- | --- | --- | --- |
| Laptop | Intel UHD Graphics 620 (Kaby Lake-R GT2, 2017), i7-8550U | i915, Mesa 25.3.6, kernel 7.2.11 azoth | `renderD128` |
| Laptop | NVIDIA GeForce MX150 (GP108M) | nouveau, same Mesa | `renderD129` |
| Desktop | NVIDIA GeForce RTX 4070 Ti SUPER | NVIDIA 615.71.09 | `renderD128` |
| Desktop | NVIDIA GeForce RTX 3060 | NVIDIA 615.71.09 | `renderD129` |

On the laptop, the cosmic-comp session on tty1 was idle during the runs. The i915 clock
(`gt_act_freq_mhz`, at most 1150 MHz) was sampled every 0.25 s during the long run at
1920 × 1080: of 40 samples, two read 300 MHz, one 1150 MHz, and the other 37 between 600 and
867 MHz, below the maximum. The clock was not sampled during the long run at 2560 × 1440 of the
table below. A later long run at 2560 × 1440, sampled the same way, read 1150 MHz in 55 of 56
samples and 300 MHz in one, and its medians were within 3 % of the table's. The MX150's clock was not sampled.

## 2. Results

### 2.1 The long runs

Times are in milliseconds. Each cell is median / 95th percentile.

| Case | UHD 620, 1920 × 1080 | UHD 620, 2560 × 1440 | MX150 nouveau, 1920 × 1080 | RTX 4070 Ti S, 1920 × 1080 | RTX 4070 Ti S, 3840 × 2160 | RTX 3060, 1920 × 1080 |
| --- | --- | --- | --- | --- | --- | --- |
| `scene` | 2.45 / 2.82 | 4.23 / 4.55 | 11.24 / 11.62 | 0.16 / 0.18 | 0.33 / 0.35 | 0.36 / 0.38 |
| `dock-2` | 2.81 / 3.23 | 4.48 / 4.84 | 11.68 / 12.56 | 0.31 / 0.32 | 0.42 / 0.43 | 0.61 / 0.63 |
| `dock-3` | 2.96 / 3.43 | 4.64 / 4.97 | 12.38 / 12.82 | 0.42 / 0.45 | 0.47 / 0.63 | 0.72 / 0.73 |
| `dock-4` | 3.09 / 3.68 | 4.70 / 5.25 | 12.45 / 12.81 | 0.53 / 0.54 | 0.52 / 0.60 | 0.83 / 0.86 |
| `full-2` | 5.77 / 6.61 | 8.33 / 8.70 | 18.42 / 18.82 | 0.33 / 0.36 | 0.58 / 0.64 | 0.73 / 0.75 |
| `full-3` | 5.88 / 6.55 | 8.63 / 9.00 | 18.59 / 19.50 | 0.44 / 0.46 | 0.62 / 0.72 | 0.85 / 0.93 |
| `full-4` | 5.89 / 6.57 | 8.76 / 8.89 | 18.43 / 19.23 | 0.56 / 0.74 | 0.67 / 0.80 | 0.97 / 0.98 |

The cost of M2's blur per frame is the blur case minus `scene`, at the median:

| | UHD 620, 1080p | UHD 620, 1440p | MX150 nouveau | RTX 4070 Ti S, 1080p | RTX 3060 |
| --- | --- | --- | --- | --- | --- |
| Dock region, 3 to 4 passes | +0.51 to +0.64 | +0.41 to +0.47 | +1.14 to +1.20 | +0.26 to +0.37 | +0.36 to +0.47 |
| Whole output, 3 to 4 passes | +3.43 to +3.44 | +4.40 to +4.53 | +7.19 to +7.35 | +0.28 to +0.40 | +0.49 to +0.61 |

On the RTX cards a frame costs about 0.15 ms at least, each pass adds about 0.05 ms, and the dock
and the whole output cost almost the same. The inferred cause is that these figures are
submission and fence latency more than GPU work.

### 2.2 The proposed probe, from an idle GPU

The probe (`probe`: `scene`, `dock-3` and `full-3`, 30 rounds of warm-up and 60 measured), and
`probe-copy`, the same with the backdrop's copy, which is the probe of section 4, were run as a separate process after 15 s of idle each time, at 95th percentiles in
milliseconds. Wall time is the whole process: opening the device, compiling the shaders,
uploading the textures and drawing.

| GPU and size | Runs | `scene` p95 | `dock-3` p95 | `full-3` p95 | Wall time |
| --- | --- | --- | --- | --- | --- |
| UHD 620, 1920 × 1080 | 10 | 2.76 to 2.98 | 3.33 to 3.61 | 6.49 to 7.56 | 1.12 to 1.27 s |
| UHD 620, 2560 × 1440 | 3 | 4.54 to 4.65 | 5.05 to 5.10 | 9.05 to 9.10 | 1.73 to 1.75 s |
| MX150 nouveau, 1920 × 1080 | 1 | 11.79 | 12.98 | 19.34 | 4.45 s |
| UHD 620, 1920 × 1080, `probe-copy` | 8 | 1.95 to 2.91 | 2.34 to 3.54 | 5.18 to 7.66 | 1.21 to 1.29 s |
| UHD 620, 2560 × 1440, `probe-copy` | 1 | 4.39 | 4.88 | 10.46 | not timed |

Wall time was taken in the first five `probe` runs at 1920 × 1080, in two of the three at
2560 × 1440, in the MX150's run, and in three of the `probe-copy` runs at 1920 × 1080.
`probe-copy` is the same probe with the copy of the backdrop: before each blur, the region is
copied out of the scene into a texture of its own with one textured draw, as a compositor drawing
into a swapchain buffer copies what lies under a surface, and the blur reads that texture. Five
`probe` and `probe-copy` runs were made in pairs, without sampling the clock. Over the five
pairs, the copy changed the whole output's blur over the scene, at the median, by −0.45, +0.10,
+0.28, −0.37 and +0.29 ms, and the dock's by −0.20 to +0.12 ms. That spread, about ±0.4 ms, is
noise, probably of runs at different clocks (inferred), and it is larger than the copy's cost at 1920 × 1080: the
pairs do not resolve that cost. At 2560 × 1440 one pair gave the copy 1.15 ms for the whole
output, which one pair does not tell from the same noise. The tiers of section 4 are read from
the `probe-copy` figures themselves, which include the copy whatever it costs.

During three `probe-copy` runs at 1920 × 1080 the i915 clock was sampled about every 60 ms,
from 0.2 s before the process started. It read 300 MHz until 66 to 126 ms into the process and
at least 567 MHz from 126 to 183 ms. From then it read between 567 and 917 MHz; the last sample
of two runs, taken at or just after the end of the process, read 300 MHz. The first five `probe` runs were sampled
about every 50 ms with the same rise, without a recorded start. Compared like for like, without
the copy, the `probe` runs' `full-3` medians are 6.24 to 6.89 ms against 5.88 ms in the long run
at 1920 × 1080, 6 to 17 % above: a short probe overstates the cost, which errs towards a lower
tier.

## 3. Findings

1. **On the old integrated GPU, M2 is affordable for the dock and close to the limit for a
   full-screen surface.** On the UHD 620 at 1080p and 60 Hz, a frame lasts 16.67 ms. The dock's
   live blur costs about 0.6 ms of it. A full-screen live blur costs about 3.4 ms, a fifth of the
   frame. At 1440p it costs about 4.5 ms. The copy of the backdrop costs less at 1080p than the
   runs of section 2.2 resolve, about ±0.4 ms; the probe that includes it keeps the UHD 620 in
   M2 at 1080p (section 4).
2. **The GPU's kind does not predict its frame time.** Under nouveau, the MX150 draws the bare
   scene in 11.2 ms, 4.6 times slower than the integrated GPU beside it. The scene alone takes
   two thirds of a 60 Hz frame. The probable cause, not measured, is that nouveau does not
   reclock Pascal GPUs, which then run at their boot clock. This one pair of GPUs agrees with VL13's
   choice by measured frame time rather than by brand.
3. **The time must be measured under load, and a short probe is enough.** An integrated GPU's
   clock follows its load (section 1). From an idle GPU, the proposed probe of about 90 rounds
   without the copy reads 6 to 17 % above a run of 330 rounds: the clock rises within its warm-up, and what is
   left of the difference overstates the cost (section 2.2).
4. **M2's memory is about VL13's estimate plus the levels.** VL13 counts one more buffer per
   output, about 8 MB at 1920 × 1080 in RGBA (8.3 MB): the backdrop, copied into a texture the
   blur can sample. The long runs blur a scene that is already a texture and do not copy it;
   `probe-copy` does, and its time is in section 2.2. The copy's buffer is the size of the
   region, 8.3 MB for the whole output and 0.49 MB for the dock's. The blur's levels,
   from half resolution, add 2.72 MB for 3 levels and 2.75 MB for 4 at 1920 × 1080, and 0.16 MB
   for the dock's region. These figures are calculated from the levels' sizes, not measured.
5. **A client can time what M1 costs it, and only that** (reasoned, not measured). M1's work
   that grows with the frame is in the client's own buffer: one blur of the wallpaper at each
   change of wallpaper, and one draw a frame of the snapshot under the surface. A client under
   cosmic-comp can time both itself. It cannot time the compositor's work. That work includes
   blending a translucent surface, which the bench's scene includes. It also includes losing
   direct scan-out under a full-screen translucent surface, which the spike did not measure. So
   a client-side measurement can never offer M2. Sal draws on the CPU by default (`vello_cpu`,
   `system/athanor-sal/Cargo.toml`). On the CPU, the cost of the wallpaper's blur is unknown: the
   GPU figures of section 2 do not apply to it.

## 4. Proposals

These proposals settle the open points of VL13. A revision of `doc_visual_language.md` and
the maintainer's approval are needed before they are implemented.

- **What is timed:** a probe, drawn off screen by the compositor at the output's mode, on that
  output's GPU. Each round draws the output's current scene, then the scene with the dock's
  region copied out and blurred, then the scene with the whole output copied out and blurred,
  as `probe-copy` does. Each is timed from its first GL call until its sync point signals, as in
  section 1. The probe never blocks the compositor's event loop: it draws in a GL context of its
  own, on a thread of its own. It still shares the GPU with the frames drawn meanwhile on any
  output of that GPU, and slows them for as long as it runs, 1.2 s on the UHD 620 and up to
  its 3 s deadline (section 5).
- **When, and for how long:** at each output enable, mode change and GPU change, before M2 or
  M1 is first offered on that output. The probe draws 30 rounds of warm-up and 60 measured
  rounds. It took 1.1 to 1.3 s on the UHD 620 at 1080p and 1.7 s at 1440p (section 2.2). Its
  deadline is 3 s, and a probe that fails or runs out of time is M0, as VL13 already says. On
  nouveau the probe took 4.4 s, so the deadline gives M0, the tier its figures give anyway.
- **Thresholds**, with T the output's frame interval (16.67 ms at 60 Hz), on the 95th percentiles
  of the probe:
  - **M2:** the dock case ≤ T/4, and the full-output case ≤ T/2;
  - **M1:** otherwise, when the compositor renders on a GPU and the scene ≤ T/2;
  - **M0:** otherwise, and always under software rendering.

  Applied to section 2 at 60 Hz (T/4 = 4.17 ms, T/2 = 8.33 ms):
  - The RTX cards are M2 at every size measured.
  - The UHD 620 is M2 at 1920 × 1080: in the probe with the copy, the dock takes 2.34 to 3.54
    and the full output 5.18 to 7.66. The full output's margin is 0.67 ms at worst, so the
    UHD 620 is close to the limit of M2.
  - On a 2560 × 1440 monitor the UHD 620 is M1: with the copy, the dock takes 4.88 > 4.17 and
    the full output 10.46 > 8.33.
  - nouveau on the MX150 is M0: the scene takes 11.79 > 8.33.

  At 120 Hz (T/4 = 2.08 ms) the UHD 620 at 1080p becomes M1, because the dock takes 2.34 or more.
  In the long runs, 4 passes in place of 3 change no tier above.
- **The live check:** while an output is in M2, the compositor also times its real frames. The
  median of its last 120 damaged frames is compared with T/2. When the median exceeds T/2, the
  output falls to M1 and stays there until the next enable, mode change or GPU change, whose
  probe gives it its tier again. No probe runs on a timer, so an output whose real frames cost
  more than its probe cannot move between M1 and M2 over and over. Frames drawn while a probe
  runs on the same GPU are not counted, so another output's probe cannot move it. A transient
  load, a game or a video, can: the output then stays in M1 until its next enable, mode change
  or GPU change. That cost is accepted for a tier that cannot flap; a way back to M2 within a
  mode would need a revision of its own. A real frame can be timed
  through the swapchain element's sync point. `udev.rs` waits on it today only when the plane cannot take the fence; otherwise KMS
  takes it as IN_FENCE_FD. The proposal is to export it as a fence fd as well, polled in the
  event loop without blocking. No such code exists yet.
- **The blur:** dual Kawase from half resolution, with `material.saturate` applied in the last
  pass, as the tool does. Whether 3 or 4 passes at an offset of 2.5 half-texels match
  `material.blur` = 20 px is open. It is settled by eye against a Gaussian blur of 20 px before
  the implementation, and changes no tier above.
- **Client side under cosmic-comp:** a toolkit client may offer M1 on its own measurement. It
  times its own frames, which must stay ≤ T/2, and its own blur of the wallpaper snapshot, under
  the same 3 s deadline. A failure keeps M0, as VL13 says for a wallpaper that cannot be blurred.
  Under cosmic-comp, M2 is never chosen, as VL13 already says.

## 5. Not measured

- **The copy as Sulphur will make it.** `probe-copy` copies with one textured draw. Sulphur may
  copy otherwise, with a blit or by drawing the scene straight into a texture; the probe times
  whichever it uses. The copy's memory is calculated (finding 4), not measured.
- **A probe beside live frames.** Every probe ran alone. A probe drawn while the output draws
  its frames shares the GPU with them, which slows both for as long as the probe runs, up to its
  3 s deadline. How much it slows them was not measured.
- **Scan-out.** The probe ran off screen. KMS commits, the cursor plane and direct scan-out were
  not measured, nor what a translucent full-screen surface costs by preventing direct scan-out.
- **A real session's scene.** The scene is synthetic, full damage every frame. A real session
  with partial damage costs less, so the figures are an upper bound for the blur and a fair
  bound for the scene.
- **A loaded laptop.** On battery, under thermal throttling or with another GPU client active,
  the clock may sit lower. The live check of section 4 is the guard against that.
- **The look of the blur.** Its quality and whether 3 or 4 passes match 20 px were not checked.
- **Sal's blur for M1.** The cost of Sal's own blur of the wallpaper, on `vello_cpu` or on
  `wgpu`, was not measured: it belongs to the implementation of M1 in Sal.
- **The MX150's clock**, and nouveau's other GPUs.
- **Other old integrated GPUs.** The only old integrated GPU measured is the UHD 620.
