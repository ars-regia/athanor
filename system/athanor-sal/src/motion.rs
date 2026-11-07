//! A surface's motion: its opacity and its offset, moved from one pose to another along a curve
//! over a duration, one step per frame callback of the surface (doc_osd.md OD10, requirement S3).
//! The values come from the application, Athanor's shell taking them from `athanor-style`
//! (doc_visual_language.md VL9); the toolkit reads no setting itself, the reduced-motion switch
//! included ([`crate::Shell::set_reduced_motion`]).

use std::time::Duration;

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing curve, from (0, 0) to (1, 1). `x1` and `x2` lie
/// in 0 to 1, as CSS requires, so that the curve gives one value for each moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve(pub [f32; 4]);

impl Curve {
    /// Progress proportional to time.
    pub const LINEAR: Curve = Curve([0.0, 0.0, 1.0, 1.0]);

    /// The progress, from 0 to 1, at `time`, the share of the duration elapsed, from 0 to 1.
    pub fn at(&self, time: f32) -> f32 {
        let [x1, y1, x2, y2] = self.0;
        let (x1, x2) = (x1.clamp(0.0, 1.0), x2.clamp(0.0, 1.0));
        // The ends exactly, which the halving below only approaches.
        if time <= 0.0 {
            return 0.0;
        }
        if time >= 1.0 {
            return 1.0;
        }
        let x = time;
        // x(t) rises from 0 to 1 when x1 and x2 lie in 0 to 1: halving finds its t to within
        // 2^-30, far under a physical pixel or one step of an 8-bit alpha.
        let (mut low, mut high) = (0.0_f32, 1.0_f32);
        for _ in 0..30 {
            let mid = (low + high) / 2.0;
            if bezier(x1, x2, mid) < x {
                low = mid;
            } else {
                high = mid;
            }
        }
        bezier(y1, y2, (low + high) / 2.0)
    }
}

/// One coordinate of the curve from 0 through `p1` and `p2` to 1, at the parameter `t`.
fn bezier(p1: f32, p2: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

/// How a surface's content is drawn: its opacity, from 0 to 1, and how far it is moved from its
/// place, in logical pixels, right and down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// From 0, invisible, to 1, opaque.
    pub opacity: f32,
    /// Right and down, in logical pixels.
    pub offset: (f32, f32),
}

impl Pose {
    /// Opaque and in place: how a surface is drawn without motion.
    pub const REST: Pose = Pose {
        opacity: 1.0,
        offset: (0.0, 0.0),
    };
}

impl Default for Pose {
    fn default() -> Self {
        Self::REST
    }
}

/// A move from the pose `from` to the pose `to` along `curve` over `duration`. With reduced
/// motion the offset takes `to`'s at once and only the opacity moves, over `reduced`
/// (doc_visual_language.md VL9: fades of about 100 ms); a `reduced` of zero takes the end pose at
/// once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// Where the motion starts.
    pub from: Pose,
    /// Where it ends, and the surface stays.
    pub to: Pose,
    /// How long it lasts with full motion.
    pub duration: Duration,
    /// How it advances over that time.
    pub curve: Curve,
    /// How long the opacity fades with reduced motion.
    pub reduced: Duration,
}

impl Motion {
    /// The pose `elapsed` after the motion started, with reduced motion when `reduced`; `to`'s
    /// from its end on.
    pub fn at(&self, elapsed: Duration, reduced: bool) -> Pose {
        let duration = self.length(reduced);
        if elapsed >= duration {
            return self.to;
        }
        let progress = self
            .curve
            .at(elapsed.as_secs_f32() / duration.as_secs_f32());
        let between = |from: f32, to: f32| from + (to - from) * progress;
        let (from, to) = (self.from.offset, self.to.offset);
        Pose {
            opacity: between(self.from.opacity, self.to.opacity),
            offset: if reduced {
                to
            } else {
                (between(from.0, to.0), between(from.1, to.1))
            },
        }
    }

    /// Whether the motion has reached its end `elapsed` after it started.
    pub fn ended(&self, elapsed: Duration, reduced: bool) -> bool {
        elapsed >= self.length(reduced)
    }

    fn length(&self, reduced: bool) -> Duration {
        if reduced {
            self.reduced
        } else {
            self.duration
        }
    }
}

/// A motion under way on a surface, timed by the surface's frame callbacks: it starts at the
/// first callback after it was asked for, as the first pose reaches the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Running {
    pub(crate) motion: Motion,
    /// The time of the first frame callback, in the callback's milliseconds.
    start: Option<u32>,
}

impl Running {
    pub(crate) fn new(motion: Motion) -> Self {
        Self {
            motion,
            start: None,
        }
    }

    /// The pose to draw after the frame callback of `time`, in its milliseconds, and whether the
    /// motion ended with it. The milliseconds wrap around, as wl_callback's do.
    pub(crate) fn frame(&mut self, time: u32, reduced: bool) -> (Pose, bool) {
        let start = *self.start.get_or_insert(time);
        let elapsed = Duration::from_millis(u64::from(time.wrapping_sub(start)));
        (
            self.motion.at(elapsed, reduced),
            self.motion.ended(elapsed, reduced),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: Curve = Curve([0.2, 0.0, 0.0, 1.0]);
    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// The overlay's entry of doc_osd.md OD10: from transparent and 12 px down to its place in
    /// 200 ms, opacity only in 100 ms with reduced motion.
    fn entry() -> Motion {
        Motion {
            from: Pose {
                opacity: 0.0,
                offset: (0.0, 12.0),
            },
            to: Pose::REST,
            duration: MS(200),
            curve: ENTRY,
            reduced: MS(100),
        }
    }

    #[test]
    fn a_curve_runs_from_0_to_1_and_matches_css() {
        // Values of CSS's cubic-bezier, solved to 1e-5 by bisection in double precision.
        let cases = [
            (ENTRY, [0.15625, 0.60722, 0.87783, 0.97548]),
            (
                Curve([0.25, 0.46, 0.45, 0.94]),
                [0.18725, 0.45338, 0.77132, 0.93605],
            ),
            (
                Curve([0.25, 0.1, 0.25, 1.0]),
                [0.0948, 0.40851, 0.8024, 0.96046],
            ),
        ];
        for (curve, expected) in cases {
            assert!(
                close(curve.at(0.0), 0.0) && close(curve.at(1.0), 1.0),
                "{curve:?}"
            );
            for (time, value) in [0.1, 0.25, 0.5, 0.75].into_iter().zip(expected) {
                assert!(
                    close(curve.at(time), value),
                    "{curve:?} at {time}: {}",
                    curve.at(time)
                );
            }
        }
        assert!(close(Curve::LINEAR.at(0.3), 0.3));
        assert_eq!(
            (ENTRY.at(0.0), ENTRY.at(1.0)),
            (0.0, 1.0),
            "the ends exactly"
        );
        // Outside 0 to 1, the ends.
        assert!(close(ENTRY.at(-1.0), 0.0) && close(ENTRY.at(2.0), 1.0));
    }

    #[test]
    fn a_motion_moves_from_its_start_to_its_end_and_stays() {
        let motion = entry();
        assert_eq!(motion.at(MS(0), false), motion.from);
        let half = motion.at(MS(100), false);
        assert!(close(half.opacity, ENTRY.at(0.5)), "{half:?}");
        assert!(
            close(half.offset.1, 12.0 * (1.0 - ENTRY.at(0.5))),
            "{half:?}"
        );
        assert!(!motion.ended(MS(199), false));
        for later in [MS(200), MS(5000)] {
            assert_eq!(motion.at(later, false), Pose::REST);
            assert!(motion.ended(later, false));
        }
    }

    #[test]
    fn reduced_motion_fades_in_place_over_its_own_time() {
        let motion = entry();
        let half = motion.at(MS(50), true);
        assert_eq!(half.offset, (0.0, 0.0), "no rise with reduced motion");
        assert!(close(half.opacity, ENTRY.at(0.5)), "{half:?}");
        assert!(motion.ended(MS(100), true) && !motion.ended(MS(100), false));
        assert_eq!(motion.at(MS(100), true), Pose::REST);
        // A reduced time of zero is the end pose at once.
        let instant = Motion {
            reduced: Duration::ZERO,
            ..motion
        };
        assert_eq!(instant.at(MS(0), true), Pose::REST);
        assert!(instant.ended(MS(0), true));
    }

    #[test]
    fn a_running_motion_starts_at_its_first_frame_callback_and_survives_the_wrap() {
        let mut running = Running::new(entry());
        // The first callback comes once the first pose is on screen: elapsed zero.
        let start = u32::MAX - 50;
        assert_eq!(running.frame(start, false), (entry().from, false));
        let (pose, ended) = running.frame(start.wrapping_add(100), false);
        assert!(!ended && close(pose.opacity, ENTRY.at(0.5)), "{pose:?}");
        // The callback's milliseconds wrapped past u32::MAX.
        assert_eq!(
            running.frame(start.wrapping_add(200), false),
            (Pose::REST, true)
        );
        // The switch is read at each frame: turned on midway, the motion ends at the reduced time.
        let mut running = Running::new(entry());
        running.frame(1000, false);
        assert_eq!(running.frame(1100, true), (Pose::REST, true));
    }
}
