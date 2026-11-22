//! `athanor-sessiond` (`doc_session_daemons.md` SD1): confines itself, tells systemd it is
//! ready, resets COSMIC's appearance values (SD16) and runs until the session stops it.

use std::env;
use std::error::Error;
use std::process::ExitCode;

use athanor_sessiond::{appearance, confine};
use athanor_unit::{journal, notify, sandbox};

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

/// The watchdog and the reset, then nothing until the session stops the unit. The battery
/// notices (SD14, `athanor_sessiond::bus::run`) start at SD22 step 8 (S3), when
/// cosmic-settings-daemon and its own notices leave, so that one process notifies at a time
/// (NC13; maintainer decision, 2026-11-07). Until then the daemon's connection to the session
/// bus serves only the watchdog's roundtrip (doc_session.md SN6); without it the manager would
/// end the daemon for want of a ping, so a failed connection is a failed run.
async fn serve() -> ExitCode {
    let connection = match zbus::Connection::session().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::error!(%err, "cannot connect to the session bus, which the watchdog pings through");
            return ExitCode::FAILURE;
        }
    };
    athanor_unit::watchdog::spawn(&connection);
    reset_appearance().await;
    std::future::pending::<ExitCode>().await
}

fn main() -> ExitCode {
    athanor_unit::panic::install();
    journal::init();
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
    runtime.block_on(serve())
}
