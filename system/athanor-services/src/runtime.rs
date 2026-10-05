//! The runtime the models run on, and the buses they speak on (doc_control_center.md, CC3).
//! A GTK process starts one [`Runtime`] in `main`, before GTK, and hands its handle to every
//! model: the interface thread only awaits the models' channels, so no reply is decoded and
//! no service is waited on where the interface draws.

use std::io;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use tokio::runtime::{Builder, Handle};
use tokio::sync::OnceCell;
use zbus::Connection;

const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(30);

/// The wait before retry number `attempt`, counted from 0: one second, doubling to thirty. A
/// model that lost its service waits this long, and only while the service is unhealthy.
pub fn backoff(attempt: u32) -> Duration {
    FIRST_RETRY
        .checked_mul(1u32.checked_shl(attempt).unwrap_or(u32::MAX))
        .map_or(LAST_RETRY, |delay| delay.min(LAST_RETRY))
}

/// A single-threaded Tokio runtime on a thread of its own, for the life of the process.
pub struct Runtime {
    handle: Handle,
}

impl Runtime {
    /// Starts the runtime on a thread named `athanor-services`. The thread parks the runtime
    /// on a future that never ends, so the tasks spawned on [`Runtime::handle`] run until the
    /// process exits; the process has no orderly shutdown of its models to wait for.
    pub fn start() -> io::Result<Runtime> {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("athanor-services".to_owned())
            .spawn(move || {
                let runtime = match Builder::new_current_thread().enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        // The caller holds the receiver until it reads this.
                        tx.send(Err(err)).ok();
                        return;
                    }
                };
                tx.send(Ok(runtime.handle().clone())).ok();
                runtime.block_on(std::future::pending::<()>());
            })?;
        let handle = rx.recv().map_err(|_| {
            io::Error::other("the runtime thread ended before its runtime was built")
        })??;
        Ok(Runtime { handle })
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// The bus a service lives on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    System,
    Session,
}

/// One connection per bus for all of a process's models, opened on first use. A connection
/// costs a socket, a reader task and the bus's bookkeeping, so models share it; each
/// [`crate::mirror`] adds only its match rules. Cloning shares the connections: every clone
/// opens each bus once, between them.
#[derive(Clone)]
pub struct Buses(Arc<Inner>);

struct Inner {
    handle: Option<Handle>,
    system: OnceCell<Connection>,
    session: OnceCell<Connection>,
}

impl Buses {
    /// Buses opened on `handle`'s runtime, which runs the connections' reader tasks.
    pub fn new(handle: Handle) -> Buses {
        Buses(Arc::new(Inner {
            handle: Some(handle),
            system: OnceCell::new(),
            session: OnceCell::new(),
        }))
    }

    /// Buses that both answer `connection`, such as a test's private bus.
    pub fn with(connection: Connection) -> Buses {
        Buses(Arc::new(Inner {
            handle: None,
            system: OnceCell::new_with(Some(connection.clone())),
            session: OnceCell::new_with(Some(connection)),
        }))
    }

    /// The connection to `bus`, opened once. It may be awaited from any executor: the
    /// connection is opened on the runtime, since zbus spawns its reader task there.
    pub async fn connection(&self, bus: Bus) -> zbus::Result<Connection> {
        let cell = match bus {
            Bus::System => &self.0.system,
            Bus::Session => &self.0.session,
        };
        cell.get_or_try_init(|| async {
            let handle = self.0.handle.as_ref().ok_or_else(|| {
                zbus::Error::Failure("these buses were given a connection, not a runtime".into())
            })?;
            let open = match bus {
                Bus::System => handle.spawn(Connection::system()),
                Bus::Session => handle.spawn(Connection::session()),
            };
            open.await
                .map_err(|err| zbus::Error::Failure(format!("opening the bus: {err}")))?
        })
        .await
        .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testbus;

    #[tokio::test(flavor = "current_thread")]
    async fn buses_given_a_connection_answer_it_for_both() {
        let (_bus, _server, client) = testbus::start("os.athanor.Test").await;
        let buses = Buses::with(client.clone());
        for bus in [Bus::System, Bus::Session] {
            let connection = buses.connection(bus).await.expect("connection");
            assert_eq!(connection.unique_name(), client.unique_name());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clones_share_their_connections() {
        let (_bus, _server, client) = testbus::start("os.athanor.Test").await;
        let buses = Buses::with(client);
        let clone = buses.clone();
        let (a, b) = (
            buses.connection(Bus::System).await.expect("connection"),
            clone.connection(Bus::System).await.expect("connection"),
        );
        assert_eq!(a.unique_name(), b.unique_name());
    }

    #[test]
    fn a_task_spawned_on_the_handle_runs_off_the_calling_thread() {
        let rt = Runtime::start().expect("runtime");
        let caller = std::thread::current().id();
        let (tx, rx) = std::sync::mpsc::channel();
        rt.handle()
            .spawn(async move { tx.send(std::thread::current().id()).unwrap() });
        assert_ne!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            caller
        );
    }
}
