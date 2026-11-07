//! Who may call `os.athanor.Notifications1` (doc_bar.md BR1): the main process of
//! athanor-bar.service, as [`bus_caller::admit`] checks it, asked of the bus and of systemd and
//! never read from `/proc` (issue #1047). Every question goes on connections that serve
//! nothing, subscribe to nothing and own no name, so a handler can await them (issue #1035).

use std::time::Duration;

use athanor_unit::bus_caller::{self, OnBus, Service};
use zbus::fdo::DBusProxy;
use zbus::proxy::CacheProperties;
use zbus::Connection;

/// The bar's unit and the executable of its one `ExecStart`.
pub const BAR: Service<'static> = Service {
    unit: "athanor-bar.service",
    exec: "/usr/bin/athanor-bar",
};

/// Bound on the whole admission, every question to the bus and to both systemds included.
const ASK: Duration = Duration::from_secs(2);

pub struct BarUnit {
    bus: DBusProxy<'static>,
    session: Connection,
    system: Option<Connection>,
}

impl BarUnit {
    /// Asks on `query`, a session bus connection, and on `system`, a system bus connection;
    /// neither serves anything. Without `system` the user's systemd cannot be verified, and
    /// every caller is refused.
    pub async fn new(query: &Connection, system: Option<Connection>) -> zbus::Result<BarUnit> {
        let bus = DBusProxy::builder(query)
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        Ok(BarUnit {
            bus,
            session: query.clone(),
            system,
        })
    }

    /// `Ok` only when `peer` is the main process of [`BAR`]; the error says which check
    /// refused it.
    pub async fn admit(&self, peer: &str) -> Result<(), String> {
        let system = self
            .system
            .as_ref()
            .ok_or("no system bus to verify the user's systemd")?;
        let questions = OnBus {
            bus: &self.bus,
            session: &self.session,
            system,
        };
        match tokio::time::timeout(ASK, bus_caller::admit(&questions, peer, BAR)).await {
            Ok(answer) => answer,
            Err(_) => Err("the bus or systemd did not answer in time".into()),
        }
    }
}
