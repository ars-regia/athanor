//! zbus runs the logind thread's connections on that thread's Tokio runtime and starts no
//! runtime of its own (plan decision M3): the thread adds itself to the process and nothing
//! else. A test binary of its own, with one test, so that no other test's threads are counted.

mod common;

use athanor_lock::logind::{Call, Report};
use athanor_lock::state::Event;
use common::{Bus, Emit, Fake, Lock, Record, Setup};

/// This process's threads, by name.
fn threads() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir("/proc/self/task")
        .expect("tasks")
        .map(|task| {
            let comm = task.expect("task").path().join("comm");
            std::fs::read_to_string(comm)
                .expect("comm")
                .trim()
                .to_owned()
        })
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

    // The control, for what the count cannot tell: zbus's own runtime starts no thread either,
    // but runs only inside its own `block_on`. A connection built off Tokio, then driven by a
    // Tokio `block_on` as the logind thread drives its own, gets no answer, so the logind
    // thread's answers above came through Tokio.
    let address = bus.address.clone();
    let connection = std::thread::spawn(move || {
        zbus::block_on(zbus::connection::Builder::address(address.as_str()).build())
            .expect("a connection off Tokio")
    })
    .join()
    .expect("the control's thread");
    let answer = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let bus = zbus::fdo::DBusProxy::new(&connection)
                .await
                .expect("bus proxy");
            tokio::time::timeout(std::time::Duration::from_secs(1), bus.get_id()).await
        });
    assert!(
        answer.is_err(),
        "a connection off Tokio answered under Tokio: {answer:?}"
    );
    assert_eq!(threads(), expected, "nor did it start a thread");
}
