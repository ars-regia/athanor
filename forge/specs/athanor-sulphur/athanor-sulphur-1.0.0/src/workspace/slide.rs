//! The workspace switch's slide (doc_visual_language.md VL9): the workspace left and the one
//! activated move together across the output for `DURATION`, eased by `EaseInOutCubic`, a later
//! workspace coming in from the right and an earlier one from the left, mirrored when the bar
//! is drawn right to left, as its strip is (doc_overview.md): the direction is the bar's own,
//! from the catalog it loads (`athanor_i18n::is_rtl`), so the two never disagree. Only the
//! drawing moves: the switch itself is made at once, so the windows left are already out of the
//! space and the input goes to the windows activated, at their places, from the first frame. A
//! switch during a slide starts from where the slide stands, so nothing jumps, and every
//! workspace still on its way out keeps its own motion until it is off the output. Under reduced
//! motion (doc_accessibility.md AX9) there is no slide. The slide is horizontal: Sulphur has no
//! vertical workspace layout, cosmic-comp's `workspace_layout`.

use crate::state::State;
use crate::workspace::WorkspaceId;
use smithay::desktop::Window;
use smithay::output::Output;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::utils::{Logical, Point, Rectangle};
use std::cell::Cell;
use std::time::{Duration, Instant};

/// How long a slide lasts, cosmic-comp's (VL9).
pub(crate) const DURATION: Duration = Duration::from_millis(200);
/// How often a slide asks for a frame; the backend draws no more than one a refresh.
const TICK: Duration = Duration::from_millis(4);

/// The slide of an output, in its user data.
#[derive(Default)]
struct Sliding(Cell<Option<Slide>>);

struct Slide {
    /// The workspace activated, coming to its place.
    incoming: Motion,
    /// The workspaces left that may still be on the output, the oldest first: one entry a
    /// workspace, each gone once its motion is over, so no more than the output's workspaces.
    leaving: Vec<(WorkspaceId, Motion)>,
}

/// A workspace's way across the output, in widths of the output from its place.
#[derive(Clone, Copy)]
struct Motion {
    started: Instant,
    from: f64,
    to: f64,
}

impl Motion {
    fn running(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) < DURATION
    }

    /// Where the workspace stands at `now`.
    fn at(&self, now: Instant) -> f64 {
        let done =
            now.saturating_duration_since(self.started).as_secs_f64() / DURATION.as_secs_f64();
        self.to + (self.from - self.to) * (1.0 - ease(done.min(1.0)))
    }
}

/// `EaseInOutCubic` at `t`, from 0 to 1.
fn ease(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (2.0 - 2.0 * t).powi(3) / 2.0
    }
}

/// The time a slide reads; the tests set it (`advance`).
#[cfg(not(test))]
fn clock() -> Instant {
    Instant::now()
}

/// Whether the bar, whose strip the slide follows, is drawn right to left, read once for the
/// process; the tests set it (`right_to_left`). A catalog of the bar that cannot be read is
/// logged, as the bar logs it, and the slide runs left to right, as the bar's strip does then.
#[cfg(not(test))]
fn rtl() -> bool {
    static RTL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *RTL.get_or_init(|| {
        athanor_i18n::is_rtl("athanor-bar").unwrap_or_else(|(path, error)| {
            tracing::error!(
                path = %path.display(),
                %error,
                "the bar's translations are unavailable: the workspace slide runs left to right"
            );
            false
        })
    })
}

/// Reads the direction at the compositor's start (`run`), so that the first slide's frame
/// reads no catalog.
pub(crate) fn settle_direction() {
    rtl();
}

fn sliding(output: &Output) -> &Cell<Option<Slide>> {
    &output.user_data().get_or_insert(Sliding::default).0
}

/// What `read` makes of the slide in `slot`, which keeps it.
fn reading<T>(slot: &Cell<Option<Slide>>, read: impl FnOnce(Option<&Slide>) -> T) -> T {
    let slide = slot.take();
    let value = read(slide.as_ref());
    slot.set(slide);
    value
}

/// Whether a slide runs on `output` now, so that its frame shows more than the workspace
/// activated (`tearing::conditions`).
pub(crate) fn running(output: &Output) -> bool {
    reading(sliding(output), |slide| {
        slide.is_some_and(|it| it.incoming.running(clock()))
    })
}

/// A window of a workspace left, at its place, and how far from it it is drawn.
pub(crate) type Leaving = (Window, Rectangle<i32, Logical>, Point<i32, Logical>);

/// What a frame shows of an output's slide (`Marks::slides`).
pub struct Shown {
    pub(crate) output: Output,
    /// How far from their places the windows of the workspace activated are drawn.
    shift: Point<i32, Logical>,
    incoming: Vec<Window>,
    /// The windows of the workspaces left, bottom first.
    pub(crate) leaving: Vec<Leaving>,
}

impl Shown {
    /// How far from its place `window`, in the space, is drawn.
    pub(crate) fn shift(&self, window: &Window) -> Point<i32, Logical> {
        if self.incoming.contains(window) {
            self.shift
        } else {
            Point::default()
        }
    }
}

impl State {
    /// Starts the slide of the switch from `left` to `to`, just made (`activate_workspace`).
    pub(crate) fn slide(&mut self, left: WorkspaceId, to: WorkspaceId) {
        let Some(output) = self.output_of_workspace(to) else {
            return;
        };
        let list = self.workspaces(&output);
        let place = |id| list.iter().position(|kept| *kept == id);
        let (Some(from), Some(into)) = (place(left), place(to)) else {
            return;
        };
        if self.bell.reduced_motion {
            return;
        }
        // Taken only once the slide starts: a slot still set has its timer, and one emptied on
        // the returns above would start a second.
        let slot = sliding(&output);
        let now = clock();
        let before = slot.take();
        let ticking = before.is_some();
        let direction = if (into > from) != rtl() { 1.0 } else { -1.0 };
        let (stands, mut leaving) = before
            .filter(|slide| slide.incoming.running(now))
            .map_or((0.0, Vec::new()), |slide| {
                (slide.incoming.at(now), slide.leaving)
            });
        leaving.retain(|(_, motion)| motion.running(now));
        // A workspace still on its way out comes back from where it stands, any other in
        // beside the one left, which goes out from where it stands.
        let back = leaving.iter().find(|(id, _)| *id == to);
        let coming = back.map_or(stands + direction, |(_, motion)| motion.at(now));
        leaving.retain(|(id, _)| *id != to && *id != left);
        let out = Motion {
            started: now,
            from: stands,
            to: -direction,
        };
        leaving.push((left, out));
        let incoming = Motion {
            started: now,
            from: coming,
            to: 0.0,
        };
        slot.set(Some(Slide { incoming, leaving }));
        if ticking {
            return;
        }
        let ticked = output.clone();
        let timer =
            self.loop_handle
                .insert_source(Timer::from_duration(TICK), move |_, _, state| {
                    let slot = sliding(&ticked);
                    state.request_redraw();
                    // The workspaces left started no later than the one activated: they are
                    // over by the time it is.
                    let now = clock();
                    let running =
                        |slide: Option<&Slide>| slide.is_some_and(|it| it.incoming.running(now));
                    if reading(slot, running) {
                        TimeoutAction::ToDuration(TICK)
                    } else {
                        slot.set(None);
                        TimeoutAction::Drop
                    }
                });
        if let Err(error) = timer {
            tracing::warn!(%error, "the workspace slide has no timer: the switch shows at once");
            slot.set(None);
        }
    }

    /// What this frame shows of each output's slide.
    pub(crate) fn slides(&self) -> Vec<Shown> {
        if self.bell.reduced_motion {
            return Vec::new();
        }
        let now = clock();
        self.space
            .outputs()
            .filter_map(|output| {
                let slot = &output.user_data().get::<Sliding>()?.0;
                reading(slot, |slide| self.shown(output, slide?, now))
            })
            .collect()
    }

    fn shown(&self, output: &Output, slide: &Slide, now: Instant) -> Option<Shown> {
        if !slide.incoming.running(now) {
            return None;
        }
        let width = f64::from(self.space.output_geometry(output)?.size.w);
        let active = self.active_workspace(output)?;
        let shift = |by: f64| Point::from(((by * width).round() as i32, 0));
        let incoming = self
            .space
            .elements()
            .filter(|window| self.workspace_of(window) == Some(active))
            .cloned()
            .collect();
        let leaving = slide
            .leaving
            .iter()
            .filter(|(_, motion)| motion.running(now))
            .flat_map(|(id, motion)| {
                let by = shift(motion.at(now));
                self.parked
                    .iter()
                    .filter(move |(window, _)| self.workspace_of(window) == Some(*id))
                    .map(move |(window, kept)| {
                        let located = Rectangle::new(*kept, window.geometry().size);
                        (window.clone(), located, by)
                    })
            })
            .collect();
        Some(Shown {
            output: output.clone(),
            shift: shift(slide.incoming.at(now)),
            incoming,
            leaving,
        })
    }
}

#[cfg(test)]
thread_local! {
    /// The time of the tests, once `advance` has set it.
    static NOW: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Whether the tests' language is written right to left (`right_to_left`).
    static RTL: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
fn clock() -> Instant {
    NOW.with(Cell::get).unwrap_or_else(Instant::now)
}

#[cfg(test)]
fn rtl() -> bool {
    RTL.with(Cell::get)
}

/// Stops the clock the slides read and takes it `by` further on.
#[cfg(test)]
pub(crate) fn advance(by: Duration) {
    let at = clock() + by;
    NOW.with(|time| time.set(Some(at)));
}

/// Makes the bar's language one written right to left, or not.
#[cfg(test)]
pub(crate) fn right_to_left(on: bool) {
    RTL.with(|rtl| rtl.set(on));
}

/// Whether a slide is under way on `output`, with its timer.
#[cfg(test)]
pub(crate) fn under_way(output: &Output) -> bool {
    reading(sliding(output), |slide| slide.is_some())
}
