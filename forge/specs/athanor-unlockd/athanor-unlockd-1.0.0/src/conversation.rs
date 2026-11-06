//! The PAM conversation as a pure function over the client's stream, so that its close rules
//! are tested without libpam (LP8, D27, D37). `ffi` calls it from PAM's callback.

use std::io::{self, Read, Write};

use athanor_auth::{encode, read_line, Kind, ReadError, Secret, Service, MAX_LINE};

/// One message of a PAM conversation. `kind` is `None` for a style the helper does not speak
/// (`PAM_RADIO_TYPE`, `PAM_BINARY_PROMPT`).
pub struct Message<'a> {
    pub kind: Option<Kind>,
    pub text: &'a [u8],
}

/// Why the callback gives PAM no answers.
#[derive(Debug, PartialEq, Eq)]
pub enum ConvError {
    /// The client closed before answering any prompt of the password service: the helper ends
    /// with `_exit(2)` and no verdict, and no failure is counted (D27, polkit#713).
    Cancelled,
    /// The callback returns `PAM_CONV_ERR`, and the stack counts the failure.
    Failed,
}

/// The client's side of the conversation, and whether it has answered a prompt yet.
pub struct Conversation<R, W> {
    pub reader: R,
    pub writer: W,
    pub service: Service,
    pub uid: u32,
    answered: bool,
}

impl<R: Read, W: Write> Conversation<R, W> {
    pub fn new(reader: R, writer: W, service: Service, uid: u32) -> Self {
        Conversation {
            reader,
            writer,
            service,
            uid,
            answered: false,
        }
    }

    /// Sends each message to the client and reads one answer per prompt. The answers line up
    /// with `messages`, `None` where a message asks nothing.
    ///
    /// The close rules: on the password service, a client gone (end of stream, `EPIPE`,
    /// `ECONNRESET`) before its first answer cancels the transaction, and after it fails it. On
    /// the fingerprint service a failed write does not end the helper (D37): a prompt it cannot
    /// send fails the conversation, a message it cannot send is passed over, so the stack runs
    /// to `pam_faillock.so authfail`. A line past `MAX_LINE`, a NUL in an answer or a style
    /// the helper does not speak fails it.
    pub fn converse(&mut self, messages: &[Message<'_>]) -> Result<Vec<Option<Secret>>, ConvError> {
        let mut answers = Vec::with_capacity(messages.len());
        for message in messages {
            let kind = message.kind.ok_or(ConvError::Failed)?;
            let prompt = matches!(kind, Kind::PromptSecret | Kind::PromptVisible);
            let line = encode(kind, message.text);
            if line.len() > MAX_LINE {
                return Err(ConvError::Failed);
            }
            if let Err(error) = self.send(&line) {
                if self.service == Service::Fingerprint && !prompt {
                    answers.push(None);
                    continue;
                }
                return Err(self.ended(hung_up(&error)));
            }
            if !prompt {
                answers.push(None);
                continue;
            }
            let mut answer = Secret::new();
            match read_line(&mut self.reader, &mut answer) {
                Ok(()) => {
                    self.answered = true;
                    answers.push(Some(answer));
                }
                Err(ReadError::Eof) => return Err(self.ended(true)),
                Err(ReadError::Io(error)) => return Err(self.ended(hung_up(&error))),
                Err(ReadError::TooLong | ReadError::Nul) => return Err(ConvError::Failed),
            }
        }
        Ok(answers)
    }

    fn send(&mut self, line: &[u8]) -> io::Result<()> {
        self.writer.write_all(line)?;
        self.writer.flush()
    }

    /// What a failed exchange means: a cancel only on the password service, for a client gone
    /// before it answered anything.
    fn ended(&self, gone: bool) -> ConvError {
        if gone && self.service == Service::Password && !self.answered {
            ConvError::Cancelled
        } else {
            ConvError::Failed
        }
    }
}

fn hung_up(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The client's end for the helper's writes: they land in `output` until `writes_left` runs
    /// out, then fail with `EPIPE`.
    struct Client {
        output: Vec<u8>,
        writes_left: usize,
    }

    impl Write for Client {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.writes_left == 0 {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            self.writes_left -= 1;
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn conversation(
        service: Service,
        input: &'static [u8],
        writes: usize,
    ) -> Conversation<&'static [u8], Client> {
        let client = Client {
            output: Vec::new(),
            writes_left: writes,
        };
        Conversation::new(input, client, service, 1000)
    }

    const PASSWORD: Message<'static> = Message {
        kind: Some(Kind::PromptSecret),
        text: b"Password: ",
    };
    const INFO: Message<'static> = Message {
        kind: Some(Kind::Info),
        text: b"Place your finger on the reader\n",
    };
    const MISMATCH: Message<'static> = Message {
        kind: Some(Kind::Error),
        text: b"Failed to match fingerprint",
    };

    fn answer(answers: &[Option<Secret>], i: usize) -> Option<&[u8]> {
        answers[i].as_ref().map(Secret::as_bytes)
    }

    #[test]
    fn each_prompt_goes_out_escaped_and_gets_one_answer() {
        let mut conv = conversation(Service::Password, b"hunter2\n", usize::MAX);
        let answers = conv.converse(&[INFO, PASSWORD]).expect("answers");
        assert_eq!(answer(&answers, 0), None);
        assert_eq!(answer(&answers, 1), Some(&b"hunter2"[..]));
        assert_eq!(
            conv.writer.output,
            b"PAM_TEXT_INFO Place your finger on the reader\nPAM_PROMPT_ECHO_OFF Password: \n"
        );
    }

    #[test]
    fn a_close_before_the_first_answer_cancels_the_password_service() {
        let mut conv = conversation(Service::Password, b"", usize::MAX);
        assert_eq!(conv.converse(&[PASSWORD]).err(), Some(ConvError::Cancelled));
        let mut conv = conversation(Service::Password, b"gu", usize::MAX);
        assert_eq!(
            conv.converse(&[PASSWORD]).err(),
            Some(ConvError::Cancelled),
            "a partial line is not an answer"
        );
        let mut conv = conversation(Service::Password, b"", 0);
        assert_eq!(conv.converse(&[PASSWORD]).err(), Some(ConvError::Cancelled));
        let mut conv = conversation(Service::Password, b"", 0);
        assert_eq!(conv.converse(&[INFO]).err(), Some(ConvError::Cancelled));
    }

    #[test]
    fn a_close_after_an_answer_fails_and_is_counted() {
        let mut conv = conversation(Service::Password, b"hunter2\n", usize::MAX);
        conv.converse(&[PASSWORD]).expect("the first answer");
        assert_eq!(conv.converse(&[PASSWORD]).err(), Some(ConvError::Failed));
        let mut conv = conversation(Service::Password, b"hunter2\n", 1);
        conv.converse(&[PASSWORD]).expect("the first answer");
        assert_eq!(conv.converse(&[MISMATCH]).err(), Some(ConvError::Failed));
    }

    #[test]
    fn a_line_too_long_or_a_nul_fails_even_before_an_answer() {
        let long: &'static [u8] = Box::leak(vec![b'a'; MAX_LINE + 1].into_boxed_slice());
        let mut conv = conversation(Service::Password, long, usize::MAX);
        assert_eq!(conv.converse(&[PASSWORD]).err(), Some(ConvError::Failed));
        let mut conv = conversation(Service::Password, b"gu\0ess\n", usize::MAX);
        assert_eq!(conv.converse(&[PASSWORD]).err(), Some(ConvError::Failed));
    }

    #[test]
    fn a_message_too_long_for_a_line_or_of_an_unknown_style_fails() {
        let text: &'static [u8] = Box::leak(vec![0x80; MAX_LINE].into_boxed_slice());
        let mut conv = conversation(Service::Password, b"", usize::MAX);
        let long = Message {
            kind: Some(Kind::Info),
            text,
        };
        assert_eq!(conv.converse(&[long]).err(), Some(ConvError::Failed));
        assert!(conv.writer.output.is_empty());
        let radio = Message {
            kind: None,
            text: b"choose",
        };
        assert_eq!(conv.converse(&[radio]).err(), Some(ConvError::Failed));
    }

    #[test]
    fn the_fingerprint_service_passes_over_a_message_it_cannot_send() {
        let mut conv = conversation(Service::Fingerprint, b"", 0);
        let answers = conv.converse(&[MISMATCH, INFO]).expect("passed over");
        assert_eq!(answers.len(), 2);
        assert!(answers.iter().all(Option::is_none));
    }

    #[test]
    fn the_fingerprint_service_fails_a_prompt_and_never_cancels() {
        let mut conv = conversation(Service::Fingerprint, b"", 0);
        assert_eq!(conv.converse(&[PASSWORD]).err(), Some(ConvError::Failed));
        let mut conv = conversation(Service::Fingerprint, b"", usize::MAX);
        assert_eq!(conv.converse(&[PASSWORD]).err(), Some(ConvError::Failed));
    }
}
