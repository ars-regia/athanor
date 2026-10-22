//! Readiness under a `Type=notify` unit (doc_session.md, SN13 step 6): once the socket
//! listens, the user manager learns `WAYLAND_DISPLAY`, then systemd learns the compositor is
//! ready, so the units `ExecStartPost=` starts find the display. Outside a notify unit (in a
//! window, headless, a test) nothing is sent: the compositor never writes the environment of
//! a manager that did not start it.

use crate::Error;
use std::ffi::OsStr;
use std::time::Duration;

/// How long the user manager has to answer.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Publishes `socket` to the user manager and reports readiness, under a notify unit only.
/// A failure is fatal: the session's units would start without a display.
pub fn announce(socket: &OsStr) -> Result<(), Error> {
    if std::env::var_os("NOTIFY_SOCKET").is_none() {
        return Ok(());
    }
    let name = socket
        .to_str()
        .ok_or("the Wayland socket's name is not UTF-8")?;
    zbus::blocking::connection::Builder::session()?
        .method_timeout(TIMEOUT)
        .build()?
        .call_method(
            Some("org.freedesktop.systemd1"),
            "/org/freedesktop/systemd1",
            Some("org.freedesktop.systemd1.Manager"),
            "SetEnvironment",
            &(vec![format!("WAYLAND_DISPLAY={name}")],),
        )?;
    athanor_unit::notify::notify_ready()?;
    tracing::info!(socket = name, "ready");
    Ok(())
}
