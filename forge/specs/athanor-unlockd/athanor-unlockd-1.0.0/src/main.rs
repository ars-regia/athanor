//! athanor-unlockd: the root helper that runs PAM for the session lock (doc_lock_and_prompts.md,
//! LP8). systemd starts one `athanor-unlockd@.service` per connection to
//! `/run/athanor/unlockd.socket`, with the connection as fd 0 and fd 1, and it runs one
//! transaction and exits:
//!
//! 1. no core dump and no ptrace attach by the user (D26);
//! 2. the locale of the system, for PAM's texts (D32);
//! 3. the peer: the lock's unit of its uid, with an active Wayland session; otherwise `FAILURE`;
//! 4. the client's first line, the PAM service (D28); end of stream ends with no verdict;
//! 5. the transaction for the account of the peer's uid, and nothing the client names;
//! 6. `SUCCESS` and exit 0, or `FAILURE` and exit 2; an internal error is `FAILURE` and exit 1.
//!
//! The journal gets one line per transaction: uid, service, verdict, never the client's text.

#![deny(unsafe_code)]

mod conversation;
mod ffi;
mod peer;

use std::error::Error;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::process::ExitCode;

use athanor_auth::{read_line_within, ReadError, Secret, Service, FAILURE, MAX_SELECTOR, SUCCESS};
use nix::unistd::{Uid, User};

use crate::conversation::Conversation;
use crate::ffi::Transaction;
use crate::peer::{Internal, Refused};

/// How the helper ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// The transaction ran: `true` for `SUCCESS`.
    Verdict(bool),
    /// The peer may not ask, or named no service of the lock: no PAM transaction ran.
    Refused,
    /// The client closed before naming a service.
    Closed,
}

/// The line written to the client and the exit status, for an end or an internal error.
fn outcome(end: &Result<End, Box<dyn Error>>) -> (Option<&'static [u8]>, u8) {
    match end {
        Ok(End::Verdict(true)) => (Some(SUCCESS), 0),
        Ok(End::Verdict(false) | End::Refused) => (Some(FAILURE), 2),
        Ok(End::Closed) => (None, 2),
        Err(_) => (Some(FAILURE), 1),
    }
}

fn main() -> ExitCode {
    athanor_unit::journal::init();
    let end = run();
    if let Err(error) = &end {
        tracing::error!(%error, "the helper could not run the transaction");
    }
    let (line, status) = outcome(&end);
    if let Some(line) = line {
        let mut stdout = io::stdout().lock();
        if let Err(error) = stdout.write_all(line).and_then(|()| stdout.flush()) {
            // The client is gone: a verdict written to no one changes nothing.
            tracing::warn!(%error, "the verdict could not be written");
        }
    }
    ExitCode::from(status)
}

fn run() -> Result<End, Box<dyn Error>> {
    nix::sys::prctl::set_dumpable(false)?;
    nix::sys::resource::setrlimit(nix::sys::resource::Resource::RLIMIT_CORE, 0, 0)?;
    if let Err(error) = ffi::set_locale() {
        tracing::warn!(%error, "PAM's texts stay in the C locale");
    }
    serve(
        io::stdin().as_fd(),
        io::stdout().as_fd(),
        peer::check_socket,
        ffi::authenticate,
    )
}

/// The connection's work, from the peer check to the verdict, over the connection's read and
/// write ends, the peer check and the PAM transaction. The account is the one of the peer's
/// uid from `SO_PEERCRED`; the check runs before any byte of the client is read.
fn serve(
    socket: BorrowedFd<'_>,
    out: BorrowedFd<'_>,
    check_socket: impl FnOnce(BorrowedFd<'_>, u32) -> Result<Result<(), Refused>, Internal>,
    authenticate: impl FnOnce(&CStr, &mut Conversation<File, File>) -> Transaction,
) -> Result<End, Box<dyn Error>> {
    let uid = peer::peer_uid(socket)?;
    if let Err(refused) = check_socket(socket, uid)? {
        match &refused {
            Refused::Cgroup { cgroup } => {
                tracing::warn!(uid, cgroup = ?cgroup, "refused: {refused}");
            }
            Refused::Gone | Refused::NoSession => tracing::warn!(uid, "refused: {refused}"),
        }
        return Ok(End::Refused);
    }

    // Raw descriptors, not Stdin and Stdout: their buffers would keep copies of the answers.
    let mut reader = File::from(socket.try_clone_to_owned()?);
    let writer = File::from(out.try_clone_to_owned()?);

    let mut selector = Secret::new();
    let service = match read_line_within(&mut reader, &mut selector, MAX_SELECTOR) {
        Ok(()) => Service::from_selector(selector.as_bytes()),
        Err(ReadError::Eof) => return Ok(End::Closed),
        Err(ReadError::TooLong | ReadError::Nul) => None,
        Err(ReadError::Io(error)) => return Err(error.into()),
    };
    let Some(service) = service else {
        tracing::warn!(uid, "refused: the client named no PAM service of the lock");
        return Ok(End::Refused);
    };

    let account = User::from_uid(Uid::from_raw(uid))?.ok_or("the peer's uid has no account")?;
    let user = CString::new(account.name)?;
    let mut conversation = Conversation::new(reader, writer, service, uid);
    let transaction = authenticate(&user, &mut conversation);
    let success = transaction.succeeded();
    tracing::info!(
        uid,
        service = service.name(),
        verdict = if success { "success" } else { "failure" },
        start = transaction.start,
        authenticate = ?transaction.authenticate,
        account = ?transaction.account,
        user_kept = transaction.user_kept,
        "transaction"
    );
    Ok(End::Verdict(success))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::os::unix::net::UnixStream;

    /// Runs `serve` on a socket pair whose client wrote `input` and closed its write end, with
    /// `checked` as the peer check's answer; returns the end, the uid the check got, and the
    /// user PAM was started for, if it was.
    fn serve_with(
        input: &[u8],
        checked: Result<(), Refused>,
    ) -> (End, Option<u32>, Option<CString>) {
        let (helper, mut client) = UnixStream::pair().expect("a socket pair");
        client.write_all(input).expect("the client's lines");
        client
            .shutdown(std::net::Shutdown::Write)
            .expect("the client's end of stream");
        let asked = RefCell::new(None);
        let started = RefCell::new(None);
        let end = serve(
            helper.as_fd(),
            helper.as_fd(),
            |_, uid| {
                *asked.borrow_mut() = Some(uid);
                Ok(checked)
            },
            |user, _| {
                *started.borrow_mut() = Some(user.to_owned());
                Transaction {
                    start: 4,
                    authenticate: None,
                    user_kept: false,
                    account: None,
                }
            },
        )
        .expect("no internal error");
        (end, asked.into_inner(), started.into_inner())
    }

    #[test]
    fn a_refused_peer_starts_no_pam_transaction() {
        for refused in [
            Refused::Gone,
            Refused::NoSession,
            Refused::Cgroup {
                cgroup: "0::/user.slice\n".into(),
            },
        ] {
            let (end, asked, started) = serve_with(b"athanor-lock\n", Err(refused));
            assert_eq!(end, End::Refused);
            assert_eq!(asked, Some(nix::unistd::getuid().as_raw()));
            assert_eq!(started, None, "PAM started for a refused peer");
        }
    }

    #[test]
    fn the_account_is_the_peers_uid_whatever_the_client_names() {
        let own = User::from_uid(nix::unistd::getuid())
            .expect("the passwd database")
            .expect("an account for the test's uid");
        let (end, asked, started) = serve_with(b"athanor-lock\nnobody\n", Ok(()));
        assert_eq!(end, End::Verdict(false));
        assert_eq!(asked, Some(own.uid.as_raw()));
        assert_eq!(
            started,
            Some(CString::new(own.name).expect("a name without NUL"))
        );
        let (end, _, started) = serve_with(b"nobody\n", Ok(()));
        assert_eq!((end, started), (End::Refused, None));
    }

    #[test]
    fn each_end_writes_its_line_and_exits_with_its_status() {
        assert_eq!(outcome(&Ok(End::Verdict(true))), (Some(SUCCESS), 0));
        assert_eq!(outcome(&Ok(End::Verdict(false))), (Some(FAILURE), 2));
        assert_eq!(outcome(&Ok(End::Refused)), (Some(FAILURE), 2));
        assert_eq!(outcome(&Ok(End::Closed)), (None, 2));
        assert_eq!(outcome(&Err("no pidfd".into())), (Some(FAILURE), 1));
    }

    #[test]
    fn no_selector_names_a_user() {
        for line in [
            &b"root"[..],
            b"hr-mes",
            b"athanor-lock root",
            b"athanor-lock\0",
        ] {
            assert_eq!(Service::from_selector(line), None);
        }
        let mut selector = Secret::new();
        let mut long: &[u8] = &[b'a'; MAX_SELECTOR + 1];
        assert!(matches!(
            read_line_within(&mut long, &mut selector, MAX_SELECTOR),
            Err(ReadError::TooLong)
        ));
    }
}
