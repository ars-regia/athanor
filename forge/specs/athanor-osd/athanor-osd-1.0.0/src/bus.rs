//! `os.athanor.Osd1` (doc_osd.md OD4), with the methods of step 1: `VolumeUp`, `VolumeDown`,
//! `ToggleMute` and `ToggleMicMute`. Any peer on the session bus may call; more than
//! [`logic::RATE_LIMIT`] calls a second from one sender are dropped, and a caller that is not
//! the compositor or the bar is logged, as BR1's informative check. A method only hands its key
//! to the core and returns: it awaits nothing, on any connection.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;
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

struct Osd {
    keys: UnboundedSender<Key>,
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
                .spawn(log_unexpected(self.query.clone(), sender));
        }
        if self.keys.send(key).is_err() {
            tracing::error!(?key, "the overlay's core stopped; the key is lost");
        }
    }
}

/// Logs `sender` when its cgroup is not the compositor's or the bar's (OD4). Informative: the
/// call was taken either way.
async fn log_unexpected(query: Connection, sender: OwnedUniqueName) {
    let credentials = tokio::time::timeout(ASK, async {
        DBusProxy::new(&query)
            .await?
            .get_connection_credentials((&sender).into())
            .await
    })
    .await;
    let pid = match credentials {
        Ok(Ok(credentials)) => credentials.process_id(),
        Ok(Err(error)) => {
            tracing::info!(%sender, %error, "the caller's credentials are unavailable; it is not checked");
            return;
        }
        Err(_) => {
            tracing::info!(%sender, "the bus did not give the caller's credentials in time; it is not checked");
            return;
        }
    };
    let Some(pid) = pid else {
        tracing::info!(%sender, "the bus gave no process id for the caller; it is not checked");
        return;
    };
    match std::fs::read_to_string(format!("/proc/{pid}/cgroup")) {
        Ok(cgroup) if logic::expected_caller(&cgroup) => {}
        Ok(cgroup) => tracing::info!(
            %sender,
            pid,
            cgroup = cgroup.trim(),
            "a caller that is neither the compositor nor the bar"
        ),
        Err(error) => {
            tracing::info!(%sender, pid, %error, "the caller's cgroup is unreadable; it is not checked")
        }
    }
}

/// Serves `os.athanor.Osd1` on the session bus and takes its name, which a second instance
/// cannot (`zbus::Error::NameTaken`). The returned connections must live as long as the
/// service.
pub async fn serve(keys: UnboundedSender<Key>) -> zbus::Result<(Connection, Connection)> {
    let query = connection::Builder::session()?.build().await?;
    let osd = Osd {
        keys,
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
