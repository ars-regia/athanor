//! The authentication conversation shared by the session lock, the polkit agent and their PAM
//! helpers (doc_lock_and_prompts.md, LP2, LP8): the line protocol of polkit-agent-helper-1, its
//! parser, the bounded buffers that hold answers and zeroise them, and the classification of
//! a transaction's messages. No toolkit type and no I/O policy (D34): the caller owns the
//! stream.

#![deny(unsafe_code)]

mod buffer;
mod protocol;

use std::io::{self, Read, Write};

pub use buffer::{read_line, read_line_within, Full, ReadError, Secret};
pub use protocol::{
    encode, escape, parse, unescape, Kind, Line, ProtocolError, FAILURE, SUCCESS, UNAVAILABLE,
};

/// The bound of a line in both directions, its newline included (LP8).
pub const MAX_LINE: usize = 4096;

/// The bound of the client's first line, the PAM service it selects, newline included (LP8).
pub const MAX_SELECTOR: usize = 64;

/// The two PAM services of `athanor-unlockd`'s socket; the client's first line names one (D28).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// `/usr/lib/pam.d/athanor-lock`: the password.
    Password,
    /// `/usr/lib/pam.d/athanor-lock-fingerprint`: a finger on the reader (LP9).
    Fingerprint,
}

impl Service {
    pub const fn name(self) -> &'static str {
        match self {
            Service::Password => "athanor-lock",
            Service::Fingerprint => "athanor-lock-fingerprint",
        }
    }

    /// The service a selector line names, matched exactly.
    pub fn from_selector(line: &[u8]) -> Option<Service> {
        [Service::Password, Service::Fingerprint]
            .into_iter()
            .find(|service| service.name().as_bytes() == line)
    }
}

/// The verdict of a transaction (LP8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    /// Refused, or a finger that did not match: a counted failure.
    Failure,
    /// A fingerprint transaction that ended uncounted, with no reader or no enrolled finger:
    /// the lock offers the finger again only on a user action or a new reader.
    Unavailable,
}

impl Outcome {
    /// The verdict line, newline included.
    pub const fn line(self) -> &'static [u8] {
        match self {
            Outcome::Success => SUCCESS,
            Outcome::Failure => FAILURE,
            Outcome::Unavailable => UNAVAILABLE,
        }
    }
}

/// A question PAM asks, in PAM's own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    /// An answer hidden as it is typed (PAM_PROMPT_ECHO_OFF): a password.
    Secret(String),
    /// An answer shown as it is typed (PAM_PROMPT_ECHO_ON): a one-time code.
    Visible(String),
}

/// How a transaction ended, with the messages PAM sent on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub outcome: Outcome,
    /// PAM_ERROR_MSG texts, in order.
    pub errors: Vec<String>,
    /// PAM_TEXT_INFO texts, in order.
    pub info: Vec<String>,
}

/// What a refused transaction tells the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// PAM's own error text, such as pam_faillock's lockout message.
    Pam(String),
    /// No text from PAM: the caller shows its own translated "Wrong password".
    WrongPassword,
}

/// PAM's error text where PAM sent one, else [`Refusal::WrongPassword`] (LP6), as the greeter
/// does (`athanor-greeter-ui` `auth.rs`, `refusal`).
pub fn refusal(errors: &[String]) -> Refusal {
    if errors.is_empty() {
        Refusal::WrongPassword
    } else {
        Refusal::Pam(errors.join("\n"))
    }
}

/// Why a transaction ended without a verdict.
#[derive(Debug, thiserror::Error)]
pub enum TransactError {
    #[error("the helper's line could not be read: {0}")]
    Read(#[from] ReadError),
    #[error("the helper's line is malformed: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("the answer could not be written: {0}")]
    Write(#[from] io::Error),
    #[error("the answer is longer than its bound")]
    AnswerTooLong,
}

/// Runs one transaction as the client, blocking: writes the selector of `service`, then for
/// each prompt calls `answer`, which fills the `Secret` it is given, and writes the answer and
/// its newline as two writes, so that no buffer holds them together. The `Secret` is cleared
/// after each answer. An answer must leave room for its newline within `MAX_LINE`.
pub fn transact<S, F>(
    stream: &mut S,
    service: Service,
    mut answer: F,
) -> Result<Verdict, TransactError>
where
    S: Read + Write,
    F: FnMut(&Prompt, &mut Secret),
{
    stream.write_all(service.name().as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = Secret::new();
    let mut reply = Secret::new();
    let mut verdict = Verdict {
        outcome: Outcome::Failure,
        errors: Vec::new(),
        info: Vec::new(),
    };
    loop {
        read_line(stream, &mut line)?;
        let prompt = match parse(line.as_bytes())? {
            Line::Success => {
                verdict.outcome = Outcome::Success;
                return Ok(verdict);
            }
            Line::Failure => return Ok(verdict),
            Line::Unavailable => {
                verdict.outcome = Outcome::Unavailable;
                return Ok(verdict);
            }
            Line::Error(text) => {
                verdict.errors.push(text);
                continue;
            }
            Line::Info(text) => {
                verdict.info.push(text);
                continue;
            }
            Line::PromptSecret(text) => Prompt::Secret(text),
            Line::PromptVisible(text) => Prompt::Visible(text),
        };
        reply.clear();
        answer(&prompt, &mut reply);
        let sent = send(stream, &reply);
        reply.clear();
        sent?;
    }
}

/// Writes one answer and its newline, as two writes.
fn send(stream: &mut impl Write, reply: &Secret) -> Result<(), TransactError> {
    if reply.len() >= MAX_LINE {
        return Err(TransactError::AnswerTooLong);
    }
    stream.write_all(reply.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A helper played from a script: reads come from `script`, each write is recorded.
    struct Helper {
        script: &'static [u8],
        writes: Vec<Vec<u8>>,
    }

    impl Read for Helper {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.script.read(buf)
        }
    }

    impl Write for Helper {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.writes.push(buf.to_vec());
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn helper(script: &'static [u8]) -> Helper {
        Helper {
            script,
            writes: Vec::new(),
        }
    }

    fn type_in(text: &[u8]) -> impl FnMut(&Prompt, &mut Secret) + '_ {
        move |_, reply| {
            for &b in text {
                reply.try_push(b).expect("within the bound");
            }
        }
    }

    #[test]
    fn a_transaction_sends_the_selector_and_each_answer_as_two_writes() {
        let mut stream = helper(
            b"PAM_TEXT_INFO hello\nPAM_PROMPT_ECHO_OFF Password: \nPAM_ERROR_MSG nope\nSUCCESS\n",
        );
        let mut asked = Vec::new();
        let verdict = transact(&mut stream, Service::Password, |prompt, reply| {
            asked.push(prompt.clone());
            type_in(b"hunter2")(prompt, reply);
        })
        .expect("a verdict");
        assert_eq!(asked, [Prompt::Secret("Password: ".into())]);
        assert_eq!(
            verdict,
            Verdict {
                outcome: Outcome::Success,
                errors: vec!["nope".into()],
                info: vec!["hello".into()],
            }
        );
        let writes: Vec<&[u8]> = stream.writes.iter().map(Vec::as_slice).collect();
        assert_eq!(writes, [&b"athanor-lock"[..], b"\n", b"hunter2", b"\n"]);
    }

    #[test]
    fn a_failure_is_a_verdict_and_names_the_fingerprint_service() {
        let mut stream = helper(b"FAILURE\n");
        let verdict = transact(&mut stream, Service::Fingerprint, type_in(b"")).expect("a verdict");
        assert_eq!(verdict.outcome, Outcome::Failure);
        assert_eq!(stream.writes[0], b"athanor-lock-fingerprint");
    }

    #[test]
    fn an_unavailable_reader_is_its_own_verdict() {
        let mut stream = helper(b"PAM_ERROR_MSG No reader\nUNAVAILABLE\n");
        let verdict = transact(&mut stream, Service::Fingerprint, type_in(b"")).expect("a verdict");
        assert_eq!(verdict.outcome, Outcome::Unavailable);
        assert_eq!(verdict.errors, ["No reader"]);
    }

    #[test]
    fn each_outcome_has_its_line() {
        for outcome in [Outcome::Success, Outcome::Failure, Outcome::Unavailable] {
            let line = outcome.line();
            let want = match parse(&line[..line.len() - 1]).expect("a verdict line") {
                Line::Success => Outcome::Success,
                Line::Failure => Outcome::Failure,
                Line::Unavailable => Outcome::Unavailable,
                other => panic!("{other:?} is no verdict"),
            };
            assert_eq!(want, outcome);
        }
    }

    #[test]
    fn an_answer_with_no_room_for_its_newline_is_not_sent() {
        let mut stream = helper(b"PAM_PROMPT_ECHO_OFF Password: \n");
        let long = [b'a'; MAX_LINE];
        let ended = transact(&mut stream, Service::Password, type_in(&long));
        assert!(matches!(ended, Err(TransactError::AnswerTooLong)));
        assert_eq!(stream.writes.len(), 2, "only the selector went out");
    }

    #[test]
    fn a_helper_that_ends_without_a_verdict_is_an_error() {
        let mut stream = helper(b"PAM_TEXT_INFO hello\n");
        let ended = transact(&mut stream, Service::Password, type_in(b""));
        assert!(matches!(ended, Err(TransactError::Read(ReadError::Eof))));
        let mut stream = helper(b"GARBAGE\n");
        let ended = transact(&mut stream, Service::Password, type_in(b""));
        assert!(matches!(
            ended,
            Err(TransactError::Protocol(ProtocolError::UnknownPrefix))
        ));
    }

    #[test]
    fn refusal_is_pams_text_else_wrong_password() {
        assert_eq!(refusal(&[]), Refusal::WrongPassword);
        assert_eq!(
            refusal(&["locked".into(), "10 minutes left".into()]),
            Refusal::Pam("locked\n10 minutes left".into())
        );
    }

    #[test]
    fn the_selector_matches_the_two_names_exactly() {
        assert_eq!(
            Service::from_selector(b"athanor-lock"),
            Some(Service::Password)
        );
        assert_eq!(
            Service::from_selector(b"athanor-lock-fingerprint"),
            Some(Service::Fingerprint)
        );
        for line in [
            &b"athanor-lock "[..],
            b"login",
            b"",
            b"ATHANOR-LOCK",
            b"athanor-lock\r",
        ] {
            assert_eq!(Service::from_selector(line), None);
        }
        assert!(Service::Fingerprint.name().len() < MAX_SELECTOR);
    }
}
