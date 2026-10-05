//! One connection, the daemon's objects, then its names (doc_bar.md BR1). The names are
//! requested last and without queueing: when another process owns one (a second daemon, or
//! the cosmic-notifications and cosmic-panel that owned them before the switch of stage 2),
//! the start fails at once instead of waiting in line.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, watch, Notify};
use zbus::connection::Builder;
use zbus::fdo::RequestNameFlags;
use zbus::Connection;

use crate::battery::LowBattery;
use crate::notifications::{Calendar, Notifications, Private, Shared, State};
use crate::sender::Admitted;
use crate::sound::Player;
use crate::watcher::{self, Watcher};
use crate::{clock, dnd, notifications, rules};

pub const NOTIFICATIONS_NAME: &str = "org.freedesktop.Notifications";
pub const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";
pub const PRIVATE_PATH: &str = "/os/athanor/Notifications1";
pub const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
pub const WATCHER_PATH: &str = "/StatusNotifierWatcher";

pub struct Config {
    /// `$XDG_STATE_HOME/athanor/shelld`: the do-not-disturb state and the history. Writable.
    pub state_dir: PathBuf,
    /// `$XDG_CONFIG_HOME/athanor/notifications`: the rules and the settings. Writable.
    pub config_dir: PathBuf,
    /// Where the callers' `cgroup` files are read: `/proc`.
    pub proc_root: PathBuf,
    /// Which unit a caller of the private interface is.
    pub admitted: Admitted,
    /// Finds and plays the sound of a notification.
    pub player: Player,
    /// The clock the retention follows.
    pub calendar: Calendar,
}

/// What the daemon keeps running: the connection, and the state to write on the way out.
pub struct Daemon {
    pub connection: Connection,
    state: Shared,
}

impl Daemon {
    /// Writes the history now: the daemon is about to exit.
    pub fn flush(&self) {
        notifications::write_history(&self.state);
    }
}

pub async fn start(builder: Builder<'_>, config: Config) -> zbus::Result<Daemon> {
    let dnd = dnd::Dnd::load(&config.state_dir).map_err(|err| {
        zbus::Error::Failure(format!("cannot read the do-not-disturb state: {err}"))
    })?;
    let (wake_tx, wake_rx) = watch::channel(0);
    let dirty = Arc::new(Notify::new());
    let state: Shared = Arc::new(Mutex::new(State::new(
        config.state_dir,
        config.config_dir.clone(),
        config.proc_root,
        dnd,
        wake_tx,
        Arc::clone(&dirty),
        config.player,
    )
    .with_calendar(config.calendar)));
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
                state: Arc::clone(&state),
                admitted: config.admitted,
            },
        )?
        .serve_at(WATCHER_PATH, Watcher::default())?
        .build()
        .await?;
    notifications::restore(&conn, &state).await?;
    notifications::follow_owners(&conn, Arc::clone(&state)).await?;
    watcher::follow_owners(&conn).await?;
    spawn_clock(&conn, &state, wake_rx);
    spawn_battery(&conn, &state);
    spawn_rules(&conn, &state, config.config_dir);
    tokio::spawn(notifications::prune_periodically(conn.clone(), Arc::clone(&state)));
    tokio::spawn(notifications::history_writer(Arc::clone(&state), dirty));
    for name in [NOTIFICATIONS_NAME, WATCHER_NAME] {
        conn.request_name_with_flags(name, RequestNameFlags::DoNotQueue.into())
            .await?;
    }
    Ok(Daemon {
        connection: conn,
        state,
    })
}

/// Evaluates do not disturb again at each wake of the clock (a window ending, a timezone
/// change, a resume from sleep).
fn spawn_clock(conn: &Connection, state: &Shared, wake: watch::Receiver<u64>) {
    let (tx, mut rx) = mpsc::channel(4);
    tokio::spawn(clock::changes(tx, wake));
    let (conn, state) = (conn.clone(), Arc::clone(state));
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            let settled = notifications::lock(&state).evaluate();
            notifications::settle(&conn, &state, settled).await;
        }
    });
}

/// Warns once per discharge when UPower says the battery is low. Without the system bus, or
/// when Landlock hides `/sys/class/backlight`, the model stays quiet: the backlight is not
/// read here and an empty battery warns of nothing.
fn spawn_battery(conn: &Connection, state: &Shared) {
    let (mut battery, commands) = athanor_services::battery::spawn(
        &tokio::runtime::Handle::current(),
        athanor_services::Buses::new(tokio::runtime::Handle::current()),
        PathBuf::from(athanor_services::battery::BACKLIGHT_ROOT),
    );
    let (conn, state) = (conn.clone(), Arc::clone(state));
    tokio::spawn(async move {
        // The model ends when its command channel closes: the sender lives as long as we read.
        let _commands = commands;
        let mut low = LowBattery::new();
        while battery.changed().await.is_ok() {
            let reading = battery.borrow_and_update().battery;
            if let Some(level) = low.observe(reading.as_ref()) {
                let seconds = reading.and_then(|b| b.seconds);
                notifications::post_low_battery(&conn, &state, level, seconds).await;
            }
        }
    });
}

/// Tells the units when a rules or settings file changes under the daemon's feet or its own.
fn spawn_rules(conn: &Connection, state: &Shared, dir: PathBuf) {
    let (tx, mut rx) = mpsc::channel(16);
    tokio::spawn(async move {
        if let Err(err) = rules::watch(dir, tx).await {
            tracing::error!(error = %err, "the rules are no longer watched; edits need a restart");
        }
    });
    let (conn, state) = (conn.clone(), Arc::clone(state));
    tokio::spawn(async move {
        while let Some(relative) = rx.recv().await {
            notifications::rules_changed(&conn, &state, &relative).await;
        }
    });
}
