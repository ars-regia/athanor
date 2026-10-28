//! The session-bus worker of the key bindings (doc_own_stack.md OS13, plan D5 and D7): one
//! thread, `session-calls`, makes the calls a bound key asks for, each a method without
//! arguments, and waits for its reply there, so the event loop never waits on the bus. The loop
//! hands it requests through a bounded queue; a full queue drops the request with a warning.
//! A name with a service file is started by the bus at its first call.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::Duration;

/// A method of the session bus without arguments, and how a key's request for it is queued.
#[derive(Debug, PartialEq)]
pub(crate) struct Call {
    pub destination: &'static str,
    pub path: &'static str,
    pub interface: &'static str,
    pub method: &'static str,
    /// Whether the key acts while the session is locked; a request for a call without it,
    /// queued before a lock, is dropped.
    pub while_locked: bool,
    /// Whether requests for the call queued behind one that was just sent are dropped: a
    /// toggle pressed again while its first call waits is not replayed.
    pub coalesce: bool,
}

/// Super alone: the launcher shows, or hides when shown (LA8,
/// `athanor-launcher-1.0.0/src/bus.rs`).
pub(crate) const LAUNCHER: Call = Call {
    destination: "os.athanor.Launcher1",
    path: "/os/athanor/Launcher1",
    interface: "os.athanor.Launcher1",
    method: "Show",
    while_locked: false,
    coalesce: true,
};

/// Super+C: the control center opens, or closes when open (CC9,
/// `athanor-control-center-1.0.0/src/bus.rs`).
pub(crate) const CONTROL_CENTER: Call = Call {
    destination: "os.athanor.ControlCenter1",
    path: "/os/athanor/ControlCenter1",
    interface: "os.athanor.ControlCenter1",
    method: "Toggle",
    while_locked: false,
    coalesce: true,
};

/// A call a key asked for, stamped with the lock epoch it was asked in (`State::lock_epoch`).
#[derive(Debug, PartialEq)]
pub(crate) struct Request {
    pub call: &'static Call,
    pub epoch: u64,
}

/// The requests the queue holds; the worker holds as many again that it took to coalesce.
const QUEUE: usize = 8;

/// How long a call waits for its reply, as logind's `Lock` (`lock.rs`).
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// The loop's end of the worker's queue.
pub(crate) struct SessionCalls {
    queue: SyncSender<Request>,
}

impl SessionCalls {
    /// Queues `request` without waiting: a full queue, or a worker that has ended, drops it
    /// with a warning, and the next press asks again.
    pub(crate) fn send(&self, request: Request) {
        let method = request.call.method;
        let destination = request.call.destination;
        match self.queue.try_send(request) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::warn!(
                    destination,
                    method,
                    "the session-bus queue is full; the key's call is dropped"
                );
            }
            Err(TrySendError::Disconnected(_)) => {
                tracing::warn!(
                    destination,
                    method,
                    "the session-bus worker has ended; the key's call is dropped"
                );
            }
        }
    }
}

/// Starts the worker, which reads the lock epoch from `epoch`; `None`, logged, when the thread
/// cannot start, and the keys of the bus then do nothing.
pub(crate) fn start(epoch: Arc<AtomicU64>) -> Option<SessionCalls> {
    let (queue, requests) = sync_channel(QUEUE);
    let spawned = std::thread::Builder::new()
        .name("session-calls".into())
        .spawn(move || work(&requests, &epoch, &mut SessionBus::default()));
    match spawned {
        Ok(_) => Some(SessionCalls { queue }),
        Err(error) => {
            tracing::error!(%error, "the session-bus worker did not start; Super and Super+C do nothing");
            None
        }
    }
}

/// Where the worker sends a call; the bus, or a test's record.
trait Bus {
    fn call(&mut self, call: &Call);
}

/// The session bus, connected at the first call and again after an error.
#[derive(Default)]
struct SessionBus {
    connection: Option<zbus::blocking::Connection>,
}

impl Bus for SessionBus {
    fn call(&mut self, call: &Call) {
        let connection = match self.connection.take() {
            Some(connection) => Ok(connection),
            None => zbus::blocking::connection::Builder::session()
                .and_then(|builder| builder.method_timeout(CALL_TIMEOUT).build()),
        };
        let sent = connection.and_then(|connection| {
            connection.call_method(
                Some(call.destination),
                call.path,
                Some(call.interface),
                call.method,
                &(),
            )?;
            Ok(connection)
        });
        match sent {
            Ok(connection) => self.connection = Some(connection),
            Err(error) => tracing::warn!(
                %error,
                destination = call.destination,
                method = call.method,
                "a key's call on the session bus failed"
            ),
        }
    }
}

/// Sends each request of `requests` in order until the loop's end goes: a request asked
/// before the session locked is dropped unless its key acts while locked, and after each
/// call the run of identical requests queued behind it is dropped when its call coalesces.
fn work(requests: &Receiver<Request>, epoch: &AtomicU64, bus: &mut impl Bus) {
    let mut pending = VecDeque::new();
    loop {
        let request = match pending.pop_front() {
            Some(request) => request,
            None => match requests.recv() {
                Ok(request) => request,
                Err(_) => return,
            },
        };
        let Request { call, epoch: asked } = request;
        if asked != epoch.load(Ordering::SeqCst) && !call.while_locked {
            tracing::info!(
                destination = call.destination,
                method = call.method,
                "the session locked after the key; its call is dropped"
            );
            continue;
        }
        bus.call(call);
        while pending.len() < QUEUE {
            match requests.try_recv() {
                Ok(request) => pending.push_back(request),
                Err(_) => break,
            }
        }
        if call.coalesce {
            while pending.front().is_some_and(|next| next.call == call) {
                pending.pop_front();
            }
        }
    }
}

#[cfg(test)]
impl SessionCalls {
    /// A queue no worker reads: the test reads it.
    pub(crate) fn detached() -> (Self, Receiver<Request>) {
        let (queue, requests) = sync_channel(QUEUE);
        (Self { queue }, requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Record(Vec<&'static str>);

    impl Bus for Record {
        fn call(&mut self, call: &Call) {
            self.0.push(call.method);
        }
    }

    const MUTE: Call = Call {
        destination: "test.Volume",
        path: "/test",
        interface: "test.Volume",
        method: "Mute",
        while_locked: true,
        coalesce: false,
    };

    /// Runs the worker over `requests`, the lock epoch then being `now`: the methods it called.
    fn worked(requests: Vec<Request>, now: u64) -> Vec<&'static str> {
        let (queue, receiver) = sync_channel(requests.len());
        for request in requests {
            queue.try_send(request).expect("room");
        }
        drop(queue);
        let mut bus = Record::default();
        work(&receiver, &AtomicU64::new(now), &mut bus);
        bus.0
    }

    #[test]
    fn the_launcher_and_control_center_calls_are_their_programs() {
        let launcher = include_str!("../../../athanor-launcher/athanor-launcher-1.0.0/src/bus.rs");
        let control_center =
            include_str!("../../../athanor-control-center/athanor-control-center-1.0.0/src/bus.rs");
        for (call, source, command) in [
            (
                &LAUNCHER,
                launcher,
                format!(
                    "--dest {} --object-path {} --method {}.{}",
                    LAUNCHER.destination, LAUNCHER.path, LAUNCHER.interface, LAUNCHER.method
                ),
            ),
            (
                &CONTROL_CENTER,
                control_center,
                format!(
                    "call {} {} {} {}",
                    CONTROL_CENTER.destination,
                    CONTROL_CENTER.path,
                    CONTROL_CENTER.interface,
                    CONTROL_CENTER.method
                ),
            ),
        ] {
            assert!(source.contains(&command), "{call:?}: {command}");
        }
    }

    #[test]
    fn a_full_queue_drops_without_waiting() {
        let (calls, _requests) = SessionCalls::detached();
        for _ in 0..QUEUE + 1 {
            calls.send(Request {
                call: &LAUNCHER,
                epoch: 0,
            });
        }
        // The ninth returned: the queue holds eight.
        let (calls, requests) = SessionCalls::detached();
        drop(requests);
        calls.send(Request {
            call: &LAUNCHER,
            epoch: 0,
        });
    }

    #[test]
    fn a_request_from_before_the_lock_is_dropped_unless_it_acts_locked() {
        let at = |call, epoch| Request { call, epoch };
        assert_eq!(
            worked(
                vec![at(&LAUNCHER, 0), at(&MUTE, 0), at(&CONTROL_CENTER, 1),],
                1
            ),
            ["Mute", "Toggle"],
            "the Show asked before the lock is not sent after it"
        );
    }

    #[test]
    fn only_the_run_behind_a_sent_toggle_is_coalesced() {
        let at = |call| Request { call, epoch: 0 };
        assert_eq!(
            worked(
                vec![
                    at(&LAUNCHER),
                    at(&LAUNCHER),
                    at(&CONTROL_CENTER),
                    at(&LAUNCHER),
                ],
                0
            ),
            ["Show", "Toggle", "Show"],
            "Show, Toggle, Show: the third Show is still sent"
        );
        assert_eq!(
            worked(vec![at(&MUTE), at(&MUTE), at(&MUTE)], 0),
            ["Mute", "Mute", "Mute"],
            "a call that does not coalesce is sent every time"
        );
    }
}
