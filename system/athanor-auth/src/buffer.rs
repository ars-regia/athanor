//! The buffer an answer is held in, and the read that fills it from a stream.

use std::fmt;
use std::io::{self, Read};

use zeroize::{Zeroize, Zeroizing};

use crate::MAX_LINE;

/// An answer, a password or a line read from the peer. Its storage is allocated once, at
/// `MAX_LINE` bytes, and never grows, so no reallocation leaves an old copy in freed heap; it
/// is zeroised when the `Secret` is dropped. It has no `Clone` and no `Display`, and its
/// `Debug` shows only its length.
pub struct Secret {
    bytes: Zeroizing<Box<[u8]>>,
    len: usize,
}

/// The `Secret` already holds `MAX_LINE` bytes.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("the buffer is full ({MAX_LINE} bytes)")]
pub struct Full;

impl Secret {
    pub fn new() -> Self {
        Secret {
            bytes: Zeroizing::new(vec![0; MAX_LINE].into_boxed_slice()),
            len: 0,
        }
    }

    /// Appends one byte, or refuses it when the buffer is full.
    pub fn try_push(&mut self, byte: u8) -> Result<(), Full> {
        let slot = self.bytes.get_mut(self.len).ok_or(Full)?;
        *slot = byte;
        self.len += 1;
        Ok(())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Zeroes what the buffer held and empties it.
    pub fn clear(&mut self) {
        self.bytes[..self.len].zeroize();
        self.len = 0;
    }
}

impl Default for Secret {
    fn default() -> Self {
        Secret::new()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({} bytes)", self.len)
    }
}

/// Why no line was read.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The stream ended before a newline, the bytes read so far included.
    #[error("the stream ended before the end of the line")]
    Eof,
    /// The line, its newline included, is longer than its bound.
    #[error("the line is longer than its bound")]
    TooLong,
    /// The line holds a NUL byte, which no C string can carry.
    #[error("the line holds a NUL byte")]
    Nul,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Reads one line of at most `MAX_LINE` bytes, its newline included, into `secret`, without
/// the newline. See [`read_line_within`].
pub fn read_line(reader: &mut impl Read, secret: &mut Secret) -> Result<(), ReadError> {
    read_line_within(reader, secret, MAX_LINE)
}

/// Reads one line of at most `max` bytes (no more than `MAX_LINE`), its newline included, into
/// `secret`, without the newline. What `secret` held is cleared first, and on an error it is
/// cleared again, so a partial line never stays in it.
///
/// Each byte is read by its own `read` call straight into `secret`: a `BufReader` would keep
/// its own buffer, a copy of the answer that nothing zeroises, and would read past the line.
pub fn read_line_within(
    reader: &mut impl Read,
    secret: &mut Secret,
    max: usize,
) -> Result<(), ReadError> {
    secret.clear();
    let read = fill(reader, secret, max.min(MAX_LINE));
    if read.is_err() {
        secret.clear();
    }
    read
}

fn fill(reader: &mut impl Read, secret: &mut Secret, max: usize) -> Result<(), ReadError> {
    // ponytail: one read syscall per byte, at most MAX_LINE per line; read into a zeroising
    // buffer of our own if a measurement ever shows the cost.
    let mut byte = Zeroizing::new([0u8; 1]);
    loop {
        match reader.read(&mut byte[..]) {
            Ok(0) => return Err(ReadError::Eof),
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(ReadError::Io(err)),
        }
        match byte[0] {
            b'\n' => return Ok(()),
            0 => return Err(ReadError::Nul),
            // The newline needs a byte of the bound too.
            _ if secret.len() + 1 >= max => return Err(ReadError::TooLong),
            b => secret.try_push(b).map_err(|Full| ReadError::TooLong)?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Secret {
        /// The whole buffer, past `len` included.
        fn storage(&self) -> &[u8] {
            &self.bytes
        }
    }

    fn filled(n: usize) -> Secret {
        let mut secret = Secret::new();
        for _ in 0..n {
            secret.try_push(b'x').expect("within the bound");
        }
        secret
    }

    #[test]
    fn the_buffer_refuses_its_4097th_byte_and_never_moves() {
        let mut secret = Secret::new();
        let before = secret.storage().as_ptr();
        for _ in 0..MAX_LINE {
            assert_eq!(secret.try_push(b'x'), Ok(()));
        }
        assert_eq!(secret.try_push(b'x'), Err(Full));
        assert_eq!(secret.len(), MAX_LINE);
        assert_eq!(secret.storage().as_ptr(), before);
    }

    #[test]
    fn clear_zeroes_the_bytes() {
        let mut secret = filled(10);
        secret.clear();
        assert!(secret.is_empty());
        assert!(secret.storage().iter().all(|&b| b == 0));
    }

    #[test]
    fn debug_shows_the_length_and_no_content() {
        let mut secret = Secret::new();
        for &b in b"hunter2" {
            secret.try_push(b).expect("within the bound");
        }
        assert_eq!(format!("{secret:?}"), "Secret(7 bytes)");
    }

    fn line(input: &[u8]) -> Result<Vec<u8>, ReadError> {
        let mut secret = filled(3);
        let mut reader = input;
        read_line(&mut reader, &mut secret).map(|()| secret.as_bytes().to_vec())
    }

    #[test]
    fn a_line_is_read_without_its_newline_and_replaces_what_was_held() {
        assert_eq!(line(b"guess\nnext\n").expect("a line"), b"guess");
        assert_eq!(line(b"\n").expect("an empty line"), b"");
    }

    #[test]
    fn lines_of_4095_and_4096_bytes_pass_and_4097_is_refused() {
        for total in [MAX_LINE - 1, MAX_LINE] {
            let mut input = vec![b'a'; total - 1];
            input.push(b'\n');
            assert_eq!(line(&input).expect("within the bound").len(), total - 1);
        }
        let mut input = vec![b'a'; MAX_LINE];
        input.push(b'\n');
        assert!(matches!(line(&input), Err(ReadError::TooLong)));
    }

    #[test]
    fn an_answer_of_4096_bytes_without_a_newline_is_too_long() {
        assert!(matches!(line(&[b'a'; MAX_LINE]), Err(ReadError::TooLong)));
    }

    #[test]
    fn a_stream_that_ends_early_is_eof_and_leaves_nothing_held() {
        let mut secret = Secret::new();
        let mut reader: &[u8] = b"gu";
        assert!(matches!(
            read_line(&mut reader, &mut secret),
            Err(ReadError::Eof)
        ));
        assert!(secret.is_empty());
        assert!(matches!(line(b""), Err(ReadError::Eof)));
    }

    #[test]
    fn a_nul_byte_is_refused() {
        assert!(matches!(line(b"gu\0ess\n"), Err(ReadError::Nul)));
    }

    #[test]
    fn the_read_stops_at_the_newline() {
        let mut secret = Secret::new();
        let mut reader: &[u8] = b"one\ntwo\n";
        read_line(&mut reader, &mut secret).expect("first line");
        assert_eq!(reader, b"two\n");
    }

    #[test]
    fn a_smaller_bound_counts_the_newline() {
        let mut secret = Secret::new();
        let mut reader: &[u8] = b"abc\n";
        read_line_within(&mut reader, &mut secret, 4).expect("four bytes with the newline");
        let mut reader: &[u8] = b"abcd\n";
        assert!(matches!(
            read_line_within(&mut reader, &mut secret, 4),
            Err(ReadError::TooLong)
        ));
    }
}
