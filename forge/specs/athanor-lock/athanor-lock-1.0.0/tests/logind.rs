//! The logind thread against a fake logind on a private `dbus-daemon`, with no display
//! (doc_lock_and_prompts.md, LP2, LP3, LP4).

mod common;

use athanor_lock::logind::{Call, Report};
use athanor_lock::state::{Event, Fatal};
use common::{Bus, Emit, Fake, Lock, Record, Setup};

/// A lock whose logind thread found its session and reported it ready.
fn ready(setup: Setup) -> (Bus, Fake, Lock, Report) {
    let bus = Bus::start();
    let fake = Fake::start(&bus, setup);
    let mut lock = Lock::start(&bus);
    let report = lock.next();
    (bus, fake, lock, report)
}

fn refused(setup: Setup) {
    let (_bus, _fake, mut lock, report) = ready(setup);
    assert_eq!(report, Report::Event(Event::Fatal(Fatal::NotOurSession)));
    // The guard's own, as the thread ends.
    assert_eq!(lock.next(), Report::Event(Event::Fatal(Fatal::LogindEnded)));
}

#[test]
fn a_wayland_session_of_this_uid_is_ready_with_its_locked_hint() {
    for locked_hint in [false, true] {
        let (_bus, _fake, _lock, report) = ready(Setup {
            locked_hint,
            ..Setup::default()
        });
        assert_eq!(report, Report::Ready { locked_hint });
    }
}

#[test]
fn a_session_that_is_not_wayland_is_fatal() {
    refused(Setup {
        kind: "x11",
        ..Setup::default()
    });
}

#[test]
fn a_session_of_another_uid_is_fatal() {
    refused(Setup {
        other_uid: true,
        ..Setup::default()
    });
}

#[test]
fn a_user_with_no_display_session_is_fatal() {
    refused(Setup {
        id: "",
        ..Setup::default()
    });
}

#[test]
fn lock_unlock_and_prepare_for_sleep_reach_the_main_loop() {
    let (_bus, fake, mut lock, report) = ready(Setup::default());
    assert_eq!(report, Report::Ready { locked_hint: false });
    for (signal, event) in [
        (Emit::Lock, Event::Lock),
        (Emit::Unlock, Event::Unlock),
        (Emit::PrepareForSleep(true), Event::PrepareForSleep(true)),
        (Emit::PrepareForSleep(false), Event::PrepareForSleep(false)),
    ] {
        fake.emit(signal);
        assert_eq!(lock.next(), Report::Event(event));
    }
}

#[test]
fn set_locked_hint_is_called_on_the_session() {
    let (_bus, fake, lock, _) = ready(Setup::default());
    for hint in [true, false] {
        lock.call(Call::SetLockedHint(hint));
        assert_eq!(fake.record(), Record::SetLockedHint(hint));
    }
}

#[test]
fn the_sleep_inhibitor_is_taken_in_delay_mode_and_released_by_closing_it() {
    let (_bus, fake, lock, _) = ready(Setup::default());
    lock.call(Call::TakeInhibitor);
    assert_eq!(
        fake.record(),
        Record::Inhibit(
            [
                "sleep",
                "athanor-lock",
                "Lock the screen before sleep",
                "delay"
            ]
            .map(str::to_owned)
        )
    );
    lock.call(Call::ReleaseInhibitor);
    assert_eq!(fake.record(), Record::Released);
}

#[test]
fn a_lost_bus_is_fatal() {
    let (mut bus, _fake, mut lock, report) = ready(Setup::default());
    assert_eq!(report, Report::Ready { locked_hint: false });
    bus.kill();
    assert_eq!(lock.next(), Report::Event(Event::Fatal(Fatal::BusLost)));
    assert_eq!(lock.next(), Report::Event(Event::Fatal(Fatal::LogindEnded)));
}

#[test]
fn an_unreachable_bus_is_fatal() {
    let mut bus = Bus::start();
    bus.kill();
    let mut lock = Lock::start(&bus);
    assert_eq!(lock.next(), Report::Event(Event::Fatal(Fatal::BusLost)));
    assert_eq!(lock.next(), Report::Event(Event::Fatal(Fatal::LogindEnded)));
}

#[test]
fn the_end_of_the_main_loop_ends_the_thread_with_its_fatal_event() {
    let (_bus, _fake, mut lock, _) = ready(Setup::default());
    let (calls, _) = tokio::sync::mpsc::unbounded_channel();
    drop(std::mem::replace(&mut lock.calls, calls));
    assert_eq!(lock.next(), Report::Event(Event::Fatal(Fatal::LogindEnded)));
}

#[test]
fn signals_reach_the_main_loop_in_the_order_logind_sent_them() {
    let (_bus, fake, mut lock, _) = ready(Setup::default());
    let sent: Vec<(Emit, Event)> = (0..60)
        .map(|i| match i % 4 {
            0 => (Emit::Unlock, Event::Unlock),
            1 => (Emit::PrepareForSleep(true), Event::PrepareForSleep(true)),
            2 => (Emit::Lock, Event::Lock),
            _ => (Emit::PrepareForSleep(false), Event::PrepareForSleep(false)),
        })
        .collect();
    for (signal, _) in &sent {
        fake.emit(*signal);
    }
    let received: Vec<Report> = sent.iter().map(|_| lock.next()).collect();
    let expected: Vec<Report> = sent.iter().map(|(_, e)| Report::Event(*e)).collect();
    assert_eq!(received, expected);
}
