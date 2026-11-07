//! `athanor-sessiond` (`doc_session_daemons.md` SD1): confines itself, tells systemd it is
//! ready, then raises the battery notices (SD14) until the bus ends, and resets COSMIC's
//! appearance values (SD16) beside them.

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

/// SD16's reset, given up after `appearance::RESET_TIMEOUT`. A failure is logged as an error
/// and the daemon keeps running (maintainer decision, 2026-11-07).
async fn reset_appearance() {
    match appearance::reset_bounded(appearance::RESET_TIMEOUT).await {
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
}

async fn serve(catalog: &Catalog) -> Result<(), Box<dyn Error>> {
    let system = Connection::system().await?;
    // Signals on one connection, the calls the loop awaits on another (#1035).
    let signals = Connection::session().await?;
    let query = Connection::session().await?;
    // Beside the notices, never before them: a hung dconf delays neither.
    tokio::spawn(reset_appearance());
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
    let served = runtime.block_on(serve(&catalog));
    // A reset still waiting on dconf must not hold the exit, as dropping the runtime would.
    runtime.shutdown_background();
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "the battery notices stopped");
            ExitCode::FAILURE
        }
    }
}
