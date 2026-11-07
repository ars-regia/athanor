//! `athanor-sessiond` (`doc_session_daemons.md` SD1): confines itself, resets COSMIC's
//! appearance values (SD16), then raises the battery notices (SD14) until the bus ends.

use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use athanor_i18n::Catalog;
use athanor_sessiond::{appearance, bus};
use athanor_unit::dirs::Dirs;
use athanor_unit::{journal, notify, sandbox};
use zbus::Connection;

const DOMAIN: &str = "athanor-sessiond";

/// `$XDG_STATE_HOME/athanor/crash-reports`, the one directory the daemon may write (SD17,
/// amended 2026-10-06, #156), created if missing.
fn crash_reports() -> Result<PathBuf, Box<dyn Error>> {
    let absolute = |name: &str| {
        env::var_os(name)
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
    };
    let state = absolute("XDG_STATE_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".local/state")))
        .ok_or("neither XDG_STATE_HOME nor HOME is an absolute path")?;
    let dir = state.join("athanor/crash-reports");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Landlock while the process has one thread (SD17): reads beneath `/usr`, `/etc` and
/// dconf's databases and invalidation file, writes only its crash reports, and no TCP.
fn confine() -> Result<(), Box<dyn Error>> {
    let dirs = Dirs::from_vars(DOMAIN, |name| env::var_os(name))
        .ok_or("XDG_RUNTIME_DIR, or both XDG_CONFIG_HOME and HOME, is not an absolute path")?;
    let write = crash_reports()?;
    let dconf = dirs.config.join("dconf");
    let invalidation = dirs.runtime.join("dconf");
    sandbox::ensure_single_threaded()?;
    sandbox::restrict(
        &[Path::new("/usr"), Path::new("/etc"), &dconf, &invalidation],
        &write,
    )?;
    sandbox::deny_tcp()?;
    Ok(())
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
    if let Err(err) = confine() {
        tracing::error!(%err, "refusing to run unconfined");
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
