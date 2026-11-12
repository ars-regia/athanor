//! The daemon's side of the decoder's socket (SD17, maintainer decision, 2026-11-07): the
//! image file goes over as a descriptor with the sizes asked, and one frame comes back per
//! size. The whole exchange is bounded by [`DEADLINE`]; past it, or on any fault of the
//! decoder, the caller shows the hearth.

use std::fs::File;
use std::io::{self, IoSlice};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::path::Path;
use std::time::Duration;

use athanor_wallpaper::protocol::{self, FRAME_HEADER_LEN};
use gio::prelude::*;
use nix::sys::socket::{
    connect, sendmsg, socket, AddressFamily, ControlMessage, MsgFlags, SockFlag, SockType, UnixAddr,
};

/// How long one decode may take, the connection and every frame included: the decoder's unit
/// ends it after `RuntimeMaxSec=18` too, the time measured for the 8 sizes of a request's
/// maximum with a margin (maintainer decision, 2026-11-07).
pub const DEADLINE: Duration = Duration::from_secs(18);

/// The pixels of each size asked, in the order asked: straight RGBA8, rows packed.
pub type Frames = Vec<((u32, u32), Vec<u8>)>;

/// Why a decode gave no frames.
#[derive(Debug, PartialEq)]
pub enum Failure {
    /// The decoder answered that the picture cannot be decoded: asking again gives the same.
    Refused,
    /// The decoder could not be reached, ended the connection or did not answer in time:
    /// asking again may succeed.
    Fault(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused => f.write_str("the decoder cannot decode the picture"),
            Self::Fault(why) => f.write_str(why),
        }
    }
}

/// Decodes `file` at `sizes` through the decoder listening on `socket`, within [`DEADLINE`].
pub async fn decode(socket: &Path, file: File, sizes: &[(u32, u32)]) -> Result<Frames, Failure> {
    glib::future_with_timeout(DEADLINE, exchange(socket, file, sizes))
        .await
        .map_err(|_| Failure::Fault(format!("the decoder did not answer within {DEADLINE:?}")))?
}

async fn exchange(socket: &Path, file: File, sizes: &[(u32, u32)]) -> Result<Frames, Failure> {
    let fault = Failure::Fault;
    let request =
        protocol::request(sizes).ok_or_else(|| fault("no valid size to ask for".into()))?;
    // Non-blocking: a Unix socket whose backlog is full answers EAGAIN at once instead of
    // holding the loop, and that is a fault to retry like any other.
    let stream = connected(socket).map_err(|err| {
        fault(format!(
            "cannot reach the decoder at {}: {err}",
            socket.display()
        ))
    })?;
    send(stream.as_fd(), &file, &request)
        .map_err(|err| fault(format!("cannot send the request: {err}")))?;
    drop(file);
    let socket = gio::Socket::from_fd(stream).map_err(|err| fault(err.to_string()))?;
    // The connection owns the stream and closes it when dropped: it lives until the end.
    let connection = socket.connection_factory_create_connection();
    let input = connection.input_stream();
    let mut frames = Vec::with_capacity(sizes.len());
    for &size in sizes {
        let header = read(&input, vec![0; FRAME_HEADER_LEN]).await?;
        let header: [u8; FRAME_HEADER_LEN] = header
            .try_into()
            .map_err(|_| fault("a short frame header".into()))?;
        if frames.is_empty() && header == protocol::refusal() {
            return Err(Failure::Refused);
        }
        if !protocol::check_frame(&header, size) {
            return Err(fault(format!(
                "the decoder answered another frame than {size:?}"
            )));
        }
        frames.push((
            size,
            read(&input, vec![0; protocol::frame_len(size)]).await?,
        ));
    }
    Ok(frames)
}

/// A non-blocking stream connected to `path`.
fn connected(path: &Path) -> nix::Result<OwnedFd> {
    let fd = socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
        None,
    )?;
    connect(fd.as_raw_fd(), &UnixAddr::new(path)?)?;
    Ok(fd)
}

/// Sends `request` with `file` as `SCM_RIGHTS`, whole: a new connection's buffer holds it.
fn send(stream: BorrowedFd<'_>, file: &File, request: &[u8]) -> io::Result<()> {
    let fds = [file.as_raw_fd()];
    let sent = sendmsg::<()>(
        stream.as_raw_fd(),
        &[IoSlice::new(request)],
        &[ControlMessage::ScmRights(&fds)],
        MsgFlags::MSG_NOSIGNAL,
        None,
    )?;
    if sent != request.len() {
        return Err(io::Error::other(format!(
            "{sent} of {} bytes sent",
            request.len()
        )));
    }
    Ok(())
}

/// Fills `buf` from `input`; an end before it is full is the decoder's failure.
async fn read(input: &gio::InputStream, buf: Vec<u8>) -> Result<Vec<u8>, Failure> {
    let want = buf.len();
    match input.read_all_future(buf, glib::Priority::DEFAULT).await {
        Ok((buf, read, None)) if read == want => Ok(buf),
        Ok((_, read, None)) => Err(Failure::Fault(format!(
            "the decoder ended after {read} of {want} bytes"
        ))),
        Ok((_, _, Some(err))) | Err((_, err)) => {
            Err(Failure::Fault(format!("cannot read the decoder: {err}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    /// A listener in a fresh directory, and its path.
    fn listener(name: &str) -> (UnixListener, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("athanor-wallpaper-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("directory");
        let path = dir.join("decode");
        let _ = std::fs::remove_file(&path);
        (UnixListener::bind(&path).expect("bind"), path)
    }

    /// Runs `decode` against a decoder that does `serve` with the connection.
    fn run(name: &str, sizes: &[(u32, u32)], serve: fn(UnixStream)) -> Result<Frames, Failure> {
        let (listener, path) = listener(name);
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            serve(stream);
        });
        let file = File::open("/proc/self/exe").expect("a file");
        let result = glib::MainContext::new().block_on(decode(&path, file, sizes));
        server.join().expect("the decoder thread");
        result
    }

    fn take_request(stream: &mut UnixStream) {
        let mut request = [0u8; protocol::REQUEST_LEN];
        stream.read_exact(&mut request).expect("request");
        assert!(protocol::parse_request(&request).is_some());
    }

    #[test]
    fn the_frames_asked_are_read() {
        let frames = run("good", &[(2, 1), (1, 1)], |mut stream| {
            take_request(&mut stream);
            for (size, px) in [((2, 1), 7u8), ((1, 1), 9)] {
                stream
                    .write_all(&protocol::frame_header(size))
                    .expect("header");
                stream
                    .write_all(&vec![px; protocol::frame_len(size)])
                    .expect("pixels");
            }
        })
        .expect("decoded");
        assert_eq!(frames, vec![((2, 1), vec![7; 8]), ((1, 1), vec![9; 4])]);
    }

    #[test]
    fn a_frame_of_another_size_or_a_short_one_fails() {
        let other = run("other", &[(2, 1)], |mut stream| {
            take_request(&mut stream);
            stream
                .write_all(&protocol::frame_header((1, 2)))
                .expect("header");
            stream.write_all(&[0; 8]).expect("pixels");
        });
        assert!(other.is_err(), "{other:?}");
        let short = run("short", &[(2, 1)], |mut stream| {
            take_request(&mut stream);
            stream
                .write_all(&protocol::frame_header((2, 1)))
                .expect("header");
            stream.write_all(&[0; 7]).expect("pixels");
        });
        assert!(short.is_err(), "{short:?}");
        let closed = run("closed", &[(2, 1)], drop);
        assert!(matches!(closed, Err(Failure::Fault(_))), "{closed:?}");
    }

    #[test]
    fn a_refusal_is_told_from_a_fault() {
        let refused = run("refused", &[(2, 1), (1, 1)], |mut stream| {
            take_request(&mut stream);
            stream.write_all(&protocol::refusal()).expect("refusal");
        });
        assert_eq!(refused, Err(Failure::Refused));
    }

    #[test]
    fn a_full_backlog_does_not_hold_the_caller() {
        use nix::sys::socket::{bind, listen, Backlog};
        let dir =
            std::env::temp_dir().join(format!("athanor-wallpaper-full-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("directory");
        let path = dir.join("decode");
        let _ = std::fs::remove_file(&path);
        let listener = socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::empty(),
            None,
        )
        .expect("socket");
        bind(
            listener.as_raw_fd(),
            &UnixAddr::new(&path).expect("address"),
        )
        .expect("bind");
        listen(&listener, Backlog::new(0).expect("backlog")).expect("listen");
        // A backlog of 0 queues one connection; the next one would wait for an accept.
        let _queued = UnixStream::connect(&path).expect("the queued connection");
        let (done, result) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let file = File::open("/proc/self/exe").expect("a file");
            let _ = done.send(glib::MainContext::new().block_on(decode(&path, file, &[(1, 1)])));
        });
        let result = result
            .recv_timeout(DEADLINE + Duration::from_secs(2))
            .expect("decode returned");
        assert!(matches!(result, Err(Failure::Fault(_))), "{result:?}");
        drop(listener);
    }

    #[test]
    fn a_decoder_that_never_answers_is_given_up_at_the_deadline() {
        let started = std::time::Instant::now();
        let silent = run("silent", &[(2, 1)], |mut stream| {
            take_request(&mut stream);
            std::thread::sleep(DEADLINE + Duration::from_secs(1));
        });
        assert!(matches!(silent, Err(Failure::Fault(_))), "{silent:?}");
        assert!(started.elapsed() >= DEADLINE);
    }

    #[test]
    fn no_decoder_fails_at_once() {
        let missing = glib::MainContext::new().block_on(decode(
            Path::new("/nonexistent/decode"),
            File::open("/proc/self/exe").expect("a file"),
            &[(1, 1)],
        ));
        assert!(matches!(missing, Err(Failure::Fault(_))), "{missing:?}");
    }
}
