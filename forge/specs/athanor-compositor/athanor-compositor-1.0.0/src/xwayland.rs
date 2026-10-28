//! X11 applications through xwayland-satellite (doc_own_stack.md section 3, OS4): the compositor
//! holds an X11 display, `:N`, with its lock file and its socket, and starts the satellite when
//! the display's first client connects, so that no X11 code runs inside the compositor.
//! The satellite reaches the compositor through a Wayland socket of its own, whose every client
//! is confined (`State::satellite_state`): the standard globals only, whatever its executable.

use crate::state::State;
use crate::Error;
use smithay::reexports::calloop::channel;
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{Interest, Mode, PostAction, RegistrationToken};
use smithay::reexports::rustix;
use smithay::reexports::rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType};
use std::io::{ErrorKind, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// The satellite the image ships.
pub const SATELLITE: &str = "/usr/bin/xwayland-satellite";

/// The display numbers tried, from the lowest.
const DISPLAYS: std::ops::RangeInclusive<u32> = 0..=32;

/// A satellite that exits sooner than this after its start is not started again before it has
/// passed: a client that makes it fail at once cannot make the compositor fork in a loop.
pub const RESTART: Duration = Duration::from_secs(1);

/// An X11 display the compositor holds, `:number`: its lock file, `<dir>/.X<number>-lock`, and
/// its socket, `<dir>/.X11-unix/X<number>`, both removed when it is dropped.
pub struct Display {
    pub number: u32,
    lock: PathBuf,
    socket: PathBuf,
    listener: UnixListener,
    /// The abstract name of the same path, bound but not listening: libxcb tries it first and
    /// is refused, then uses the socket file, so another server cannot answer under it, and a
    /// process outside the file's mount namespace, a sandboxed application's, cannot reach the
    /// display through it.
    _reserved: OwnedFd,
}

impl Display {
    /// The first display under `dir` that no live process holds, taking the lock of one whose
    /// process is gone, as the X server does.
    pub fn open(dir: &Path) -> Result<Self, Error> {
        let sockets = dir.join(".X11-unix");
        match std::fs::create_dir(&sockets) {
            // As systemd-tmpfiles makes it (tmpfiles.d/x11.conf): every user's servers listen there.
            Ok(()) => std::fs::set_permissions(&sockets, std::fs::Permissions::from_mode(0o1777))?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("{}: {error}", sockets.display()).into()),
        }
        trusted(&sockets)?;
        for number in DISPLAYS {
            let lock = dir.join(format!(".X{number}-lock"));
            if !take(&lock)? {
                continue;
            }
            let socket = sockets.join(format!("X{number}"));
            match listen(&socket) {
                Ok((listener, reserved)) => {
                    return Ok(Self {
                        number,
                        lock,
                        socket,
                        listener,
                        _reserved: reserved,
                    })
                }
                Err(error) => {
                    remove(&lock);
                    // In use under the abstract name, or a socket file another user left.
                    if !matches!(
                        error.kind(),
                        ErrorKind::AddrInUse | ErrorKind::PermissionDenied
                    ) {
                        return Err(format!("{}: {error}", socket.display()).into());
                    }
                }
            }
        }
        Err(format!("no free X11 display under {}", dir.display()).into())
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        remove(&self.socket);
        remove(&self.lock);
    }
}

/// `sockets` is a directory, not a link, that this user owns, or root with the sticky bit or
/// with no write for others, as the X server checks it: no other user can swap the socket.
fn trusted(sockets: &Path) -> Result<(), Error> {
    let meta = std::fs::symlink_metadata(sockets)?;
    let uid = rustix::process::getuid().as_raw();
    let root_owned = meta.uid() == 0 && (meta.mode() & 0o1000 != 0 || meta.mode() & 0o022 == 0);
    if meta.is_dir() && (meta.uid() == uid || root_owned) {
        Ok(())
    } else {
        Err(format!(
            "{} is not a directory root or this user holds",
            sockets.display()
        )
        .into())
    }
}

/// Removes `path`, logging a failure other than its absence.
fn remove(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), %error, "could not be removed");
        }
    }
}

/// Creates the lock file `lock` with this process's id as the X server writes it, ten columns
/// and a newline, after removing one whose process is gone; false when a live process, or a
/// lock that names none, holds it.
fn take(lock: &Path) -> Result<bool, Error> {
    for _ in 0..2 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock)
        {
            Ok(mut file) => {
                if let Err(error) =
                    file.write_all(format!("{:>10}\n", std::process::id()).as_bytes())
                {
                    // An empty lock names no process and would hold the display for good.
                    remove(lock);
                    return Err(format!("{}: {error}", lock.display()).into());
                }
                return Ok(true);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                let holder = std::fs::read_to_string(lock)
                    .ok()
                    .and_then(|text| text.trim().parse::<u32>().ok());
                match holder {
                    Some(pid) if !Path::new(&format!("/proc/{pid}")).exists() => {
                        tracing::info!(lock = %lock.display(), pid, "a stale X11 lock is taken");
                        remove(lock);
                    }
                    _ => return Ok(false),
                }
            }
            Err(error) => return Err(format!("{}: {error}", lock.display()).into()),
        }
    }
    Ok(false)
}

/// The abstract name of `path` reserved, then the socket at `path`, a stale one removed first
/// (the caller holds the display's lock).
fn listen(path: &Path) -> std::io::Result<(UnixListener, OwnedFd)> {
    let reserved = rustix::net::socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )?;
    let name = SocketAddrUnix::new_abstract_name(path.as_os_str().as_encoded_bytes())?;
    rustix::net::bind(&reserved, &name)?;
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != ErrorKind::NotFound {
            return Err(error);
        }
    }
    let listener = UnixListener::bind(path)?;
    listener.set_nonblocking(true)?;
    Ok((listener, reserved))
}

/// The display and how the satellite is started on it.
pub struct X11 {
    display: Display,
    /// The satellite's Wayland socket, `<runtime dir>/athanor-x11-<number>`.
    wayland: PathBuf,
    program: PathBuf,
    /// Started through `systemd-run --user --scope`, in a unit of its own (OS4), when the
    /// compositor runs under the user manager.
    scope: bool,
    /// The display socket's source, disabled while the satellite runs, since it listens there.
    source: RegistrationToken,
    /// The source of the satellite's Wayland socket.
    satellite: RegistrationToken,
    exits: channel::Sender<std::io::Result<ExitStatus>>,
    pub running: bool,
    /// How many times the satellite was started, and when it was last tried.
    pub starts: u32,
    pub last: Option<Instant>,
}

impl Drop for X11 {
    fn drop(&mut self) {
        remove(&self.wayland);
    }
}

/// Holds an X11 display under `dir`, and the satellite's Wayland socket under `runtime`, and
/// starts `program` on the display's first client; the display's number.
///
/// Not `WAYLAND_SOCKET`: the satellite starts Xwayland before it connects, and Xwayland would
/// inherit the variable and the descriptor and take the satellite's connection.
pub fn start(
    state: &mut State,
    dir: &Path,
    runtime: &Path,
    program: PathBuf,
    scope: bool,
) -> Result<u32, Error> {
    let display = Display::open(dir)?;
    let number = display.number;
    let wayland = runtime.join(format!("athanor-x11-{number}"));
    // The display's lock is held: a file under its name is a socket a dead compositor left.
    remove(&wayland);
    let listener =
        UnixListener::bind(&wayland).map_err(|error| format!("{}: {error}", wayland.display()))?;
    listener.set_nonblocking(true)?;
    let handle = state.loop_handle.clone();
    let source = Generic::new(listener.try_clone()?, Interest::READ, Mode::Level);
    let satellite = handle
        .insert_source(source, move |_, _, state| loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let data = state.satellite_state();
                    if let Err(error) = state.display_handle.insert_client(stream, data) {
                        tracing::warn!(%error, "xwayland-satellite could not connect");
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    return Ok(PostAction::Continue)
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::ConnectionAborted | ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    // Left armed, a connection it cannot accept would wake the loop at once.
                    tracing::error!(%error, "xwayland-satellite's Wayland socket waits a second");
                    if let Some(x11) = &state.x11 {
                        state.rearm(x11.satellite, RESTART);
                    }
                    return Ok(PostAction::Disable);
                }
            }
        })
        .map_err(|e| e.error)?;
    let (exits, exited) = channel::channel();
    handle
        .insert_source(exited, |event, _, state| {
            if let channel::Event::Msg(status) = event {
                state.satellite_exited(status);
            }
        })
        .map_err(|e| e.error)?;
    let source = Generic::new(display.listener.try_clone()?, Interest::READ, Mode::Level);
    let source = handle
        .insert_source(source, |_, _, state| Ok(state.x11_client()))
        .map_err(|e| e.error)?;
    state.x11 = Some(X11 {
        display,
        wayland,
        program,
        scope,
        source,
        satellite,
        exits,
        running: false,
        starts: 0,
        last: None,
    });
    Ok(number)
}

impl State {
    /// An X11 client is waiting: the satellite starts and takes over the display's socket until
    /// it exits. Within `RESTART` of the last try the client waits for the rest of it, so that
    /// a satellite that fails at once is tried once a second at most; a start that fails drops
    /// the client.
    fn x11_client(&mut self) -> PostAction {
        let Some(x11) = &self.x11 else {
            return PostAction::Disable;
        };
        let wait = x11.last.map_or(Duration::ZERO, |last| {
            RESTART.saturating_sub(last.elapsed())
        });
        if !wait.is_zero() {
            tracing::warn!(?wait, "xwayland-satellite was tried less than a second ago");
            self.rearm(x11.source, wait);
            return PostAction::Disable;
        }
        if self.start_satellite() {
            return PostAction::Disable;
        }
        match self.drop_x11_clients() {
            Ok(()) => PostAction::Continue,
            Err(error) => {
                // Left armed, a client it cannot accept would wake the loop at once.
                tracing::error!(%error, "the X11 display waits a second");
                if let Some(x11) = &self.x11 {
                    self.rearm(x11.source, RESTART);
                }
                PostAction::Disable
            }
        }
    }

    /// Enables the disabled source `token` again after `delay`.
    fn rearm(&self, token: RegistrationToken, delay: Duration) {
        let timer = Timer::from_duration(delay);
        let inserted = self.loop_handle.insert_source(timer, move |_, _, state| {
            if let Err(error) = state.loop_handle.enable(&token) {
                tracing::error!(%error, "the X11 display stops listening");
            }
            TimeoutAction::Drop
        });
        if let Err(error) = inserted {
            tracing::error!(error = %error.error, "the X11 display stops listening");
        }
    }

    /// Starts the satellite; whether it runs.
    fn start_satellite(&mut self) -> bool {
        let Some(x11) = self.x11.as_mut() else {
            return false;
        };
        x11.last = Some(Instant::now());
        // The waiter first, so that a satellite is never left running unwaited.
        let (give, take) = std::sync::mpsc::channel::<Child>();
        let exits = x11.exits.clone();
        let waiter = std::thread::Builder::new()
            .name("xwayland-satellite".into())
            .spawn(move || {
                // No child comes when the start fails, and the thread ends.
                if let Ok(mut child) = take.recv() {
                    // The receiver lives as long as the event loop.
                    drop(exits.send(child.wait()));
                }
            });
        if let Err(error) = waiter {
            tracing::error!(%error, "xwayland-satellite cannot be waited for: it is not started");
            return false;
        }
        let child = match spawn(x11) {
            Ok(child) => child,
            Err(error) => {
                tracing::error!(%error, "xwayland-satellite could not start");
                return false;
            }
        };
        let pid = child.id();
        if let Err(std::sync::mpsc::SendError(mut child)) = give.send(child) {
            tracing::error!(
                pid,
                "xwayland-satellite cannot be waited for: it is stopped"
            );
            if let Err(error) = child.kill().and_then(|()| child.wait().map(drop)) {
                tracing::error!(pid, %error, "xwayland-satellite could not be stopped");
            }
            return false;
        }
        x11.running = true;
        x11.starts += 1;
        tracing::info!(
            display = x11.display.number,
            pid,
            "xwayland-satellite started"
        );
        true
    }

    /// Drops the X11 clients waiting on the display's socket.
    fn drop_x11_clients(&self) -> std::io::Result<()> {
        let Some(x11) = &self.x11 else {
            return Ok(());
        };
        loop {
            match x11.display.listener.accept() {
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }

    /// The satellite exited: the clients that connected meanwhile are dropped, so that one that
    /// makes it fail does not restart it in a loop, and the next client starts it again.
    fn satellite_exited(&mut self, status: std::io::Result<ExitStatus>) {
        let Some(x11) = self.x11.as_mut() else {
            return;
        };
        match status {
            Ok(status) if status.success() => tracing::info!("xwayland-satellite exited"),
            Ok(status) => tracing::warn!(%status, "xwayland-satellite exited"),
            Err(error) => tracing::error!(%error, "xwayland-satellite could not be waited for"),
        }
        x11.running = false;
        let source = x11.source;
        if let Err(error) = self.drop_x11_clients() {
            tracing::error!(%error, "the X11 clients that came meanwhile stay");
        }
        // The source was disabled when the satellite started.
        if let Err(error) = self.loop_handle.enable(&source) {
            tracing::error!(%error, "the X11 display stops listening");
        }
    }
}

/// Starts the satellite on `x11`'s display, its own Wayland socket as `WAYLAND_DISPLAY` and the
/// display's socket as `-listenfd`.
fn spawn(x11: &X11) -> std::io::Result<Child> {
    // A duplicate without close-on-exec, which the child inherits, dropped once it is spawned.
    // ponytail: a process another thread spawns meanwhile inherits it too; the compositor
    // spawns no other process, and pre_exec would need unsafe code.
    let socket = rustix::io::dup(&x11.display.listener)?;
    let mut command = if x11.scope {
        let mut command = Command::new("systemd-run");
        command
            .args(["--user", "--scope", "--collect", "--quiet", "--"])
            .arg(&x11.program);
        command
    } else {
        Command::new(&x11.program)
    };
    command
        .arg(format!(":{}", x11.display.number))
        .args(["-listenfd", &socket.as_raw_fd().to_string()])
        .env("WAYLAND_DISPLAY", &x11.wayland)
        .env_remove("WAYLAND_SOCKET")
        .env_remove("DISPLAY")
        .env_remove("NOTIFY_SOCKET")
        .stdin(Stdio::null())
        .spawn()
}

// Test-only code comes last: `verify.py panics` stops reading a file where it begins.
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixStream};

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-x11-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    /// A display a live process holds is skipped, one whose process is gone is taken, and the
    /// files go with the display.
    #[test]
    fn the_first_free_display_is_taken() {
        let dir = dir("free");
        std::fs::write(
            dir.join(".X0-lock"),
            format!("{:>10}\n", std::process::id()),
        )
        .expect("lock");
        // Above the kernel's limit of process ids: no process has it.
        std::fs::write(dir.join(".X1-lock"), "1073741824\n").expect("lock");
        std::fs::create_dir(dir.join(".X11-unix")).expect("sockets");
        std::fs::write(dir.join(".X11-unix/X1"), "").expect("stale socket");
        let display = Display::open(&dir).expect("display");
        assert_eq!(display.number, 1);
        let lock = std::fs::read_to_string(dir.join(".X1-lock")).expect("lock");
        assert_eq!(lock, format!("{:>10}\n", std::process::id()));
        UnixStream::connect(dir.join(".X11-unix/X1")).expect("the socket");
        let name = dir.join(".X11-unix/X1");
        let address =
            SocketAddr::from_abstract_name(name.as_os_str().as_encoded_bytes()).expect("address");
        let refused = UnixStream::connect_addr(&address).expect_err("the abstract name answers");
        assert_eq!(refused.kind(), ErrorKind::ConnectionRefused);
        drop(display);
        assert!(!dir.join(".X1-lock").exists(), "the lock stays");
        assert!(!dir.join(".X11-unix/X1").exists(), "the socket stays");
        assert!(dir.join(".X0-lock").exists(), "another server's lock went");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// A sockets directory that is a link, which another user could have made, holds no
    /// display.
    #[test]
    fn a_linked_sockets_directory_is_refused() {
        let dir = dir("linked");
        std::fs::create_dir(dir.join("elsewhere")).expect("target");
        std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join(".X11-unix")).expect("link");
        assert!(Display::open(&dir).is_err());
        assert!(!dir.join(".X0-lock").exists(), "a lock was taken");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// A lock that names no process is left alone: it is not known to be stale.
    #[test]
    fn an_unreadable_lock_is_held() {
        let dir = dir("unreadable");
        std::fs::write(dir.join(".X0-lock"), "not a pid\n").expect("lock");
        let display = Display::open(&dir).expect("display");
        assert_eq!(display.number, 1);
        drop(display);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
