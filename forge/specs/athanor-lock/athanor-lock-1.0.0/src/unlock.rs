//! The client of `athanor-unlockd` (doc_lock_and_prompts.md, LP8, D28), and the lock's event
//! source.
//!
//! Each attempt is one connection to `/run/athanor/unlockd.socket`, whose first line selects the
//! password service. The field's text answers the helper's first secret prompt: the caller
//! copies it into a [`Secret`], which is written and zeroised. The socket is non-blocking and
//! read on the main thread, so a slow PAM stack never holds the screen. Any other prompt, a
//! malformed or over-long line, a write that would block and an end of stream before a verdict
//! end the attempt as [`Outcome::EndOfStream`], which the lock answers with "try again" (LP10):
//! only an exact `SUCCESS` unlocks.
//!
//! The event source wakes the main loop for two things: the `calloop::ping` that the logind
//! thread fires after each report, and the current attempt's socket, a `Generic` in level mode.
//! A new attempt hands a clone of its socket to [`Watch::socket`], and an ended one calls
//! [`Watch::release`]; either fires the ping, and the source swaps its socket in `reregister`.
//! Between two attempts it watches no socket, so a closed one never wakes the loop.

use std::cell::RefCell;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::rc::Rc;

use athanor_auth::{parse, write_answer, write_selector, Line, LineReader, Secret, Service};
use calloop::generic::Generic;
use calloop::ping::{make_ping, Ping, PingSource};
use calloop::{EventSource, Interest, Mode, Poll, PostAction, Readiness, Token, TokenFactory};

use crate::state::Outcome;

/// The helper's socket (LP8).
pub const SOCKET: &str = "/run/athanor/unlockd.socket";

/// How an attempt ended, with PAM's messages on the way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ended {
    pub attempt: u32,
    pub outcome: Outcome,
    /// PAM_ERROR_MSG texts in order: a refusal's text (LP6, `athanor_auth::refusal`).
    pub errors: Vec<String>,
    /// PAM_TEXT_INFO texts in order.
    pub info: Vec<String>,
}

/// One password transaction with the helper.
pub struct Attempt {
    attempt: u32,
    stream: UnixStream,
    reader: LineReader,
    answered: bool,
    errors: Vec<String>,
    info: Vec<String>,
}

impl Attempt {
    /// Connects to the helper's socket at `path`, [`SOCKET`] outside the tests.
    pub fn connect(path: &Path, attempt: u32) -> io::Result<Attempt> {
        Attempt::over(UnixStream::connect(path)?, attempt)
    }

    /// Starts the attempt numbered `attempt` on `stream`, connected to the helper: writes the
    /// selector, then turns the socket non-blocking.
    pub fn over(mut stream: UnixStream, attempt: u32) -> io::Result<Attempt> {
        // Still blocking: the selector's bytes go whole into a socket that holds nothing yet.
        write_selector(&mut stream, Service::Password)?;
        stream.set_nonblocking(true)?;
        Ok(Attempt {
            attempt,
            stream,
            reader: LineReader::new(),
            answered: false,
            errors: Vec::new(),
            info: Vec::new(),
        })
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// A second handle on the socket, for [`Watch::socket`].
    pub fn socket(&self) -> io::Result<UnixStream> {
        self.stream.try_clone()
    }

    /// Reads every line the socket holds. At the first secret prompt, `answer` fills the
    /// `Secret` it is given with the field's text; nothing else calls it. `Some` is the end of
    /// the attempt, after which the caller drops it; `None` is a read that would block.
    pub fn step(&mut self, mut answer: impl FnMut(&mut Secret)) -> Option<Ended> {
        loop {
            let line = match self.reader.read(&mut self.stream) {
                Ok(Some(line)) => parse(line),
                Ok(None) => return None,
                Err(_) => return Some(self.end(Outcome::EndOfStream)),
            };
            let outcome = match line {
                Ok(Line::Success) => Outcome::Success,
                Ok(Line::Failure) => Outcome::Failure,
                Ok(Line::Unavailable) => Outcome::Unavailable,
                Ok(Line::Error(text)) => {
                    self.errors.push(text);
                    continue;
                }
                Ok(Line::Info(text)) => {
                    self.info.push(text);
                    continue;
                }
                Ok(Line::PromptSecret(_)) if !self.answered => {
                    self.answered = true;
                    let mut reply = Secret::new();
                    answer(&mut reply);
                    // A write that would block fails like any other: the socket held nothing
                    // when the helper asked, so it does not happen short of a broken peer.
                    match write_answer(&mut self.stream, &reply) {
                        Ok(()) => continue,
                        Err(_) => Outcome::EndOfStream,
                    }
                    // `reply` is zeroised as it drops.
                }
                // The password stack asks once: a second prompt, a visible one or a malformed
                // line is not the helper this lock knows, and gets no answer.
                Ok(Line::PromptSecret(_) | Line::PromptVisible(_)) | Err(_) => Outcome::EndOfStream,
            };
            return Some(self.end(outcome));
        }
    }

    fn end(&mut self, outcome: Outcome) -> Ended {
        Ended {
            attempt: self.attempt,
            outcome,
            errors: std::mem::take(&mut self.errors),
            info: std::mem::take(&mut self.info),
        }
    }
}

/// What the source watches after its next ping.
enum Next {
    Keep,
    Socket(Option<UnixStream>),
}

/// The main loop's handle on the socket the source watches.
#[derive(Clone)]
pub struct Watch {
    next: Rc<RefCell<Next>>,
    ping: Ping,
}

impl Watch {
    /// Watches `socket`, a new attempt's, in place of any socket before it.
    pub fn socket(&self, socket: UnixStream) {
        self.set(Some(socket));
    }

    /// Watches no socket: the attempt ended.
    pub fn release(&self) {
        self.set(None);
    }

    fn set(&self, socket: Option<UnixStream>) {
        *self.next.borrow_mut() = Next::Socket(socket);
        self.ping.ping();
    }
}

/// The lock's one event source, the one `sal::run_with` takes. It calls its callback once per
/// wake-up; the work is the application's, in `Application::turn`.
pub struct Source {
    ping: PingSource,
    socket: Option<Generic<UnixStream>>,
    next: Rc<RefCell<Next>>,
}

impl Source {
    /// The source, the ping the logind thread fires (`logind::spawn`), and the handle that
    /// hands the source each attempt's socket.
    pub fn new() -> io::Result<(Source, Ping, Watch)> {
        let (ping, source) = make_ping()?;
        let next = Rc::new(RefCell::new(Next::Keep));
        let watch = Watch {
            next: Rc::clone(&next),
            ping: ping.clone(),
        };
        let source = Source {
            ping: source,
            socket: None,
            next,
        };
        Ok((source, ping, watch))
    }
}

impl EventSource for Source {
    type Event = ();
    type Metadata = ();
    type Ret = ();
    type Error = io::Error;

    fn process_events<F>(
        &mut self,
        readiness: Readiness,
        token: Token,
        mut callback: F,
    ) -> io::Result<PostAction>
    where
        F: FnMut((), &mut ()),
    {
        let mut woken = false;
        self.ping
            .process_events(readiness, token, |(), ()| woken = true)
            .map_err(io::Error::other)?;
        if let Some(socket) = &mut self.socket {
            socket.process_events(readiness, token, |_, _| {
                woken = true;
                Ok(PostAction::Continue)
            })?;
        }
        if woken {
            callback((), &mut ());
        }
        Ok(match *self.next.borrow() {
            Next::Keep => PostAction::Continue,
            Next::Socket(_) => PostAction::Reregister,
        })
    }

    fn register(&mut self, poll: &mut Poll, factory: &mut TokenFactory) -> calloop::Result<()> {
        self.ping.register(poll, factory)?;
        if let Some(socket) = &mut self.socket {
            socket.register(poll, factory)?;
        }
        Ok(())
    }

    fn reregister(&mut self, poll: &mut Poll, factory: &mut TokenFactory) -> calloop::Result<()> {
        self.ping.reregister(poll, factory)?;
        let next = std::mem::replace(&mut *self.next.borrow_mut(), Next::Keep);
        match next {
            Next::Keep => {
                if let Some(socket) = &mut self.socket {
                    socket.reregister(poll, factory)?;
                }
            }
            Next::Socket(socket) => {
                if let Some(mut old) = self.socket.take() {
                    old.unregister(poll)?;
                }
                if let Some(socket) = socket {
                    let mut socket = Generic::new(socket, Interest::READ, Mode::Level);
                    socket.register(poll, factory)?;
                    self.socket = Some(socket);
                }
            }
        }
        Ok(())
    }

    fn unregister(&mut self, poll: &mut Poll) -> calloop::Result<()> {
        self.ping.unregister(poll)?;
        if let Some(socket) = &mut self.socket {
            socket.unregister(poll)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::time::Duration;

    use calloop::EventLoop;

    use super::*;

    const PROMPT: &[u8] = b"PAM_PROMPT_ECHO_OFF Password: \n";

    /// An attempt on one end of a socket pair; the other end plays the helper.
    fn attempt() -> (Attempt, UnixStream) {
        let (client, helper) = UnixStream::pair().expect("socket pair");
        (Attempt::over(client, 1).expect("attempt"), helper)
    }

    fn password(secret: &mut Secret) {
        for &b in b"hunter2" {
            secret.try_push(b).expect("within the bound");
        }
    }

    /// Steps until the attempt ends, with the number of times it asked for the field.
    fn run(attempt: &mut Attempt) -> (Ended, u32) {
        let mut asked = 0;
        let ended = attempt.step(|secret| {
            asked += 1;
            password(secret);
        });
        (ended.expect("the attempt ended"), asked)
    }

    fn read_all(mut helper: UnixStream) -> Vec<u8> {
        helper
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown");
        let mut sent = Vec::new();
        helper
            .read_to_end(&mut sent)
            .expect("what the client wrote");
        sent
    }

    #[test]
    fn the_password_answers_the_prompt() {
        let (mut attempt, mut helper) = attempt();
        helper.write_all(PROMPT).expect("prompt");
        assert_eq!(attempt.step(password), None);
        let mut lines = BufReader::new(helper.try_clone().expect("clone"));
        for want in ["athanor-lock\n", "hunter2\n"] {
            let mut line = String::new();
            lines.read_line(&mut line).expect("a line");
            assert_eq!(line, want);
        }
        helper
            .write_all(b"PAM_TEXT_INFO Welcome\nSUCCESS\n")
            .expect("verdict");
        let (ended, asked) = run(&mut attempt);
        assert_eq!(asked, 0, "the prompt was answered at the first step");
        assert_eq!(
            ended,
            Ended {
                attempt: 1,
                outcome: Outcome::Success,
                errors: vec![],
                info: vec!["Welcome".into()],
            }
        );
    }

    #[test]
    fn failure_ends_the_attempt() {
        let (mut attempt, mut helper) = attempt();
        helper.write_all(PROMPT).expect("prompt");
        helper
            .write_all(b"PAM_ERROR_MSG The account is locked\nFAILURE\n")
            .expect("verdict");
        let (ended, asked) = run(&mut attempt);
        assert_eq!(asked, 1);
        assert_eq!(ended.outcome, Outcome::Failure);
        assert_eq!(ended.errors, ["The account is locked"]);
    }

    #[test]
    fn an_end_of_stream_ends_the_attempt() {
        let (mut attempt, mut helper) = attempt();
        helper.write_all(b"PAM_TEXT_INFO half a li").expect("text");
        drop(helper);
        let (ended, asked) = run(&mut attempt);
        assert_eq!((ended.outcome, asked), (Outcome::EndOfStream, 0));
    }

    #[test]
    fn an_over_long_line_ends_the_attempt() {
        let (mut attempt, mut helper) = attempt();
        let mut line = b"PAM_TEXT_INFO ".to_vec();
        line.resize(athanor_auth::MAX_LINE, b'a');
        line.push(b'\n');
        helper.write_all(&line).expect("line");
        let (ended, asked) = run(&mut attempt);
        assert_eq!((ended.outcome, asked), (Outcome::EndOfStream, 0));
    }

    #[test]
    fn a_second_prompt_ends_the_attempt() {
        let (mut attempt, mut helper) = attempt();
        helper.write_all(PROMPT).expect("prompt");
        helper.write_all(PROMPT).expect("second prompt");
        let (ended, asked) = run(&mut attempt);
        assert_eq!((ended.outcome, asked), (Outcome::EndOfStream, 1));
        drop(attempt);
        // The password went once.
        assert_eq!(read_all(helper), b"athanor-lock\nhunter2\n");
    }

    #[test]
    fn a_visible_prompt_ends_the_attempt() {
        let (mut attempt, mut helper) = attempt();
        helper
            .write_all(b"PAM_PROMPT_ECHO_ON Code: \n")
            .expect("prompt");
        let (ended, asked) = run(&mut attempt);
        assert_eq!((ended.outcome, asked), (Outcome::EndOfStream, 0));
        drop(attempt);
        assert_eq!(read_all(helper), b"athanor-lock\n");
    }

    /// A loop around the source, counting its wake-ups.
    fn event_loop() -> (EventLoop<'static, u32>, Watch) {
        let event_loop = EventLoop::try_new().expect("event loop");
        let (source, _ping, watch) = Source::new().expect("source");
        event_loop
            .handle()
            .insert_source(source, |(), (), wakes: &mut u32| *wakes += 1)
            .expect("insert");
        (event_loop, watch)
    }

    fn wakes(event_loop: &mut EventLoop<'static, u32>) -> u32 {
        let mut wakes = 0;
        event_loop
            .dispatch(Some(Duration::from_millis(50)), &mut wakes)
            .expect("dispatch");
        wakes
    }

    #[test]
    fn a_new_attempt_wakes_the_loop() {
        let (mut event_loop, watch) = event_loop();
        let (attempt, mut helper) = attempt();
        watch.socket(attempt.socket().expect("socket"));
        assert_eq!(wakes(&mut event_loop), 1, "the hand-over's ping");
        assert_eq!(wakes(&mut event_loop), 0, "nothing to read yet");
        helper.write_all(PROMPT).expect("prompt");
        assert_eq!(wakes(&mut event_loop), 1, "the helper's line");
    }

    #[test]
    fn no_wake_up_between_attempts() {
        let (mut event_loop, watch) = event_loop();
        let (mut attempt, mut helper) = attempt();
        watch.socket(attempt.socket().expect("socket"));
        assert_eq!(wakes(&mut event_loop), 1);
        helper.write_all(b"FAILURE\n").expect("verdict");
        drop(helper);
        assert_eq!(wakes(&mut event_loop), 1);
        assert_eq!(run(&mut attempt).0.outcome, Outcome::Failure);
        drop(attempt);
        watch.release();
        // The release's ping, and the closed socket if its event comes before the swap.
        assert!(wakes(&mut event_loop) >= 1, "the release's ping");
        // The closed socket is readable for ever, and no longer watched.
        assert_eq!(wakes(&mut event_loop), 0);
    }
}
