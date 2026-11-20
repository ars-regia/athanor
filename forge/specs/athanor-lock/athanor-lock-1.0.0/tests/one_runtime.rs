//! zbus runs the logind thread's connections on that thread's Tokio runtime and starts no
//! runtime of its own (plan decision M3): the thread adds itself to the process and nothing
//! else. A test binary of its own, with one test, so that no other test's threads are counted.
//! The count holds whichever zbus features the build unifies: the root workspace's adds
//! `tokio-multithread` (athanor-sulphur), the package build of `-p athanor-lock` does not.

mod common;

use athanor_lock::logind::{Call, Report};
use athanor_lock::state::Event;
use common::{Bus, Emit, Fake, Lock, Record, Setup};

/// This process's threads, by name. A thread that ends while it is read is left out.
fn threads() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir("/proc/self/task")
        .expect("tasks")
        .filter_map(|task| std::fs::read_to_string(task.ok()?.path().join("comm")).ok())
        .map(|comm| comm.trim().to_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_logind_thread_is_the_only_thread_it_adds() {
    let bus = Bus::start();
    let fake = Fake::start(&bus, Setup::default());
    let before = threads();

    let mut lock = Lock::start(&bus);
    assert_eq!(lock.next(), Report::Ready { locked_hint: false });
    fake.emit(Emit::Lock);
    assert_eq!(lock.next(), Report::Event(Event::Lock));
    lock.call(Call::TakeInhibitor);
    assert!(matches!(fake.record(), Record::Inhibit(_)));
    lock.call(Call::SetLockedHint(true));
    assert_eq!(fake.record(), Record::SetLockedHint(true));

    let mut expected = before.clone();
    expected.push("logind".to_owned());
    expected.sort();
    assert_eq!(threads(), expected);
}
