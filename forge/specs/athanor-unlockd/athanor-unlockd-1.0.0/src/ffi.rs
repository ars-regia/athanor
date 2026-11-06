//! Every foreign call of the helper (LP8, D24): the PAM transaction through libpam, the peer's
//! pidfd, the locale and the exit that runs no atexit handler. Each call is one `unsafe` block;
//! the conversation itself is the pure `Conversation::converse`, and the callback here only
//! moves PAM's messages in and the answers out.
//!
//! The transaction is `pam_start`, `pam_authenticate`, `pam_get_item(PAM_USER)`,
//! `pam_acct_mgmt` and `pam_end`, nothing else (D27): no `pam_setcred`, no `pam_open_session`,
//! no `pam_chauthtok`, no `PAM_RUSER`.

#![allow(unsafe_code)]

use std::ffi::{c_char, c_int, c_void, CStr};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::ptr;

use athanor_auth::{Kind, Outcome, Secret, Service};

use crate::conversation::{ConvError, Conversation, Message};

// Linux-PAM's `_pam_types.h` and `pam_appl.h`.
const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_PERM_DENIED: c_int = 6;
const PAM_AUTHINFO_UNAVAIL: c_int = 9;
const PAM_USER_UNKNOWN: c_int = 10;
const PAM_CONV_ERR: c_int = 19;
const PAM_USER: c_int = 2;
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 0x0001;
/// The flags of `pam_authenticate` and `pam_acct_mgmt`: an empty password never passes (D27).
const FLAGS: c_int = PAM_DISALLOW_NULL_AUTHTOK;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
/// `PAM_MAX_NUM_MSG`: libpam never asks more in one call.
const PAM_MAX_NUM_MSG: c_int = 32;

#[repr(C)]
struct PamHandle {
    _opaque: [u8; 0],
}

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConvFn = extern "C" fn(
    num_msg: c_int,
    msg: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    appdata_ptr: *mut c_void,
) -> c_int;

#[repr(C)]
struct PamConv {
    conv: Option<ConvFn>,
    appdata_ptr: *mut c_void,
}

#[link(name = "pam")]
extern "C" {
    fn pam_start(
        service_name: *const c_char,
        user: *const c_char,
        pam_conversation: *const PamConv,
        pamh: *mut *mut PamHandle,
    ) -> c_int;
    /// Linux-PAM 1.4 and later: the service files are read from `confdir`. Only the tests pass
    /// one, a directory of their own control stacks.
    fn pam_start_confdir(
        service_name: *const c_char,
        user: *const c_char,
        pam_conversation: *const PamConv,
        confdir: *const c_char,
        pamh: *mut *mut PamHandle,
    ) -> c_int;
    fn pam_authenticate(pamh: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_acct_mgmt(pamh: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_get_item(pamh: *const PamHandle, item_type: c_int, item: *mut *const c_void) -> c_int;
    fn pam_end(pamh: *mut PamHandle, pam_status: c_int) -> c_int;
}

/// What each step of a transaction returned; `None` for a step that did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transaction {
    pub start: c_int,
    pub authenticate: Option<c_int>,
    /// Whether `PAM_USER` still names the account the transaction started for.
    pub user_kept: bool,
    pub account: Option<c_int>,
}

impl Transaction {
    /// `SUCCESS` only when every step succeeded and no module changed the user.
    pub fn succeeded(&self) -> bool {
        self.start == PAM_SUCCESS
            && self.authenticate == Some(PAM_SUCCESS)
            && self.user_kept
            && self.account == Some(PAM_SUCCESS)
    }

    /// The verdict for `service`. `UNAVAILABLE` only for the fingerprint service, and only
    /// for the two codes its stack ends on with `die` and without counting (no reader, no
    /// enrolled finger), from the authentication phase: the password stack's
    /// `PAM_USER_UNKNOWN` stays `FAILURE`, so no verdict tells whether an account exists.
    pub fn outcome(&self, service: Service) -> Outcome {
        if self.succeeded() {
            Outcome::Success
        } else if service == Service::Fingerprint
            && matches!(
                self.authenticate,
                Some(PAM_AUTHINFO_UNAVAIL | PAM_USER_UNKNOWN)
            )
        {
            Outcome::Unavailable
        } else {
            Outcome::Failure
        }
    }
}

/// The PAM service file of `service`, as libpam takes it.
fn service_name(service: Service) -> &'static CStr {
    match service {
        Service::Password => c"athanor-lock",
        Service::Fingerprint => c"athanor-lock-fingerprint",
    }
}

/// Runs one transaction for `user` on `conversation`'s service, with the system's PAM
/// configuration.
pub fn authenticate<R: Read, W: Write>(
    user: &CStr,
    conversation: &mut Conversation<R, W>,
) -> Transaction {
    transact(user, None, conversation)
}

fn transact<R: Read, W: Write>(
    user: &CStr,
    confdir: Option<&CStr>,
    conversation: &mut Conversation<R, W>,
) -> Transaction {
    let service = service_name(conversation.service);
    let conv = PamConv {
        conv: Some(converse::<R, W>),
        appdata_ptr: ptr::from_mut(conversation).cast(),
    };
    let mut handle: *mut PamHandle = ptr::null_mut();
    let start = match confdir {
        // SAFETY: the strings are NUL-terminated and live for the call; `conv` and the
        // conversation it points to outlive the transaction, which ends with `pam_end` below
        // before this function returns, and nothing else touches the conversation meanwhile.
        None => unsafe { pam_start(service.as_ptr(), user.as_ptr(), &conv, &mut handle) },
        // SAFETY: as for `pam_start`; `confdir` is NUL-terminated and lives for the call.
        Some(dir) => unsafe {
            pam_start_confdir(
                service.as_ptr(),
                user.as_ptr(),
                &conv,
                dir.as_ptr(),
                &mut handle,
            )
        },
    };
    let mut transaction = Transaction {
        start,
        authenticate: None,
        user_kept: false,
        account: None,
    };
    if handle.is_null() {
        return transaction;
    }
    let mut status = start;
    if start == PAM_SUCCESS {
        // SAFETY: `handle` is the live handle pam_start returned.
        status = unsafe { pam_authenticate(handle, FLAGS) };
        transaction.authenticate = Some(status);
    }
    if status == PAM_SUCCESS {
        transaction.user_kept = same_user(handle, user);
        if transaction.user_kept {
            // SAFETY: `handle` is the live handle pam_start returned.
            status = unsafe { pam_acct_mgmt(handle, FLAGS) };
            transaction.account = Some(status);
        }
    }
    // A changed user ends the transaction refused, whatever the modules said.
    if !transaction.succeeded() && status == PAM_SUCCESS {
        status = PAM_PERM_DENIED;
    }
    // SAFETY: `handle` is live, and is not used after pam_end frees it.
    unsafe { pam_end(handle, status) };
    transaction
}

/// Whether `PAM_USER` still names `user`: a module may change it, and the helper then refuses,
/// as polkit's helper does.
fn same_user(handle: *mut PamHandle, user: &CStr) -> bool {
    let mut item: *const c_void = ptr::null();
    // SAFETY: `handle` is live and `item` is a live local libpam writes one pointer into.
    let got = unsafe { pam_get_item(handle, PAM_USER, &mut item) };
    if got != PAM_SUCCESS || item.is_null() {
        return false;
    }
    // SAFETY: PAM_USER is a NUL-terminated string owned by the handle, alive until the next
    // pam_set_item or pam_end, neither of which runs while it is read here.
    unsafe { CStr::from_ptr(item.cast()) == user }
}

/// PAM's conversation callback: PAM's messages in, `Conversation::converse` over them, the
/// answers out in the `calloc`ed array libpam frees. A client gone before any answer on the
/// password service ends the process here, with `_exit(2)` and no verdict (D27).
extern "C" fn converse<R: Read, W: Write>(
    num_msg: c_int,
    msg: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    appdata_ptr: *mut c_void,
) -> c_int {
    if !(1..=PAM_MAX_NUM_MSG).contains(&num_msg)
        || msg.is_null()
        || resp.is_null()
        || appdata_ptr.is_null()
    {
        return PAM_CONV_ERR;
    }
    let count = num_msg.unsigned_abs() as usize;
    // SAFETY: `appdata_ptr` is the conversation `transact` passed, alive for the transaction
    // and borrowed by nothing else while libpam runs on this thread.
    let conversation = unsafe { &mut *appdata_ptr.cast::<Conversation<R, W>>() };
    // SAFETY: Linux-PAM passes `num_msg` message pointers in `msg`.
    let pointers = unsafe { std::slice::from_raw_parts(msg, count) };
    let mut messages = Vec::with_capacity(count);
    for &pointer in pointers {
        if pointer.is_null() {
            return PAM_CONV_ERR;
        }
        // SAFETY: each pointer names a pam_message libpam keeps alive for the call.
        let message = unsafe { &*pointer };
        if message.msg.is_null() {
            return PAM_CONV_ERR;
        }
        // SAFETY: a message's text is a NUL-terminated string libpam keeps alive for the call.
        let text = unsafe { CStr::from_ptr(message.msg) }.to_bytes();
        let kind = match message.msg_style {
            PAM_PROMPT_ECHO_OFF => Some(Kind::PromptSecret),
            PAM_PROMPT_ECHO_ON => Some(Kind::PromptVisible),
            PAM_ERROR_MSG => Some(Kind::Error),
            PAM_TEXT_INFO => Some(Kind::Info),
            _ => None,
        };
        messages.push(Message { kind, text });
    }
    match conversation.converse(&messages) {
        Ok(answers) => reply(&answers, resp),
        Err(ConvError::Failed) => PAM_CONV_ERR,
        Err(ConvError::Cancelled) => {
            tracing::info!(
                uid = conversation.uid,
                service = conversation.service.name(),
                verdict = "cancelled",
                "the client closed before answering"
            );
            exit_now(2)
        }
    }
}

/// Hands the answers to libpam: a `calloc`ed array of responses, each answer `malloc`ed with
/// its NUL, the one copy the PAM API forces. On a failed allocation what was allocated is
/// zeroed and freed.
fn reply(answers: &[Option<Secret>], resp: *mut *mut PamResponse) -> c_int {
    // SAFETY: calloc takes two sizes and returns zeroed memory or null.
    let array: *mut PamResponse =
        unsafe { libc::calloc(answers.len(), std::mem::size_of::<PamResponse>()) }.cast();
    if array.is_null() {
        return PAM_BUF_ERR;
    }
    // SAFETY: `array` holds `answers.len()` responses, zeroed, which is a valid response (a
    // null answer and a zero code), and nothing else refers to it.
    let responses = unsafe { std::slice::from_raw_parts_mut(array, answers.len()) };
    for (response, answer) in responses.iter_mut().zip(answers) {
        let Some(answer) = answer else { continue };
        let bytes = answer.as_bytes();
        // SAFETY: malloc takes a size and returns memory of that size or null.
        let copy: *mut u8 = unsafe { libc::malloc(bytes.len() + 1) }.cast();
        if copy.is_null() {
            drop_reply(array, answers.len());
            return PAM_BUF_ERR;
        }
        // SAFETY: `copy` holds `bytes.len() + 1` bytes, uninitialised bytes are written before
        // they are read, and nothing else refers to them.
        let buffer = unsafe { std::slice::from_raw_parts_mut(copy, bytes.len() + 1) };
        let (text, nul) = buffer.split_at_mut(bytes.len());
        text.copy_from_slice(bytes);
        nul[0] = 0;
        response.resp = copy.cast();
    }
    // SAFETY: `resp` is the non-null out pointer libpam passed; libpam frees the array.
    unsafe { *resp = array };
    PAM_SUCCESS
}

/// Zeroes and frees every answer in `array` of `len` responses, then the array.
fn drop_reply(array: *mut PamResponse, len: usize) {
    // SAFETY: `array` holds `len` responses, zeroed by calloc or filled by `reply`, and nothing
    // else refers to it.
    let responses = unsafe { std::slice::from_raw_parts_mut(array, len) };
    for response in responses.iter_mut() {
        let answer = response.resp;
        if answer.is_null() {
            continue;
        }
        // SAFETY: a non-null answer is a NUL-terminated string `reply` allocated.
        let len = unsafe { libc::strlen(answer) };
        // SAFETY: the answer holds `len` bytes before its NUL.
        unsafe { libc::explicit_bzero(answer.cast(), len) };
        // SAFETY: the answer came from malloc and is freed once, here.
        unsafe { libc::free(answer.cast()) };
        response.resp = ptr::null_mut();
    }
    // SAFETY: `array` came from calloc and is freed once, here; `responses` is not used after.
    unsafe { libc::free(array.cast()) };
}

/// The peer's pidfd, from `SO_PEERPIDFD` (Linux 6.5, older than every kernel Athanor ships).
/// There is no fallback: a pid from `SO_PEERCRED` can be reused before it is read.
pub fn so_peerpidfd(socket: BorrowedFd<'_>) -> io::Result<OwnedFd> {
    let mut fd: c_int = -1;
    let mut len = std::mem::size_of::<c_int>() as libc::socklen_t;
    // SAFETY: `fd` and `len` are live locals, and `len` is the size of `fd`: getsockopt writes
    // at most that many bytes into it.
    let done = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERPIDFD,
            ptr::from_mut(&mut fd).cast(),
            &mut len,
        )
    };
    if done != 0 {
        return Err(io::Error::last_os_error());
    }
    if fd < 0 {
        return Err(io::Error::other("SO_PEERPIDFD returned no descriptor"));
    }
    // SAFETY: on success the kernel installed a new pidfd at `fd` that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Takes the locale from the environment, so PAM's texts come in the system language (D32).
pub fn set_locale() -> io::Result<()> {
    // SAFETY: setlocale runs before any other thread exists, with a NUL-terminated string.
    let set = unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) };
    if set.is_null() {
        return Err(io::Error::other(
            "the environment names no locale the system has",
        ));
    }
    Ok(())
}

/// Ends the process at once, inside a PAM call, without the atexit handlers `exit` runs.
pub fn exit_now(code: c_int) -> ! {
    // SAFETY: _exit takes a status by value and does not return.
    unsafe { libc::_exit(code) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn the_service_names_are_the_selectors() {
        for service in [Service::Password, Service::Fingerprint] {
            assert_eq!(service_name(service).to_bytes(), service.name().as_bytes());
        }
    }

    #[test]
    fn only_every_step_succeeding_is_success() {
        let ok = Transaction {
            start: PAM_SUCCESS,
            authenticate: Some(PAM_SUCCESS),
            user_kept: true,
            account: Some(PAM_SUCCESS),
        };
        assert!(ok.succeeded());
        // PAM_AUTH_ERR, PAM_NEW_AUTHTOK_REQD (an expired password), PAM_ACCT_EXPIRED.
        for failed in [
            Transaction { start: 4, ..ok },
            Transaction {
                authenticate: Some(7),
                ..ok
            },
            Transaction {
                authenticate: None,
                ..ok
            },
            Transaction {
                user_kept: false,
                ..ok
            },
            Transaction {
                account: Some(12),
                ..ok
            },
            Transaction {
                account: Some(13),
                ..ok
            },
            Transaction {
                account: None,
                ..ok
            },
        ] {
            assert!(!failed.succeeded(), "{failed:?}");
        }
    }

    /// A directory of PAM service files, removed on drop.
    struct Confdir(std::path::PathBuf);

    impl Confdir {
        fn new(name: &str, password: &str, fingerprint: &str) -> Confdir {
            let dir =
                std::env::temp_dir().join(format!("athanor-unlockd-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("a temporary directory");
            std::fs::write(dir.join("athanor-lock"), password).expect("a service file");
            std::fs::write(dir.join("athanor-lock-fingerprint"), fingerprint)
                .expect("a service file");
            Confdir(dir)
        }

        fn path(&self) -> CString {
            CString::new(self.0.as_os_str().as_encoded_bytes()).expect("a path without NUL")
        }
    }

    impl Drop for Confdir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const PERMIT: &str = "auth required pam_permit.so\naccount required pam_permit.so\n";
    const DENY: &str = "auth required pam_deny.so\naccount required pam_permit.so\n";

    fn run(confdir: &Confdir, service: Service, input: &'static [u8]) -> (Transaction, Vec<u8>) {
        let mut conversation = Conversation::new(input, Vec::new(), service, 1000);
        let transaction = transact(c"athanor-test", Some(&confdir.path()), &mut conversation);
        (transaction, conversation.writer)
    }

    #[test]
    fn pam_permit_succeeds_and_pam_deny_fails() {
        let dir = Confdir::new("controls", PERMIT, DENY);
        let (permitted, _) = run(&dir, Service::Password, b"");
        assert!(permitted.succeeded(), "{permitted:?}");
        assert!(permitted.user_kept);
        let (denied, _) = run(&dir, Service::Fingerprint, b"");
        assert!(!denied.succeeded(), "{denied:?}");
        assert_ne!(denied.authenticate, Some(PAM_SUCCESS));
        assert_eq!(
            denied.account, None,
            "no account phase after a failed authentication"
        );
    }

    #[test]
    fn a_failed_account_phase_fails() {
        let stack = "auth required pam_permit.so\naccount required pam_deny.so\n";
        let dir = Confdir::new("account", stack, stack);
        let (transaction, _) = run(&dir, Service::Password, b"");
        assert_eq!(transaction.authenticate, Some(PAM_SUCCESS));
        assert_ne!(transaction.account, Some(PAM_SUCCESS));
        assert!(!transaction.succeeded());
    }

    #[test]
    fn an_uncounted_fingerprint_end_is_unavailable() {
        let fingerprint = "auth [success=done authinfo_unavail=die user_unknown=die default=bad] pam_debug.so auth=authinfo_unavail\naccount required pam_permit.so\n";
        let dir = Confdir::new("unavailable", PERMIT, fingerprint);
        let (transaction, _) = run(&dir, Service::Fingerprint, b"");
        assert_eq!(transaction.authenticate, Some(PAM_AUTHINFO_UNAVAIL));
        assert_eq!(
            transaction.outcome(Service::Fingerprint),
            Outcome::Unavailable
        );
    }

    #[test]
    fn an_unknown_user_on_the_password_stack_is_a_failure() {
        let stack =
            "auth required pam_debug.so auth=user_unknown\naccount required pam_permit.so\n";
        let dir = Confdir::new("unknown", stack, stack);
        let (password, _) = run(&dir, Service::Password, b"");
        assert_eq!(password.authenticate, Some(PAM_USER_UNKNOWN));
        assert_eq!(password.outcome(Service::Password), Outcome::Failure);
        let (fingerprint, _) = run(&dir, Service::Fingerprint, b"");
        assert_eq!(fingerprint.authenticate, Some(PAM_USER_UNKNOWN));
        assert_eq!(
            fingerprint.outcome(Service::Fingerprint),
            Outcome::Unavailable
        );
    }

    #[test]
    fn an_unavailable_account_phase_is_a_failure() {
        let stack =
            "auth required pam_permit.so\naccount required pam_debug.so acct=authinfo_unavail\n";
        let dir = Confdir::new("account-unavail", stack, stack);
        let (transaction, _) = run(&dir, Service::Fingerprint, b"");
        assert_eq!(transaction.account, Some(PAM_AUTHINFO_UNAVAIL));
        assert_eq!(transaction.outcome(Service::Fingerprint), Outcome::Failure);
    }

    #[test]
    fn a_finger_that_does_not_match_is_a_failure() {
        let fingerprint = "auth [success=done authinfo_unavail=die user_unknown=die default=bad] pam_debug.so auth=auth_err\naccount required pam_permit.so\n";
        let dir = Confdir::new("mismatch", PERMIT, fingerprint);
        let (transaction, _) = run(&dir, Service::Fingerprint, b"");
        assert_eq!(transaction.outcome(Service::Fingerprint), Outcome::Failure);
    }

    #[test]
    fn a_missing_service_file_fails() {
        let dir = Confdir::new("missing", PERMIT, PERMIT);
        std::fs::remove_file(dir.0.join("athanor-lock")).expect("removed");
        let (transaction, _) = run(&dir, Service::Password, b"");
        assert!(!transaction.succeeded(), "{transaction:?}");
    }

    #[test]
    fn a_module_message_reaches_the_client_escaped() {
        let stack = "auth required pam_echo.so caffè\nauth required pam_permit.so\naccount required pam_permit.so\n";
        let dir = Confdir::new("echo", stack, stack);
        let (transaction, sent) = run(&dir, Service::Password, b"");
        assert!(transaction.succeeded(), "{transaction:?}");
        assert_eq!(sent, b"PAM_TEXT_INFO caff\\303\\250\n");
    }

    /// pam_exec asks the password through the conversation and hands it to grep on stdin.
    const EXEC: &str = "auth required pam_exec.so expose_authtok quiet /usr/bin/grep -qxz hunter2\naccount required pam_permit.so\n";

    #[test]
    fn the_answer_reaches_the_module_and_decides() {
        let dir = Confdir::new("exec", EXEC, EXEC);
        let (right, sent) = run(&dir, Service::Password, b"hunter2\n");
        assert!(right.succeeded(), "{right:?}");
        assert!(sent.starts_with(b"PAM_PROMPT_ECHO_OFF "), "{sent:?}");
        let (wrong, _) = run(&dir, Service::Password, b"hunter3\n");
        assert!(!wrong.succeeded(), "{wrong:?}");
    }

    /// No module of the system's stacks honours the flag without an account with an empty
    /// password, which a test cannot create without root, so the calls are read from the source.
    #[test]
    fn both_phases_refuse_an_empty_password() {
        assert_eq!(FLAGS & PAM_DISALLOW_NULL_AUTHTOK, PAM_DISALLOW_NULL_AUTHTOK);
        let source = include_str!("ffi.rs");
        for call in ["pam_authenticate(", "pam_acct_mgmt("] {
            let site = format!("{{ {call}");
            assert_eq!(source.matches(&site).count(), 1, "one call of {call}");
            assert!(source.contains(&format!("{site}handle, FLAGS)")), "{call}");
        }
    }

    /// Set in the child that `a_client_gone_before_its_answer_ends_the_helper_with_2` runs.
    const CANCEL_CHILD: &str = "ATHANOR_UNLOCKD_TEST_CANCEL";

    /// On the password service, a client that closes before answering ends the process with
    /// `_exit(2)` from the conversation callback, inside pam_authenticate: no verdict, no count.
    #[test]
    fn a_client_gone_before_its_answer_ends_the_helper_with_2() {
        if std::env::var_os(CANCEL_CHILD).is_some() {
            let dir = Confdir::new("cancel", EXEC, EXEC);
            let (transaction, _) = run(&dir, Service::Password, b"");
            // Reached only if the callback did not end the process: the parent sees 0.
            println!("{transaction:?}");
            return;
        }
        let status = std::process::Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "ffi::tests::a_client_gone_before_its_answer_ends_the_helper_with_2",
            ])
            .env(CANCEL_CHILD, "1")
            .stdout(std::process::Stdio::null())
            .status()
            .expect("the child ran");
        assert_eq!(status.code(), Some(2), "{status}");
    }

    #[test]
    fn an_answer_too_long_is_a_conversation_error() {
        let dir = Confdir::new("long", EXEC, EXEC);
        let long: &'static [u8] =
            Box::leak(vec![b'a'; athanor_auth::MAX_LINE + 1].into_boxed_slice());
        let (transaction, _) = run(&dir, Service::Password, long);
        assert!(!transaction.succeeded(), "{transaction:?}");
    }
}
