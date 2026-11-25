//! The workspace switch's slide (doc_visual_language.md VL9): the workspace left and the one
//! activated move together across the output for `DURATION`, eased by `EaseInOutCubic`, a later
//! workspace coming in from the right and an earlier one from the left, mirrored when the bar
//! is drawn right to left, as its strip is (doc_overview.md): the direction is the bar's own,
//! from the catalog it loads (`athanor_i18n::is_rtl`), so the two never disagree. Only the
//! drawing moves: the switch itself is made at once, so the windows left are already out of the
//! space and the input goes to the windows activated, at their places, from the first frame. A
//! switch during a slide starts from where the slide stands, so nothing jumps; the workspace
//! left before that one is no longer drawn. Under reduced motion (doc_accessibility.md AX9)
//! there is no slide. The slide is horizontal: Sulphur has no vertical workspace layout,
//! cosmic-comp's `workspace_layout`.

use crate::dnd::moved;
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

#[derive(Clone, Copy)]
struct Slide {
    started: Instant,
    /// Where the workspace activated starts from its place, in widths of the output.
    from: f64,
    /// 1 towards a later workspace, -1 towards an earlier one.
    direction: f64,
    /// The workspace left, drawn beside the one activated.
    left: WorkspaceId,
}

impl Slide {
    fn running(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) < DURATION
    }

    /// Where the workspace activated stands at `now` from its place, in widths of the output.
    fn at(&self, now: Instant) -> f64 {
        let done =
            now.saturating_duration_since(self.started).as_secs_f64() / DURATION.as_secs_f64();
        self.from * (1.0 - ease(done.min(1.0)))
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
/// process; the tests set it (`right_to_left`).
#[cfg(not(test))]
fn rtl() -> bool {
    static RTL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *RTL.get_or_init(|| athanor_i18n::is_rtl("athanor-bar"))
}

fn sliding(output: &Output) -> &Cell<Option<Slide>> {
    &output.user_data().get_or_insert(Sliding::default).0
}

/// What a frame shows of an output's slide (`Marks::slides`).
pub struct Shown {
    pub(crate) output: Output,
    /// How far from their places the windows of the workspace activated are drawn.
    pub(crate) shift: Point<i32, Logical>,
    pub(crate) incoming: Vec<Window>,
    /// The windows of the workspace left, bottom first, where they are drawn.
    pub(crate) leaving: Vec<(Window, Rectangle<i32, Logical>)>,
}

impl Shown {
    /// Where `window`, at `located`, is drawn.
    pub(crate) fn shifted(
        &self,
        window: &Window,
        located: Rectangle<i32, Logical>,
    ) -> Rectangle<i32, Logical> {
        if self.incoming.contains(window) {
            Rectangle::new(moved(located.loc, self.shift), located.size)
        } else {
            located
        }
    }
}

impl State {
    /// Starts the slide of the switch from `left` to `to`, just made (`activate_workspace`).
    pub(crate) fn slide(&mut self, left: WorkspaceId, to: WorkspaceId) {
        let Some(output) = self.output_of_workspace(to) else {
            return;
        };
        let slot = sliding(&output);
        let now = clock();
        // A slot still set has its timer.
        let ticking = slot.get().is_some();
        let before = slot.take().filter(|slide| slide.running(now));
        let list = self.workspaces(&output);
        let place = |id| list.iter().position(|kept| *kept == id);
        let (Some(from), Some(into)) = (place(left), place(to)) else {
            return;
        };
        if self.bell.reduced_motion {
            return;
        }
        let direction = if (into > from) != rtl() { 1.0 } else { -1.0 };
        // The workspace left starts where it stands, the one activated beside it.
        let stands = before.map_or(0.0, |slide| slide.at(now));
        slot.set(Some(Slide {
            started: now,
            from: stands + direction,
            direction,
            left,
        }));
        if ticking {
            return;
        }
        let ticked = output.clone();
        let timer =
            self.loop_handle
                .insert_source(Timer::from_duration(TICK), move |_, _, state| {
                    let slot = sliding(&ticked);
                    state.request_redraw();
                    if slot.get().is_some_and(|slide| slide.running(clock())) {
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
                let slide = output
                    .user_data()
                    .get::<Sliding>()?
                    .0
                    .get()
                    .filter(|slide| slide.running(now))?;
                let width = f64::from(self.space.output_geometry(output)?.size.w);
                let active = self.active_workspace(output)?;
                let shift = |by: f64| Point::from(((by * width).round() as i32, 0));
                let at = slide.at(now);
                let incoming = self
                    .space
                    .elements()
                    .filter(|window| self.workspace_of(window) == Some(active))
                    .cloned()
                    .collect();
                let leaving = self
                    .parked
                    .iter()
                    .filter(|(window, _)| self.workspace_of(window) == Some(slide.left))
                    .map(|(window, kept)| {
                        let drawn = moved(*kept, shift(at - slide.direction));
                        (
                            window.clone(),
                            Rectangle::new(drawn, window.geometry().size),
                        )
                    })
                    .collect();
                Some(Shown {
                    output: output.clone(),
                    shift: shift(at),
                    incoming,
                    leaving,
                })
            })
            .collect()
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
