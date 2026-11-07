//! What `athanor-idle` asks of the system bus and the notification service (SD4): logind's
//! `LockSession` and `Suspend` and the display session's `LockedHint`, UPower's `OnBattery`,
//! and the notice "Automatic lock failed".
//!
//! Every question goes on a query connection: one that serves nothing and consumes no stream,
//! under a timeout (#1035). The `PropertiesChanged` stream of UPower is read on a connection
//! of its own, and each change is answered by a fresh read on the query connection.

use std::time::Duration;

use futures_util::StreamExt;
use zbus::fdo::PropertiesProxy;
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;
use zbus::Connection;

use crate::Hint;

/// How long a question may take.
pub const ASK: Duration = Duration::from_secs(2);

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Manager {
    fn lock_session(&self, session_id: &str) -> zbus::Result<()>;
    #[zbus(name = "GetUserByPID")]
    fn get_user_by_pid(&self, pid: u32) -> zbus::Result<OwnedObjectPath>;
    fn suspend(&self, interactive: bool) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.User",
    default_service = "org.freedesktop.login1"
)]
trait User {
    #[zbus(property)]
    fn display(&self) -> zbus::Result<(String, OwnedObjectPath)>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait Session {
    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;
}

#[zbus::proxy(
    interface = "org.freedesktop.UPower",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower"
)]
trait UPower {
    #[zbus(property)]
    fn on_battery(&self) -> zbus::Result<bool>;
}

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: std::collections::HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// `f` under `limit`; past it, a timeout error.
async fn within<T>(
    limit: Duration,
    f: impl std::future::Future<Output = zbus::Result<T>>,
) -> zbus::Result<T> {
    tokio::time::timeout(limit, f)
        .await
        .map_err(|_| zbus::Error::Failure(format!("no answer within {limit:?}")))?
}

/// The system bus, as `athanor-idle` asks it.
#[derive(Clone)]
pub struct System {
    query: Connection,
}

impl System {
    /// `query` is a connection that serves nothing and consumes no stream.
    pub fn new(query: Connection) -> Self {
        System { query }
    }

    /// The display session of the user: the one the lock itself uses.
    async fn display(&self) -> zbus::Result<(String, OwnedObjectPath)> {
        // `/org/freedesktop/login1/user/_<uid>`, as logind names the user of this process.
        let manager = self.manager().await?;
        let user = within(ASK, manager.get_user_by_pid(std::process::id())).await?;
        let user = UserProxy::builder(&self.query)
            .path(user)?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let (id, path) = within(ASK, user.display()).await?;
        if id.is_empty() {
            return Err(zbus::Error::Failure(
                "the user has no display session".into(),
            ));
        }
        Ok((id, path))
    }

    /// The display session's `LockedHint`; a read that fails is `Unknown`, which counts as
    /// not locked (SD4).
    pub async fn hint(&self) -> Hint {
        let read = async {
            let (_, path) = self.display().await?;
            let session = SessionProxy::builder(&self.query)
                .path(path)?
                .cache_properties(CacheProperties::No)
                .build()
                .await?;
            within(ASK, session.locked_hint()).await
        };
        match read.await {
            Ok(true) => Hint::Locked,
            Ok(false) => Hint::Unlocked,
            Err(error) => {
                tracing::warn!(%error, "cannot read the session's LockedHint; counted as not locked");
                Hint::Unknown
            }
        }
    }

    async fn manager(&self) -> zbus::Result<ManagerProxy<'static>> {
        ManagerProxy::builder(&self.query)
            .cache_properties(CacheProperties::No)
            .build()
            .await
    }

    /// logind `LockSession` on the display session, bounded by `limit`.
    pub async fn lock(&self, limit: Duration) -> zbus::Result<()> {
        within(limit, async {
            let (id, _) = self.display().await?;
            self.manager().await?.lock_session(&id).await
        })
        .await
    }

    /// logind `Suspend(false)`.
    pub async fn suspend(&self) -> zbus::Result<()> {
        within(ASK, async { self.manager().await?.suspend(false).await }).await
    }

    /// UPower's `OnBattery`.
    pub async fn on_battery(&self) -> zbus::Result<bool> {
        let upower = UPowerProxy::builder(&self.query)
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        within(ASK, upower.on_battery()).await
    }
}

/// A stream that yields once for every change of UPower's properties, read on `watch`, a
/// connection of its own that asks nothing else.
pub async fn upower_changes(
    watch: &Connection,
) -> zbus::Result<impl futures_util::Stream<Item = ()> + Unpin + 'static> {
    let properties = PropertiesProxy::builder(watch)
        .destination("org.freedesktop.UPower")?
        .path("/org/freedesktop/UPower")?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let changes = properties.receive_properties_changed().await?;
    Ok(changes.map(|_| ()))
}

/// A stream that yields, for every `NameOwnerChanged` of `org.freedesktop.UPower` read on
/// `watch`, whether the name gained an owner (`battery::gained_owner`).
pub async fn upower_owners(
    watch: &Connection,
) -> zbus::Result<impl futures_util::Stream<Item = bool> + Unpin + 'static> {
    let dbus = zbus::fdo::DBusProxy::new(watch).await?;
    let changes = dbus
        .receive_name_owner_changed_with_args(&[(0, "org.freedesktop.UPower")])
        .await?;
    Ok(changes.map(|signal| match signal.args() {
        Ok(args) => crate::battery::gained_owner(args.new_owner().as_deref()),
        Err(error) => {
            tracing::warn!(%error, "cannot decode UPower's NameOwnerChanged; read again");
            true
        }
    }))
}

/// Shows `summary` as a session notification, on `query`, a connection that serves nothing.
pub async fn notify(query: &Connection, summary: &str) -> zbus::Result<u32> {
    let notifications = NotificationsProxy::builder(query)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    within(
        ASK,
        notifications.notify(
            "athanor-idle",
            0,
            "",
            summary,
            "",
            &[],
            std::collections::HashMap::new(),
            -1,
        ),
    )
    .await
}
