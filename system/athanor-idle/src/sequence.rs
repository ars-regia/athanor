//! The sequence blank, off, lock request and its cancellation (SD4). A pure state machine:
//! events in, actions out. The caller owns the clocks, the surfaces, the outputs and logind;
//! a timer it was told to arm comes back as `Event::Timer`.

use std::time::Duration;

/// The fade to opaque black (SD4, "Blank").
pub const FADE: Duration = Duration::from_secs(5);
/// From the outputs going off to the lock request (SD4, "Lock request").
pub const LOCK_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timer {
    Fade,
    LockDelay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// `idled` of the blank notification. `session_locked` is the session's `LockedHint`;
    /// `reduced_motion` is `enable-animations` false.
    BlankIdled {
        session_locked: bool,
        reduced_motion: bool,
    },
    /// `idled` of the suspend notification.
    SuspendIdled,
    /// `resumed` of the blank notification.
    Resumed,
    /// `Timer::Fade` has elapsed.
    FadeDone,
    /// `Timer::LockDelay` has elapsed. `session_locked` is the `LockedHint` read just now: a
    /// session that locked during the fade is not asked to lock again.
    LockDue { session_locked: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// An overlay on every output, alpha rising to opaque black over `fade`; opaque at once
    /// when `fade` is zero.
    ShowOverlay {
        fade: Duration,
    },
    RemoveOverlay,
    /// `zwlr_output_power_v1` off / on on every output.
    OutputsOff,
    OutputsOn,
    Arm {
        timer: Timer,
        after: Duration,
    },
    Cancel(Timer),
    /// logind `LockSession(<id>)`.
    LockSession,
    /// logind `Suspend(false)`.
    Suspend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lock {
    /// The lock request is armed and has not been made.
    Pending,
    /// Made, or not needed because the session was locked at the blank.
    Settled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Awake,
    Fading,
    Off,
}

#[derive(Debug)]
pub struct Sequence {
    phase: Phase,
    lock: Lock,
    overlay: bool,
}

impl Default for Sequence {
    fn default() -> Self {
        Self::new()
    }
}

impl Sequence {
    pub fn new() -> Self {
        Sequence {
            phase: Phase::Awake,
            lock: Lock::Settled,
            overlay: false,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn handle(&mut self, event: Event) -> Vec<Action> {
        match (event, self.phase) {
            (Event::SuspendIdled, _) => vec![Action::Suspend],
            (
                Event::BlankIdled {
                    session_locked,
                    reduced_motion,
                },
                Phase::Awake,
            ) => self.blank(session_locked, reduced_motion),
            (Event::FadeDone, Phase::Fading) => {
                self.phase = Phase::Off;
                self.lock = Lock::Pending;
                vec![
                    Action::OutputsOff,
                    Action::Arm {
                        timer: Timer::LockDelay,
                        after: LOCK_DELAY,
                    },
                ]
            }
            (Event::LockDue { session_locked }, Phase::Off) if self.lock == Lock::Pending => {
                self.lock = Lock::Settled;
                if session_locked {
                    Vec::new()
                } else {
                    vec![Action::LockSession]
                }
            }
            (Event::Resumed, _) => self.resume(),
            // A duplicate `idled`, or a timer that was cancelled in flight.
            _ => Vec::new(),
        }
    }

    fn blank(&mut self, session_locked: bool, reduced_motion: bool) -> Vec<Action> {
        if session_locked {
            // The compositor shows only the lock surface: no fade, outputs off now, and the
            // lock request is not made.
            self.phase = Phase::Off;
            self.lock = Lock::Settled;
            return vec![Action::OutputsOff];
        }
        self.overlay = true;
        if reduced_motion {
            // Opaque at once, the outputs off at once, the lock request 500 ms later.
            self.phase = Phase::Off;
            self.lock = Lock::Pending;
            return vec![
                Action::ShowOverlay {
                    fade: Duration::ZERO,
                },
                Action::OutputsOff,
                Action::Arm {
                    timer: Timer::LockDelay,
                    after: LOCK_DELAY,
                },
            ];
        }
        self.phase = Phase::Fading;
        vec![
            Action::ShowOverlay { fade: FADE },
            Action::Arm {
                timer: Timer::Fade,
                after: FADE,
            },
        ]
    }

    fn resume(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        match self.phase {
            Phase::Awake => return out,
            Phase::Fading => out.push(Action::Cancel(Timer::Fade)),
            Phase::Off => {
                if self.lock == Lock::Pending {
                    out.push(Action::Cancel(Timer::LockDelay));
                }
            }
        }
        if self.overlay {
            out.push(Action::RemoveOverlay);
        }
        if self.phase == Phase::Off {
            out.push(Action::OutputsOn);
        }
        *self = Sequence::new();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLANK: Event = Event::BlankIdled {
        session_locked: false,
        reduced_motion: false,
    };

    fn run(s: &mut Sequence, evs: &[Event]) -> Vec<Action> {
        evs.iter().flat_map(|e| s.handle(*e)).collect()
    }

    #[test]
    fn full_sequence_fades_turns_off_then_locks_once() {
        let mut s = Sequence::new();
        assert_eq!(
            s.handle(BLANK),
            vec![
                Action::ShowOverlay { fade: FADE },
                Action::Arm {
                    timer: Timer::Fade,
                    after: FADE
                }
            ]
        );
        assert_eq!(s.phase(), Phase::Fading);
        assert_eq!(
            s.handle(Event::FadeDone),
            vec![
                Action::OutputsOff,
                Action::Arm {
                    timer: Timer::LockDelay,
                    after: LOCK_DELAY
                }
            ]
        );
        assert_eq!(
            s.handle(Event::LockDue {
                session_locked: false
            }),
            vec![Action::LockSession]
        );
        assert!(s
            .handle(Event::LockDue {
                session_locked: false
            })
            .is_empty());
    }

    #[test]
    fn resumed_during_the_fade_removes_the_surfaces_and_nothing_else() {
        let mut s = Sequence::new();
        s.handle(BLANK);
        assert_eq!(
            s.handle(Event::Resumed),
            vec![Action::Cancel(Timer::Fade), Action::RemoveOverlay]
        );
        assert_eq!(s.phase(), Phase::Awake);
        // The cancelled timer fires in flight: ignored, no lock follows.
        assert!(run(
            &mut s,
            &[
                Event::FadeDone,
                Event::LockDue {
                    session_locked: false
                }
            ]
        )
        .is_empty());
    }

    #[test]
    fn resumed_after_off_before_the_lock_request_cancels_the_lock() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone]);
        assert_eq!(
            s.handle(Event::Resumed),
            vec![
                Action::Cancel(Timer::LockDelay),
                Action::RemoveOverlay,
                Action::OutputsOn
            ]
        );
        assert!(s
            .handle(Event::LockDue {
                session_locked: false
            })
            .is_empty());
    }

    #[test]
    fn resumed_after_the_lock_request_wakes_the_outputs() {
        let mut s = Sequence::new();
        run(
            &mut s,
            &[
                BLANK,
                Event::FadeDone,
                Event::LockDue {
                    session_locked: false,
                },
            ],
        );
        assert_eq!(
            s.handle(Event::Resumed),
            vec![Action::RemoveOverlay, Action::OutputsOn]
        );
        assert_eq!(s.phase(), Phase::Awake);
    }

    #[test]
    fn a_new_period_starts_clean_after_a_cancellation() {
        let mut s = Sequence::new();
        s.handle(BLANK);
        s.handle(Event::Resumed);
        assert_eq!(s.handle(BLANK).len(), 2);
    }

    #[test]
    fn locked_session_skips_fade_and_lock_request() {
        let mut s = Sequence::new();
        let locked = Event::BlankIdled {
            session_locked: true,
            reduced_motion: false,
        };
        assert_eq!(s.handle(locked), vec![Action::OutputsOff]);
        assert!(s
            .handle(Event::LockDue {
                session_locked: false
            })
            .is_empty());
        assert_eq!(s.handle(Event::Resumed), vec![Action::OutputsOn]);
    }

    #[test]
    fn a_session_that_locks_during_the_fade_is_not_asked_to_lock() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone]);
        assert!(s
            .handle(Event::LockDue {
                session_locked: true
            })
            .is_empty());
        assert!(s
            .handle(Event::LockDue {
                session_locked: false
            })
            .is_empty());
        assert_eq!(
            s.handle(Event::Resumed),
            vec![Action::RemoveOverlay, Action::OutputsOn]
        );
    }

    #[test]
    fn reduced_motion_is_opaque_at_once() {
        let mut s = Sequence::new();
        let ev = Event::BlankIdled {
            session_locked: false,
            reduced_motion: true,
        };
        assert_eq!(
            s.handle(ev)[0],
            Action::ShowOverlay {
                fade: Duration::ZERO
            }
        );
        assert_eq!(
            s.handle(Event::LockDue {
                session_locked: false
            }),
            vec![Action::LockSession]
        );
    }

    #[test]
    fn resumed_when_awake_and_duplicate_idled_do_nothing() {
        let mut s = Sequence::new();
        assert!(s.handle(Event::Resumed).is_empty());
        s.handle(BLANK);
        assert!(s.handle(BLANK).is_empty());
    }

    #[test]
    fn suspend_is_independent_of_the_blank() {
        let mut s = Sequence::new();
        assert_eq!(s.handle(Event::SuspendIdled), vec![Action::Suspend]);
        s.handle(BLANK);
        assert_eq!(s.handle(Event::SuspendIdled), vec![Action::Suspend]);
        assert_eq!(s.phase(), Phase::Fading);
    }
}
