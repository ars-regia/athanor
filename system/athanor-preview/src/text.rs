//! The first lines of a text file, read in the launcher (doc_launcher.md, LA6): at most 64
//! KB, cut at a character boundary, every line cleaned of control and bidirectional
//! characters.

use std::fs::File;
use std::io::{self, Read};

pub const MAX_BYTES: usize = 64 * 1024;
/// The body shows about one screen: 40 lines, and BODY_CHARS (2048, about 26 wrapped rows
/// of 80 characters) in all, so neither a file of newlines nor one 64 KB line makes the
/// label taller than that.
pub const MAX_LINES: usize = 40;

pub fn head(file: &File) -> io::Result<String> {
    let mut bytes = Vec::with_capacity(MAX_BYTES);
    file.take(MAX_BYTES as u64).read_to_end(&mut bytes)?;
    Ok(clean(&decoded(&bytes)))
}

/// The text of what was read. A character cut by the limit is dropped; anything else
/// invalid shows as U+FFFD.
fn decoded(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(err) if err.error_len().is_none() => String::from_utf8_lossy(&bytes[..err.valid_up_to()]).into_owned(),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// At most MAX_LINES lines, then the shared filter over the whole body.
fn clean(text: &str) -> String {
    let body = text.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n");
    athanor_unit::text::lines(&body, athanor_unit::text::BODY_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_huge_file_is_read_to_its_first_64_kilobytes_and_cleaned() {
        let path = std::env::temp_dir().join(format!("athanor-preview-head-{}", std::process::id()));
        std::fs::write(&path, "line\u{202E}one\nline two\n").expect("write");
        // Sparse: 2 GB on paper, a few blocks on disk.
        let file = std::fs::OpenOptions::new().write(true).open(&path).expect("open");
        file.set_len(2 << 30).expect("grow");
        let text = head(&std::fs::File::open(&path).expect("open")).expect("read");
        assert!(text.starts_with("lineone\nline two"), "bidirectional controls removed: {text:?}");
        assert!(text.len() <= MAX_BYTES);
        std::fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn a_file_of_newlines_is_cut_to_the_line_cap() {
        let text = clean(&"x\n".repeat(MAX_LINES * 3));
        assert_eq!(text.lines().count(), MAX_LINES);
    }

    #[test]
    fn a_character_cut_by_the_limit_is_dropped() {
        let bytes = [vec![b'a'; MAX_BYTES - 1], "é".as_bytes().to_vec()].concat();
        let text = decoded(&bytes[..MAX_BYTES]);
        assert_eq!(text.len(), MAX_BYTES - 1, "the é is dropped, the 'a's stay");
        assert!(text.chars().all(|c| c == 'a'));
        assert!(decoded(b"a\xFFb").contains('\u{FFFD}'), "other invalid bytes show as U+FFFD");
    }

    #[test]
    fn one_long_line_is_cut_to_the_body_cap() {
        let path = std::env::temp_dir().join(format!("athanor-preview-long-{}", std::process::id()));
        std::fs::write(&path, vec![b'a'; 3 * MAX_BYTES]).expect("write");
        let text = head(&File::open(&path).expect("open")).expect("read");
        assert_eq!(text.chars().count(), athanor_unit::text::BODY_CHARS);
        std::fs::remove_file(path).expect("cleanup");
    }
}
