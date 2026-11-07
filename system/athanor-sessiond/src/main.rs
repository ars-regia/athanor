//! `athanor-sessiond` (`doc_session_daemons.md` SD1): confines itself, resets COSMIC's
//! appearance values (SD16), then raises the battery notices (SD14) until the bus ends.

use std::env;
use std::error::Error;
use std::process::ExitCode;

use athanor_i18n::Catalog;
use athanor_sessiond::{appearance, bus, confine};
use athanor_unit::{journal, notify, sandbox};
use zbus::Connection;

const DOMAIN: &str = "athanor-sessiond";

/// Landlock while the process has one thread (SD17), with the grants of `confine`.
fn confine_self(grants: &confine::Grants) -> Result<(), Box<dyn Error>> {
    sandbox::ensure_single_threaded()?;
    grants.apply()
}

async fn serve(catalog: &Catalog) -> Result<(), Box<dyn Error>> {
    let system = Connection::system().await?;
    // Signals on one connection, the calls the loop awaits on another (#1035).
    let signals = Connection::session().await?;
    let query = Connection::session().await?;
    bus::run(
        &tokio::runtime::Handle::current(),
        system,
        &signals,
        &query,
        catalog,
    )
    .await
}

fn main() -> ExitCode {
    journal::init();
    let catalog = Catalog::load(DOMAIN).unwrap_or_else(|(path, err)| {
        tracing::warn!(path = %path.display(), ?err, "the catalog cannot be read; the notices are in English");
        Catalog::empty()
    });
    let grants = match confine::daemon(|name| env::var_os(name), None) {
        Ok(grants) => grants,
        Err(err) => {
            tracing::error!(%err, "the session's directories are unknown; cannot confine");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = confine_self(&grants) {
        tracing::error!(%err, "Landlock cannot be applied; refusing to run unconfined");
        return ExitCode::FAILURE;
    }
    // A failed reset is logged and the daemon goes on: its battery notices do not wait on dconf.
    match appearance::open().and_then(|settings| appearance::reset(&settings)) {
        Ok(keys) => {
            for key in keys {
                tracing::info!(
                    key,
                    "the key held cosmic-settings-daemon's value: reset to the default"
                );
            }
        }
        Err(err) => tracing::error!(%err, "the appearance values were not reset"),
    }
    // Ready once the reset is written, so the bar and the dock read the defaults.
    if let Err(err) = notify::notify_ready() {
        tracing::warn!(%err, "systemd was not told the daemon is ready");
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!(%err, "cannot start the runtime");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(serve(&catalog)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "the battery notices stopped");
            ExitCode::FAILURE
        }
    }
}
