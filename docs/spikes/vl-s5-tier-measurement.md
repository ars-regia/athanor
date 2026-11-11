# Spike VL-S5: the tier measurement of VL13

Measurement spike for VL-S5 of `docs/architecture/doc_visual_language.md` (revision 3, VL13),
run on 2026-11-11 against `origin/iso-v0` at `08cdd7c8`. The questions: which frame is timed,
and for how long; the thresholds between M0, M1 and M2; the cost of M2 per frame on an old
integrated GPU, measured rather than estimated; and whether a measurement on the client side may
offer M1 under cosmic-comp. Sections 1 to 3 are findings. Section 4 holds the proposals, which
need the maintainer's approval through a revision of `doc_visual_language.md`. Section 5 lists
what the spike did not measure.

## 1. Method

The tool is `athanor-sulphur-tier-bench`, a binary of the `athanor-sulphur` crate on the branch
`spike/vl-s5-tier`, kept as the tag `archive/spike/vl-s5-tier`. It is not merged. It draws
through Sulphur's renderer stack:

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
  the bottom edge. The blur runs N passes down from half resolution and N passes up.
- **`full-N`:** the same blur over the whole output, as the application library would need
  it in M2.

N is 2, 3 or 4. `material.blur` = 20 px falls between 3 and 4 passes at the offset used (2.5
half-texels).

A frame is timed from its first GL call until its sync point has signalled. There is one wait a
frame, on the last pass, as a compositor waits: the passes of one context complete in order. The
figure is therefore the frame's GPU time plus its submission, an upper bound of the GPU time.

The cases take turns, frame by frame: 30 frames of warm-up, then 300 frames measured, 200 on
nouveau. The order matters on an integrated GPU, which raises its clock with the load. In a first
run, each case was drawn alone, one after the other. There the UHD 620 timed the scene at
4.43 ms and `dock-4` at 2.12 ms, because the clock was still rising during the first cases. In
turns, every case is timed at the same clock.

| Machine | GPU | Driver | Render node |
| --- | --- | --- | --- |
| Laptop | Intel UHD Graphics 620 (Kaby Lake-R GT2, 2017), i7-8550U | i915, Mesa 25.3.6, kernel 7.2.11 azoth | `renderD128` |
| Laptop | NVIDIA GeForce MX150 (GP108M) | nouveau, same Mesa | `renderD129` |
| Desktop | NVIDIA GeForce RTX 4070 Ti SUPER | NVIDIA 615.71.09 | `renderD128` |
| Desktop | NVIDIA GeForce RTX 3060 | NVIDIA 615.71.09 | `renderD129` |

On the laptop, the cosmic-comp session on tty1 was idle during the runs. During the 1920 × 1080
and 2560 × 1440 runs on the UHD 620, the i915 clock read 650 to 870 MHz, out of a maximum of
1150 MHz.

## 2. Results

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

The cost of M2 per frame is the blur case minus `scene`, at the median:

| | UHD 620, 1080p | UHD 620, 1440p | MX150 nouveau | RTX 4070 Ti S, 1080p | RTX 3060 |
| --- | --- | --- | --- | --- | --- |
| Dock region, 3 to 4 passes | +0.51 to +0.64 | +0.41 to +0.47 | +1.14 to +1.21 | +0.26 to +0.37 | +0.36 to +0.47 |
| Whole output, 3 to 4 passes | +3.43 to +3.44 | +4.40 to +4.53 | +7.19 to +7.35 | +0.28 to +0.40 | +0.49 to +0.61 |

On the RTX cards every frame costs at least about 0.15 ms, and every blur pass adds a fixed cost
of the same order. That cost is submission and fence latency, not GPU work: there, the dock and
the whole output cost almost the same.

## 3. Findings

1. **On the old integrated GPU, M2 is affordable for the dock and borderline for a full-screen
   surface.** On the UHD 620 at 1080p and 60 Hz, a frame lasts 16.67 ms. The dock's live blur
   costs about 0.6 ms of it. A full-screen live blur costs about 3.4 ms, a fifth of the frame.
   At 1440p it costs about 4.5 ms.
2. **The GPU's kind does not predict the tier.** Under nouveau, the MX150 draws the bare scene in
   11.2 ms, 4.6 times slower than the integrated GPU beside it: nouveau does not reclock Pascal,
   so the card runs at its boot clock. The scene alone already takes two thirds of a 60 Hz
   frame. This confirms VL13's maintainer decision to choose by measured frame time and not by
   brand, and matters for the nouveau and NVK default image (`-nvidia` variants aside).
3. **The time must be measured under load.** An integrated GPU raises its clock with the load.
   A measurement of a few isolated frames reads the low clock and overstates the cost, by almost
   2× on the UHD 620 (`scene` at 4.43 ms alone, 2.45 ms in turns). Load here means frames drawn
   back to back.
4. **The memory cost is below VL13's estimate.** VL13 estimates one more buffer per output,
   about 8 MB at 1920 × 1080 in RGBA. With the levels starting at half resolution, as `blur =
   20` describes them, a full-output blur needs 2.7 MB at 1920 × 1080 for 4 levels. The dock's
   needs 0.16 MB. Calculated from the levels' sizes, not measured.
5. **A client can measure what M1 costs it, and only that.** M1 adds no work in the compositor
   beyond blending a translucent surface, which the scene already includes. Its costs are in the
   client: one blur of the wallpaper at each change of wallpaper, which costs about what `full-N`
   costs over `scene`, and one draw a frame of the snapshot under the surface. Both happen in the
   client's own buffer, so a client under cosmic-comp can time them itself. It cannot time the
   compositor's load, so a client-side measurement can never offer M2. Sal draws on the CPU by
   default (`vello_cpu`, `system/athanor-sal/Cargo.toml`), so its M1 blur runs on the CPU. That
   CPU blur was not measured (section 5).

## 4. Proposals

These proposals settle the open points of VL13. A revision of `doc_visual_language.md` and
the maintainer's approval are needed before they are implemented.

- **What is timed:** a probe frame, drawn off screen by the compositor at the output's mode, on
  that output's GPU. It holds the output's current scene and, after it, the blur of the dock's
  region, then the blur of the whole output. Each case is timed from its first GL call until its
  sync point signals, the cases taking turns as in section 1.
- **When, and for how long:** at each output enable, mode change and GPU change, before M2 or
  M1 is first offered on that output. The probe draws 30 warm-up rounds and 60 measured
  rounds of the three cases. From section 2, it lasts about 1.1 s on the UHD 620 at 1080p. Its
  deadline is 3 s, and a probe that fails or runs out of time is M0, as VL13 already says. On
  nouveau the probe would take about 3.8 s and give M0 by deadline, the tier its figures give
  anyway.

  Live frames are also timed, through the swapchain element's sync point. `udev.rs` waits on it
  already, when the plane cannot take a fence; otherwise it is exported as a fence fd and
  polled in the event loop, without blocking. The live time is the median of the last 120
  damaged frames of the output. When it exceeds the M2 threshold, the output falls to M1 until
  the next probe.
- **Thresholds**, with T the output's frame interval (16.67 ms at 60 Hz) and the 95th percentile
  of the probe:
  - **M2:** the dock case ≤ T/4, and the full-output case ≤ T/2;
  - **M1:** otherwise, when the compositor renders on a GPU and the scene ≤ T/2;
  - **M0:** otherwise, and always under software rendering.

  Applied to section 2 at 60 Hz:
  - The RTX cards are M2 at every size measured.
  - The UHD 620 is M2 at 1920 × 1080: dock 3.43 ≤ 4.17, full output 6.55 ≤ 8.33. On a
    2560 × 1440 monitor it is M1, because the full output takes 9.00 > 8.33.
  - nouveau on the MX150 is M0: the scene takes 11.62 > 8.33.

  At 120 Hz the UHD 620 at 1080p becomes M1, because the dock takes 3.43 > 2.08.
- **The blur:** dual Kawase, 3 passes at half resolution with an offset of 2.5 half-texels for
  `material.blur` = 20, and `material.saturate` applied in the last pass, as the tool does. Before
  the implementation, the radius is matched to 20 px visually, against a Gaussian reference.
- **Client side under cosmic-comp:** a toolkit client may offer M1 on its own measurement. It
  times its own frame, which must stay ≤ T/2, and its own blur of the wallpaper snapshot, under
  the same 3 s deadline. A failure keeps M0, as VL13 says for a wallpaper that cannot be blurred.
  Under cosmic-comp M2 stays never, as VL13 says.

## 5. Not measured

- **Scan-out.** The probe ran off screen. KMS commits, the cursor plane and direct scan-out were
  not measured, and none of them changes the GPU time of the blur.
- **A real session's scene.** The scene is synthetic, full damage every frame. A real session
  with partial damage costs less, so the figures are an upper bound for the blur and a fair
  bound for the scene.
- **A loaded laptop.** On battery, under thermal throttling or with another GPU client active,
  the clock may sit lower. The live measurement of section 4 is the guard against that.
- **The look of the blur.** Its quality and whether 3 passes match 20 px were not checked
  visually.
- **Sal's CPU blur for M1.** The cost of Sal's own blur of the wallpaper, on `vello_cpu` or on
  `wgpu`, was not measured: it belongs to the implementation of M1 in Sal.
- **Other old integrated GPUs.** The only old integrated GPU measured is the UHD 620.
