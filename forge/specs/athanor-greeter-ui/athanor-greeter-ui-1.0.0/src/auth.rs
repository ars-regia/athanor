use greetd_ipc::{AuthMessageType, Request, Response};
use std::io::Read;
use std::io::Write;
use std::os::unix::net::UnixStream;
use zeroize::{Zeroize, Zeroizing};

use crate::i18n::{tr, tr_with};

/// The largest greetd reply this greeter will allocate for. A greetd frame is a short
/// JSON object -- a response type, an auth message, at worst an error description -- so
/// a megabyte is orders of magnitude more than any legitimate reply and still small
/// enough that a malformed length prefix cannot exhaust memory: with `panic = "abort"`
/// a failed allocation would kill the greeter, and greetd would restart it until its
/// start limit. The length is peer-controlled, and the peer is trusted only as far as
/// the socket bound into the sandbox from outside.
const MAX_REPLY_BYTES: u32 = 1024 * 1024;

/// Capacity of the buffer a request is serialised into, and so the largest request this
/// greeter sends. The longest is a `PostAuthMessageResponse` carrying a password; 4 KiB
/// covers any password a person types, and a longer one is refused, never reallocated.
const REQUEST_BUFFER_BYTES: usize = 4096;

/// A writer that appends to a buffer and refuses to grow it. A growing Vec reallocates,
/// and each buffer it abandons on the way is freed without being erased: with this
/// writer the only buffer a request ever occupies is the one reserved up front, which
/// its `Zeroizing` wrapper erases.
struct FixedBuffer<'a>(&'a mut Vec<u8>);

impl Write for FixedBuffer<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.len() > self.0.capacity() - self.0.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::OutOfMemory,
                format!("request larger than {REQUEST_BUFFER_BYTES} bytes"),
            ));
        }
        // Within capacity: extend_from_slice does not reallocate.
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn send_request(stream: &mut UnixStream, req: &Request) -> Result<Response, String> {
    // The frame of a PostAuthMessageResponse carries the password in cleartext: erase
    // the serialised copy when it goes out of scope rather than leaving it in freed heap.
    // FixedBuffer keeps it in the one buffer reserved here, so no copy escapes the erasure.
    let mut json = Zeroizing::new(Vec::with_capacity(REQUEST_BUFFER_BYTES));
    serde_json::to_writer(FixedBuffer(&mut json), req).map_err(|e| e.to_string())?;
    let len = (json.len() as u32).to_ne_bytes();
    stream.write_all(&len).map_err(|e| e.to_string())?;
    stream.write_all(&json).map_err(|e| e.to_string())?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).map_err(|e| e.to_string())?;
    let reply_len = u32::from_ne_bytes(len_buf);
    if reply_len > MAX_REPLY_BYTES {
        return Err(tr_with(
            "The sign-in service sent a reply of {size} bytes, too large to read",
            "size",
            &reply_len.to_string(),
        ));
    }

    let mut reply_buf = vec![0u8; reply_len as usize];
    stream
        .read_exact(&mut reply_buf)
        .map_err(|e| e.to_string())?;

    serde_json::from_slice(&reply_buf).map_err(|e| e.to_string())
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[derive(Debug, Clone)]
pub struct UserInfo {
    pub username: String,
    pub real_name: String,
    pub avatar_path: Option<String>,
}

/// The uid below which an account is a system account, not a person: greetd runs the
/// greeter as its own daemon user, and whatever that user is called it is never the one
/// who is about to log in.
const FIRST_HUMAN_UID: u32 = 1000;

/// Whether `name` is a system account in /etc/passwd (uid below FIRST_HUMAN_UID). An
/// unknown name counts as a system account: there is no person to greet by that name.
fn is_system_account(name: &str) -> bool {
    let Ok(content) = std::fs::read_to_string("/etc/passwd") else {
        return true;
    };
    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.split(':');
            let user = parts.next()?;
            let uid = parts.nth(1)?.parse::<u32>().ok()?;
            (user == name).then_some(uid)
        })
        .next()
        .is_none_or(|uid| uid < FIRST_HUMAN_UID)
}

pub fn discover_target_user() -> UserInfo {
    let env_user = std::env::var("USER").unwrap_or_default();
    // Running as the greeter (greetd's daemon user, whatever its name) or with no user at
    // all: find the person to greet instead of showing the daemon on the card.
    let target_user = if env_user.is_empty() || is_system_account(&env_user) {
        std::env::var("ATHANOR_LOGIN_USER").unwrap_or_else(|_| {
            if let Ok(content) = std::fs::read_to_string("/etc/passwd") {
                for line in content.lines() {
                    let parts: Vec<&str> = line.split(':').collect();
                    if parts.len() >= 7 {
                        if let Ok(uid) = parts[2].parse::<u32>() {
                            if (FIRST_HUMAN_UID..65534).contains(&uid)
                                && (parts[6].ends_with("bash")
                                    || parts[6].ends_with("zsh")
                                    || parts[6].ends_with("fish"))
                            {
                                return parts[0].to_string();
                            }
                        }
                    }
                }
            }
            "athanor".to_string()
        })
    } else {
        env_user
    };

    let mut real_name = capitalize_first(&target_user);
    let mut home_dir = format!("/home/{}", target_user);
    if let Ok(content) = std::fs::read_to_string("/etc/passwd") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 6 && parts[0] == target_user {
                let gecos = parts[4].split(',').next().unwrap_or(parts[0]);
                if !gecos.trim().is_empty() {
                    real_name = gecos.trim().to_string();
                }
                home_dir = parts[5].to_string();
                break;
            }
        }
    }

    let face_path = format!("{}/.face", home_dir);
    let acc_path = format!("/var/lib/AccountsService/icons/{}", target_user);
    let avatar_path = if std::path::Path::new(&face_path).exists() {
        Some(face_path)
    } else if std::path::Path::new(&acc_path).exists() {
        Some(acc_path)
    } else {
        None
    };

    UserInfo {
        username: target_user,
        real_name,
        avatar_path,
    }
}

/// The session type the greeter asks greetd for. The badge on the greeter card reads it
/// from here, so what the card says and what the greeter requests cannot drift apart.
pub const SESSION_TYPE: &str = "wayland";

/// Where the session command is looked for, most specific first.
const SESSION_COMMAND_PATHS: [&str; 3] = [
    "/usr/bin/athanor-session",
    "/etc/greetd/athanor-session",
    "/usr/local/bin/athanor-session",
];

/// What to ask greetd for when none of those paths is installed: the bare name, left to
/// greetd's own PATH.
const SESSION_COMMAND_FALLBACK: &str = "athanor-session";

/// The first of `candidates` that is installed, or `fallback`. Split out of
/// session_command so that the choice can be tested against a tree the test owns: on a
/// build machine none of the real paths exists, and a test that only ever sees the
/// fallback would pass on a function that never looked at the filesystem.
fn first_installed<'a>(candidates: &[&'a str], fallback: &'a str) -> &'a str {
    candidates
        .iter()
        .copied()
        .find(|candidate| std::path::Path::new(candidate).exists())
        .unwrap_or(fallback)
}

/// The command the greeter asks greetd to run once the password is accepted: the first
/// of SESSION_COMMAND_PATHS that is installed, or SESSION_COMMAND_FALLBACK.
pub fn session_command() -> String {
    first_installed(&SESSION_COMMAND_PATHS, SESSION_COMMAND_FALLBACK).to_string()
}

/// A question PAM puts, through greetd, to the person at the greeter, in PAM's own words.
#[derive(Debug, PartialEq, Eq)]
pub enum Prompt {
    /// An answer hidden as it is typed (PAM_PROMPT_ECHO_OFF): a password, a new password.
    Secret(String),
    /// An answer shown as it is typed (PAM_PROMPT_ECHO_ON): a one-time code, a name.
    Visible(String),
}

/// The person's side of one greetd conversation. The greeter's window plays it; the tests
/// play it from a script.
pub trait Conversation {
    /// Shows a line of progress or PAM's PAM_TEXT_INFO.
    fn status(&self, text: &str);
    /// Shows PAM's PAM_ERROR_MSG as an error.
    fn error(&self, text: &str);
    /// Puts `prompt` to the person and waits for the answer; None when none will come.
    fn ask(&self, prompt: Prompt) -> impl std::future::Future<Output = Option<Zeroizing<String>>>;
}

/// The most messages one login answers before the greeter gives up on it.
const MAX_AUTH_STEPS: usize = 15;

/// Answers greetd's pending message with `answer`. The copy greetd_ipc owns is erased as
/// soon as the frame is on the socket, before the result is propagated, so that no path
/// out leaves the answer in freed heap.
fn post_answer(stream: &mut UnixStream, answer: &str) -> Result<Response, String> {
    let mut req = Request::PostAuthMessageResponse {
        response: Some(answer.to_string()),
    };
    let sent = send_request(stream, &req);
    if let Request::PostAuthMessageResponse {
        response: Some(ref mut secret),
    } = req
    {
        secret.zeroize();
    }
    sent
}

/// Answers an informational or error message: greetd reads no text for them.
fn post_no_answer(stream: &mut UnixStream) -> Result<Response, String> {
    send_request(stream, &Request::PostAuthMessageResponse { response: None })
}

/// Puts `prompt` to the person and answers greetd with their reply. With no reply the
/// session is cancelled, so that greetd is never left waiting on this connection.
async fn ask_and_answer<C: Conversation>(
    stream: &mut UnixStream,
    person: &C,
    prompt: Prompt,
) -> Result<Response, String> {
    match person.ask(prompt).await {
        Some(reply) => post_answer(stream, &reply),
        None => {
            send_request(stream, &Request::CancelSession)?;
            Err(tr("Sign-in was cancelled."))
        }
    }
}

/// One login over an open greetd connection: creates the session for `username`, answers
/// PAM's messages by their type and never by their text, and starts `session_cmd` once
/// greetd accepts.
async fn converse<C: Conversation>(
    stream: &mut UnixStream,
    username: &str,
    password: &str,
    session_cmd: String,
    person: &C,
) -> Result<(), String> {
    let mut resp = send_request(
        stream,
        &Request::CreateSession {
            username: username.to_string(),
        },
    )?;
    // The password typed on the card answers PAM's first secret question and no other: a
    // further one (a new password, its confirmation) is put to the person in PAM's words.
    let mut password = Some(password);
    // PAM's error messages, kept to say why an attempt ended: a locked account or an expired
    // password is told in PAM's words, not guessed from greetd's code.
    let mut errors: Vec<String> = Vec::new();
    for _ in 0..MAX_AUTH_STEPS {
        resp = match resp {
            Response::AuthMessage {
                auth_message_type,
                auth_message,
            } => match auth_message_type {
                AuthMessageType::Info => {
                    person.status(&auth_message);
                    post_no_answer(stream)?
                }
                AuthMessageType::Error => {
                    person.error(&auth_message);
                    errors.push(auth_message);
                    post_no_answer(stream)?
                }
                AuthMessageType::Secret => match password.take() {
                    Some(password) => {
                        person.status(&tr("Checking your credentials…"));
                        post_answer(stream, password)?
                    }
                    None => ask_and_answer(stream, person, Prompt::Secret(auth_message)).await?,
                },
                AuthMessageType::Visible => {
                    ask_and_answer(stream, person, Prompt::Visible(auth_message)).await?
                }
            },
            Response::Success => {
                // The password goes to greetd and nowhere else: PAM, run by greetd for the
                // session it is about to start, is what unlocks the keyring.
                let req = Request::StartSession {
                    cmd: vec![session_cmd],
                    env: vec![
                        format!("XDG_SESSION_TYPE={}", SESSION_TYPE),
                        "XDG_CURRENT_DESKTOP=Athanor".to_string(),
                    ],
                };
                return match send_request(stream, &req)? {
                    Response::Success => Ok(()),
                    Response::Error { description, .. } => Err(description),
                    Response::AuthMessage { .. } => {
                        Err(tr("The login service gave an unexpected answer."))
                    }
                };
            }
            Response::Error { description, .. } => {
                errors.push(description);
                return Err(errors.join("\n"));
            }
        };
    }
    Err(tr("Too many authentication steps."))
}

pub async fn authenticate_interactive<C: Conversation>(
    password: &str,
    person: &C,
) -> Result<(), String> {
    let path = std::env::var("GREETD_SOCK").unwrap_or_else(|_| "/run/greetd.sock".to_string());
    if !std::path::Path::new(&path).exists() {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        return Err(tr("The login service cannot be reached."));
    }

    let mut stream = UnixStream::connect(path).map_err(|e| e.to_string())?;
    let username = discover_target_user().username;
    converse(&mut stream, &username, password, session_command(), person).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_longer_than_the_bound_is_refused_before_it_is_allocated() {
        let (mut ours, theirs) = UnixStream::pair().expect("a socket pair");
        // A peer announcing one byte more than the bound, and nothing after it. Its
        // write half is then closed so that a greeter without the bound fails at once
        // with UnexpectedEof instead of blocking: the assertion below is pinned to the
        // bound's own message, so that error cannot pass for a refusal.
        (&theirs)
            .write_all(&(MAX_REPLY_BYTES + 1).to_ne_bytes())
            .expect("the length prefix");
        theirs
            .shutdown(std::net::Shutdown::Write)
            .expect("the peer's write half closes");

        let err = send_request(
            &mut ours,
            &Request::CreateSession {
                username: "tester".to_string(),
            },
        )
        .expect_err("an oversized reply must be refused");
        assert!(err.contains("too large to read"), "{err}");
    }

    #[test]
    fn a_request_longer_than_the_buffer_is_refused_before_anything_is_sent() {
        let (mut ours, theirs) = UnixStream::pair().expect("a socket pair");
        let err = send_request(
            &mut ours,
            &Request::PostAuthMessageResponse {
                response: Some("x".repeat(REQUEST_BUFFER_BYTES)),
            },
        )
        .expect_err("an oversized request must be refused");
        assert!(err.contains("request larger than"), "{err}");

        drop(ours);
        let mut sent = Vec::new();
        (&theirs)
            .read_to_end(&mut sent)
            .expect("the peer reads to EOF");
        assert!(sent.is_empty(), "{} bytes reached the socket", sent.len());
    }

    /// A directory of this test's own, named after the case, removed at the end.
    fn scratch(case: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "athanor-session-command-{}-{}",
            std::process::id(),
            case
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn session_command_takes_the_first_candidate_that_is_installed() {
        let dir = scratch("first");
        let first = dir.join("first").to_str().expect("utf-8 path").to_string();
        let second = dir.join("second").to_str().expect("utf-8 path").to_string();
        std::fs::write(&first, b"").expect("the first candidate");
        std::fs::write(&second, b"").expect("the second candidate");

        assert_eq!(
            first_installed(&[&first, &second], "fallback"),
            first,
            "with both installed the greeter must ask for the first"
        );

        std::fs::remove_file(&first).expect("removing the first candidate");
        assert_eq!(
            first_installed(&[&first, &second], "fallback"),
            second,
            "with the first missing the greeter must fall through to the second"
        );

        std::fs::remove_dir_all(&dir).expect("cleaning up");
    }

    #[test]
    fn session_command_falls_back_to_the_bare_name_when_nothing_is_installed() {
        let dir = scratch("none");
        let missing = dir
            .join("missing")
            .to_str()
            .expect("utf-8 path")
            .to_string();
        assert!(!std::path::Path::new(&missing).exists());

        assert_eq!(
            first_installed(&[&missing], SESSION_COMMAND_FALLBACK),
            SESSION_COMMAND_FALLBACK
        );
        assert!(
            !SESSION_COMMAND_FALLBACK.contains('/'),
            "the fallback is a bare command name, left to greetd's PATH"
        );

        std::fs::remove_dir_all(&dir).expect("cleaning up");
    }

    /// The password typed on the card in the conversation tests.
    const PASSWORD: &str = "correct horse battery staple";

    /// Reads one length-prefixed greetd frame as JSON.
    fn read_frame(stream: &mut UnixStream) -> std::io::Result<serde_json::Value> {
        let mut len = [0u8; 4];
        stream.read_exact(&mut len)?;
        let mut frame = vec![0u8; u32::from_ne_bytes(len) as usize];
        stream.read_exact(&mut frame)?;
        serde_json::from_slice(&frame).map_err(std::io::Error::other)
    }

    /// A greetd on the far end of a Unix socket pair. For each reply of `script` it reads
    /// one request, keeps it, and writes the reply. Then it closes its write half, so that a
    /// greeter waiting on a reply the script does not hold fails instead of hanging, and
    /// reads whatever else the greeter sends until the greeter closes its end. Its thread
    /// returns every request, in order.
    fn fake_greetd(
        script: Vec<Response>,
    ) -> (UnixStream, std::thread::JoinHandle<Vec<serde_json::Value>>) {
        let (greeter, mut greetd) = UnixStream::pair().expect("a socket pair");
        let thread = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in script {
                requests.push(read_frame(&mut greetd).expect("a request from the greeter"));
                let frame = serde_json::to_vec(&reply).expect("a reply frame");
                greetd
                    .write_all(&(frame.len() as u32).to_ne_bytes())
                    .expect("the reply length");
                greetd.write_all(&frame).expect("the reply");
            }
            greetd
                .shutdown(std::net::Shutdown::Write)
                .expect("the fake greetd's write half closes");
            while let Ok(extra) = read_frame(&mut greetd) {
                requests.push(extra);
            }
            requests
        });
        (greeter, thread)
    }

    /// The person at the greeter, answering from a script and noting what was shown.
    #[derive(Default)]
    struct Person {
        answers: std::cell::RefCell<std::collections::VecDeque<&'static str>>,
        asked: std::cell::RefCell<Vec<Prompt>>,
        statuses: std::cell::RefCell<Vec<String>>,
        errors: std::cell::RefCell<Vec<String>>,
    }

    impl Person {
        fn answering(answers: &[&'static str]) -> Self {
            Person {
                answers: std::cell::RefCell::new(answers.iter().copied().collect()),
                ..Person::default()
            }
        }
    }

    impl Conversation for Person {
        fn status(&self, text: &str) {
            self.statuses.borrow_mut().push(text.to_string());
        }
        fn error(&self, text: &str) {
            self.errors.borrow_mut().push(text.to_string());
        }
        fn ask(
            &self,
            prompt: Prompt,
        ) -> impl std::future::Future<Output = Option<Zeroizing<String>>> {
            self.asked.borrow_mut().push(prompt);
            let answer = self.answers.borrow_mut().pop_front();
            std::future::ready(answer.map(|answer| Zeroizing::new(answer.to_string())))
        }
    }

    fn message(kind: AuthMessageType, text: &str) -> Response {
        Response::AuthMessage {
            auth_message_type: kind,
            auth_message: text.to_string(),
        }
    }

    fn auth_error(description: &str) -> Response {
        Response::Error {
            error_type: greetd_ipc::ErrorType::AuthError,
            description: description.to_string(),
        }
    }

    /// Plays one login against the fake greetd: the result, and every request it received.
    fn login(
        script: Vec<Response>,
        person: &Person,
    ) -> (Result<(), String>, Vec<serde_json::Value>) {
        let (mut greeter, greetd) = fake_greetd(script);
        let result = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime")
            .block_on(converse(
                &mut greeter,
                "tester",
                PASSWORD,
                "athanor-session".to_string(),
                person,
            ));
        drop(greeter);
        (result, greetd.join().expect("the fake greetd"))
    }

    /// The `response` of every PostAuthMessageResponse, in order.
    fn answers(requests: &[serde_json::Value]) -> Vec<Option<String>> {
        requests
            .iter()
            .filter(|request| request["type"] == "post_auth_message_response")
            .map(|request| request["response"].as_str().map(str::to_string))
            .collect()
    }

    /// How many requests carry the password anywhere.
    fn password_sent(requests: &[serde_json::Value]) -> usize {
        requests
            .iter()
            .filter(|request| request.to_string().contains(PASSWORD))
            .count()
    }

    #[test]
    fn info_and_error_messages_are_shown_and_answered_with_nothing() {
        let person = Person::default();
        let (result, requests) = login(
            vec![
                message(AuthMessageType::Info, "Last login: yesterday"),
                message(AuthMessageType::Error, "There were 2 failed login attempts"),
                message(AuthMessageType::Secret, "Password: "),
                Response::Success,
                Response::Success,
            ],
            &person,
        );
        assert_eq!(result, Ok(()));
        assert_eq!(requests[0]["type"], "create_session");
        assert_eq!(requests[0]["username"], "tester");
        assert_eq!(
            answers(&requests),
            [None, None, Some(PASSWORD.to_string())],
            "Info and Error get no answer; only the password prompt gets the password"
        );
        assert_eq!(requests[4]["type"], "start_session");
        assert_eq!(requests[4]["cmd"][0], "athanor-session");
        assert_eq!(person.statuses.borrow()[0], "Last login: yesterday");
        assert_eq!(
            *person.errors.borrow(),
            ["There were 2 failed login attempts"]
        );
        assert!(person.asked.borrow().is_empty(), "nothing is asked twice");
    }

    #[test]
    fn a_visible_question_is_answered_from_the_person_never_with_the_password() {
        let person = Person::answering(&["493021"]);
        let (result, requests) = login(
            vec![
                message(AuthMessageType::Secret, "Password: "),
                message(AuthMessageType::Visible, "Verification code: "),
                Response::Success,
                Response::Success,
            ],
            &person,
        );
        assert_eq!(result, Ok(()));
        assert_eq!(
            answers(&requests),
            [Some(PASSWORD.to_string()), Some("493021".to_string())]
        );
        assert_eq!(
            *person.asked.borrow(),
            [Prompt::Visible("Verification code: ".to_string())]
        );
        assert_eq!(password_sent(&requests), 1);
    }

    #[test]
    fn a_second_secret_question_is_asked_of_the_person_never_filled_with_the_password() {
        // A PAM stack that changes an expired password in the conversation asks the old
        // password, then the new one twice.
        let person = Person::answering(&["a new passphrase", "a new passphrase"]);
        let (result, requests) = login(
            vec![
                message(AuthMessageType::Secret, "Password: "),
                message(AuthMessageType::Secret, "New password: "),
                message(AuthMessageType::Secret, "Retype new password: "),
                Response::Success,
                Response::Success,
            ],
            &person,
        );
        assert_eq!(result, Ok(()));
        assert_eq!(
            answers(&requests),
            [
                Some(PASSWORD.to_string()),
                Some("a new passphrase".to_string()),
                Some("a new passphrase".to_string()),
            ]
        );
        assert_eq!(
            *person.asked.borrow(),
            [
                Prompt::Secret("New password: ".to_string()),
                Prompt::Secret("Retype new password: ".to_string()),
            ],
            "each further question is put in PAM's own words"
        );
        assert_eq!(password_sent(&requests), 1);
    }

    #[test]
    fn a_locked_account_is_reported_in_pams_words_and_gets_no_password() {
        // pam_faillock's preauth, without `silent`, says so and stops before the password.
        let locked = "The account is locked due to 5 failed logins. (10 minutes left to unlock)";
        let person = Person::default();
        let (result, requests) = login(
            vec![
                message(AuthMessageType::Error, locked),
                auth_error("authentication error: pam_authenticate: AUTH_ERR"),
            ],
            &person,
        );
        let reason = result.expect_err("a locked account does not sign in");
        assert!(reason.contains(locked), "{reason}");
        assert!(reason.contains("AUTH_ERR"), "{reason}");
        assert_eq!(*person.errors.borrow(), [locked]);
        assert_eq!(answers(&requests), [None]);
        assert_eq!(password_sent(&requests), 0);
    }

    #[test]
    fn an_expired_password_is_reported_in_pams_words() {
        // greetd 0.10.3 runs pam_acct_mgmt and no pam_chauthtok: pam_unix says the password
        // has expired, and greetd ends the attempt with NEW_AUTHTOK_REQD.
        let expired = "You are required to change your password immediately (password expired)";
        let person = Person::default();
        let (result, requests) = login(
            vec![
                message(AuthMessageType::Secret, "Password: "),
                message(AuthMessageType::Error, expired),
                auth_error("authentication error: pam_acct_mgmt: NEW_AUTHTOK_REQD"),
            ],
            &person,
        );
        let reason = result.expect_err("an expired password does not sign in");
        assert!(reason.contains(expired), "{reason}");
        assert!(reason.contains("NEW_AUTHTOK_REQD"), "{reason}");
        assert_eq!(answers(&requests), [Some(PASSWORD.to_string()), None]);
        assert_eq!(password_sent(&requests), 1);
    }

    #[test]
    fn a_question_left_unanswered_cancels_the_session() {
        let person = Person::default();
        let (result, requests) = login(
            vec![
                message(AuthMessageType::Visible, "Verification code: "),
                Response::Success,
            ],
            &person,
        );
        assert!(result.is_err());
        assert_eq!(requests[1]["type"], "cancel_session");
        assert_eq!(password_sent(&requests), 0);
    }

    #[test]
    fn the_real_candidates_are_absolute_and_end_in_the_session_command() {
        for candidate in SESSION_COMMAND_PATHS {
            assert!(
                candidate.starts_with('/'),
                "an installed session command is an absolute path: {candidate}"
            );
            assert!(candidate.ends_with(SESSION_COMMAND_FALLBACK), "{candidate}");
        }
        assert_eq!(
            session_command(),
            first_installed(&SESSION_COMMAND_PATHS, SESSION_COMMAND_FALLBACK),
            "session_command must be first_installed over the real candidates"
        );
    }
}
