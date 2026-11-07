//! `os.athanor.Osd1` (doc_osd.md OD4), with the methods of step 1: `VolumeUp`, `VolumeDown`,
//! `ToggleMute` and `ToggleMicMute`. Any peer on the session bus may call; more than
//! [`logic::RATE_LIMIT`] calls a second from one sender are dropped, and each caller is logged
//! once by its unique name and user. A method only hands its key
//! to the core and returns: it awaits nothing, on any connection. The core's queue holds
//! [`KEYS`] keys: past that, while the core is behind, a key is dropped here.
//! The core holds keys only until the audio state settles.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use tokio::runtime::Handle;
use tokio::sync::mpsc::{error::TrySendError, Sender};
use zbus::fdo::DBusProxy;
use zbus::message::Header;
use zbus::names::OwnedUniqueName;
use zbus::{connection, interface, Connection};

use crate::logic::{self, Admit, Key, Rate};

pub const NAME: &str = "os.athanor.Osd1";
pub const PATH: &str = "/os/athanor/Osd1";

/// The bus's answer to the credentials of a caller, at most.
const ASK: Duration = Duration::from_secs(2);
/// Callers checked once each; past this many the set starts again, so a peer that opens
/// connection after connection cannot grow it.
const CHECKED: usize = 256;
/// The keys queued for the core at most.
pub const KEYS: usize = 16;

struct Osd {
    keys: Sender<Key>,
    /// The queue was full at the last key: the drop was logged.
    full: bool,
    rate: Rate,
    /// Serves nothing, subscribes to nothing and owns no name: the caller checks ask the bus
    /// on it, so a reply is never awaited on the connection that serves (issue #1035).
    query: Connection,
    runtime: Handle,
    checked: HashSet<OwnedUniqueName>,
}

#[interface(name = "os.athanor.Osd1")]
impl Osd {
    fn volume_up(&mut self, #[zbus(header)] header: Header<'_>) {
        self.key(Key::VolumeUp, &header);
    }

    fn volume_down(&mut self, #[zbus(header)] header: Header<'_>) {
        self.key(Key::VolumeDown, &header);
    }

    fn toggle_mute(&mut self, #[zbus(header)] header: Header<'_>) {
        self.key(Key::ToggleMute, &header);
    }

    fn toggle_mic_mute(&mut self, #[zbus(header)] header: Header<'_>) {
        self.key(Key::ToggleMicMute, &header);
    }
}

impl Osd {
    fn key(&mut self, key: Key, header: &Header<'_>) {
        let Some(sender) = header.sender() else {
            tracing::warn!(?key, "a call with no sender; dropped");
            return;
        };
        match self.rate.admit(sender.as_str(), Instant::now()) {
            Admit::Yes => {}
            Admit::Dropped { first } => {
                if first {
                    tracing::warn!(%sender, limit = logic::RATE_LIMIT, "more calls a second than the limit; the rest of the second is dropped");
                }
                return;
            }
        }
        let sender = OwnedUniqueName::from(sender.to_owned());
        if !self.checked.contains(&sender) {
            if self.checked.len() >= CHECKED {
                self.checked.clear();
            }
            self.checked.insert(sender.clone());
            self.runtime
                .spawn(log_caller(self.query.clone(), sender));
        }
        match queue(&self.keys, key, &mut self.full) {
            Queued::Yes | Queued::Dropped { first: false } => {}
            Queued::Dropped { first: true } => {
                tracing::warn!(limit = KEYS, "the core is behind; keys are dropped until it catches up");
            }
            Queued::Closed => tracing::error!(?key, "the overlay's core stopped; the key is lost"),
        }
    }
}

/// What became of a key offered to the core.
#[derive(Debug, PartialEq, Eq)]
enum Queued {
    Yes,
    /// The queue is full; `first` when the key before was queued.
    Dropped { first: bool },
    Closed,
}

/// Queues `key` without waiting, or drops it when the queue is full: a method never blocks
/// the connection that serves, and a burst holds no more than the queue.
fn queue(keys: &Sender<Key>, key: Key, full: &mut bool) -> Queued {
    match keys.try_send(key) {
        Ok(()) => {
            *full = false;
            Queued::Yes
        }
        Err(TrySendError::Full(_)) => Queued::Dropped {
            first: !std::mem::replace(full, true),
        },
        Err(TrySendError::Closed(_)) => Queued::Closed,
    }
}

/// Logs `sender` once with its user, from the bus's credentials (OD4). Informative: the call
/// was taken either way. Nothing is read beneath `/proc`, which the overlay is not granted.
async fn log_caller(query: Connection, sender: OwnedUniqueName) {
    let credentials = tokio::time::timeout(ASK, async {
        DBusProxy::new(&query)
            .await?
            .get_connection_credentials((&sender).into())
            .await
    })
    .await;
    match credentials {
        Ok(Ok(credentials)) => tracing::info!(
            %sender,
            uid = ?credentials.unix_user_id(),
            "a caller of os.athanor.Osd1"
        ),
        Ok(Err(error)) => {
            tracing::info!(%sender, %error, "a caller of os.athanor.Osd1, whose credentials are unavailable")
        }
        Err(_) => tracing::info!(
            %sender,
            "a caller of os.athanor.Osd1; the bus did not give its credentials in time"
        ),
    }
}

/// Serves `os.athanor.Osd1` on the session bus and takes its name, which a second instance
/// cannot (`zbus::Error::NameTaken`). The returned connections must live as long as the
/// service.
pub async fn serve(keys: Sender<Key>) -> zbus::Result<(Connection, Connection)> {
    let query = connection::Builder::session()?.build().await?;
    let osd = Osd {
        keys,
        full: false,
        rate: Rate::default(),
        query: query.clone(),
        runtime: Handle::current(),
        checked: HashSet::new(),
    };
    // The builder requests the name with DoNotQueue: taken, the build fails.
    let served = connection::Builder::session()?
        .serve_at(PATH, osd)?
        .name(NAME)?
        .build()
        .await?;
    Ok((served, query))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_queue_drops_the_key_and_says_so_once() {
        let (keys, mut rx) = tokio::sync::mpsc::channel(2);
        let mut full = false;
        let found: Vec<Queued> = (0..4)
            .map(|_| queue(&keys, Key::VolumeUp, &mut full))
            .collect();
        assert_eq!(
            found,
            [Queued::Yes, Queued::Yes, Queued::Dropped { first: true }, Queued::Dropped { first: false }]
        );
        assert_eq!(rx.try_recv(), Ok(Key::VolumeUp));
        assert_eq!(queue(&keys, Key::VolumeDown, &mut full), Queued::Yes);
        assert_eq!(queue(&keys, Key::VolumeDown, &mut full), Queued::Dropped { first: true });
        drop(rx);
        assert_eq!(queue(&keys, Key::ToggleMute, &mut full), Queued::Closed);
    }
}
