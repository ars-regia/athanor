//! athanor-shelld: the shell's headless daemon (doc_bar.md BR1). athanor-shelld.service runs
//! it; the bar's unit wants it. `--record-exit` is the unit's ExecStopPost: it counts a failed
//! run towards the crash-loop limit (doc_shell.md SH8).

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use athanor_shelld::sender::BarUnit;
use athanor_shelld::server::{self, Config};
use athanor_unit::{crash_loop, journal, notify, sandbox};

fn main() -> ExitCode {
    journal::init();
    let Some((state_dir, failures)) = dirs() else {
        tracing::error!("no absolute XDG_STATE_HOME or HOME, or no absolute XDG_RUNTIME_DIR");
        return ExitCode::FAILURE;
    };
    let now = match crash_loop::boottime() {
        Ok(now) => now,
        Err(err) => {
            tracing::error!(error = %err, "cannot read CLOCK_BOOTTIME");
            return ExitCode::FAILURE;
        }
    };
    if env::args().nth(1).as_deref() == Some("--record-exit") {
        let result = env::var("SERVICE_RESULT").ok();
        return match crash_loop::record_exit(&failures, now, result.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                tracing::error!(error = %err, "cannot record the failed run");
                ExitCode::FAILURE
            }
        };
    }
    if let Err(err) = crash_loop::record_start(&failures, now) {
        tracing::error!(error = %err, "cannot update the crash-loop record");
        return ExitCode::FAILURE;
    }
    match crash_loop::given_up(&failures, now) {
        Ok(false) => {}
        Ok(true) => {
            tracing::error!(
                failures = crash_loop::GIVE_UP_AFTER,
                window_seconds = crash_loop::FAILURE_WINDOW_SECONDS,
                "athanor-shelld keeps failing; notifications and the tray stay off until the next session"
            );
            // Type=notify: exiting without READY=1 is Result=protocol, which Restart=on-failure
            // restarts on same as any other failure. Reporting ready right before this clean
            // exit is what actually lets the unit settle at inactive/Result=success.
            if let Err(err) = notify::notify_ready() {
                tracing::error!(error = %err, "cannot tell systemd the daemon gave up");
            }
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            tracing::error!(error = %err, "cannot read the crash-loop record");
            return ExitCode::FAILURE;
        }
    }
    if let Err(err) = std::fs::create_dir_all(&state_dir) {
        tracing::error!(error = %err, dir = %state_dir.display(), "cannot create the state directory");
        return ExitCode::FAILURE;
    }
    // Before any thread exists: the runtime below starts none, but zbus and tokio must be
    // confined from their first instruction. /proc for the callers' cgroups (BR1).
    let confined = sandbox::ensure_single_threaded().and_then(|()| {
        sandbox::restrict(
            &[Path::new("/usr"), Path::new("/etc"), Path::new("/proc")],
            &state_dir,
        )
    });
    if let Err(err) = confined {
        tracing::error!(error = %err, "cannot confine the daemon with Landlock");
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!(error = %err, "cannot start the runtime");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(serve(state_dir))
}

async fn serve(state_dir: PathBuf) -> ExitCode {
    let builder = match zbus::connection::Builder::session() {
        Ok(builder) => builder,
        Err(err) => {
            tracing::error!(error = %err, "no session bus");
            return ExitCode::FAILURE;
        }
    };
    let _connection = match server::start(
        builder,
        Config {
            state_dir,
            bar: BarUnit::from_proc(),
        },
    )
    .await
    {
        Ok(connection) => connection,
        Err(err) => {
            tracing::error!(error = %err, "cannot serve the notifications and the tray watcher");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = notify::notify_ready() {
        tracing::error!(error = %err, "cannot tell systemd the daemon is ready");
        return ExitCode::FAILURE;
    }
    tracing::info!("serving org.freedesktop.Notifications and org.kde.StatusNotifierWatcher");
    std::future::pending::<ExitCode>().await
}

/// `None` if `value` is unset, empty, or relative: the XDG Base Directory spec says a relative
/// value for one of these variables must be treated as though it were not set at all, not
/// resolved as a relative path.
fn xdg_absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|dir| dir.is_absolute())
}

/// `$XDG_STATE_HOME/athanor/shelld` for the do-not-disturb switch, and the crash-loop record
/// in the unit's runtime directory.
fn dirs() -> Option<(PathBuf, PathBuf)> {
    let state = xdg_absolute(env::var_os("XDG_STATE_HOME"))
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .filter(|dir| dir.is_absolute())?
        .join("athanor/shelld");
    let runtime = xdg_absolute(env::var_os("RUNTIME_DIRECTORY")).or_else(|| {
        xdg_absolute(env::var_os("XDG_RUNTIME_DIR")).map(|dir| dir.join("athanor-shelld"))
    })?;
    Some((state, runtime.join("failures")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_absolute_rejects_missing_empty_and_relative_values() {
        assert_eq!(xdg_absolute(None), None);
        assert_eq!(xdg_absolute(Some(OsString::from(""))), None);
        assert_eq!(xdg_absolute(Some(OsString::from("relative/path"))), None);
        assert_eq!(
            xdg_absolute(Some(OsString::from("/absolute/path"))),
            Some(PathBuf::from("/absolute/path"))
        );
    }
}
