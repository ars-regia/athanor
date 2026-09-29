//! One connection, the daemon's objects, then its names (doc_bar.md BR1). The names are
//! requested last and without queueing: when another process owns one (cosmic-notifications,
//! COSMIC's watcher, a second daemon), the start fails at once instead of waiting in line.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use zbus::connection::Builder;
use zbus::fdo::RequestNameFlags;
use zbus::Connection;

use crate::dnd;
use crate::notifications::{Notifications, Private, State};
use crate::sender::BarUnit;
use crate::store::Store;
use crate::watcher::{self, Watcher};

pub const NOTIFICATIONS_NAME: &str = "org.freedesktop.Notifications";
pub const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";
pub const PRIVATE_PATH: &str = "/os/athanor/Notifications1";
pub const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";

pub struct Config {
    /// `$XDG_STATE_HOME/athanor/shelld`: the do-not-disturb switch.
    pub state_dir: PathBuf,
    pub bar: BarUnit,
}

pub async fn start(builder: Builder<'_>, config: Config) -> zbus::Result<Connection> {
    let dnd = dnd::load(&config.state_dir).map_err(|err| {
        zbus::Error::Failure(format!("cannot read the do-not-disturb switch: {err}"))
    })?;
    let state = Arc::new(Mutex::new(State::new(Store::new(dnd), config.state_dir)));
    let conn = builder
        .serve_at(
            NOTIFICATIONS_PATH,
            Notifications {
                state: Arc::clone(&state),
            },
        )?
        .serve_at(
            PRIVATE_PATH,
            Private {
                state,
                bar: config.bar,
            },
        )?
        .serve_at(WATCHER_PATH, Watcher::default())?
        .build()
        .await?;
    watcher::follow_owners(&conn).await?;
    for name in [NOTIFICATIONS_NAME, WATCHER_NAME] {
        conn.request_name_with_flags(name, RequestNameFlags::DoNotQueue.into())
            .await?;
    }
    Ok(conn)
}
