//! The lock's states (doc_lock_and_prompts.md, LP2): unlocked, locking, locked, authenticating
//! and unlocking. A pure state machine, as athanor-idle's sequence is: events in, actions out.
//! The caller owns logind, the compositor's session lock, the lock surfaces, the password field,
//! the helper's socket and the clock; a timer it was told to arm comes back as `RetryDue`.
//!
//! No type here carries a password, or any text: the field's text goes from Sal's field to
//! `athanor-unlockd` in the caller, at the helper's prompt, and is never held between attempts
//! (LP1, LP8). Every event and action is `Copy`.
//!
//! Caller contract:
//! - one `Machine` lives as long as the process, so the `attempt` counter never restarts;
//! - `Action::Unlock` is `Shell::unlock`, which sends `unlock_and_destroy`; the driver's own
//!   answer to the compositor's `finished` is the only other one, and it comes before
//!   `Event::Finished` (`athanor-sal` `driver.rs`, `SessionLockHandler::finished`);
//! - a surface event names a lock surface of the current lock; the driver closes them all on
//!   `Shell::unlock` and on `finished`;
//! - a timer for which the machine emitted `CancelRetry` must never deliver its event late.

use std::collections::BTreeSet;
use std::time::Duration;

/// From a refusal, the compositor's `finished` before `locked`, to the next request (LP3).
pub const RETRY: Duration = Duration::from_secs(1);

/// The caller's key for one lock surface, one per output.
pub type Surface = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    /// No lock is asked for or held.
    Unlocked,
    /// `Shell::lock` was sent and the compositor has not answered (`waiting` false), or the
    /// compositor refused the lock and the machine asks again at `RetryDue` (`waiting` true).
    /// The hint is true from the request on (D41); the session counts as locked only from
    /// `Locked`.
    Locking { waiting: bool },
    /// The compositor confirmed the lock: only the lock surfaces show.
    Locked,
    /// Locked, with a password transaction in flight.
    Authenticating,
    /// Asked for and not yet confirmed, with logind's `Unlock` kept: the lock ends at `locked`,
    /// or at `finished` with no new request.
    Unlocking,
}

/// What ends the process non-zero, so that the unit's restart brings the lock back (LP10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fatal {
    /// The logind thread ended, whatever the cause.
    LogindEnded,
    /// The connection to the system bus was lost.
    BusLost,
    /// The session is not a Wayland session of this user (LP2).
    NotOurSession,
}

/// How a password transaction with `athanor-unlockd` ended (LP8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    Success,
    Failure,
    /// The fingerprint service's verdict; the password service never sends it.
    Unavailable,
    /// The stream ended before a verdict: the helper crashed or refused the connection (LP10).
    EndOfStream,
}

/// What the lock tells the user after an attempt that did not unlock (LP6, LP10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Notice {
    /// PAM's error text where PAM sent one, else "Wrong password"; the caller has PAM's text.
    Refused,
    /// "Authentication failed, try again".
    TryAgain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// logind's `Lock` on the session's object (LP4).
    Lock,
    /// logind's `Unlock` on the session's object (LP4).
    Unlock,
    /// logind's `PrepareForSleep` (LP4).
    PrepareForSleep(bool),
    /// The compositor confirmed the lock (`Application::locked`).
    Locked,
    /// The compositor refused or ended the lock (`Application::finished`), after the driver's
    /// answer.
    Finished,
    /// A lock surface was made for an output.
    SurfaceAdded(Surface),
    /// A lock surface's output left.
    SurfaceRemoved(Surface),
    /// A frame of a lock surface was presented (`wp_presentation` feedback, spike L4).
    Presented(Surface),
    /// The user submitted the field. It carries nothing: the caller reads the field at the
    /// helper's prompt.
    Submit,
    /// The transaction numbered `attempt` ended.
    Outcome { attempt: u32, outcome: Outcome },
    /// The `RETRY` timer elapsed.
    RetryDue,
    /// Something the lock cannot run without has gone.
    Fatal(Fatal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// `Shell::lock`.
    Lock,
    /// `Shell::unlock`, which sends `unlock_and_destroy` (LP3).
    Unlock,
    /// logind's `SetLockedHint` on the session.
    SetLockedHint(bool),
    /// logind's `sleep` inhibitor in `delay` mode (LP4).
    TakeInhibitor,
    ReleaseInhibitor,
    ArmRetry(Duration),
    CancelRetry,
    /// Connect to `athanor-unlockd`; its result comes back as `Event::Outcome` with `attempt`.
    StartTransaction {
        attempt: u32,
    },
    ClearField,
    FieldSensitive(bool),
    Show(Notice),
    /// `Shell::fail`: the process ends non-zero.
    Fail(Fatal),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Machine {
    state: State,
    /// The session's `LockedHint` as the machine last read or set it.
    hint: bool,
    /// Between `PrepareForSleep(true)` and `PrepareForSleep(false)`.
    sleeping: bool,
    /// The sleep inhibitor is held.
    inhibitor: bool,
    /// The current lock's surfaces, and those of them with a presented frame.
    surfaces: BTreeSet<Surface>,
    presented: BTreeSet<Surface>,
    /// The number of the latest transaction.
    attempt: u32,
}

impl Machine {
    /// The machine at start, with the session's `LockedHint`: it takes the sleep inhibitor, and
    /// locks at once if a previous instance asked for a lock and did not unlock (LP10, D41). The start of a
    /// session never locks otherwise (LP4).
    pub fn start(locked_hint: bool) -> (Self, Vec<Action>) {
        let mut machine = Machine {
            state: State::Unlocked,
            hint: locked_hint,
            sleeping: false,
            inhibitor: true,
            surfaces: BTreeSet::new(),
            presented: BTreeSet::new(),
            attempt: 0,
        };
        let mut out = vec![Action::TakeInhibitor];
        if locked_hint {
            machine.request_lock(&mut out);
        }
        (machine, out)
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Whether the session counts as locked: only once the compositor confirmed it (LP3).
    pub fn is_locked(&self) -> bool {
        matches!(self.state, State::Locked | State::Authenticating)
    }

    pub fn handle(&mut self, event: Event) -> Vec<Action> {
        let mut out = Vec::new();
        match event {
            Event::Lock => self.request_lock(&mut out),
            Event::PrepareForSleep(true) => {
                self.sleeping = true;
                self.request_lock(&mut out);
                self.release_if_shown(&mut out);
            }
            Event::PrepareForSleep(false) => {
                self.sleeping = false;
                if !self.inhibitor {
                    self.inhibitor = true;
                    out.push(Action::TakeInhibitor);
                }
            }
            Event::Unlock => match self.state {
                State::Locked | State::Authenticating => self.unlock(&mut out),
                State::Locking { waiting: false } => self.state = State::Unlocking,
                State::Locking { waiting: true } => {
                    out.push(Action::CancelRetry);
                    self.end_unlocked(&mut out);
                }
                State::Unlocked | State::Unlocking => {}
            },
            Event::Locked => match self.state {
                State::Locking { waiting: false } => {
                    self.state = State::Locked;
                    out.extend([Action::ClearField, Action::FieldSensitive(true)]);
                    self.release_if_shown(&mut out);
                }
                State::Unlocking => self.unlock(&mut out),
                _ => {}
            },
            Event::Finished => {
                self.surfaces.clear();
                self.presented.clear();
                match self.state {
                    State::Unlocking => self.end_unlocked(&mut out),
                    // A refusal keeps the hint, since another live lock client may hold the
                    // session, and a crash in the wait comes back locked (LP3, LP10, D41).
                    State::Locking { waiting: false } => {
                        self.state = State::Locking { waiting: true };
                        out.push(Action::ArmRetry(RETRY));
                    }
                    // The compositor ended a lock it had confirmed: asked for again at once.
                    State::Locked | State::Authenticating => self.ask(&mut out),
                    State::Unlocked | State::Locking { waiting: true } => {}
                }
            }
            Event::RetryDue => {
                if self.state == (State::Locking { waiting: true }) {
                    self.ask(&mut out);
                }
            }
            Event::SurfaceAdded(surface) => {
                if self.lock_asked() {
                    self.surfaces.insert(surface);
                }
            }
            Event::SurfaceRemoved(surface) => {
                self.surfaces.remove(&surface);
                self.presented.remove(&surface);
                self.release_if_shown(&mut out);
            }
            Event::Presented(surface) => {
                if self.surfaces.contains(&surface) {
                    self.presented.insert(surface);
                    self.release_if_shown(&mut out);
                }
            }
            Event::Submit => {
                if self.state == State::Locked {
                    self.attempt = self.attempt.wrapping_add(1);
                    self.state = State::Authenticating;
                    out.extend([
                        Action::FieldSensitive(false),
                        Action::StartTransaction {
                            attempt: self.attempt,
                        },
                    ]);
                }
            }
            Event::Outcome { attempt, outcome } => {
                // An attempt that a `finished` or logind's `Unlock` overtook answers nothing.
                if self.state == State::Authenticating && attempt == self.attempt {
                    self.answered(outcome, &mut out);
                }
            }
            Event::Fatal(fatal) => out.push(Action::Fail(fatal)),
        }
        out
    }

    fn lock_asked(&self) -> bool {
        matches!(
            self.state,
            State::Locking { waiting: false }
                | State::Locked
                | State::Authenticating
                | State::Unlocking
        )
    }

    fn request_lock(&mut self, out: &mut Vec<Action>) {
        match self.state {
            State::Unlocked => self.ask(out),
            // A later request to lock cancels the `Unlock` kept; the lock is already asked for.
            State::Unlocking => self.state = State::Locking { waiting: false },
            State::Locking { .. } | State::Locked | State::Authenticating => {}
        }
    }

    /// Every request, a retry included, sends the hint first, so that a request granted after
    /// another client unlocked leaves the hint true (LP3, D41). The surfaces were cleared where
    /// the driver closed them: at `finished` and at the lock's own unlock.
    fn ask(&mut self, out: &mut Vec<Action>) {
        self.state = State::Locking { waiting: false };
        self.hint = true;
        out.extend([Action::SetLockedHint(true), Action::Lock]);
    }

    fn answered(&mut self, outcome: Outcome, out: &mut Vec<Action>) {
        let notice = match outcome {
            Outcome::Success => return self.unlock(out),
            Outcome::Failure => Notice::Refused,
            Outcome::Unavailable | Outcome::EndOfStream => Notice::TryAgain,
        };
        // PAM's delay (`pam_faildelay`) runs before the helper sends its verdict, so the field
        // was insensitive for all of it (LP6).
        self.state = State::Locked;
        out.extend([
            Action::ClearField,
            Action::FieldSensitive(true),
            Action::Show(notice),
        ]);
    }

    /// The lock's own `unlock_and_destroy`: on the helper's `SUCCESS` and on logind's `Unlock`
    /// (LP3), nowhere else.
    fn unlock(&mut self, out: &mut Vec<Action>) {
        out.extend([Action::ClearField, Action::Unlock]);
        self.surfaces.clear();
        self.presented.clear();
        self.end_unlocked(out);
    }

    fn end_unlocked(&mut self, out: &mut Vec<Action>) {
        self.state = State::Unlocked;
        self.set_hint(false, out);
    }

    fn set_hint(&mut self, hint: bool, out: &mut Vec<Action>) {
        if self.hint != hint {
            self.hint = hint;
            out.push(Action::SetLockedHint(hint));
        }
    }

    /// Before sleep, the inhibitor goes once the lock is confirmed and every lock surface has
    /// presented a frame, so that resume never shows the session (LP4). With no lock surface
    /// nothing has been shown, so it stays held until logind's `InhibitDelayMaxSec`.
    fn release_if_shown(&mut self, out: &mut Vec<Action>) {
        if self.sleeping
            && self.inhibitor
            && self.is_locked()
            && !self.surfaces.is_empty()
            && self.surfaces.is_subset(&self.presented)
        {
            self.inhibitor = false;
            out.push(Action::ReleaseInhibitor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn outcome(machine: &Machine, outcome: Outcome) -> Event {
        Event::Outcome {
            attempt: machine.attempt,
            outcome,
        }
    }

    /// A machine whose lock the compositor confirmed, on two outputs.
    fn locked() -> Machine {
        let (mut m, _) = Machine::start(false);
        for event in [
            Event::Lock,
            Event::SurfaceAdded(1),
            Event::SurfaceAdded(2),
            Event::Locked,
        ] {
            m.handle(event);
        }
        m
    }

    fn authenticating() -> Machine {
        let mut m = locked();
        m.handle(Event::Submit);
        m
    }

    fn unlocks(actions: &[Action]) -> bool {
        actions.contains(&Action::Unlock)
    }

    #[test]
    fn the_session_counts_as_locked_only_after_locked() {
        let (mut m, _) = Machine::start(false);
        assert_eq!(
            m.handle(Event::Lock),
            vec![Action::SetLockedHint(true), Action::Lock],
            "the hint goes out with the request"
        );
        m.handle(Event::SurfaceAdded(1));
        m.handle(Event::Presented(1));
        assert!(!m.is_locked());
        assert_eq!(
            m.handle(Event::Submit),
            vec![],
            "no attempt before the lock holds"
        );
        assert_eq!(
            m.handle(Event::Locked),
            vec![Action::ClearField, Action::FieldSensitive(true)]
        );
        assert!(m.is_locked());
    }

    #[test]
    fn a_refusal_keeps_the_hint_and_asks_again_after_one_second() {
        let (mut m, _) = Machine::start(false);
        m.handle(Event::Lock);
        assert_eq!(m.handle(Event::Finished), vec![Action::ArmRetry(RETRY)]);
        assert!(!m.is_locked());
        assert_eq!(m.handle(Event::Lock), vec![], "the wait is not cut short");
        assert_eq!(
            m.handle(Event::RetryDue),
            vec![Action::SetLockedHint(true), Action::Lock],
            "a retry sends the hint again"
        );
        assert_eq!(m.state(), State::Locking { waiting: false });
    }

    #[test]
    fn the_helpers_success_unlocks() {
        let mut m = authenticating();
        let actions = m.handle(outcome(&m, Outcome::Success));
        assert_eq!(
            actions,
            vec![
                Action::ClearField,
                Action::Unlock,
                Action::SetLockedHint(false)
            ]
        );
        assert_eq!(m.state(), State::Unlocked);
    }

    #[test]
    fn logind_unlock_unlocks() {
        let mut m = locked();
        assert_eq!(
            m.handle(Event::Unlock),
            vec![
                Action::ClearField,
                Action::Unlock,
                Action::SetLockedHint(false)
            ]
        );
        assert_eq!(m.state(), State::Unlocked);
    }

    #[test]
    fn finished_leaves_unlock_and_destroy_to_the_driver() {
        // The driver has answered `finished` itself; `Shell::unlock` would be refused.
        // A lock the compositor confirmed and then ended is asked for again at once.
        let mut m = locked();
        assert_eq!(
            m.handle(Event::Finished),
            vec![Action::SetLockedHint(true), Action::Lock]
        );
        assert_eq!(m.state(), State::Locking { waiting: false });
    }

    #[test]
    fn nothing_else_reaches_unlock() {
        // Every machine reachable in seven events from either start, every event from each.
        let alphabet = |m: &Machine| {
            let mut events = vec![
                Event::Lock,
                Event::Unlock,
                Event::PrepareForSleep(true),
                Event::PrepareForSleep(false),
                Event::Locked,
                Event::Finished,
                Event::SurfaceAdded(1),
                Event::SurfaceRemoved(1),
                Event::Presented(1),
                Event::Submit,
                Event::RetryDue,
                Event::Fatal(Fatal::BusLost),
            ];
            for o in [
                Outcome::Success,
                Outcome::Failure,
                Outcome::Unavailable,
                Outcome::EndOfStream,
            ] {
                for attempt in [m.attempt, m.attempt.wrapping_sub(1)] {
                    events.push(Event::Outcome {
                        attempt,
                        outcome: o,
                    });
                }
            }
            events
        };
        let mut seen = HashSet::new();
        let mut frontier = vec![Machine::start(false).0, Machine::start(true).0];
        let mut causes = [false; 3];
        for _ in 0..8 {
            let mut next = Vec::new();
            for m in frontier {
                if !seen.insert(m.clone()) {
                    continue;
                }
                for event in alphabet(&m) {
                    let mut after = m.clone();
                    let actions = after.handle(event);
                    let success = event
                        == Event::Outcome {
                            attempt: m.attempt,
                            outcome: Outcome::Success,
                        }
                        && m.state == State::Authenticating;
                    let logind = event == Event::Unlock && m.is_locked();
                    let kept = event == Event::Locked && m.state == State::Unlocking;
                    if unlocks(&actions) {
                        assert!(success || logind || kept, "{m:?} unlocked on {event:?}");
                        causes[usize::from(logind) + 2 * usize::from(kept)] = true;
                    }
                    // Only an unlock, the helper's or logind's (kept or not), ends a lock or
                    // clears the hint (LP3, D41).
                    let ended = m.state != State::Unlocked && after.state == State::Unlocked;
                    if ended || actions.contains(&Action::SetLockedHint(false)) {
                        assert!(
                            success
                                || event == Event::Unlock
                                || (m.state == State::Unlocking
                                    && matches!(event, Event::Locked | Event::Finished)),
                            "{m:?} ended the lock on {event:?}"
                        );
                    }
                    assert_eq!(
                        after.hint,
                        after.state != State::Unlocked,
                        "{m:?} on {event:?}: the hint is true exactly while a lock is asked for"
                    );
                    if let Some(lock) = actions.iter().position(|a| *a == Action::Lock) {
                        assert_eq!(
                            actions[..lock].last(),
                            Some(&Action::SetLockedHint(true)),
                            "{m:?} on {event:?}: a request without the hint"
                        );
                    }
                    if actions.contains(&Action::ReleaseInhibitor) {
                        assert!(
                            after.is_locked()
                                && !after.surfaces.is_empty()
                                && after.surfaces.is_subset(&after.presented),
                            "{m:?} released the inhibitor on {event:?}"
                        );
                    }
                    next.push(after);
                }
            }
            frontier = next;
        }
        assert_eq!(causes, [true; 3], "the search reached every path");
    }

    #[test]
    fn the_inhibitor_goes_after_locked_and_a_frame_on_every_output() {
        let (mut m, _) = Machine::start(false);
        assert!(m
            .handle(Event::PrepareForSleep(true))
            .contains(&Action::Lock));
        m.handle(Event::SurfaceAdded(1));
        m.handle(Event::SurfaceAdded(2));
        assert!(!m
            .handle(Event::Presented(1))
            .contains(&Action::ReleaseInhibitor));
        assert!(!m.handle(Event::Locked).contains(&Action::ReleaseInhibitor));
        assert_eq!(
            m.handle(Event::Presented(2)),
            vec![Action::ReleaseInhibitor]
        );
        assert_eq!(
            m.handle(Event::PrepareForSleep(false)),
            vec![Action::TakeInhibitor]
        );

        // No lock surface: nothing was shown, so nothing is released.
        let (mut m, _) = Machine::start(false);
        m.handle(Event::PrepareForSleep(true));
        assert!(!m.handle(Event::Locked).contains(&Action::ReleaseInhibitor));

        // An output added while authenticating waits for its own frame.
        let mut m = authenticating();
        m.handle(Event::PrepareForSleep(true));
        m.handle(Event::Presented(1));
        m.handle(Event::SurfaceAdded(3));
        assert!(!m
            .handle(Event::Presented(2))
            .contains(&Action::ReleaseInhibitor));
        assert_eq!(
            m.handle(Event::Presented(3)),
            vec![Action::ReleaseInhibitor]
        );

        // Already locked and shown: at once.
        let mut m = locked();
        m.handle(Event::Presented(1));
        m.handle(Event::Presented(2));
        assert_eq!(
            m.handle(Event::PrepareForSleep(true)),
            vec![Action::ReleaseInhibitor]
        );

        // The last output without a frame goes: every output left has presented.
        let mut m = locked();
        m.handle(Event::PrepareForSleep(true));
        m.handle(Event::Presented(1));
        assert_eq!(
            m.handle(Event::SurfaceRemoved(2)),
            vec![Action::ReleaseInhibitor]
        );

        // Every frame presented before `locked`: released at `locked`.
        let (mut m, _) = Machine::start(false);
        m.handle(Event::PrepareForSleep(true));
        m.handle(Event::SurfaceAdded(1));
        m.handle(Event::Presented(1));
        assert!(m.handle(Event::Locked).contains(&Action::ReleaseInhibitor));
    }

    #[test]
    fn a_new_lock_waits_for_its_own_frames() {
        // Surface keys come back with the next lock; the frames of the one that ended, at
        // `finished` or at an unlock, do not count.
        for end in [Event::Finished, Event::Unlock] {
            let mut m = locked();
            m.handle(Event::Presented(1));
            m.handle(Event::Presented(2));
            m.handle(end);
            m.handle(Event::Lock);
            m.handle(Event::SurfaceAdded(1));
            m.handle(Event::SurfaceAdded(2));
            m.handle(Event::Locked);
            assert!(
                !m.handle(Event::PrepareForSleep(true))
                    .contains(&Action::ReleaseInhibitor),
                "{end:?}"
            );
            m.handle(Event::Presented(1));
            assert_eq!(
                m.handle(Event::Presented(2)),
                vec![Action::ReleaseInhibitor],
                "{end:?}"
            );

            // A lock on fewer outputs: an output of the lock that ended is not waited for.
            let mut m = locked();
            m.handle(end);
            m.handle(Event::Lock);
            m.handle(Event::SurfaceAdded(1));
            m.handle(Event::Locked);
            m.handle(Event::PrepareForSleep(true));
            assert_eq!(
                m.handle(Event::Presented(1)),
                vec![Action::ReleaseInhibitor],
                "{end:?}"
            );
        }
    }

    #[test]
    fn an_output_that_comes_back_waits_for_a_new_frame() {
        let mut m = locked();
        m.handle(Event::Presented(1));
        m.handle(Event::SurfaceRemoved(1));
        m.handle(Event::SurfaceAdded(1));
        m.handle(Event::PrepareForSleep(true));
        assert!(!m
            .handle(Event::Presented(2))
            .contains(&Action::ReleaseInhibitor));
        assert_eq!(
            m.handle(Event::Presented(1)),
            vec![Action::ReleaseInhibitor]
        );
    }

    #[test]
    fn a_start_with_locked_hint_locks_at_once() {
        let (m, actions) = Machine::start(true);
        assert_eq!(
            actions,
            vec![
                Action::TakeInhibitor,
                Action::SetLockedHint(true),
                Action::Lock
            ]
        );
        assert_eq!(m.state(), State::Locking { waiting: false });
        let (m, actions) = Machine::start(false);
        assert_eq!(actions, vec![Action::TakeInhibitor]);
        assert_eq!(m.state(), State::Unlocked);
    }

    #[test]
    fn an_early_unlock_is_kept_until_locked() {
        let (mut m, _) = Machine::start(true);
        assert_eq!(m.handle(Event::Unlock), vec![]);
        assert_eq!(m.state(), State::Unlocking);
        assert_eq!(
            m.handle(Event::Locked),
            vec![
                Action::ClearField,
                Action::Unlock,
                Action::SetLockedHint(false)
            ]
        );
        assert_eq!(m.state(), State::Unlocked);
    }

    #[test]
    fn a_later_request_to_lock_cancels_the_kept_unlock() {
        for request in [Event::Lock, Event::PrepareForSleep(true)] {
            let (mut m, _) = Machine::start(false);
            m.handle(Event::Lock);
            m.handle(Event::Unlock);
            assert_eq!(m.handle(request), vec![], "the lock is already asked for");
            assert!(!unlocks(&m.handle(Event::Locked)));
            assert!(m.is_locked());
        }
    }

    #[test]
    fn finished_with_an_unlock_kept_ends_the_request() {
        let (mut m, _) = Machine::start(true);
        m.handle(Event::Unlock);
        assert_eq!(
            m.handle(Event::Finished),
            vec![Action::SetLockedHint(false)]
        );
        assert_eq!(m.state(), State::Unlocked);
        assert_eq!(m.handle(Event::RetryDue), vec![], "no re-lock follows");
    }

    #[test]
    fn an_unlock_during_the_wait_cancels_the_retry() {
        let (mut m, _) = Machine::start(false);
        m.handle(Event::Lock);
        m.handle(Event::Finished);
        assert_eq!(
            m.handle(Event::Unlock),
            vec![Action::CancelRetry, Action::SetLockedHint(false)]
        );
        assert_eq!(m.state(), State::Unlocked);
    }

    #[test]
    fn a_fatal_event_ends_the_process() {
        for fatal in [Fatal::LogindEnded, Fatal::BusLost, Fatal::NotOurSession] {
            for mut m in [Machine::start(false).0, locked(), authenticating()] {
                assert_eq!(m.handle(Event::Fatal(fatal)), vec![Action::Fail(fatal)]);
            }
        }
    }

    #[test]
    fn an_end_of_stream_is_try_again() {
        let mut m = authenticating();
        assert_eq!(
            m.handle(outcome(&m, Outcome::EndOfStream)),
            vec![
                Action::ClearField,
                Action::FieldSensitive(true),
                Action::Show(Notice::TryAgain)
            ]
        );
        assert_eq!(m.state(), State::Locked);
    }

    #[test]
    fn a_failure_is_refused_and_the_field_clears() {
        let mut m = authenticating();
        let actions = m.handle(outcome(&m, Outcome::Failure));
        assert_eq!(
            actions,
            vec![
                Action::ClearField,
                Action::FieldSensitive(true),
                Action::Show(Notice::Refused)
            ]
        );
        assert!(m.is_locked());
    }

    #[test]
    fn the_field_stays_insensitive_until_the_verdict() {
        let mut m = locked();
        assert_eq!(
            m.handle(Event::Submit),
            vec![
                Action::FieldSensitive(false),
                Action::StartTransaction { attempt: 1 }
            ]
        );
        // PAM's delay passes before the verdict; nothing in it makes the field sensitive or
        // starts a second attempt.
        for event in [
            Event::Submit,
            Event::Lock,
            Event::Presented(1),
            Event::SurfaceAdded(3),
            Event::RetryDue,
            Event::Outcome {
                attempt: 0,
                outcome: Outcome::Failure,
            },
        ] {
            let actions = m.handle(event);
            assert!(
                !actions.contains(&Action::FieldSensitive(true)),
                "{event:?}"
            );
            assert!(!actions
                .iter()
                .any(|a| matches!(a, Action::StartTransaction { .. })));
        }
        assert!(m
            .handle(outcome(&m, Outcome::Failure))
            .contains(&Action::FieldSensitive(true)));
    }

    #[test]
    fn an_overtaken_attempt_answers_nothing() {
        let mut m = authenticating();
        let first = m.attempt;
        m.handle(Event::Finished);
        m.handle(Event::RetryDue);
        m.handle(Event::Locked);
        m.handle(Event::Submit);
        let stale = Event::Outcome {
            attempt: first,
            outcome: Outcome::Success,
        };
        assert_eq!(m.handle(stale), vec![]);
        assert_eq!(m.state(), State::Authenticating);
    }

    #[test]
    fn logind_unlock_while_authenticating_unlocks() {
        let mut m = authenticating();
        assert!(unlocks(&m.handle(Event::Unlock)));
        assert_eq!(m.handle(outcome(&m, Outcome::Failure)), vec![]);
        assert_eq!(m.state(), State::Unlocked);
    }

    #[test]
    fn finished_while_authenticating_relocks() {
        let mut m = authenticating();
        assert_eq!(
            m.handle(Event::Finished),
            vec![Action::SetLockedHint(true), Action::Lock]
        );
        assert_eq!(m.handle(outcome(&m, Outcome::Success)), vec![]);
        assert_eq!(m.state(), State::Locking { waiting: false });
    }

    #[test]
    fn a_resume_during_authentication_keeps_the_attempt() {
        let mut m = authenticating();
        m.handle(Event::Presented(1));
        m.handle(Event::Presented(2));
        assert_eq!(
            m.handle(Event::PrepareForSleep(true)),
            vec![Action::ReleaseInhibitor]
        );
        assert_eq!(
            m.handle(Event::PrepareForSleep(false)),
            vec![Action::TakeInhibitor]
        );
        assert_eq!(m.state(), State::Authenticating);
        assert!(unlocks(&m.handle(outcome(&m, Outcome::Success))));
    }

    #[test]
    fn no_event_or_action_carries_a_password() {
        // Exhaustive and without a wildcard: a variant or a field added to either type fails to
        // compile here until it is checked to carry no text.
        fn event(e: Event) {
            match e {
                Event::Lock
                | Event::Unlock
                | Event::Locked
                | Event::Finished
                | Event::Submit
                | Event::RetryDue => {}
                Event::PrepareForSleep(b) => {
                    let _: bool = b;
                }
                Event::SurfaceAdded(s) | Event::SurfaceRemoved(s) | Event::Presented(s) => {
                    let _: u32 = s;
                }
                Event::Outcome { attempt, outcome } => {
                    let _: (u32, Outcome) = (attempt, outcome);
                }
                Event::Fatal(f) => {
                    let _: Fatal = f;
                }
            }
        }
        fn action(a: Action) {
            match a {
                Action::Lock
                | Action::Unlock
                | Action::TakeInhibitor
                | Action::ReleaseInhibitor
                | Action::CancelRetry
                | Action::ClearField => {}
                Action::SetLockedHint(b) | Action::FieldSensitive(b) => {
                    let _: bool = b;
                }
                Action::ArmRetry(d) => {
                    let _: Duration = d;
                }
                Action::StartTransaction { attempt } => {
                    let _: u32 = attempt;
                }
                Action::Show(n) => {
                    let _: Notice = n;
                }
                Action::Fail(f) => {
                    let _: Fatal = f;
                }
            }
        }
        // The machine's own fields, without `..`: a field added fails to compile here.
        fn machine(m: &Machine) {
            let Machine {
                state,
                hint,
                sleeping,
                inhibitor,
                surfaces,
                presented,
                attempt,
            } = m;
            let _: (&State, &bool, &bool, &bool) = (state, hint, sleeping, inhibitor);
            let _: (&BTreeSet<Surface>, &BTreeSet<Surface>, &u32) = (surfaces, presented, attempt);
        }
        fn copy<T: Copy>() {}
        copy::<Event>();
        copy::<Action>();
        copy::<State>();
        copy::<Fatal>();
        copy::<Outcome>();
        copy::<Notice>();
        machine(&authenticating());
        event(Event::Submit);
        action(Action::ClearField);
    }
}
