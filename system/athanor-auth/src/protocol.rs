//! The line protocol of polkit-agent-helper-1, which `athanor-unlockd` speaks too (LP8): the
//! helper writes `PAM_PROMPT_ECHO_OFF <text>`, `PAM_PROMPT_ECHO_ON <text>`, `PAM_ERROR_MSG
//! <text>` or `PAM_TEXT_INFO <text>`, then `SUCCESS` or `FAILURE`; the client writes one line
//! per prompt. `athanor-unlockd` adds a third verdict, `UNAVAILABLE`, for a fingerprint
//! transaction that ended uncounted: no reader, or no enrolled finger (LP8). The text is escaped as glib's `g_strescape` does and read back as
//! `g_strcompress` does; answers travel unescaped.

use crate::MAX_LINE;

/// A message of the helper that carries text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An answer hidden as it is typed: a password.
    PromptSecret,
    /// An answer shown as it is typed: a one-time code, a name.
    PromptVisible,
    Error,
    Info,
}

impl Kind {
    pub const fn prefix(self) -> &'static str {
        match self {
            Kind::PromptSecret => "PAM_PROMPT_ECHO_OFF",
            Kind::PromptVisible => "PAM_PROMPT_ECHO_ON",
            Kind::Error => "PAM_ERROR_MSG",
            Kind::Info => "PAM_TEXT_INFO",
        }
    }
}

/// The verdict lines, newline included.
pub const SUCCESS: &[u8] = b"SUCCESS\n";
pub const FAILURE: &[u8] = b"FAILURE\n";
pub const UNAVAILABLE: &[u8] = b"UNAVAILABLE\n";

/// A line of the helper, parsed. PAM's messages are not secrets, so they are plain strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    PromptSecret(String),
    PromptVisible(String),
    Error(String),
    Info(String),
    Success,
    Failure,
    Unavailable,
}

/// Why a line of the helper was refused.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("the line starts with no known prefix")]
    UnknownPrefix,
    #[error("the text holds an escape g_strcompress does not define, or one past a byte")]
    BadEscape,
    #[error("the text holds a NUL byte once unescaped")]
    Nul,
    #[error("the text is not UTF-8 once unescaped")]
    NotUtf8,
    #[error("the line is longer than {MAX_LINE} bytes with its newline")]
    TooLong,
}

/// Parses one line of the helper, without its newline.
pub fn parse(line: &[u8]) -> Result<Line, ProtocolError> {
    // The newline the reader stripped counts toward the bound.
    if line.len() >= MAX_LINE {
        return Err(ProtocolError::TooLong);
    }
    match line {
        b"SUCCESS" => return Ok(Line::Success),
        b"FAILURE" => return Ok(Line::Failure),
        b"UNAVAILABLE" => return Ok(Line::Unavailable),
        _ => {}
    }
    let space = line
        .iter()
        .position(|&b| b == b' ')
        .ok_or(ProtocolError::UnknownPrefix)?;
    let (prefix, text) = (&line[..space], &line[space + 1..]);
    let kind = [
        Kind::PromptSecret,
        Kind::PromptVisible,
        Kind::Error,
        Kind::Info,
    ]
    .into_iter()
    .find(|kind| kind.prefix().as_bytes() == prefix)
    .ok_or(ProtocolError::UnknownPrefix)?;
    let text = String::from_utf8(unescape(text)?).map_err(|_| ProtocolError::NotUtf8)?;
    Ok(match kind {
        Kind::PromptSecret => Line::PromptSecret(text),
        Kind::PromptVisible => Line::PromptVisible(text),
        Kind::Error => Line::Error(text),
        Kind::Info => Line::Info(text),
    })
}

/// One line of the helper, newline included: the prefix, a space and `text` escaped, without
/// one trailing newline, as polkit's helper writes it (`send_to_helper`).
pub fn encode(kind: Kind, text: &[u8]) -> Vec<u8> {
    let text = text.strip_suffix(b"\n").unwrap_or(text);
    let mut out = Vec::with_capacity(kind.prefix().len() + 2 + text.len() * 4);
    out.extend_from_slice(kind.prefix().as_bytes());
    out.push(b' ');
    escape(text, &mut out);
    out.push(b'\n');
    out
}

/// Appends `text` escaped as `g_strescape(text, NULL)` does. PAM's text is bytes in the C
/// locale's sense, so it is taken as bytes.
pub fn escape(text: &[u8], out: &mut Vec<u8>) {
    for &b in text {
        match b {
            0x08 => out.extend_from_slice(b"\\b"),
            0x0c => out.extend_from_slice(b"\\f"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x0b => out.extend_from_slice(b"\\v"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'"' => out.extend_from_slice(b"\\\""),
            b if !(b' '..0o177).contains(&b) => {
                out.push(b'\\');
                out.push(b'0' + (b >> 6));
                out.push(b'0' + ((b >> 3) & 7));
                out.push(b'0' + (b & 7));
            }
            b => out.push(b),
        }
    }
}

/// Reads back what [`escape`] wrote, as `g_strcompress` does, more strictly: an octal escape
/// past 0o377 and a NUL in the result are refused where glib would wrap or truncate.
pub fn unescape(text: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.iter().copied().peekable();
    while let Some(b) = bytes.next() {
        if b != b'\\' {
            out.push(b);
            continue;
        }
        let byte = match bytes.next().ok_or(ProtocolError::BadEscape)? {
            d @ b'0'..=b'7' => {
                let mut value = u32::from(d - b'0');
                for _ in 0..2 {
                    match bytes.peek() {
                        Some(&d @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(d - b'0');
                            bytes.next();
                        }
                        _ => break,
                    }
                }
                u8::try_from(value).map_err(|_| ProtocolError::BadEscape)?
            }
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            // `\\`, `\"` and any other escaped byte stand for themselves.
            other => other,
        };
        out.push(byte);
    }
    if out.contains(&0) {
        return Err(ProtocolError::Nul);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_kind_parses() {
        let cases: [(&[u8], Line); 7] = [
            (
                b"PAM_PROMPT_ECHO_OFF Password: ",
                Line::PromptSecret("Password: ".into()),
            ),
            (
                b"PAM_PROMPT_ECHO_ON Code:",
                Line::PromptVisible("Code:".into()),
            ),
            (
                b"PAM_ERROR_MSG The account is locked",
                Line::Error("The account is locked".into()),
            ),
            (
                b"PAM_TEXT_INFO Place your finger",
                Line::Info("Place your finger".into()),
            ),
            (b"SUCCESS", Line::Success),
            (b"FAILURE", Line::Failure),
            (b"UNAVAILABLE", Line::Unavailable),
        ];
        for (line, want) in cases {
            assert_eq!(parse(line), Ok(want), "{}", String::from_utf8_lossy(line));
        }
    }

    #[test]
    fn an_empty_text_parses() {
        assert_eq!(parse(b"PAM_TEXT_INFO "), Ok(Line::Info(String::new())));
    }

    #[test]
    fn prefixes_and_verdicts_match_exactly() {
        for line in [
            &b"PAM_PROMPT_ECHO_OFF"[..],
            b"pam_text_info x",
            b"PAM_TEXT_INFOx y",
            b"SUCCESS ",
            b"FAILURE\r",
            b"SUCCESSFUL",
            b"UNAVAILABLE ",
            b"unavailable",
            b"",
            b" SUCCESS",
        ] {
            assert_eq!(
                parse(line),
                Err(ProtocolError::UnknownPrefix),
                "{}",
                String::from_utf8_lossy(line)
            );
        }
    }

    #[test]
    fn lines_of_4095_bytes_parse_and_4096_are_refused() {
        let line = |len: usize| {
            let mut line = b"PAM_TEXT_INFO ".to_vec();
            line.resize(len, b'a');
            line
        };
        // Without the newline the reader stripped: 4095 and 4096 bytes with it.
        assert!(parse(&line(MAX_LINE - 2)).is_ok());
        assert!(parse(&line(MAX_LINE - 1)).is_ok());
        assert_eq!(parse(&line(MAX_LINE)), Err(ProtocolError::TooLong));
    }

    #[test]
    fn a_text_not_utf8_once_unescaped_is_refused() {
        assert_eq!(parse(b"PAM_TEXT_INFO \\377"), Err(ProtocolError::NotUtf8));
        assert_eq!(parse(b"PAM_TEXT_INFO \xff"), Err(ProtocolError::NotUtf8));
    }

    /// Pairs of `g_strescape(text, NULL)` and its input, from glib's own definition
    /// (`gstrfuncs.c`); `g_strcompress` reads each back.
    const GLIB: &[(&[u8], &[u8])] = &[
        (b"plain text", b"plain text"),
        (b"two\nlines", b"two\\nlines"),
        (b"back\\slash", b"back\\\\slash"),
        (b"a \"quote\"", b"a \\\"quote\\\""),
        (b"\x08\x0c\r\t\x0b", b"\\b\\f\\r\\t\\v"),
        (b"\x01\x1f\x7f", b"\\001\\037\\177"),
        ("è".as_bytes(), b"\\303\\250"),
        (b"Password: ", b"Password: "),
    ];

    #[test]
    fn escape_matches_glib() {
        for (text, escaped) in GLIB {
            let mut out = Vec::new();
            escape(text, &mut out);
            assert_eq!(out, *escaped, "{}", String::from_utf8_lossy(text));
        }
    }

    #[test]
    fn unescape_reads_back_glib() {
        for (text, escaped) in GLIB {
            assert_eq!(unescape(escaped).as_deref(), Ok(*text));
        }
    }

    #[test]
    fn unescape_follows_g_strcompress_on_short_and_unknown_escapes() {
        // One or two octal digits end at the first non-octal byte; an unknown escape is the byte.
        assert_eq!(unescape(b"\\101B\\12x").as_deref(), Ok(&b"AB\nx"[..]));
        assert_eq!(unescape(b"\\1018").as_deref(), Ok(&b"A8"[..]));
        assert_eq!(unescape(b"\\q\\'").as_deref(), Ok(&b"q'"[..]));
    }

    #[test]
    fn unescape_refuses_a_nul_a_trailing_backslash_and_an_octal_past_a_byte() {
        assert_eq!(unescape(b"a\\0b"), Err(ProtocolError::Nul));
        assert_eq!(unescape(b"a\\000"), Err(ProtocolError::Nul));
        assert_eq!(unescape(b"a\\"), Err(ProtocolError::BadEscape));
        assert_eq!(unescape(b"\\400"), Err(ProtocolError::BadEscape));
        assert_eq!(unescape(b"\\377").as_deref(), Ok(&[0xff][..]));
    }

    #[test]
    fn encode_writes_polkits_line_and_parse_reads_it_back() {
        assert_eq!(
            encode(Kind::PromptSecret, b"Password: "),
            b"PAM_PROMPT_ECHO_OFF Password: \n"
        );
        // One trailing newline is dropped, as polkit's helper does; an inner one is escaped.
        assert_eq!(
            encode(Kind::Error, b"locked\nfor 10 minutes\n"),
            b"PAM_ERROR_MSG locked\\nfor 10 minutes\n"
        );
        assert_eq!(encode(Kind::Info, b""), b"PAM_TEXT_INFO \n");
        let line = encode(Kind::Info, "Appoggia il dito sul lettore".as_bytes());
        assert_eq!(
            parse(&line[..line.len() - 1]),
            Ok(Line::Info("Appoggia il dito sul lettore".into()))
        );
    }

    #[test]
    fn the_longest_pam_message_fits_a_line() {
        // PAM_MAX_MSG_SIZE is 512; each byte escapes to at most four.
        let line = encode(Kind::PromptVisible, &[0x80; 512]);
        assert!(line.len() <= MAX_LINE);
        assert!(parse(&line[..line.len() - 1]).is_err_and(|e| e == ProtocolError::NotUtf8));
    }
}
