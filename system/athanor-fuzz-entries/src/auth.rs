//! The line protocol the session lock and the polkit agent read from their PAM helpers
//! (`athanor-auth`, doc_lock_and_prompts.md LP8).

use std::io;

use athanor_auth::{encode, parse, Kind, Line, LineReader, ReadError, MAX_LINE};

use crate::Setup;

/// Every line of `data` through the incremental reader and the parser. A text line that
/// parses is encoded again, as the helper writes it, and must parse back to the same line.
pub fn protocol(data: &[u8]) -> Setup {
    let mut stream = data;
    let mut reader = LineReader::new();
    loop {
        let line = match reader.read(&mut stream) {
            Ok(Some(line)) => line,
            // A slice never blocks: `None` would be the end too.
            Ok(None) | Err(ReadError::Eof) => return Ok(()),
            Err(_) => continue,
        };
        let (kind, text) = match parse(line) {
            Ok(Line::PromptSecret(text)) => (Kind::PromptSecret, text),
            Ok(Line::PromptVisible(text)) => (Kind::PromptVisible, text),
            Ok(Line::Error(text)) => (Kind::Error, text),
            Ok(Line::Info(text)) => (Kind::Info, text),
            Ok(Line::Success | Line::Failure | Line::Unavailable) | Err(_) => continue,
        };
        let encoded = encode(kind, text.as_bytes());
        // Escaping can grow a text past the bound, which the parser then refuses.
        if encoded.len() > MAX_LINE {
            continue;
        }
        // The encoder drops one trailing newline, as polkit's helper does.
        let want = text.strip_suffix('\n').unwrap_or(&text).to_owned();
        let want = match kind {
            Kind::PromptSecret => Line::PromptSecret(want),
            Kind::PromptVisible => Line::PromptVisible(want),
            Kind::Error => Line::Error(want),
            Kind::Info => Line::Info(want),
        };
        let back = parse(&encoded[..encoded.len() - 1]);
        if back.as_ref() != Ok(&want) {
            return Err(io::Error::other(format!(
                "{want:?} encoded as {:?} parses back as {back:?}",
                String::from_utf8_lossy(&encoded)
            )));
        }
    }
}
