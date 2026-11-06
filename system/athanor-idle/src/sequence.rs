//! The sequence blank, off, lock request and its cancellation (SD4). A pure state machine:
//! events in, actions out. The caller owns the clocks, the surfaces, the outputs and logind;
//! a timer it was told to arm comes back as `FadeDone`, `LockDue` or `LockRetryDue`.
//!
//! The lock request outlives the blank: input during the fade cancels it, input after the
//! outputs went off does not (the `LockDelay` timer keeps running).
//!
//! Caller contract:
//! - one `Sequence` lives as long as the daemon, so the `attempt` counter never restarts;
//! - a timer for which the sequence emitted `Cancel` must never deliver its event late.

use std::time::Duration;

/// The fade to opaque black (SD4, "Blank").
pub const FADE: Duration = Duration::from_secs(5);
/// From the outputs going off to the lock request (SD4, "Lock request").
pub const LOCK_DELAY: Duration = Duration::from_millis(500);
/// From a failed `LockSession` to its single retry.
pub const LOCK_RETRY: Duration = Duration::from_secs(1);
/// How long `LockSession` may take before it counts as failed. Shorter than D-Bus's 25 s
/// default: the lock is the point of the blank and the retry needs room, and logind answers
/// `LockSession` at once (it only emits a signal). The caller also sets a bounded method
/// timeout on the call.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
/// The notification shown on wake after the lock failed twice. The caller shows it.
pub const LOCK_FAILED_NOTICE: &str = "Automatic lock failed";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timer {
    Fade,
    LockDelay,
    LockRetry,
    LockTimeout,
}

/// The session's `LockedHint` as the caller read it. A failed read is `Unknown`, which counts
/// as not locked: the fade runs and the lock is requested (fail closed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    Locked,
    Unlocked,
    Unknown,
}

impl Hint {
    fn locked(self) -> bool {
        self == Hint::Locked
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// `idled` of the blank notification; `reduced_motion` is `enable-animations` false.
    BlankIdled { hint: Hint, reduced_motion: bool },
    /// `idled` of the suspend notification.
    SuspendIdled,
    /// `resumed` of the blank notification.
    Resumed,
    /// The blank notification was destroyed without a `resumed` (an inhibitor appeared, or it
    /// was recreated after a key change). The caller MUST send this before it destroys the
    /// notification. `hint` is the `LockedHint` read now.
    Withdrawn { hint: Hint },
    /// `Timer::Fade` has elapsed.
    FadeDone,
    /// `Timer::LockDelay` has elapsed; `hint` is the `LockedHint` read just now, so a session
    /// that locked during the fade is not asked to lock again.
    LockDue { hint: Hint },
    /// `Timer::LockRetry` has elapsed; `hint` is read just now.
    LockRetryDue { hint: Hint },
    /// `LockSession` returned; `attempt` is the number the action carried. An answer for an
    /// attempt that already timed out or was replaced is ignored.
    LockDone { attempt: u32 },
    /// `LockSession` failed.
    LockFailed { attempt: u32 },
    /// `Timer::LockTimeout` has elapsed: the call never returned, which counts as a failure.
    LockTimeout,
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
    /// logind `LockSession(<id>)`, with a `LockTimeout` armed beside it; the caller answers
    /// `LockDone` or `LockFailed` with the same `attempt`.
    LockSession {
        attempt: u32,
    },
    /// logind `Suspend(false)`.
    Suspend,
    /// The lock failed twice: log a warning.
    LogLockFailure,
    /// Show this notification now (the outputs are on).
    Notify(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lock {
    /// Nothing in flight.
    Settled,
    /// `LockDelay` is armed.
    Pending,
    /// `LockSession` was issued and its outcome is not known.
    Requested { retried: bool },
    /// The first attempt failed and `LockRetry` is armed.
    Retrying,
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
    /// The lock failed twice while the outputs were off: the notice waits for the wake.
    notice_due: bool,
    /// The number of the latest `LockSession` attempt.
    attempt: u32,
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
            notice_due: false,
            attempt: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn handle(&mut self, event: Event) -> Vec<Action> {
        match event {
            Event::SuspendIdled => vec![Action::Suspend],
            Event::BlankIdled {
                hint,
                reduced_motion,
            } if self.phase == Phase::Awake => self.blank(hint, reduced_motion),
            Event::FadeDone if self.phase == Phase::Fading => {
                self.phase = Phase::Off;
                let mut out = vec![Action::OutputsOff];
                out.extend(self.arm_lock());
                out
            }
            Event::Resumed => self.wake(),
            Event::Withdrawn { hint } => self.withdrawn(hint),
            Event::LockDue { hint } if self.lock == Lock::Pending => self.request(hint, false),
            Event::LockRetryDue { hint } if self.lock == Lock::Retrying => self.request(hint, true),
            Event::LockDone { attempt } if self.answers(attempt) => {
                self.lock = Lock::Settled;
                vec![Action::Cancel(Timer::LockTimeout)]
            }
            Event::LockFailed { attempt } if self.answers(attempt) => {
                let mut out = vec![Action::Cancel(Timer::LockTimeout)];
                out.extend(self.failed());
                out
            }
            Event::LockTimeout => self.failed(),
            // A duplicate `idled`, or a timer that was cancelled in flight.
            _ => Vec::new(),
        }
    }

    fn arm_lock(&mut self) -> Option<Action> {
        // A request still in flight from an earlier blank is not duplicated.
        if self.lock != Lock::Settled {
            return None;
        }
        self.lock = Lock::Pending;
        Some(Action::Arm {
            timer: Timer::LockDelay,
            after: LOCK_DELAY,
        })
    }

    fn blank(&mut self, hint: Hint, reduced_motion: bool) -> Vec<Action> {
        if hint.locked() {
            // The compositor shows only the lock surface: no overlay, no fade, outputs off
            // now, and no lock request.
            self.phase = Phase::Off;
            return vec![Action::OutputsOff];
        }
        self.overlay = true;
        if reduced_motion {
            // Opaque at once, the outputs off at once, the lock request 500 ms later.
            self.phase = Phase::Off;
            let mut out = vec![
                Action::ShowOverlay {
                    fade: Duration::ZERO,
                },
                Action::OutputsOff,
            ];
            out.extend(self.arm_lock());
            return out;
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

    /// Issues `LockSession` unless the session is already locked.
    fn request(&mut self, hint: Hint, retried: bool) -> Vec<Action> {
        if hint.locked() {
            self.lock = Lock::Settled;
            return Vec::new();
        }
        self.lock = Lock::Requested { retried };
        self.attempt = self.attempt.wrapping_add(1);
        vec![
            Action::LockSession {
                attempt: self.attempt,
            },
            Action::Arm {
                timer: Timer::LockTimeout,
                after: LOCK_TIMEOUT,
            },
        ]
    }

    /// Whether an answer belongs to the request in flight.
    fn answers(&self, attempt: u32) -> bool {
        matches!(self.lock, Lock::Requested { .. }) && attempt == self.attempt
    }

    fn failed(&mut self) -> Vec<Action> {
        match self.lock {
            Lock::Requested { retried: false } => {
                self.lock = Lock::Retrying;
                vec![Action::Arm {
                    timer: Timer::LockRetry,
                    after: LOCK_RETRY,
                }]
            }
            Lock::Requested { retried: true } => {
                self.lock = Lock::Settled;
                let mut out = vec![Action::LogLockFailure];
                if self.phase != Phase::Awake {
                    self.notice_due = true;
                } else {
                    out.push(Action::Notify(LOCK_FAILED_NOTICE));
                }
                out
            }
            _ => Vec::new(),
        }
    }

    /// Input. During the fade it cancels everything; once the outputs are off it only wakes
    /// them, and a lock request already armed still happens.
    fn wake(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        match self.phase {
            Phase::Awake => return out,
            Phase::Fading => out.push(Action::Cancel(Timer::Fade)),
            Phase::Off => {}
        }
        self.finish_wake(&mut out);
        out
    }

    /// The notification is gone, so no `resumed` will come. Fails closed.
    fn withdrawn(&mut self, hint: Hint) -> Vec<Action> {
        let mut out = Vec::new();
        match self.phase {
            Phase::Awake => return out,
            Phase::Fading => out.push(Action::Cancel(Timer::Fade)),
            Phase::Off => {
                if self.lock == Lock::Pending {
                    out.push(Action::Cancel(Timer::LockDelay));
                    out.extend(self.request(hint, false));
                }
            }
        }
        self.finish_wake(&mut out);
        out
    }

    fn finish_wake(&mut self, out: &mut Vec<Action>) {
        if self.phase == Phase::Off {
            out.push(Action::OutputsOn);
        }
        if self.overlay {
            out.push(Action::RemoveOverlay);
        }
        if self.notice_due {
            out.push(Action::Notify(LOCK_FAILED_NOTICE));
        }
        self.phase = Phase::Awake;
        self.overlay = false;
        self.notice_due = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLANK: Event = Event::BlankIdled {
        hint: Hint::Unlocked,
        reduced_motion: false,
    };
    const REDUCED: Event = Event::BlankIdled {
        hint: Hint::Unlocked,
        reduced_motion: true,
    };
    const DUE: Event = Event::LockDue {
        hint: Hint::Unlocked,
    };
    const ARM_LOCK: Action = Action::Arm {
        timer: Timer::LockDelay,
        after: LOCK_DELAY,
    };

    fn req(attempt: u32) -> Vec<Action> {
        vec![
            Action::LockSession { attempt },
            Action::Arm {
                timer: Timer::LockTimeout,
                after: LOCK_TIMEOUT,
            },
        ]
    }

    fn fail(attempt: u32) -> Event {
        Event::LockFailed { attempt }
    }

    fn done(attempt: u32) -> Event {
        Event::LockDone { attempt }
    }

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
            vec![Action::OutputsOff, ARM_LOCK]
        );
        assert_eq!(s.handle(DUE), req(1));
        assert!(s.handle(DUE).is_empty());
    }

    #[test]
    fn resumed_during_the_fade_cancels_everything_and_no_lock_follows() {
        let mut s = Sequence::new();
        s.handle(BLANK);
        assert_eq!(
            s.handle(Event::Resumed),
            vec![Action::Cancel(Timer::Fade), Action::RemoveOverlay]
        );
        assert_eq!(s.phase(), Phase::Awake);
        assert!(run(&mut s, &[Event::FadeDone, DUE]).is_empty());
    }

    #[test]
    fn resumed_after_off_wakes_the_outputs_but_the_lock_still_happens() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone]);
        assert_eq!(
            s.handle(Event::Resumed),
            vec![Action::OutputsOn, Action::RemoveOverlay]
        );
        assert_eq!(s.phase(), Phase::Awake);
        assert_eq!(s.handle(DUE), req(1));
    }

    #[test]
    fn reduced_motion_resumed_before_the_lock_request_still_locks() {
        let mut s = Sequence::new();
        s.handle(REDUCED);
        assert_eq!(
            s.handle(Event::Resumed),
            vec![Action::OutputsOn, Action::RemoveOverlay]
        );
        assert_eq!(s.handle(DUE), req(1));
    }

    #[test]
    fn resumed_after_the_lock_request_wakes_the_outputs() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone, DUE]);
        assert_eq!(
            s.handle(Event::Resumed),
            vec![Action::OutputsOn, Action::RemoveOverlay]
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
            hint: Hint::Locked,
            reduced_motion: false,
        };
        assert_eq!(s.handle(locked), vec![Action::OutputsOff]);
        assert!(s.handle(DUE).is_empty());
        assert_eq!(s.handle(Event::Resumed), vec![Action::OutputsOn]);
    }

    #[test]
    fn a_session_that_locks_during_the_fade_is_not_asked_to_lock() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone]);
        assert!(s.handle(Event::LockDue { hint: Hint::Locked }).is_empty());
        assert!(s.handle(DUE).is_empty());
        assert_eq!(
            s.handle(Event::Resumed),
            vec![Action::OutputsOn, Action::RemoveOverlay]
        );
    }

    #[test]
    fn an_unknown_hint_counts_as_not_locked() {
        let mut s = Sequence::new();
        let unknown = Event::BlankIdled {
            hint: Hint::Unknown,
            reduced_motion: false,
        };
        assert_eq!(s.handle(unknown)[0], Action::ShowOverlay { fade: FADE });
        s.handle(Event::FadeDone);
        assert_eq!(
            s.handle(Event::LockDue {
                hint: Hint::Unknown
            }),
            req(1)
        );
    }

    #[test]
    fn reduced_motion_is_opaque_at_once_and_locks_500_ms_later() {
        let mut s = Sequence::new();
        assert_eq!(
            s.handle(REDUCED),
            vec![
                Action::ShowOverlay {
                    fade: Duration::ZERO
                },
                Action::OutputsOff,
                ARM_LOCK
            ]
        );
        assert_eq!(s.handle(DUE), req(1));
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

    #[test]
    fn suspend_while_off_with_the_lock_pending_leaves_the_lock_alone() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone]);
        assert_eq!(s.handle(Event::SuspendIdled), vec![Action::Suspend]);
        assert_eq!(s.phase(), Phase::Off);
        assert_eq!(s.handle(DUE), req(1));
    }

    const RETRY_ARM: Action = Action::Arm {
        timer: Timer::LockRetry,
        after: LOCK_RETRY,
    };
    const CANCEL_TIMEOUT: Action = Action::Cancel(Timer::LockTimeout);
    const RETRY_DUE: Event = Event::LockRetryDue {
        hint: Hint::Unlocked,
    };

    #[test]
    fn a_failed_lock_is_retried_once_after_1_s() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone, DUE]);
        assert_eq!(s.handle(fail(1)), vec![CANCEL_TIMEOUT, RETRY_ARM]);
        assert_eq!(s.handle(RETRY_DUE), req(2));
        assert_eq!(s.handle(done(2)), vec![CANCEL_TIMEOUT]);
        // Settled: nothing more happens.
        assert!(s.handle(fail(2)).is_empty());
    }

    #[test]
    fn the_retry_is_skipped_for_a_session_that_locked_meanwhile() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone, DUE, fail(1)]);
        assert!(s
            .handle(Event::LockRetryDue { hint: Hint::Locked })
            .is_empty());
    }

    #[test]
    fn two_failures_log_and_notify_on_wake() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone, DUE, fail(1), RETRY_DUE]);
        assert_eq!(
            s.handle(fail(2)),
            vec![CANCEL_TIMEOUT, Action::LogLockFailure]
        );
        assert_eq!(
            s.handle(Event::Resumed),
            vec![
                Action::OutputsOn,
                Action::RemoveOverlay,
                Action::Notify(LOCK_FAILED_NOTICE)
            ]
        );
        assert!(s.handle(Event::Resumed).is_empty());
    }

    #[test]
    fn two_failures_after_the_wake_notify_at_once() {
        let mut s = Sequence::new();
        run(
            &mut s,
            &[
                BLANK,
                Event::FadeDone,
                Event::Resumed,
                DUE,
                fail(1),
                RETRY_DUE,
            ],
        );
        assert_eq!(
            s.handle(fail(2)),
            vec![
                CANCEL_TIMEOUT,
                Action::LogLockFailure,
                Action::Notify(LOCK_FAILED_NOTICE)
            ]
        );
    }

    #[test]
    fn a_timeout_is_a_failure_and_is_retried() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone, DUE]);
        assert_eq!(s.handle(Event::LockTimeout), vec![RETRY_ARM]);
        assert_eq!(s.handle(RETRY_DUE), req(2));
        // The retry times out too: warning, and the notice waits for the wake.
        assert_eq!(s.handle(Event::LockTimeout), vec![Action::LogLockFailure]);
        assert_eq!(
            s.handle(Event::Resumed),
            vec![
                Action::OutputsOn,
                Action::RemoveOverlay,
                Action::Notify(LOCK_FAILED_NOTICE)
            ]
        );
    }

    #[test]
    fn a_second_failure_during_a_fade_defers_the_notice_to_the_wake() {
        let mut s = Sequence::new();
        run(
            &mut s,
            &[
                BLANK,
                Event::FadeDone,
                DUE,
                Event::Resumed,
                fail(1),
                RETRY_DUE,
            ],
        );
        s.handle(BLANK);
        assert_eq!(s.phase(), Phase::Fading);
        assert_eq!(
            s.handle(fail(2)),
            vec![CANCEL_TIMEOUT, Action::LogLockFailure]
        );
        assert_eq!(
            s.handle(Event::Resumed),
            vec![
                Action::Cancel(Timer::Fade),
                Action::RemoveOverlay,
                Action::Notify(LOCK_FAILED_NOTICE)
            ]
        );
    }

    #[test]
    fn a_late_answer_after_the_timeout_is_ignored() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone, DUE, Event::LockTimeout]);
        // While the retry is armed.
        assert!(s.handle(done(1)).is_empty());
        assert!(s.handle(fail(1)).is_empty());
        // While the retry is in flight: attempt 1 does not answer attempt 2.
        assert_eq!(s.handle(RETRY_DUE), req(2));
        assert!(s.handle(done(1)).is_empty());
        assert!(s.handle(fail(1)).is_empty());
        assert_eq!(
            s.handle(fail(2)),
            vec![CANCEL_TIMEOUT, Action::LogLockFailure]
        );
        // After the final failure.
        assert!(s.handle(done(2)).is_empty());
    }

    #[test]
    fn a_new_blank_after_a_timeout_still_locks() {
        let mut s = Sequence::new();
        run(
            &mut s,
            &[
                BLANK,
                Event::FadeDone,
                DUE,
                Event::LockTimeout,
                RETRY_DUE,
                Event::LockTimeout,
            ],
        );
        s.handle(Event::Resumed);
        assert_eq!(s.handle(REDUCED).len(), 3);
        assert_eq!(s.handle(DUE), req(3));
    }

    #[test]
    fn the_lock_is_not_settled_before_its_outcome_is_known() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone, DUE]);
        // A new blank while the request is in flight does not arm a second lock.
        s.handle(Event::Resumed);
        assert_eq!(
            s.handle(REDUCED),
            vec![
                Action::ShowOverlay {
                    fade: Duration::ZERO
                },
                Action::OutputsOff
            ]
        );
        s.handle(done(1));
        s.handle(Event::Resumed);
        assert_eq!(s.handle(REDUCED).len(), 3);
    }

    #[test]
    fn withdrawn_while_fading_behaves_as_resumed() {
        let mut s = Sequence::new();
        s.handle(BLANK);
        assert_eq!(
            s.handle(Event::Withdrawn {
                hint: Hint::Unlocked
            }),
            vec![Action::Cancel(Timer::Fade), Action::RemoveOverlay]
        );
        assert!(s.handle(DUE).is_empty());
    }

    #[test]
    fn withdrawn_while_off_locks_now_then_wakes() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone]);
        assert_eq!(
            s.handle(Event::Withdrawn {
                hint: Hint::Unlocked
            }),
            vec![
                Action::Cancel(Timer::LockDelay),
                Action::LockSession { attempt: 1 },
                Action::Arm {
                    timer: Timer::LockTimeout,
                    after: LOCK_TIMEOUT
                },
                Action::OutputsOn,
                Action::RemoveOverlay
            ]
        );
        assert_eq!(s.phase(), Phase::Awake);
        assert!(s.handle(DUE).is_empty());
        // The immediate request still has the retry.
        assert_eq!(s.handle(fail(1)), vec![CANCEL_TIMEOUT, RETRY_ARM]);
    }

    #[test]
    fn withdrawn_while_off_skips_the_lock_for_a_locked_session() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone]);
        assert_eq!(
            s.handle(Event::Withdrawn { hint: Hint::Locked }),
            vec![
                Action::Cancel(Timer::LockDelay),
                Action::OutputsOn,
                Action::RemoveOverlay
            ]
        );
    }

    #[test]
    fn withdrawn_after_the_lock_request_only_wakes() {
        let mut s = Sequence::new();
        run(&mut s, &[BLANK, Event::FadeDone, DUE]);
        assert_eq!(
            s.handle(Event::Withdrawn {
                hint: Hint::Unlocked
            }),
            vec![Action::OutputsOn, Action::RemoveOverlay]
        );
    }

    #[test]
    fn withdrawn_when_awake_does_nothing() {
        let mut s = Sequence::new();
        assert!(s
            .handle(Event::Withdrawn {
                hint: Hint::Unlocked
            })
            .is_empty());
    }
}
