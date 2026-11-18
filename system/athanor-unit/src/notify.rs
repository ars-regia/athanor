//! Notifications to the manager (sd_notify(3)): `READY=1` for `Type=notify` units, and
//! `RESTART_RESET=1` for a guard that has run healthily (doc_session.md, SN6), each one
//! datagram to `$NOTIFY_SOCKET`. Does nothing outside systemd.

use std::env;
use std::ffi::OsStr;
use std::io;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::time::Duration;

/// Tells systemd the layout is applied (Type=notify). Does nothing outside systemd.
pub fn notify_ready() -> io::Result<()> {
    match env::var_os("NOTIFY_SOCKET") {
        Some(socket) => notify_ready_to(&socket),
        None => Ok(()),
    }
}

/// Tells the manager that the unit has run healthily, which resets the restart counter its
/// `RestartSteps=` reads. The manager accepts it only from a unit whose `NotifyAccess=`
/// allows it. Does nothing outside systemd.
pub fn notify_restart_reset() -> io::Result<()> {
    match env::var_os("NOTIFY_SOCKET") {
        Some(socket) => notify_to(&socket, b"RESTART_RESET=1"),
        None => Ok(()),
    }
}

/// Sends `RESTART_RESET=1` once the process has run for `after`, from a thread of its own.
/// A process that exits sooner sends nothing. A failure is logged at warning level.
pub fn spawn_restart_reset(after: Duration) -> io::Result<()> {
    std::thread::Builder::new()
        .name("restart-reset".into())
        .spawn(move || {
            std::thread::sleep(after);
            if let Err(error) = notify_restart_reset() {
                tracing::warn!(%error, "cannot send RESTART_RESET=1");
            }
        })
        .map(drop)
}

fn notify_ready_to(socket: &OsStr) -> io::Result<()> {
    notify_to(socket, b"READY=1")
}

/// `socket` is a path, or an abstract name when it starts with '@' (sd_notify(3)).
fn notify_to(socket: &OsStr, message: &[u8]) -> io::Result<()> {
    let address = match socket.as_bytes().strip_prefix(b"@") {
        Some(name) => SocketAddr::from_abstract_name(name)?,
        None => SocketAddr::from_pathname(socket)?,
    };
    UnixDatagram::unbound()?.send_to_addr(message, &address)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_reaches_a_path_socket_and_an_abstract_one() {
        let path = env::temp_dir().join(format!("athanor-unit-notify-{}", std::process::id()));
        let listener = UnixDatagram::bind(&path).expect("bind");
        notify_ready_to(path.as_os_str()).expect("notify");
        let mut buffer = [0u8; 16];
        let read = listener.recv(&mut buffer).expect("recv");
        assert_eq!(&buffer[..read], b"READY=1");

        let name = format!("athanor-unit-notify-{}", std::process::id());
        let address = SocketAddr::from_abstract_name(name.as_bytes()).expect("address");
        let listener = UnixDatagram::bind_addr(&address).expect("bind");
        notify_ready_to(OsStr::new(&format!("@{name}"))).expect("notify");
        let read = listener.recv(&mut buffer).expect("recv");
        assert_eq!(&buffer[..read], b"READY=1");
    }

    #[test]
    fn a_restart_reset_is_one_restart_reset_datagram() {
        let path = env::temp_dir().join(format!("athanor-unit-reset-{}", std::process::id()));
        let listener = UnixDatagram::bind(&path).expect("bind");
        notify_to(path.as_os_str(), b"RESTART_RESET=1").expect("notify");
        let mut buffer = [0u8; 32];
        let read = listener.recv(&mut buffer).expect("recv");
        assert_eq!(&buffer[..read], b"RESTART_RESET=1");
    }
}
