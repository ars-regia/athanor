//! The auto-hide knob (doc_bar.md, BR7): "no exclusive zone; it appears after a short
//! delay when the pointer reaches a strip a few pixels wide on its edge." A state machine
//! with no GTK type: the surface feeds it pointer, hold and timer events, runs the one
//! timer it asks for, and draws the dock while `shown`.

use std::time::Duration;

/// The pointer rests this long on the strip before the dock shows: a pass on the way to
/// something else never shows it.
pub const REVEAL_DELAY: Duration = Duration::from_millis(200);
/// The dock stays this long after the pointer leaves it.
pub const HIDE_DELAY: Duration = Duration::from_millis(1000);
/// The strip left on the edge while the dock is hidden, in logical pixels.
pub const STRIP_PX: i32 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Hidden,
    /// The pointer is on the strip; the dock shows when the timer fires.
    Revealing,
    Shown,
    /// The pointer left; the dock hides when the timer fires.
    Hiding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    PointerIn,
    PointerOut,
    /// A menu or a drag of the dock started (`true`) or ended (`false`), on any output.
    Held(bool),
    /// The timer last started ran out.
    Timer,
}

/// What the surface does with its one timer after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timer {
    Start(Duration),
    Cancel,
    Keep,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AutoHide {
    phase: Phase,
    pointer: bool,
    held: bool,
}

impl AutoHide {
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether the dock is drawn; the strip is, otherwise.
    pub fn shown(&self) -> bool {
        matches!(self.phase, Phase::Shown | Phase::Hiding)
    }

    /// A hold keeps a shown dock shown and never reveals a hidden one: a menu open on
    /// another output must not bring this one up.
    pub fn feed(&mut self, event: Event) -> Timer {
        match event {
            Event::PointerIn => self.pointer = true,
            Event::PointerOut => self.pointer = false,
            Event::Held(held) => self.held = held,
            Event::Timer => {}
        }
        let (phase, timer) = match (self.phase, event) {
            (Phase::Hidden, Event::PointerIn) => (Phase::Revealing, Timer::Start(REVEAL_DELAY)),
            (Phase::Hiding, Event::PointerIn) => (Phase::Shown, Timer::Cancel),
            (Phase::Revealing, Event::PointerOut) => (Phase::Hidden, Timer::Cancel),
            (Phase::Shown, Event::PointerOut) if !self.held => {
                (Phase::Hiding, Timer::Start(HIDE_DELAY))
            }
            // A hold only keeps a dock that is on screen: one that is still revealing
            // waits for its delay, so a hold on another output reveals nothing here.
            (Phase::Hiding, Event::Held(true)) => (Phase::Shown, Timer::Cancel),
            (Phase::Shown, Event::Held(false)) if !self.pointer => {
                (Phase::Hiding, Timer::Start(HIDE_DELAY))
            }
            (Phase::Revealing, Event::Timer) => (Phase::Shown, Timer::Keep),
            (Phase::Hiding, Event::Timer) => (Phase::Hidden, Timer::Keep),
            (phase, _) => (phase, Timer::Keep),
        };
        self.phase = phase;
        timer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fed(events: &[Event]) -> (AutoHide, Vec<Timer>) {
        let mut state = AutoHide::default();
        let timers = events.iter().map(|event| state.feed(*event)).collect();
        (state, timers)
    }

    #[test]
    fn a_quick_pass_never_shows() {
        let (state, timers) = fed(&[Event::PointerIn, Event::PointerOut]);
        assert_eq!(timers, vec![Timer::Start(REVEAL_DELAY), Timer::Cancel]);
        assert_eq!(state.phase(), Phase::Hidden);
        assert!(!state.shown());
    }

    #[test]
    fn the_dock_shows_after_the_delay() {
        let (state, _) = fed(&[Event::PointerIn, Event::Timer]);
        assert_eq!(state.phase(), Phase::Shown);
        assert!(state.shown());
    }

    #[test]
    fn the_dock_hides_a_while_after_the_pointer_leaves() {
        let (mut state, timers) = fed(&[Event::PointerIn, Event::Timer, Event::PointerOut]);
        assert_eq!(timers.last(), Some(&Timer::Start(HIDE_DELAY)));
        // Still drawn until the delay runs out.
        assert!(state.shown());
        assert_eq!(state.feed(Event::Timer), Timer::Keep);
        assert_eq!(state.phase(), Phase::Hidden);
    }

    #[test]
    fn coming_back_before_the_hide_cancels_it() {
        let (state, timers) = fed(&[
            Event::PointerIn,
            Event::Timer,
            Event::PointerOut,
            Event::PointerIn,
        ]);
        assert_eq!(timers.last(), Some(&Timer::Cancel));
        assert_eq!(state.phase(), Phase::Shown);
    }

    #[test]
    fn a_held_menu_keeps_the_dock_shown_and_its_release_starts_the_hide() {
        let (mut state, timers) = fed(&[
            Event::PointerIn,
            Event::Timer,
            Event::Held(true),
            Event::PointerOut,
        ]);
        assert_eq!(timers.last(), Some(&Timer::Keep));
        assert_eq!(state.phase(), Phase::Shown);
        assert_eq!(state.feed(Event::Held(false)), Timer::Start(HIDE_DELAY));
        assert_eq!(state.phase(), Phase::Hiding);
    }

    #[test]
    fn a_release_with_the_pointer_still_on_the_dock_keeps_it() {
        let (mut state, _) = fed(&[Event::PointerIn, Event::Timer, Event::Held(true)]);
        assert_eq!(state.feed(Event::Held(false)), Timer::Keep);
        assert_eq!(state.phase(), Phase::Shown);
    }

    #[test]
    fn a_hold_on_another_output_never_reveals() {
        let (mut state, timers) = fed(&[Event::Held(true)]);
        assert_eq!(timers, vec![Timer::Keep]);
        assert_eq!(state.phase(), Phase::Hidden);
        assert_eq!(state.feed(Event::Held(false)), Timer::Keep);
        assert_eq!(state.phase(), Phase::Hidden);
    }

    #[test]
    fn a_hold_during_the_hide_brings_the_dock_back() {
        let (state, timers) = fed(&[
            Event::PointerIn,
            Event::Timer,
            Event::PointerOut,
            Event::Held(true),
        ]);
        assert_eq!(timers.last(), Some(&Timer::Cancel));
        assert_eq!(state.phase(), Phase::Shown);
    }

    #[test]
    fn a_stray_timer_changes_nothing() {
        let (state, timers) = fed(&[Event::Timer]);
        assert_eq!(timers, vec![Timer::Keep]);
        assert_eq!(state.phase(), Phase::Hidden);
        let (state, _) = fed(&[Event::PointerIn, Event::Timer, Event::Timer]);
        assert_eq!(state.phase(), Phase::Shown);
    }

    #[test]
    fn a_hold_while_revealing_waits_for_the_delay() {
        let (mut state, timers) = fed(&[Event::PointerIn, Event::Held(true)]);
        assert_eq!(timers.last(), Some(&Timer::Keep));
        assert_eq!(state.phase(), Phase::Revealing);
        assert_eq!(state.feed(Event::PointerOut), Timer::Cancel);
        assert_eq!(state.phase(), Phase::Hidden);
    }
}
