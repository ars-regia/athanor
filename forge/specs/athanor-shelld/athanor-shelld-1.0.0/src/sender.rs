//! Who may call `os.athanor.Notifications1` (doc_bar.md BR1): the main process of
//! athanor-bar.service, as [`bus_caller::admit`] checks it, asked of the bus and of systemd and
//! never read from `/proc` (issue #1047). Every question goes on connections that serve
//! nothing, subscribe to nothing and own no name, so a handler can await them (issue #1035).

use std::time::Duration;

use athanor_unit::bus_caller::{self, OnBus, Service};
use tokio::sync::Mutex;
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

/// A connection to the system bus that serves nothing: at `address` if given, else the system's.
async fn connect(address: Option<&str>) -> zbus::Result<Connection> {
    match address {
        Some(address) => zbus::connection::Builder::address(address)?.build().await,
        None => zbus::connection::Builder::system()?.build().await,
    }
}

pub struct BarUnit {
    bus: DBusProxy<'static>,
    session: Connection,
    /// Where the system bus is; `None` is the system's own.
    address: Option<String>,
    /// Built at start if the bus answers within [`ASK`], else on the first admission that
    /// finds it missing or closed. Never replaced by anything unverified.
    system: Mutex<Option<Connection>>,
}

impl BarUnit {
    /// Asks on `query`, a session bus connection that serves nothing, and on a system bus
    /// connection of its own, built here under [`ASK`]. Without one the user's systemd cannot
    /// be verified and every caller is refused, until a later admission builds it.
    pub async fn new(query: &Connection, address: Option<String>) -> zbus::Result<BarUnit> {
        let bus = DBusProxy::builder(query)
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let system = match tokio::time::timeout(ASK, connect(address.as_deref())).await {
            Ok(Ok(system)) => Some(system),
            Ok(Err(err)) => {
                tracing::error!(error = %err, "no system bus: the private interface refuses every caller until it answers");
                None
            }
            Err(_) => {
                tracing::error!("the system bus did not answer in time: the private interface refuses every caller until it does");
                None
            }
        };
        Ok(BarUnit {
            bus,
            session: query.clone(),
            address,
            system: Mutex::new(system),
        })
    }

    /// `Ok` only when `peer` is the main process of [`BAR`]; the error says which check
    /// refused it. The whole admission, a reconnection to the system bus included, is bound
    /// by [`ASK`].
    pub async fn admit(&self, peer: &str) -> Result<(), String> {
        let admission = async {
            let system = {
                let mut slot = self.system.lock().await;
                if slot.as_ref().is_none_or(Connection::is_closed) {
                    let system = connect(self.address.as_deref()).await.map_err(|err| {
                        format!("no system bus to verify the user's systemd: {err}")
                    })?;
                    *slot = Some(system);
                }
                slot.clone()
                    .ok_or("no system bus to verify the user's systemd")?
            };
            let questions = OnBus {
                bus: &self.bus,
                session: &self.session,
                system: &system,
            };
            bus_caller::admit(&questions, peer, BAR).await
        };
        match tokio::time::timeout(ASK, admission).await {
            Ok(answer) => answer,
            Err(_) => Err("the bus or systemd did not answer in time".into()),
        }
    }
}
